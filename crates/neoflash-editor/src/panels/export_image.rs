//! Export image modal dialog and live floating body (NF2 `Dialog / Export image`, `DL-04`,
//! `W3.1` / `W3.2`; reference `dialogs-export.png`), opened from `File ▸ Export ▸ Export image…`
//! and `Window ▸ Panels ▸ Planned panels ▸ Export image`.

use super::dialog_helpers::{
    DialogShell, fixed_dialog_size, footer_note, form_line, form_row, group_caption, note,
};
use super::planned::caption;
use crate::app::Editor;
use eframe::egui::{self, Ui};
use neoflash_core::Color;
use neoflash_export::{
    ImageExportFormat, ImageExportOptions, ImageExportRange, ImageExportReport,
    estimate_image_export_summary, export_project_image, render_project_frame_rgba_with_base_dir,
};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius};
use std::path::{Path, PathBuf};

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

fn output_path_id() -> egui::Id {
    egui::Id::new("nf2-export-image-output-path")
}

pub(crate) fn default_image_output_path(
    project: &neoflash_core::Project,
    project_path: Option<&Path>,
    format: ImageExportFormat,
) -> PathBuf {
    let raw_stem = project_path
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(project.name.as_str());
    let sanitized: String = raw_stem
        .trim()
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
        .map(|c| {
            if c == ' ' {
                '_'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    let stem = if sanitized.trim_matches('_').is_empty() {
        "untitled".to_owned()
    } else {
        crate::output_paths::safe_output_file_stem(sanitized.trim_matches('_'), "untitled")
    };
    let file_name = format!("{stem}.{}", format.extension());
    match project_path
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
    {
        Some(dir) => dir.join(file_name),
        None => PathBuf::from(file_name),
    }
}

pub(crate) fn sync_image_output_path_extension(path: &mut PathBuf, format: ImageExportFormat) {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    let raw_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled.png");
    let mut stem = raw_name;
    for ext in [
        ".png", ".jpg", ".jpeg", ".webp", ".svg", ".gif", ".bmp", ".psd", ".ai", ".pdf",
    ] {
        if raw_name.len() > ext.len()
            && raw_name[raw_name.len() - ext.len()..].eq_ignore_ascii_case(ext)
        {
            stem = &raw_name[..raw_name.len() - ext.len()];
            break;
        }
    }
    let stem = if stem.trim().is_empty() {
        "untitled".to_owned()
    } else {
        crate::output_paths::safe_output_file_stem(stem, "untitled")
    };
    let next_name = format!("{stem}.{}", format.extension());
    *path = match parent {
        Some(dir) => dir.join(next_name),
        None => PathBuf::from(next_name),
    };
}

pub(crate) fn export_image_output_path(ctx: &egui::Context) -> Option<PathBuf> {
    ctx.data(|d| d.get_temp::<PathBuf>(output_path_id()))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn set_export_image_output_path(ctx: &egui::Context, path: PathBuf) {
    ctx.data_mut(|d| d.insert_temp(output_path_id(), path));
}

fn sync_stored_image_output_path(
    ctx: &egui::Context,
    project: &neoflash_core::Project,
    project_path: Option<&Path>,
    format: ImageExportFormat,
) {
    let mut path = export_image_output_path(ctx)
        .unwrap_or_else(|| default_image_output_path(project, project_path, format));
    sync_image_output_path_extension(&mut path, format);
    ctx.data_mut(|d| d.insert_temp(output_path_id(), path));
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
    let mut updated_format = None;
    for id in [draft_id(), inline_draft_id()] {
        if let Some(mut draft) = ctx.data(|d| d.get_temp::<ExportImageDraft>(id)) {
            let prev_fmt = draft.options.format;
            f(&mut draft);
            if draft.options.format != prev_fmt {
                updated_format = Some(draft.options.format);
            }
            ctx.data_mut(|d| d.insert_temp(id, draft));
        }
    }
    if let Some(fmt) = updated_format
        && let Some(mut path) = export_image_output_path(ctx)
    {
        sync_image_output_path_extension(&mut path, fmt);
        ctx.data_mut(|d| d.insert_temp(output_path_id(), path));
    }
}

#[cfg(test)]
pub(crate) fn set_inline_export_image_draft(ctx: &egui::Context, draft: ExportImageDraft) {
    ctx.data_mut(|data| data.insert_temp(inline_draft_id(), draft));
}

/// Select the format of the open `Export image` draft by its extension (captures, layout tests).
pub(crate) fn select_export_image_format_slug(ctx: &egui::Context, slug: &str) {
    let Some(format) = ImageExportFormat::ALL
        .into_iter()
        .find(|format| format.extension().eq_ignore_ascii_case(slug))
    else {
        return;
    };
    if let Some(mut draft) = ctx.data(|d| d.get_temp::<ExportImageDraft>(draft_id())) {
        draft.options.format = format;
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }
}

#[derive(Clone, PartialEq)]
struct ExportImagePreviewKey {
    revision: u64,
    symbol_id: Option<String>,
    frame: u32,
    transparent: bool,
    runtime_capture: bool,
    pw: u32,
    ph: u32,
    project_w: u32,
    project_h: u32,
    background: Color,
    base_dir: Option<PathBuf>,
}

#[derive(Clone)]
struct ExportImagePreviewCache {
    key: ExportImagePreviewKey,
    tex: Option<egui::TextureHandle>,
    unavailable: bool,
}

/// `Export image` has one size per density for every format.
pub(crate) fn export_image_dialog_size(ctx: &egui::Context) -> egui::Vec2 {
    fixed_dialog_size(ctx, metrics::dialog_l(), metrics::dialog_l_height())
}

fn export_image_preview_cache_id() -> egui::Id {
    egui::Id::new("nf2-export-image-preview-cache")
}

fn paint_export_image_preview_box(
    ui: &mut Ui,
    editor: &Editor,
    options: &ImageExportOptions,
    preview_frame: u32,
    res_w: u32,
    res_h: u32,
    size: egui::Vec2,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let r = egui::CornerRadius::same(radius::lg() as u8);
    ui.painter().rect_filled(rect, r, nf.bg_canvas);

    // Render resolution: the box minus the pasteboard margin.
    let max_w = (size.x * 0.9).round().max(1.0);
    let max_h = (size.y * 0.87).round().max(1.0);
    let aspect = res_w.max(1) as f32 / res_h.max(1) as f32;
    let (pw, ph) = if aspect >= max_w / max_h {
        let w = max_w.round() as u32;
        let h = ((max_w / aspect).round() as u32).clamp(1, max_h as u32);
        (w.max(1), h)
    } else {
        let h = max_h.round() as u32;
        let w = ((max_h * aspect).round() as u32).clamp(1, max_w as u32);
        (w, h.max(1))
    };

    let base_dir = editor
        .path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let key = ExportImagePreviewKey {
        revision: editor.history.revision(),
        symbol_id: options.symbol_id.clone(),
        frame: preview_frame,
        transparent: options.transparent_background,
        runtime_capture: options.runtime_capture,
        pw,
        ph,
        project_w: editor.project.width,
        project_h: editor.project.height,
        background: editor.project.background,
        base_dir: base_dir.clone(),
    };

    let mut cache = ui
        .ctx()
        .data(|d| d.get_temp::<ExportImagePreviewCache>(export_image_preview_cache_id()));
    if cache.as_ref().is_none_or(|c| c.key != key) {
        let (tex, unavailable) = match render_project_frame_rgba_with_base_dir(
            &editor.project,
            key.symbol_id.as_deref(),
            key.frame,
            key.pw,
            key.ph,
            key.transparent,
            key.runtime_capture,
            base_dir.as_deref(),
        ) {
            Ok(rgba) if rgba.len() == (key.pw as usize) * (key.ph as usize) * 4 => {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [key.pw as usize, key.ph as usize],
                    &rgba,
                );
                let tex = ui.ctx().load_texture(
                    "export-image-live-preview",
                    image,
                    egui::TextureOptions::LINEAR,
                );
                (Some(tex), false)
            }
            _ => (None, true),
        };
        let next = ExportImagePreviewCache {
            key,
            tex,
            unavailable,
        };
        ui.ctx()
            .data_mut(|d| d.insert_temp(export_image_preview_cache_id(), next.clone()));
        cache = Some(next);
    }

    if let Some(tex) = cache.as_ref().and_then(|c| c.tex.as_ref()) {
        let tex_size = tex.size_vec2();
        let max_size = rect.size() * 0.86;
        let fit_scale = (max_size.x / tex_size.x.max(1.0))
            .min(max_size.y / tex_size.y.max(1.0))
            .min(2.0);
        let stage_rect = egui::Rect::from_center_size(rect.center(), tex_size * fit_scale);
        if options.transparent_background {
            ui.painter().rect_filled(stage_rect, 0.0, nf.bg_field);
            let cell = 8.0_f32;
            let cols = (stage_rect.width() / cell).ceil() as i32;
            let rows = (stage_rect.height() / cell).ceil() as i32;
            for row in 0..rows {
                for col in 0..cols {
                    if (row + col) % 2 == 0 {
                        let min = egui::pos2(
                            stage_rect.left() + col as f32 * cell,
                            stage_rect.top() + row as f32 * cell,
                        );
                        let max = egui::pos2(
                            (min.x + cell).min(stage_rect.right()),
                            (min.y + cell).min(stage_rect.bottom()),
                        );
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(min, max),
                            0.0,
                            nf.bg_raised,
                        );
                    }
                }
            }
        } else {
            ui.painter().rect_filled(stage_rect, 0.0, nf.bg_stage);
        }
        ui.painter().image(
            tex.id(),
            stage_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        let stage = egui::Rect::from_center_size(rect.center(), rect.size() * 0.8);
        ui.painter().rect_filled(stage, 0.0, nf.bg_stage);
        let msg = if cache.as_ref().is_some_and(|c| c.unavailable) {
            "Preview unavailable"
        } else {
            "Rendering…"
        };
        ui.painter().text(
            stage.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            neoflash_ui::typography::font(TextStyle::Small),
            nf.text_tertiary,
        );
    }

    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
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
        let output_path =
            default_image_output_path(&self.project, self.path.as_deref(), options.format);
        let draft = ExportImageDraft { options };
        ctx.data_mut(|d| {
            d.insert_temp(draft_id(), draft);
            d.insert_temp(output_path_id(), output_path);
        });
    }

    pub(crate) fn cancel_export_image(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.remove::<ExportImageDraft>(draft_id());
            data.remove::<ExportImageDraft>(inline_draft_id());
            data.remove::<PathBuf>(output_path_id());
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
        let is_sequence = !matches!(draft.options.range, ImageExportRange::CurrentFrame(_));
        let output_path = if !is_sequence {
            path.to_path_buf()
        } else {
            let chosen_parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let prepared_output_dir =
                crate::output_paths::project_output_dir(chosen_parent, &self.project);
            crate::output_paths::project_sequence_output_path(
                path,
                &prepared_output_dir,
                "export",
                draft.options.format.extension(),
            )?
        };
        let report = export_project_image(&self.project, &output_path, &draft.options)
            .map_err(|e| e.to_string())?;
        self.workspace.export_image_options = Some(draft.options.clone());
        ctx.data_mut(|data| {
            data.remove::<ExportImageDraft>(draft_id());
            data.remove::<ExportImageDraft>(inline_draft_id());
            data.remove::<PathBuf>(output_path_id());
        });
        self.message(format!(
            "Exported {} to {} ({}×{}, {} file(s)).",
            report.format.label(),
            output_path.display(),
            report.width,
            report.height,
            report.files.len()
        ));
        Ok(report)
    }

    /// Render the `Export image` body into the fixed dialog shell: artwork column, form and footer.
    /// Returns `(cancel, export)`.
    fn export_image_body(
        &mut self,
        ui: &mut Ui,
        shell: &DialogShell,
        aside_width: f32,
        draft: &mut ExportImageDraft,
    ) -> (bool, bool) {
        let mut close_cancel = false;
        let mut do_export = false;

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

        let preview_frame = match draft.options.range {
            ImageExportRange::CurrentFrame(f) => f.min(total_frames.saturating_sub(1)),
            ImageExportRange::WholeMovie | ImageExportRange::CurrentScene { .. } => {
                self.frame.min(total_frames.saturating_sub(1))
            }
            ImageExportRange::Frames { start, .. } => start.min(end_f),
        };

        shell.aside(ui, aside_width, |ui| {
            paint_export_image_preview_box(
                ui,
                self,
                &draft.options,
                preview_frame,
                res_w,
                res_h,
                egui::vec2(aside_width, (aside_width * 0.625).round()),
            );
            note(
                ui,
                &format!(
                    "Frame {} of {} · {} × {}",
                    (preview_frame + 1).min(total_frames),
                    total_frames,
                    res_w,
                    res_h
                ),
            );
        });
        // The common rows (format, range, size, background) never move; only the
        // "… options" group changes with the format.
        shell.form(ui, "export-image-form", aside_width, |ui, _| {
            form_line(ui, |ui| {
                let mut fmt_idx = draft.options.format.index();
                if c::Segmented::show(ui, &mut fmt_idx, &IMAGE_FORMATS, false).is_some() {
                    draft.options.format = ImageExportFormat::from_index(fmt_idx);
                    if draft.options.format == ImageExportFormat::Svg {
                        draft.options.transparent_background = true;
                    }
                    sync_stored_image_output_path(
                        ui.ctx(),
                        &self.project,
                        self.path.as_deref(),
                        draft.options.format,
                    );
                }
            });
            form_row(ui, "Range", |ui| {
                let range_lbl = draft.options.range.label();
                c::Select::new(ui.id().with("export-image-range"), range_lbl)
                    .width(metrics::field_m())
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
                            .width(metrics::field_s()),
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
            form_row(ui, "Size", |ui| {
                let aspect =
                    (self.project.width.max(1) as f64) / (self.project.height.max(1) as f64);
                let mut w_val = f64::from(draft.options.width.max(1));
                let mut h_val = f64::from(draft.options.height.max(1));
                if ui
                    .add(
                        c::Field::new(&mut w_val, "Width")
                            .prefix("W")
                            .unit("px")
                            .range(1.0..=8192.0)
                            .decimals(0)
                            .width(metrics::field_s()),
                    )
                    .changed()
                    && w_val.is_finite()
                {
                    draft.options.width = (w_val.round().clamp(1.0, 8192.0) as u32).clamp(1, 8192);
                    if draft.options.constrain_proportions {
                        draft.options.height =
                            ((draft.options.width as f64 / aspect).round() as u32).clamp(1, 8192);
                    }
                }
                if ui
                    .add(
                        c::Field::new(&mut h_val, "Height")
                            .prefix("H")
                            .unit("px")
                            .range(1.0..=8192.0)
                            .decimals(0)
                            .width(metrics::field_s()),
                    )
                    .changed()
                    && h_val.is_finite()
                {
                    draft.options.height = (h_val.round().clamp(1.0, 8192.0) as u32).clamp(1, 8192);
                    if draft.options.constrain_proportions {
                        draft.options.width =
                            ((draft.options.height as f64 * aspect).round() as u32).clamp(1, 8192);
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
                    .width(metrics::field_num())
                    .show(ui, |ui| {
                        for (scale_val, label) in SCALES {
                            let checked = (draft.options.scale - scale_val).abs() < 0.05;
                            if ui.add(c::MenuItem::new(label).checked(checked)).clicked() {
                                draft.options.scale = scale_val;
                                draft.options.width =
                                    ((self.project.width.max(1) as f32 * scale_val).round() as u32)
                                        .clamp(1, 8192);
                                draft.options.height =
                                    ((self.project.height.max(1) as f32 * scale_val).round()
                                        as u32)
                                        .clamp(1, 8192);
                                ui.close();
                            }
                        }
                    });
            });
            form_row(ui, "Background", |ui| {
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

            let runtime_capture = |ui: &mut Ui, draft: &mut ExportImageDraft| {
                form_row(ui, "Content", |ui| {
                    ui.add(c::Checkbox::new(
                        &mut draft.options.runtime_capture,
                        "Render nested movie clip animation and scripts",
                    ));
                });
            };
            match draft.options.format {
                ImageExportFormat::Svg => {
                    group_caption(ui, "SVG options");
                    form_row(ui, "Images", |ui| {
                        let svg_img = ui
                            .scope(|ui| {
                                ui.disable();
                                c::Select::new(ui.id().with("svg-images"), "Embed")
                                    .width(metrics::field_m())
                                    .show(ui, |_| {})
                                    .0
                            })
                            .inner;
                        let _ = c::not_available(svg_img, "SVG linked images");
                    });
                    form_row(ui, "Text", |ui| {
                        let txt_lbl = if draft.options.svg_text_as_outlines {
                            "Convert to outlines"
                        } else {
                            "Editable <text>"
                        };
                        c::Select::new(ui.id().with("svg-text"), txt_lbl)
                            .width(metrics::field_l())
                            .show(ui, |ui| {
                                for (outlines, label) in
                                    [(true, "Convert to outlines"), (false, "Editable <text>")]
                                {
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
                    form_row(ui, "Output", |ui| {
                        ui.add(c::Checkbox::new(
                            &mut draft.options.svg_minify,
                            "Minify output",
                        ));
                    });
                    form_row(ui, "", |ui| {
                        let css_resp = ui.add_enabled(
                            false,
                            c::Checkbox::new(
                                &mut draft.options.svg_css_classes,
                                "Use CSS classes for styles",
                            ),
                        );
                        let _ = c::not_available(css_resp, "SVG CSS class extraction");
                    });
                    form_row(ui, "", |ui| {
                        ui.add(c::Checkbox::new(
                            &mut draft.options.svg_include_hidden_guides,
                            "Include hidden & guide layers",
                        ));
                    });
                    form_row(ui, "Precision", |ui| {
                        let mut prec = draft.options.svg_precision as f64;
                        if ui
                            .add(
                                c::Field::new(&mut prec, "Precision")
                                    .range(1.0..=6.0)
                                    .decimals(0)
                                    .width(metrics::field_num()),
                            )
                            .changed()
                            && prec.is_finite()
                        {
                            draft.options.svg_precision = prec.round().clamp(1.0, 6.0) as usize;
                        }
                    });
                    form_row(ui, "ID prefix", |ui| {
                        ui.add(
                            c::TextField::new(&mut draft.options.svg_id_prefix, "ID prefix")
                                .width(metrics::field_s()),
                        );
                    });
                }
                ImageExportFormat::Psd => {
                    group_caption(ui, "PSD options");
                    form_row(ui, "Layers", |ui| {
                        if ui
                            .add(c::Radio::new(
                                draft.options.psd_keep_layers,
                                "Keep timeline layers as Photoshop layers",
                            ))
                            .clicked()
                        {
                            draft.options.psd_keep_layers = true;
                        }
                    });
                    form_row(ui, "", |ui| {
                        if ui
                            .add(c::Radio::new(
                                !draft.options.psd_keep_layers,
                                "Flatten to one layer",
                            ))
                            .clicked()
                        {
                            draft.options.psd_keep_layers = false;
                        }
                    });
                    form_row(ui, "Convert", |ui| {
                        let r1 = ui.add_enabled(
                            false,
                            c::Checkbox::new(
                                &mut draft.options.psd_folders_to_groups,
                                "Folders → layer groups",
                            ),
                        );
                        let _ = c::not_available(r1, "PSD folder groups");
                    });
                    form_row(ui, "", |ui| {
                        let r2 = ui.add_enabled(
                            false,
                            c::Checkbox::new(
                                &mut draft.options.psd_symbols_to_smart_objects,
                                "Symbols → smart objects",
                            ),
                        );
                        let _ = c::not_available(r2, "PSD smart objects");
                    });
                    form_row(ui, "", |ui| {
                        let r3 = ui.add_enabled(
                            false,
                            c::Checkbox::new(
                                &mut draft.options.psd_keep_text_editable,
                                "Keep text editable",
                            ),
                        );
                        let _ = c::not_available(r3, "Editable PSD text layers");
                    });
                    form_row(ui, "Colour", |ui| {
                        let col_resp = ui
                            .scope(|ui| {
                                ui.disable();
                                c::Select::new(
                                    ui.id().with("psd-color"),
                                    &draft.options.psd_color_space,
                                )
                                .width(metrics::field_m())
                                .show(ui, |_| {})
                                .0
                            })
                            .inner;
                        let _ = c::not_available(col_resp, "PSD colour profiles");
                    });
                    form_row(ui, "Resolution", |ui| {
                        let res_lbl = format!("{} ppi", draft.options.psd_resolution_ppi);
                        c::Select::new(ui.id().with("psd-res"), &res_lbl)
                            .width(metrics::field_s())
                            .show(ui, |ui| {
                                for ppi in [72u32, 144, 300] {
                                    if ui
                                        .add(
                                            c::MenuItem::new(&format!("{ppi} ppi"))
                                                .checked(draft.options.psd_resolution_ppi == ppi),
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
                    group_caption(ui, "AI options");
                    form_row(ui, "Version", |ui| {
                        let ver_lbl = draft.options.ai_version.clone();
                        c::Select::new(ui.id().with("ai-version"), &ver_lbl)
                            .width(metrics::field_l())
                            .show(ui, |ui| {
                                for ver in ["Illustrator CS6+", "Illustrator CC (2020+)"] {
                                    if ui
                                        .add(
                                            c::MenuItem::new(ver)
                                                .checked(draft.options.ai_version == ver),
                                        )
                                        .clicked()
                                    {
                                        draft.options.ai_version = ver.to_owned();
                                        ui.close();
                                    }
                                }
                            });
                    });
                    form_row(ui, "Layers", |ui| {
                        if ui
                            .add(c::Radio::new(
                                draft.options.ai_each_layer,
                                "Each layer → AI layer",
                            ))
                            .clicked()
                        {
                            draft.options.ai_each_layer = true;
                        }
                    });
                    form_row(ui, "", |ui| {
                        if ui
                            .add(c::Radio::new(
                                !draft.options.ai_each_layer,
                                "Each keyframe → artboard",
                            ))
                            .clicked()
                        {
                            draft.options.ai_each_layer = false;
                        }
                    });
                    form_row(ui, "Strokes", |ui| {
                        ui.add(c::Checkbox::new(
                            &mut draft.options.ai_strokes_as_strokes,
                            "Strokes as strokes (not outlines)",
                        ));
                    });
                    form_row(ui, "Text", |ui| {
                        ui.add(c::Checkbox::new(
                            &mut draft.options.ai_text_as_outlines,
                            "Convert text to outlines",
                        ));
                    });
                    runtime_capture(ui, draft);
                }
                ImageExportFormat::Png
                | ImageExportFormat::Jpeg
                | ImageExportFormat::Webp
                | ImageExportFormat::Gif
                | ImageExportFormat::Bmp => {
                    group_caption(ui, "Raster options");
                    form_row(ui, "Format", |ui| {
                        c::Select::new(
                            ui.id().with("raster-subformat"),
                            draft.options.format.label(),
                        )
                        .width(metrics::field_m())
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
                                        c::MenuItem::new(sub.label())
                                            .checked(draft.options.format == sub),
                                    )
                                    .clicked()
                                {
                                    draft.options.format = sub;
                                    sync_stored_image_output_path(
                                        ui.ctx(),
                                        &self.project,
                                        self.path.as_deref(),
                                        draft.options.format,
                                    );
                                    ui.close();
                                }
                            }
                        });
                    });
                    // The JPEG quality row keeps its place for every raster format.
                    form_row(ui, "Quality", |ui| {
                        ui.add_enabled_ui(draft.options.format == ImageExportFormat::Jpeg, |ui| {
                            let mut q = f64::from(draft.options.jpeg_quality.clamp(1, 100));
                            if ui
                                .add(
                                    c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality")
                                        .step(1.0)
                                        .width(metrics::field_m()),
                                )
                                .changed()
                                && q.is_finite()
                            {
                                draft.options.jpeg_quality = q.round().clamp(1.0, 100.0) as u8;
                            }
                            caption(ui, &format!("{} %", draft.options.jpeg_quality));
                        });
                    });
                    runtime_capture(ui, draft);
                }
                ImageExportFormat::Pdf => {
                    group_caption(ui, "PDF options");
                    runtime_capture(ui, draft);
                }
            }
        });

        let summary = estimate_image_export_summary(&self.project, &draft.options);
        let button = draft.options.format.button_label();
        shell.footer(
            ui,
            |ui| {
                footer_note(ui, &summary);
            },
            |ui| {
                do_export |= ui
                    .add(c::Button::primary(button).icon(i16::UPLOAD))
                    .clicked();
                close_cancel |= ui.add(c::Button::new("Cancel")).clicked();
            },
        );
        (close_cancel, do_export)
    }

    pub(crate) fn export_image_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<ExportImageDraft>(draft_id())) else {
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_export = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);

        // v3: one size for every format.
        let size = export_image_dialog_size(ctx);
        let aside_width = (208.0 * size.x / metrics::DIALOG_L).round();
        let modal = egui::Modal::new(egui::Id::new("export-image"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let shell = DialogShell::single(ui, "Export image", size);
                close_cancel |= shell.close;
                let (c, exp) = self.export_image_body(ui, &shell, aside_width, &mut draft);
                close_cancel |= c;
                do_export |= exp;
            });

        if !close_cancel && !do_export {
            match kb.resolve(ctx, &modal, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => do_export = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }

        ctx.data_mut(|d| d.insert_temp(draft_id(), draft.clone()));
        sync_stored_image_output_path(
            ctx,
            &self.project,
            self.path.as_deref(),
            draft.options.format,
        );

        if close_cancel {
            self.cancel_export_image(ctx);
            super::dialog_helpers::record_end_of_frame_focus(ctx);
            return;
        }
        if do_export {
            let ext = draft.options.format.extension();
            let resolved_out = export_image_output_path(ctx).unwrap_or_else(|| {
                default_image_output_path(&self.project, self.path.as_deref(), draft.options.format)
            });
            let default_name = resolved_out
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("untitled.{ext}"));
            let default_name =
                crate::output_paths::safe_output_file_name(&default_name, "untitled", ext);
            let initial_dir = resolved_out
                .parent()
                .filter(|path| !path.as_os_str().is_empty());
            let mut prepared_output_dir = None;
            let picked = if matches!(draft.options.range, ImageExportRange::CurrentFrame(_)) {
                let mut dialog = rfd::FileDialog::new()
                    .set_title(draft.options.format.button_label())
                    .set_file_name(&default_name)
                    .add_filter(draft.options.format.label(), &[ext]);
                if let Some(dir) = initial_dir {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file()
            } else {
                let mut parent_dialog =
                    rfd::FileDialog::new().set_title("Choose image sequence parent folder");
                if let Some(dir) = initial_dir {
                    parent_dialog = parent_dialog.set_directory(dir);
                }
                match parent_dialog.pick_folder() {
                    Some(parent) => {
                        match crate::output_paths::prepare_project_output_dir(
                            &parent,
                            &self.project,
                        ) {
                            Ok((output_dir, created)) => {
                                let dialog = rfd::FileDialog::new()
                                    .set_title(draft.options.format.button_label())
                                    .set_directory(&output_dir)
                                    .set_file_name(&default_name)
                                    .add_filter(draft.options.format.label(), &[ext]);
                                prepared_output_dir = Some((output_dir, created));
                                dialog.save_file()
                            }
                            Err(err) => {
                                self.message(format!("Export image failed: {err}"));
                                None
                            }
                        }
                    }
                    None => None,
                }
            };
            if let Some(path) = picked {
                let resolved_path = if let Some((dir, _)) = prepared_output_dir.as_ref() {
                    crate::output_paths::project_sequence_output_path(&path, dir, "export", ext)
                } else {
                    Ok(path)
                };
                if let Err(err) = resolved_path
                    .and_then(|path| self.confirm_export_image_to_path(ctx, &path).map(|_| ()))
                {
                    self.message(format!("Export image failed: {err}"));
                    if let Some((dir, created)) = prepared_output_dir.as_ref() {
                        crate::output_paths::remove_created_output_dir_if_empty(dir, *created);
                    }
                }
            } else if let Some((dir, created)) = prepared_output_dir.as_ref() {
                crate::output_paths::remove_created_output_dir_if_empty(dir, *created);
            }
        }
        super::dialog_helpers::record_end_of_frame_focus(ctx);
    }
}
