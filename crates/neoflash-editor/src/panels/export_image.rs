//! Export image modal dialog and live floating body (NF2 `Dialog / Export image`, `DL-04`,
//! `W3.1` / `W3.2`; reference `dialogs-export.png`), opened from `File ▸ Export ▸ Export image…`
//! and `Window ▸ Panels ▸ Planned panels ▸ Export image`.

use super::dialog_helpers::{group, preview_box};
use super::planned::{FooterClick, caption, footer_clicked};
use crate::app::Editor;
use eframe::egui::{self, Ui};
use neoflash_export::{
    ImageExportFormat, ImageExportOptions, ImageExportRange, ImageExportReport,
    estimate_image_export_summary, export_project_image,
};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::path::Path;

const IMAGE_FORMATS: [(usize, &str); 5] =
    [(0, "PNG"), (1, "SVG"), (2, "PSD"), (3, "AI"), (4, "PDF")];

const SCALES: [(f32, &str); 4] = [(0.5, "0.5×"), (1.0, "1×"), (2.0, "2×"), (4.0, "4×")];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExportImageDraft {
    pub(crate) options: ImageExportOptions,
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-export-image-draft")
}

fn inline_draft_id() -> egui::Id {
    egui::Id::new("nf2-export-image-inline-draft")
}

pub(crate) fn has_export_image_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<ExportImageDraft>(draft_id()))
        .is_some()
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn export_image_draft(ctx: &egui::Context) -> Option<ExportImageDraft> {
    ctx.data(|d| d.get_temp::<ExportImageDraft>(draft_id()))
        .or_else(|| ctx.data(|d| d.get_temp::<ExportImageDraft>(inline_draft_id())))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn update_export_image_draft(
    ctx: &egui::Context,
    mut f: impl FnMut(&mut ExportImageDraft),
) {
    for id in [draft_id(), inline_draft_id()] {
        if let Some(mut draft) = ctx.data(|d| d.get_temp::<ExportImageDraft>(id)) {
            f(&mut draft);
            ctx.data_mut(|d| d.insert_temp(id, draft));
        }
    }
}

#[cfg(test)]
pub(crate) fn set_inline_export_image_draft(ctx: &egui::Context, draft: ExportImageDraft) {
    ctx.data_mut(|data| data.insert_temp(inline_draft_id(), draft));
}

impl Editor {
    fn default_export_image_options(&self) -> ImageExportOptions {
        let symbol_id = self.symbol_id().to_owned();
        let total_frames = self.symbol().timeline.frame_count.max(1);
        if let Some(mut saved) = self.workspace.export_image_options.clone() {
            match &mut saved.range {
                ImageExportRange::CurrentFrame(frame) => {
                    *frame = self.frame;
                    saved.symbol_id = Some(symbol_id);
                }
                ImageExportRange::WholeMovie => {
                    saved.symbol_id = None;
                }
                ImageExportRange::CurrentScene { scene_id } => {
                    *scene_id = symbol_id.clone();
                    saved.symbol_id = Some(symbol_id);
                }
                ImageExportRange::Frames { start, end } => {
                    let max_f = total_frames.saturating_sub(1);
                    *start = (*start).min(max_f);
                    *end = (*end).clamp(*start, max_f);
                    saved.symbol_id = Some(symbol_id);
                }
            }
            if saved.constrain_proportions && saved.scale.is_finite() && saved.scale > 0.0 {
                saved.width = ((self.project.width.max(1) as f32 * saved.scale).round() as u32)
                    .clamp(1, 8192);
                saved.height = ((self.project.height.max(1) as f32 * saved.scale).round() as u32)
                    .clamp(1, 8192);
            }
            saved
        } else {
            let mut options = ImageExportOptions::for_project(&self.project, self.frame);
            options.symbol_id = Some(symbol_id);
            options
        }
    }

    /// Open the `Export image` modal dialog (`DL-04`).
    pub(crate) fn open_export_image(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let options = self.default_export_image_options();
        let draft = ExportImageDraft { options };
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_export_image(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.remove::<ExportImageDraft>(draft_id());
            data.remove::<ExportImageDraft>(inline_draft_id());
        });
    }

    /// Export image(s) to `path` using the active `Export image` draft and close the dialog on success.
    pub(crate) fn confirm_export_image_to_path(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Result<ImageExportReport, String> {
        let draft = ctx
            .data(|d| d.get_temp::<ExportImageDraft>(draft_id()))
            .or_else(|| ctx.data(|d| d.get_temp::<ExportImageDraft>(inline_draft_id())))
            .unwrap_or_else(|| ExportImageDraft {
                options: self.default_export_image_options(),
            });
        let report =
            export_project_image(&self.project, path, &draft.options).map_err(|e| e.to_string())?;
        self.workspace.export_image_options = Some(draft.options.clone());
        ctx.data_mut(|data| {
            data.remove::<ExportImageDraft>(draft_id());
            data.remove::<ExportImageDraft>(inline_draft_id());
        });
        self.message(format!(
            "Exported {} to {} ({}×{}, {} file(s)).",
            report.format.label(),
            path.display(),
            report.width,
            report.height,
            report.files.len()
        ));
        Ok(report)
    }

    /// Render the interactive `Export image` body inside either the modal dialog or the floating panel.
    /// Returns `true` when the dialog/window should close.
    pub(crate) fn export_image_body(
        &mut self,
        ui: &mut Ui,
        draft: &mut ExportImageDraft,
        mut external_format: Option<&mut usize>,
    ) -> (bool, bool) {
        let mut close_cancel = false;
        let mut do_export = false;
        if let Some(ext_fmt) = external_format.as_deref()
            && draft.options.format.index() != *ext_fmt
        {
            draft.options.format = ImageExportFormat::from_index(*ext_fmt);
        }

        let symbol_id = self.symbol_id().to_owned();
        match &mut draft.options.range {
            ImageExportRange::WholeMovie => draft.options.symbol_id = None,
            ImageExportRange::CurrentScene { scene_id } => {
                *scene_id = symbol_id.clone();
                draft.options.symbol_id = Some(symbol_id.clone());
            }
            ImageExportRange::CurrentFrame(_) | ImageExportRange::Frames { .. } => {
                draft.options.symbol_id = Some(symbol_id.clone());
            }
        }
        let total_frames = self.symbol().timeline.frame_count.max(1);
        let (res_w, res_h) = draft.options.resolved_dimensions(&self.project);
        let (start_f, end_f) = draft
            .options
            .range
            .resolve_frames(&self.project, draft.options.symbol_id.as_deref());
        let span_frames = end_f.saturating_sub(start_f).saturating_add(1).max(1);

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                preview_box(ui, 300.0, 188.0, "Stage preview");
                caption(
                    ui,
                    &format!(
                        "Frame {} of {} · {} × {}",
                        (self.frame + 1).min(total_frames),
                        total_frames,
                        res_w,
                        res_h
                    ),
                );
            });
            ui.add_space(space::S5);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::S2;
                let mut fmt_idx = draft.options.format.index();
                if c::Segmented::show(ui, &mut fmt_idx, &IMAGE_FORMATS, false).is_some() {
                    draft.options.format = ImageExportFormat::from_index(fmt_idx);
                    if draft.options.format == ImageExportFormat::Svg {
                        draft.options.transparent_background = true;
                    }
                    if let Some(ext_fmt) = &mut external_format {
                        **ext_fmt = fmt_idx;
                    }
                }

                group(ui, "Range");
                ui.horizontal(|ui| {
                    let range_lbl = draft.options.range.label();
                    c::Select::new(ui.id().with("export-image-range"), range_lbl)
                        .width(200.0)
                        .show(ui, |ui| {
                            if ui
                                .add(c::MenuItem::new("Current frame").checked(matches!(
                                    draft.options.range,
                                    ImageExportRange::CurrentFrame(_)
                                )))
                                .clicked()
                            {
                                draft.options.range = ImageExportRange::CurrentFrame(self.frame);
                                draft.options.symbol_id = Some(symbol_id.clone());
                                ui.close();
                            }
                            if ui
                                .add(c::MenuItem::new("Whole movie").checked(matches!(
                                    draft.options.range,
                                    ImageExportRange::WholeMovie
                                )))
                                .clicked()
                            {
                                draft.options.range = ImageExportRange::WholeMovie;
                                draft.options.symbol_id = None;
                                ui.close();
                            }
                            if ui
                                .add(c::MenuItem::new("Current scene").checked(matches!(
                                    draft.options.range,
                                    ImageExportRange::CurrentScene { .. }
                                )))
                                .clicked()
                            {
                                draft.options.range = ImageExportRange::CurrentScene {
                                    scene_id: symbol_id.clone(),
                                };
                                draft.options.symbol_id = Some(symbol_id.clone());
                                ui.close();
                            }
                            if ui
                                .add(c::MenuItem::new("Frame range").checked(matches!(
                                    draft.options.range,
                                    ImageExportRange::Frames { .. }
                                )))
                                .clicked()
                            {
                                draft.options.range = ImageExportRange::Frames {
                                    start: 0,
                                    end: total_frames.saturating_sub(1),
                                };
                                draft.options.symbol_id = Some(symbol_id.clone());
                                ui.close();
                            }
                        });
                    let mut frames_val = f64::from(span_frames);
                    if ui
                        .add(
                            c::Field::new(&mut frames_val, "Frames")
                                .prefix("Frames")
                                .range(1.0..=f64::from(total_frames.max(1)))
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        let count = if frames_val.is_finite() {
                            (frames_val
                                .round()
                                .clamp(1.0, f64::from(total_frames.max(1)))
                                as u32)
                                .clamp(1, total_frames.max(1))
                        } else {
                            span_frames
                        };
                        draft.options.range = ImageExportRange::Frames {
                            start: start_f,
                            end: (start_f + count - 1).min(total_frames.saturating_sub(1)),
                        };
                        draft.options.symbol_id = Some(symbol_id.clone());
                    }
                });

                group(ui, "Size");
                ui.horizontal(|ui| {
                    let aspect =
                        (self.project.width.max(1) as f64) / (self.project.height.max(1) as f64);
                    let mut w_val = f64::from(draft.options.width.max(1));
                    let mut h_val = f64::from(draft.options.height.max(1));
                    if ui
                        .add(
                            c::Field::new(&mut w_val, "Width")
                                .prefix("W")
                                .range(1.0..=8192.0)
                                .decimals(0)
                                .width(120.0),
                        )
                        .changed()
                        && w_val.is_finite()
                    {
                        draft.options.width =
                            (w_val.round().clamp(1.0, 8192.0) as u32).clamp(1, 8192);
                        if draft.options.constrain_proportions {
                            draft.options.height = ((draft.options.width as f64 / aspect).round()
                                as u32)
                                .clamp(1, 8192);
                        }
                    }
                    if ui
                        .add(
                            c::Field::new(&mut h_val, "Height")
                                .prefix("H")
                                .range(1.0..=8192.0)
                                .decimals(0)
                                .width(120.0),
                        )
                        .changed()
                        && h_val.is_finite()
                    {
                        draft.options.height =
                            (h_val.round().clamp(1.0, 8192.0) as u32).clamp(1, 8192);
                        if draft.options.constrain_proportions {
                            draft.options.width = ((draft.options.height as f64 * aspect).round()
                                as u32)
                                .clamp(1, 8192);
                        }
                    }
                    if ui
                        .add(
                            c::IconButton::new(i16::LINK, "Constrain proportions")
                                .selected(draft.options.constrain_proportions),
                        )
                        .clicked()
                    {
                        draft.options.constrain_proportions = !draft.options.constrain_proportions;
                    }
                    let scale_lbl = SCALES
                        .iter()
                        .find(|(s, _)| (*s - draft.options.scale).abs() < 0.05)
                        .map(|(_, l)| *l)
                        .unwrap_or("1×");
                    c::Select::new(ui.id().with("export-image-scale"), scale_lbl)
                        .width(80.0)
                        .show(ui, |ui| {
                            for (scale_val, label) in SCALES {
                                let checked = (draft.options.scale - scale_val).abs() < 0.05;
                                if ui.add(c::MenuItem::new(label).checked(checked)).clicked() {
                                    draft.options.scale = scale_val;
                                    ui.close();
                                }
                            }
                        });
                });

                group(ui, "Background");
                ui.horizontal(|ui| {
                    if ui
                        .add(c::Radio::new(
                            !draft.options.transparent_background,
                            "Stage colour",
                        ))
                        .clicked()
                    {
                        draft.options.transparent_background = false;
                    }
                    if ui
                        .add(c::Radio::new(
                            draft.options.transparent_background,
                            "Transparent",
                        ))
                        .clicked()
                    {
                        draft.options.transparent_background = true;
                    }
                });

                match draft.options.format {
                    ImageExportFormat::Svg => {
                        group(ui, "SVG options");
                        ui.horizontal(|ui| {
                            let img_lbl = if draft.options.svg_embed_images {
                                "Images: Embed"
                            } else {
                                "Images: Link"
                            };
                            c::Select::new(ui.id().with("svg-images"), img_lbl)
                                .width(180.0)
                                .show(ui, |ui| {
                                    for (embed, label) in
                                        [(true, "Images: Embed"), (false, "Images: Link")]
                                    {
                                        if ui
                                            .add(
                                                c::MenuItem::new(label).checked(
                                                    draft.options.svg_embed_images == embed,
                                                ),
                                            )
                                            .clicked()
                                        {
                                            draft.options.svg_embed_images = embed;
                                            ui.close();
                                        }
                                    }
                                });
                            let txt_lbl = if draft.options.svg_text_as_outlines {
                                "Text: Convert to outlines"
                            } else {
                                "Text: Editable <text>"
                            };
                            c::Select::new(ui.id().with("svg-text"), txt_lbl)
                                .width(200.0)
                                .show(ui, |ui| {
                                    for (outlines, label) in [
                                        (true, "Text: Convert to outlines"),
                                        (false, "Text: Editable <text>"),
                                    ] {
                                        if ui
                                            .add(c::MenuItem::new(label).checked(
                                                draft.options.svg_text_as_outlines == outlines,
                                            ))
                                            .clicked()
                                        {
                                            draft.options.svg_text_as_outlines = outlines;
                                            ui.close();
                                        }
                                    }
                                });
                        });
                        ui.add(c::Switch::new(
                            &mut draft.options.svg_minify,
                            "Minify output",
                        ));
                        ui.add(c::Switch::new(
                            &mut draft.options.svg_css_classes,
                            "Use CSS classes for styles",
                        ));
                        ui.add(c::Checkbox::new(
                            &mut draft.options.svg_include_hidden_guides,
                            "Include hidden & guide layers",
                        ));
                        ui.horizontal(|ui| {
                            let mut prec = draft.options.svg_precision as f64;
                            if ui
                                .add(
                                    c::Field::new(&mut prec, "Precision")
                                        .prefix("Precision")
                                        .range(1.0..=6.0)
                                        .decimals(0)
                                        .width(120.0),
                                )
                                .changed()
                            {
                                draft.options.svg_precision = (prec.round() as usize).clamp(1, 6);
                            }
                            ui.add(
                                c::TextField::new(&mut draft.options.svg_id_prefix, "ID prefix")
                                    .prefix("ID prefix")
                                    .width(180.0),
                            );
                        });
                    }
                    ImageExportFormat::Psd => {
                        group(ui, "PSD options");
                        if ui
                            .add(c::Radio::new(
                                draft.options.psd_keep_layers,
                                "Keep timeline layers as Photoshop layers",
                            ))
                            .clicked()
                        {
                            draft.options.psd_keep_layers = true;
                        }
                        if ui
                            .add(c::Radio::new(
                                !draft.options.psd_keep_layers,
                                "Flatten to one layer",
                            ))
                            .clicked()
                        {
                            draft.options.psd_keep_layers = false;
                        }
                        ui.add(c::Checkbox::new(
                            &mut draft.options.psd_folders_to_groups,
                            "Folders → layer groups",
                        ));
                        ui.add(c::Checkbox::new(
                            &mut draft.options.psd_symbols_to_smart_objects,
                            "Symbols → smart objects",
                        ));
                        ui.add(c::Checkbox::new(
                            &mut draft.options.psd_keep_text_editable,
                            "Keep text editable",
                        ));
                        ui.horizontal(|ui| {
                            let col_lbl = format!("Colour: {}", draft.options.psd_color_space);
                            c::Select::new(ui.id().with("psd-color"), &col_lbl)
                                .width(200.0)
                                .show(ui, |ui| {
                                    for cs in ["sRGB 8-bit", "Display P3 8-bit"] {
                                        if ui
                                            .add(
                                                c::MenuItem::new(&format!("Colour: {cs}"))
                                                    .checked(draft.options.psd_color_space == cs),
                                            )
                                            .clicked()
                                        {
                                            draft.options.psd_color_space = cs.to_owned();
                                            ui.close();
                                        }
                                    }
                                });
                            let res_lbl =
                                format!("Resolution: {} ppi", draft.options.psd_resolution_ppi);
                            c::Select::new(ui.id().with("psd-res"), &res_lbl)
                                .width(180.0)
                                .show(ui, |ui| {
                                    for ppi in [72u32, 144, 300] {
                                        if ui
                                            .add(
                                                c::MenuItem::new(&format!("Resolution: {ppi} ppi"))
                                                    .checked(
                                                        draft.options.psd_resolution_ppi == ppi,
                                                    ),
                                            )
                                            .clicked()
                                        {
                                            draft.options.psd_resolution_ppi = ppi;
                                            ui.close();
                                        }
                                    }
                                });
                        });
                    }
                    ImageExportFormat::Ai => {
                        group(ui, "AI options");
                        let ver_lbl = format!("Version: {}", draft.options.ai_version);
                        c::Select::new(ui.id().with("ai-version"), &ver_lbl)
                            .width(240.0)
                            .show(ui, |ui| {
                                for ver in ["Illustrator CS6+", "Illustrator CC (2020+)"] {
                                    if ui
                                        .add(
                                            c::MenuItem::new(&format!("Version: {ver}"))
                                                .checked(draft.options.ai_version == ver),
                                        )
                                        .clicked()
                                    {
                                        draft.options.ai_version = ver.to_owned();
                                        ui.close();
                                    }
                                }
                            });
                        if ui
                            .add(c::Radio::new(
                                draft.options.ai_each_layer,
                                "Each layer → AI layer",
                            ))
                            .clicked()
                        {
                            draft.options.ai_each_layer = true;
                        }
                        if ui
                            .add(c::Radio::new(
                                !draft.options.ai_each_layer,
                                "Each keyframe → artboard",
                            ))
                            .clicked()
                        {
                            draft.options.ai_each_layer = false;
                        }
                        ui.add(c::Checkbox::new(
                            &mut draft.options.ai_strokes_as_strokes,
                            "Strokes as strokes (not outlines)",
                        ));
                        ui.add(c::Checkbox::new(
                            &mut draft.options.ai_text_as_outlines,
                            "Convert text to outlines",
                        ));
                        ui.add(c::Checkbox::new(
                            &mut draft.options.runtime_capture,
                            "Render nested movie clip animation and scripts",
                        ));
                    }
                    ImageExportFormat::Png
                    | ImageExportFormat::Jpeg
                    | ImageExportFormat::Webp
                    | ImageExportFormat::Gif
                    | ImageExportFormat::Bmp => {
                        group(ui, "Raster options");
                        ui.horizontal(|ui| {
                            let fmt_lbl = format!("Format: {}", draft.options.format.label());
                            c::Select::new(ui.id().with("raster-subformat"), &fmt_lbl)
                                .width(180.0)
                                .show(ui, |ui| {
                                    for sub in [
                                        ImageExportFormat::Png,
                                        ImageExportFormat::Jpeg,
                                        ImageExportFormat::Webp,
                                        ImageExportFormat::Gif,
                                        ImageExportFormat::Bmp,
                                    ] {
                                        if ui
                                            .add(
                                                c::MenuItem::new(&format!(
                                                    "Format: {}",
                                                    sub.label()
                                                ))
                                                .checked(draft.options.format == sub),
                                            )
                                            .clicked()
                                        {
                                            draft.options.format = sub;
                                            ui.close();
                                        }
                                    }
                                });
                            if draft.options.format == ImageExportFormat::Jpeg {
                                let mut q = f64::from(draft.options.jpeg_quality.clamp(1, 100));
                                if ui
                                    .add(
                                        c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality")
                                            .step(1.0)
                                            .width(150.0),
                                    )
                                    .changed()
                                {
                                    draft.options.jpeg_quality = q.round().clamp(1.0, 100.0) as u8;
                                }
                                caption(ui, &format!("{} %", draft.options.jpeg_quality));
                            }
                        });
                        ui.add(c::Checkbox::new(
                            &mut draft.options.runtime_capture,
                            "Render nested movie clip animation and scripts",
                        ));
                    }
                    ImageExportFormat::Pdf => {
                        ui.add(c::Checkbox::new(
                            &mut draft.options.runtime_capture,
                            "Render nested movie clip animation and scripts",
                        ));
                    }
                }
            });
        });

        let summary = estimate_image_export_summary(&self.project, &draft.options);
        let button = draft.options.format.button_label();
        match footer_clicked(ui, &summary, None, &["Cancel", button]) {
            Some(FooterClick::Button(0)) => close_cancel = true,
            Some(FooterClick::Button(1)) => do_export = true,
            _ => {}
        }
        (close_cancel, do_export)
    }

    pub(crate) fn export_image_inline_panel(
        &mut self,
        ui: &mut Ui,
        export_format: &mut usize,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let mut draft = ctx
            .data(|d| d.get_temp::<ExportImageDraft>(inline_draft_id()))
            .unwrap_or_else(|| ExportImageDraft {
                options: self.default_export_image_options(),
            });
        let (close_cancel, do_export) = self.export_image_body(ui, &mut draft, Some(export_format));
        ctx.data_mut(|d| d.insert_temp(inline_draft_id(), draft.clone()));
        if close_cancel {
            self.cancel_export_image(&ctx);
            return true;
        }
        if do_export {
            let ext = draft.options.format.extension();
            let default_name = format!(
                "{}.{ext}",
                self.project.name.trim().replace(' ', "_").to_lowercase()
            );
            if let Some(path) = rfd::FileDialog::new()
                .set_title(draft.options.format.button_label())
                .set_file_name(&default_name)
                .add_filter(draft.options.format.label(), &[ext])
                .save_file()
            {
                return match self.confirm_export_image_to_path(&ctx, &path) {
                    Ok(_) => true,
                    Err(err) => {
                        self.message(format!("Export image failed: {err}"));
                        false
                    }
                };
            }
        }
        false
    }

    pub(crate) fn export_image_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<ExportImageDraft>(draft_id())) else {
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_export = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);

        let modal = egui::Modal::new(egui::Id::new("export-image"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(760.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Export image",
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
                        let (c, exp) = self.export_image_body(ui, &mut draft, None);
                        close_cancel |= c;
                        do_export |= exp;
                    });
            });

        if !close_cancel && !do_export {
            match kb.resolve(ctx, &modal, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => do_export = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }

        ctx.data_mut(|d| d.insert_temp(draft_id(), draft.clone()));

        if close_cancel {
            self.cancel_export_image(ctx);
            return;
        }
        if do_export {
            let ext = draft.options.format.extension();
            let default_name = format!(
                "{}.{ext}",
                self.project.name.trim().replace(' ', "_").to_lowercase()
            );
            if let Some(path) = rfd::FileDialog::new()
                .set_title(draft.options.format.button_label())
                .set_file_name(&default_name)
                .add_filter(draft.options.format.label(), &[ext])
                .save_file()
                && let Err(err) = self.confirm_export_image_to_path(ctx, &path)
            {
                self.message(format!("Export image failed: {err}"));
            }
        }
    }
}
