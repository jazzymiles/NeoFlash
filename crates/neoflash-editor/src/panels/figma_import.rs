//! Interactive NF v3 `Import Figma` modal dialog (`T-391`), opened when importing or opening a
//! `.fig` / `.figma` / `.figma.json` file via Stage drag-and-drop, `File ▸ Import ▸ Import graphics
//! to Stage…`, `Library ▸ Import…`, `File ▸ Import ▸ Import to Library…`, or `File ▸ Open…`.
//!
//! Keeps one fixed outer rectangle per density (`metrics::dialog_m()`) across every phase
//! (background inspection, 1..N pages, 0 checked pages, error state, background import) and runs
//! multi-megabyte `.fig` inspection and selective import on background worker threads so the UI
//! and macOS menu bar never block.

use super::dialog_helpers::{
    DialogKeyboardOutcome, DialogKeyboardState, FormDialog, field_block, fixed_dialog_size,
    footer_note, form_dialog_height, form_line, group_caption,
};
use crate::app::Editor;
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::{
    Element, ElementKind, InstancePlayback, Keyframe, LayerRole, Project, Symbol, SymbolKind,
    Timeline, Transform, new_id,
};
use neoflash_import::{
    FigmaDocumentSummary, FigmaImportOptions, FigmaLayerMapping, FigmaPageMapping,
    FigmaPageSummary, ImportReport, import_figma_path, inspect_figma_path,
};
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography;
use std::cell::RefCell;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, TryRecvError},
};
use std::time::Duration;

/// Files above 2 MiB inspect asynchronously on a background worker thread.
const ASYNC_INSPECT_THRESHOLD_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FigmaImportTarget {
    /// Place onto active Stage/symbol timeline (optional drop position in edit coordinates).
    Stage { point: Option<[f64; 2]> },
    /// Import into the active document's Library without placing on Stage.
    Library,
    /// Open as a new document session tab (`File ▸ Open…`).
    OpenDocument,
}

#[derive(Clone, Debug)]
struct QueuedFigmaRequest {
    paths: Vec<PathBuf>,
    target: FigmaImportTarget,
}

thread_local! {
    /// Bridges menu/dialog entry points that do not carry `&egui::Context` (`import_graphics_dialog`,
    /// `import_to_library_dialog`) onto the active UI thread's `egui::Context` on the next pass.
    static PENDING_FIGMA_QUEUE: RefCell<Option<QueuedFigmaRequest>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FigmaPageChoice {
    pub(crate) summary: FigmaPageSummary,
    pub(crate) selected: bool,
}

type InspectOutcome = Result<FigmaDocumentSummary, String>;
type ImportOutcome = Result<(Project, ImportReport), String>;

#[derive(Clone)]
enum FigmaModalPhase {
    Inspecting(Arc<Mutex<Receiver<InspectOutcome>>>),
    Ready,
    Importing(Arc<Mutex<Receiver<ImportOutcome>>>),
}

impl std::fmt::Debug for FigmaModalPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inspecting(_) => f.write_str("Inspecting"),
            Self::Ready => f.write_str("Ready"),
            Self::Importing(_) => f.write_str("Importing"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FigmaImportDraft {
    pub(crate) path: PathBuf,
    pub(crate) remaining_paths: Vec<PathBuf>,
    pub(crate) target: FigmaImportTarget,
    pub(crate) doc_name: String,
    pub(crate) pages: Vec<FigmaPageChoice>,
    pub(crate) create_movie_clip: bool,
    pub(crate) import_hidden_layers: bool,
    pub(crate) error: Option<String>,
    phase: FigmaModalPhase,
}

impl FigmaImportDraft {
    fn page_mapping(&self) -> FigmaPageMapping {
        if matches!(self.target, FigmaImportTarget::OpenDocument) && !self.create_movie_clip {
            FigmaPageMapping::Scenes
        } else {
            FigmaPageMapping::Layers
        }
    }
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf3-figma-import-draft")
}

/// Fixed v3 dialog size (`dialog_m`), identical across inspecting, 1..N pages, errors, and import.
pub(crate) fn figma_import_dialog_size(ctx: &egui::Context) -> egui::Vec2 {
    fixed_dialog_size(
        ctx,
        metrics::dialog_m(),
        form_dialog_height(3, 2, 7.0 * metrics::row()),
    )
}

pub(crate) fn has_figma_import_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<FigmaImportDraft>(draft_id()))
        .is_some()
        || PENDING_FIGMA_QUEUE.with(|q| q.borrow().is_some())
}

#[cfg(test)]
pub(crate) fn figma_import_draft(ctx: &egui::Context) -> Option<FigmaImportDraft> {
    ctx.data(|d| d.get_temp::<FigmaImportDraft>(draft_id()))
}

#[cfg(test)]
pub(crate) fn edit_figma_import_draft(ctx: &egui::Context, f: impl FnOnce(&mut FigmaImportDraft)) {
    let id = draft_id();
    if let Some(mut draft) = ctx.data(|d| d.get_temp::<FigmaImportDraft>(id)) {
        f(&mut draft);
        ctx.data_mut(|d| d.insert_temp(id, draft));
    }
}

/// Returns `true` when `path` is a Figma `.fig`, `.figma`, `.figma.json`, or REST JSON export.
pub(crate) fn is_figma_file_path(path: &Path) -> bool {
    if path.is_dir() {
        return false;
    }
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".fig") || lower.ends_with(".figma") || lower.ends_with(".figma.json") {
            return true;
        }
        if lower.ends_with(".json")
            && let Ok(file) = std::fs::File::open(path)
        {
            let mut head = Vec::with_capacity(8192);
            if file.take(8192).read_to_end(&mut head).is_ok() && !head.is_empty() {
                return neoflash_import::is_figma_bytes(&head);
            }
        }
    }
    false
}

fn build_page_choices(
    summary: &FigmaDocumentSummary,
    target: &FigmaImportTarget,
) -> Vec<FigmaPageChoice> {
    let n = summary.pages.len();
    let default_idx = if n == 0 {
        None
    } else if n == 1 {
        Some(0)
    } else {
        summary
            .pages
            .iter()
            .position(|p| p.visible && p.node_count > 0)
            .or_else(|| summary.pages.iter().position(|p| p.node_count > 0))
            .or_else(|| summary.pages.iter().position(|p| p.visible))
            .or(Some(0))
    };

    summary
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| FigmaPageChoice {
            summary: p.clone(),
            selected: matches!(target, FigmaImportTarget::OpenDocument) || default_idx == Some(i),
        })
        .collect()
}

fn spawn_inspect_worker(path: PathBuf) -> Arc<Mutex<Receiver<InspectOutcome>>> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("neoflash-figma-inspect".into())
        .spawn(move || {
            let res = inspect_figma_path(&path).map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    Arc::new(Mutex::new(rx))
}

fn spawn_import_worker(
    path: PathBuf,
    options: FigmaImportOptions,
) -> Arc<Mutex<Receiver<ImportOutcome>>> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("neoflash-figma-import".into())
        .spawn(move || {
            let res = import_figma_path(&path, &options).map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    Arc::new(Mutex::new(rx))
}

fn create_draft(
    path: PathBuf,
    remaining_paths: Vec<PathBuf>,
    target: FigmaImportTarget,
    force_async_inspect: bool,
) -> FigmaImportDraft {
    let default_mc = match target {
        FigmaImportTarget::Stage { .. } | FigmaImportTarget::Library => true,
        FigmaImportTarget::OpenDocument => false,
    };
    let import_hidden_layers = matches!(target, FigmaImportTarget::OpenDocument);
    let fallback_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|n| {
            n.strip_suffix(".figma.json")
                .or_else(|| n.strip_suffix(".FIGMA.JSON"))
        })
        .or_else(|| path.file_stem().and_then(|s| s.to_str()))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Figma")
        .to_owned();

    let file_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if force_async_inspect || file_len > ASYNC_INSPECT_THRESHOLD_BYTES {
        return FigmaImportDraft {
            remaining_paths,
            target,
            doc_name: fallback_name,
            pages: Vec::new(),
            create_movie_clip: default_mc,
            import_hidden_layers,
            error: None,
            phase: FigmaModalPhase::Inspecting(spawn_inspect_worker(path.clone())),
            path,
        };
    }

    match inspect_figma_path(&path) {
        Ok(summary) => {
            let pages = build_page_choices(&summary, &target);
            let err = pages
                .is_empty()
                .then(|| "This Figma file contains no importable pages.".to_owned());
            FigmaImportDraft {
                path,
                remaining_paths,
                target,
                doc_name: if summary.name.trim().is_empty() {
                    fallback_name
                } else {
                    summary.name
                },
                pages,
                create_movie_clip: default_mc,
                import_hidden_layers,
                error: err,
                phase: FigmaModalPhase::Ready,
            }
        }
        Err(err) => FigmaImportDraft {
            path,
            remaining_paths,
            target,
            doc_name: fallback_name,
            pages: Vec::new(),
            create_movie_clip: default_mc,
            import_hidden_layers,
            error: Some(err.to_string()),
            phase: FigmaModalPhase::Ready,
        },
    }
}

fn page_counts_text(page: &FigmaPageSummary) -> String {
    let mut parts = Vec::new();
    if !page.visible {
        parts.push("Hidden".to_owned());
    }
    if page.node_count == 0 {
        parts.push("Empty".to_owned());
    } else if page.top_frame_count > 0 {
        parts.push(format!(
            "{} frame{} · {} item{}",
            page.top_frame_count,
            if page.top_frame_count == 1 { "" } else { "s" },
            page.node_count,
            if page.node_count == 1 { "" } else { "s" },
        ));
    } else {
        parts.push(format!(
            "{} item{}",
            page.node_count,
            if page.node_count == 1 { "" } else { "s" },
        ));
    }
    parts.join(" · ")
}

fn draw_page_row(ui: &mut Ui, idx: usize, entry: &mut FigmaPageChoice, enabled: bool) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (row_rect, row_resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), metrics::row()),
        egui::Sense::click(),
    );
    if enabled && row_resp.hovered() {
        ui.painter().rect_filled(
            row_rect,
            egui::CornerRadius::same(radius::sm() as u8),
            nf.bg_hover,
        );
    }

    let counts_label = page_counts_text(&entry.summary);
    let counts_job = egui::text::LayoutJob::single_section(
        counts_label,
        typography::format_line(TextStyle::Small, nf.text_tertiary),
    );
    let counts_galley = ui.fonts_mut(|f| f.layout_job(counts_job));
    let right_pad = space::s3();
    let counts_x = (row_rect.right() - right_pad - counts_galley.size().x)
        .max(row_rect.left() + space::s2() + metrics::check_box());
    ui.painter().galley(
        egui::pos2(counts_x, row_rect.center().y - counts_galley.size().y * 0.5),
        counts_galley,
        nf.text_tertiary,
    );

    let left_max_x =
        (counts_x - space::s2()).max(row_rect.left() + space::s2() + metrics::check_box());
    let left_rect = egui::Rect::from_min_max(
        egui::pos2(row_rect.left() + space::s2(), row_rect.top()),
        egui::pos2(left_max_x, row_rect.bottom()),
    );
    let mut left_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("figma-page-row-check", idx))
            .max_rect(left_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    left_ui.set_clip_rect(left_rect.intersect(ui.clip_rect()));
    let check_resp = left_ui.add_enabled(
        enabled,
        c::Checkbox::new(&mut entry.selected, &entry.summary.name),
    );

    if enabled && row_resp.clicked() && !check_resp.clicked() {
        entry.selected = !entry.selected;
    }
}

fn is_untouched_blank_layer(layer: &neoflash_core::Layer) -> bool {
    layer.visible
        && !layer.locked
        && layer.properties.role == LayerRole::Normal
        && layer.properties.is_default()
        && layer.keyframes.len() == 1
        && layer.keyframes[0].frame == 0
        && layer.keyframes[0].elements.is_empty()
        && layer.keyframes[0].bitmap_fills.is_empty()
        && layer.keyframes[0].label.is_empty()
        && layer.keyframes[0].sound.is_none()
        && layer.keyframes[0].video.is_none()
        && layer.keyframes[0].tween.is_none()
}

impl Editor {
    pub(crate) fn enqueue_figma_import_dialog(
        &mut self,
        paths: Vec<PathBuf>,
        target: FigmaImportTarget,
    ) {
        if paths.is_empty() {
            return;
        }
        PENDING_FIGMA_QUEUE.with(|q| {
            *q.borrow_mut() = Some(QueuedFigmaRequest { paths, target });
        });
    }

    pub(crate) fn open_figma_import_dialog(
        &mut self,
        ctx: &egui::Context,
        mut paths: Vec<PathBuf>,
        target: FigmaImportTarget,
    ) {
        if paths.is_empty() {
            return;
        }
        self.finish_edits();
        let first = paths.remove(0);
        let draft = create_draft(first, paths, target, false);
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn open_figma_import_dialog_for_open(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.open_figma_import_dialog(ctx, vec![path], FigmaImportTarget::OpenDocument);
    }

    /// Screenshot aid (`NEOFLASH_SCREENSHOT_DIALOG=figma_import`): opens a multi-page sample draft.
    pub(crate) fn open_figma_import_screenshot_sample(&mut self, ctx: &egui::Context) {
        let pages = vec![
            FigmaPageChoice {
                summary: FigmaPageSummary {
                    id: "0:1".into(),
                    name: "01 · Hero & Navigation".into(),
                    visible: true,
                    top_frame_count: 3,
                    node_count: 148,
                },
                selected: true,
            },
            FigmaPageChoice {
                summary: FigmaPageSummary {
                    id: "0:2".into(),
                    name: "02 · Interactive Cards".into(),
                    visible: true,
                    top_frame_count: 6,
                    node_count: 312,
                },
                selected: true,
            },
            FigmaPageChoice {
                summary: FigmaPageSummary {
                    id: "0:3".into(),
                    name: "03 · Component Library".into(),
                    visible: true,
                    top_frame_count: 14,
                    node_count: 890,
                },
                selected: false,
            },
            FigmaPageChoice {
                summary: FigmaPageSummary {
                    id: "0:4".into(),
                    name: "99 · Scratchpad".into(),
                    visible: false,
                    top_frame_count: 1,
                    node_count: 24,
                },
                selected: false,
            },
        ];
        ctx.data_mut(|d| {
            d.insert_temp(
                draft_id(),
                FigmaImportDraft {
                    path: PathBuf::from("LandingPage.fig"),
                    remaining_paths: Vec::new(),
                    target: FigmaImportTarget::Stage { point: None },
                    doc_name: "Landing Page — v3".into(),
                    pages,
                    create_movie_clip: true,
                    import_hidden_layers: false,
                    error: None,
                    phase: FigmaModalPhase::Ready,
                },
            );
        });
    }

    pub(crate) fn figma_import_dialog(&mut self, ctx: &egui::Context) {
        if let Some(req) = PENDING_FIGMA_QUEUE.with(|q| q.borrow_mut().take()) {
            self.open_figma_import_dialog(ctx, req.paths, req.target);
        }

        let Some(mut draft) = ctx.data(|d| d.get_temp::<FigmaImportDraft>(draft_id())) else {
            return;
        };

        // Poll background workers without blocking the UI thread.
        let mut completed_import: Option<(Project, ImportReport)> = None;
        match &draft.phase {
            FigmaModalPhase::Inspecting(rx_arc) => {
                let outcome = rx_arc
                    .lock()
                    .ok()
                    .map(|rx| rx.try_recv())
                    .unwrap_or(Err(TryRecvError::Disconnected));
                match outcome {
                    Ok(Ok(summary)) => {
                        draft.pages = build_page_choices(&summary, &draft.target);
                        if !summary.name.trim().is_empty() {
                            draft.doc_name = summary.name;
                        }
                        draft.error = draft
                            .pages
                            .is_empty()
                            .then(|| "This Figma file contains no importable pages.".to_owned());
                        draft.phase = FigmaModalPhase::Ready;
                    }
                    Ok(Err(err)) => {
                        draft.error = Some(err);
                        draft.phase = FigmaModalPhase::Ready;
                    }
                    Err(TryRecvError::Empty) => {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                    Err(TryRecvError::Disconnected) => {
                        draft.error = Some("Figma inspection worker stopped unexpectedly.".into());
                        draft.phase = FigmaModalPhase::Ready;
                    }
                }
            }
            FigmaModalPhase::Importing(rx_arc) => {
                let outcome = rx_arc
                    .lock()
                    .ok()
                    .map(|rx| rx.try_recv())
                    .unwrap_or(Err(TryRecvError::Disconnected));
                match outcome {
                    Ok(Ok(pair)) => {
                        completed_import = Some(pair);
                    }
                    Ok(Err(err)) => {
                        draft.error = Some(err);
                        draft.phase = FigmaModalPhase::Ready;
                    }
                    Err(TryRecvError::Empty) => {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    }
                    Err(TryRecvError::Disconnected) => {
                        draft.error = Some("Figma import worker stopped unexpectedly.".into());
                        draft.phase = FigmaModalPhase::Ready;
                    }
                }
            }
            FigmaModalPhase::Ready => {}
        }

        if let Some((imported_project, report)) = completed_import {
            let path = draft.path.clone();
            let remaining = draft.remaining_paths.clone();
            let target = draft.target.clone();
            let create_mc = draft.create_movie_clip;
            if let Err(err) = self.commit_interactive_figma_import(
                ctx,
                &path,
                imported_project,
                report,
                target.clone(),
                create_mc,
            ) {
                draft.error = Some(err.clone());
                draft.phase = FigmaModalPhase::Ready;
                ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
                self.notify_with_output(c::ToastType::Error, format!("Figma import failed: {err}"));
                return;
            }
            ctx.data_mut(|d| d.remove::<FigmaImportDraft>(draft_id()));
            if !remaining.is_empty() {
                let next_target = match target {
                    FigmaImportTarget::Stage { point } => FigmaImportTarget::Stage {
                        point: point.map(|[x, y]| [x + 16.0, y + 16.0]),
                    },
                    other => other,
                };
                self.open_figma_import_dialog(ctx, remaining, next_target);
            }
            return;
        }

        let is_ready = matches!(draft.phase, FigmaModalPhase::Ready);
        let is_inspecting = matches!(draft.phase, FigmaModalPhase::Inspecting(_));
        let is_importing = matches!(draft.phase, FigmaModalPhase::Importing(_));
        let selected_count = draft.pages.iter().filter(|p| p.selected).count();
        let can_edit = is_ready && !draft.pages.is_empty();
        let can_import = is_ready && selected_count > 0;

        let mut cancel_clicked = false;
        let mut import_clicked = false;
        let mut select_all_clicked = false;
        let mut deselect_all_clicked = false;

        let kb = DialogKeyboardState::capture(ctx);
        let nf = neoflash_ui::palette(ctx);
        let size = figma_import_dialog_size(ctx);

        let modal = egui::Modal::new(egui::Id::new("import-figma"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Import Figma", size.x, Some(size.y));
                cancel_clicked |= dialog.close;

                dialog.page(ui, "import-figma-page", |ui, area| {
                    group_caption(ui, "Pages");

                    form_line(ui, |ui| {
                        select_all_clicked |= ui
                            .add_enabled(
                                can_edit && selected_count < draft.pages.len(),
                                c::Button::ghost("Select all"),
                            )
                            .clicked();
                        deselect_all_clicked |= ui
                            .add_enabled(
                                can_edit && selected_count > 0,
                                c::Button::ghost("Deselect all"),
                            )
                            .clicked();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let summary_txt = if is_inspecting {
                                draft.doc_name.clone()
                            } else {
                                format!("{} of {} selected", selected_count, draft.pages.len())
                            };
                            ui.add(
                                egui::Label::new(
                                    typography::rich(summary_txt, TextStyle::Small)
                                        .color(nf.text_tertiary),
                                )
                                .truncate(),
                            );
                        });
                    });

                    let below_estimate = metrics::section_header()
                        + 2.0 * metrics::control()
                        + 3.0 * metrics::row_gap();
                    let list_height = area.fill_height(
                        ui,
                        "figma-import-pages",
                        below_estimate,
                        3.0 * metrics::row(),
                    );
                    let block = field_block(ui, egui::vec2(ui.available_width(), list_height));
                    let inner = block.shrink2(egui::vec2(space::s1(), space::s1()));
                    let mut list_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("figma-import-pages-block")
                            .max_rect(inner)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    list_ui.set_clip_rect(inner.intersect(ui.clip_rect()));

                    if is_inspecting {
                        list_ui.add_space(space::s3());
                        list_ui.horizontal(|ui| {
                            ui.add_space(space::s3());
                            c::spinner(ui);
                            ui.label(
                                typography::rich("Reading Figma pages…", TextStyle::Body)
                                    .color(nf.text_secondary),
                            );
                        });
                    } else if draft.pages.is_empty() {
                        list_ui.add_space(space::s2());
                        let msg = draft
                            .error
                            .as_deref()
                            .unwrap_or("No importable pages found in this Figma file.");
                        list_ui.horizontal(|ui| {
                            ui.add_space(space::s2());
                            let w = (ui.available_width() - space::s2()).max(0.0);
                            ui.add(c::InlineMessage::error(msg).width(w));
                        });
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("figma-import-pages-scroll")
                            .max_height(inner.height())
                            .auto_shrink([false, false])
                            .themed()
                            .show(&mut list_ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (idx, page_entry) in draft.pages.iter_mut().enumerate() {
                                    draw_page_row(ui, idx, page_entry, can_edit);
                                }
                            });
                    }

                    group_caption(ui, "Options");
                    form_line(ui, |ui| {
                        ui.add_enabled(
                            can_edit,
                            c::Checkbox::new(
                                &mut draft.create_movie_clip,
                                "Create MovieClip symbol",
                            ),
                        );
                    });
                    form_line(ui, |ui| {
                        ui.add_enabled(
                            can_edit,
                            c::Checkbox::new(
                                &mut draft.import_hidden_layers,
                                "Include hidden layers",
                            ),
                        );
                    });

                    area.remember_below(ui, "figma-import-pages", block.bottom());
                });

                dialog.footer(
                    ui,
                    |ui| {
                        if is_inspecting {
                            c::spinner(ui);
                            footer_note(ui, "Inspecting Figma file…");
                        } else if is_importing {
                            c::spinner(ui);
                            footer_note(ui, "Importing selected pages…");
                        } else if let Some(err) = &draft.error {
                            ui.add(
                                egui::Label::new(
                                    typography::rich(err, TextStyle::Small)
                                        .color(nf.danger_default),
                                )
                                .truncate(),
                            );
                        } else if selected_count == 0 {
                            footer_note(ui, "Select at least one page");
                        } else {
                            let suffix = if selected_count == 1 { "" } else { "s" };
                            let destination = if draft.page_mapping() == FigmaPageMapping::Scenes {
                                format!("scene{suffix}")
                            } else {
                                let mode_label = match (&draft.target, draft.create_movie_clip) {
                                    (_, true) => "MovieClip",
                                    (FigmaImportTarget::Library, false) => "Library symbols",
                                    _ => "Timeline layers",
                                };
                                format!("layer{suffix} · {mode_label}")
                            };
                            footer_note(
                                ui,
                                &format!(
                                    "{selected_count} page{suffix} → {selected_count} {destination}"
                                ),
                            );
                        }
                    },
                    |ui| {
                        import_clicked |= ui
                            .add_enabled(can_import, c::Button::primary("Import"))
                            .clicked();
                        cancel_clicked |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });

        if select_all_clicked {
            for p in &mut draft.pages {
                p.selected = true;
            }
        }
        if deselect_all_clicked {
            for p in &mut draft.pages {
                p.selected = false;
            }
        }

        if !cancel_clicked && !import_clicked {
            let now_selected = draft.pages.iter().filter(|p| p.selected).count();
            match kb.resolve(ctx, &modal, is_ready && now_selected > 0) {
                DialogKeyboardOutcome::Cancel => cancel_clicked = true,
                DialogKeyboardOutcome::Confirm => import_clicked = true,
                DialogKeyboardOutcome::None => {}
            }
        }

        if cancel_clicked {
            ctx.data_mut(|d| d.remove::<FigmaImportDraft>(draft_id()));
            return;
        }

        if import_clicked {
            let selected_ids: Vec<String> = draft
                .pages
                .iter()
                .filter(|p| p.selected)
                .map(|p| p.summary.id.clone())
                .collect();
            if !selected_ids.is_empty() {
                draft.error = None;
                // A complete document also retains unused component masters from the Library.
                let page_mapping = draft.page_mapping();
                let full_document = page_mapping == FigmaPageMapping::Scenes
                    && draft.import_hidden_layers
                    && selected_ids.len() == draft.pages.len();
                let opts = FigmaImportOptions {
                    layer_mapping: FigmaLayerMapping::TimelineLayers,
                    import_hidden_layers: draft.import_hidden_layers,
                    preserve_masks: true,
                    preserve_blend_modes: true,
                    resize_stage_to_artboard: true,
                    selected_page_ids: (!full_document).then_some(selected_ids),
                    page_mapping,
                };
                draft.phase =
                    FigmaModalPhase::Importing(spawn_import_worker(draft.path.clone(), opts));
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }

        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn commit_interactive_figma_import(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        mut imported: Project,
        report: ImportReport,
        target: FigmaImportTarget,
        create_movie_clip: bool,
    ) -> Result<(), String> {
        match target {
            FigmaImportTarget::OpenDocument => {
                if create_movie_clip {
                    wrap_project_root_layers_in_movie_clip(&mut imported)?;
                }
                crate::timeline_commands::upgrade_all_format_versions(&mut imported);
                imported.validate().map_err(|e| e.to_string())?;

                self.workspace.general.push_recent(path);
                self.open_new_session_tab(ctx, imported, None);
                self.import_source =
                    Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
                self.saved_project = None;
                self.sync_disk_mtime();
                self.sync_session_tabs_to_prefs();
                self.load_project_fonts(ctx);

                let file_loc = crate::output_pipeline::OutputLocation::for_file(path.to_path_buf());
                for d in &report.diagnostics {
                    let sev = match d.severity {
                        neoflash_import::Severity::Info => {
                            crate::output_pipeline::OutputSeverity::Info
                        }
                        neoflash_import::Severity::Warning => {
                            crate::output_pipeline::OutputSeverity::Warning
                        }
                        neoflash_import::Severity::Error => {
                            crate::output_pipeline::OutputSeverity::Error
                        }
                    };
                    self.push_output_coalesced(
                        crate::output_pipeline::OutputEvent::new(
                            sev,
                            crate::output_pipeline::OutputSource::Import,
                            format!("{}: {}", d.location, d.message),
                        )
                        .with_location(file_loc.clone()),
                    );
                }
                self.message(
                    "Imported Figma composition. Save as a NeoFlash project (.nflash) or Flash FLA (.fla).",
                );
                Ok(())
            }
            FigmaImportTarget::Stage { point } => {
                let res = if create_movie_clip {
                    self.commit_figma_as_movie_clip_symbol(path, imported, &report, point, true)
                } else {
                    self.commit_figma_pages_directly_as_layers(path, imported, &report, point)
                };
                if res.is_ok() {
                    self.load_project_fonts(ctx);
                }
                res
            }
            FigmaImportTarget::Library => {
                let res = if create_movie_clip {
                    self.commit_figma_as_movie_clip_symbol(path, imported, &report, None, false)
                } else {
                    self.commit_figma_pages_as_separate_library_symbols(path, imported, &report)
                };
                if res.is_ok() {
                    self.load_project_fonts(ctx);
                }
                res
            }
        }
    }
}

fn wrap_project_root_layers_in_movie_clip(project: &mut Project) -> Result<(), String> {
    let root_id = project.root.clone();
    let mc_name = if project.name.trim().is_empty() {
        "Figma Composition".into()
    } else {
        project.name.clone()
    };
    let root_layers = project
        .symbol_mut(&root_id)
        .map(|s| std::mem::take(&mut s.timeline.layers))
        .ok_or_else(|| "Imported Figma project has no root symbol.".to_owned())?;
    let mc_id = new_id("figmc");
    project.symbols.push(Symbol {
        id: mc_id.clone(),
        name: mc_name.clone(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            layers: root_layers,
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    if let Some(root_sym) = project.symbol_mut(&root_id) {
        root_sym.timeline.layers = vec![neoflash_core::Layer {
            id: new_id("layer"),
            name: mc_name,
            visible: true,
            locked: false,
            properties: Default::default(),
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: new_id("object"),
                    name: String::new(),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: mc_id,
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            }],
        }];
    }
    Ok(())
}

pub(crate) fn active_timeline_is_single_blank_layer(timeline: &Timeline, frame: u32) -> bool {
    frame == 0 && timeline.layers.len() == 1 && is_untouched_blank_layer(&timeline.layers[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_ui::metrics::Density;

    fn sample_multipage_figma_json() -> String {
        r#"{
          "name": "UI Screens Kit",
          "schemaVersion": 0,
          "document": {
            "id": "0:0",
            "name": "Document",
            "type": "DOCUMENT",
            "children": [
              {
                "id": "0:99",
                "name": "Internal Only Canvas",
                "type": "CANVAS",
                "internalOnly": true,
                "children": [
                  {
                    "id": "50:1",
                    "name": "ChipMaster",
                    "type": "COMPONENT",
                    "absoluteBoundingBox": {"x": 0, "y": 0, "width": 80, "height": 28},
                    "children": [
                      {
                        "id": "50:2",
                        "name": "ChipRect",
                        "type": "RECTANGLE",
                        "cornerRadius": 8,
                        "absoluteBoundingBox": {"x": 0, "y": 0, "width": 80, "height": 28},
                        "fills": [{"type": "SOLID", "color": {"r": 0.2, "g": 0.5, "b": 0.9, "a": 1.0}}]
                      }
                    ]
                  }
                ]
              },
              {
                "id": "0:1",
                "name": "Empty Cover",
                "type": "CANVAS",
                "children": []
              },
              {
                "id": "0:2",
                "name": "Onboarding",
                "type": "CANVAS",
                "children": [
                  {
                    "id": "1:1",
                    "name": "Onboard Frame",
                    "type": "FRAME",
                    "clipsContent": true,
                    "absoluteBoundingBox": {"x": 0, "y": 0, "width": 400, "height": 300},
                    "fills": [{"type": "SOLID", "color": {"r": 0.1, "g": 0.12, "b": 0.16, "a": 1.0}}],
                    "children": [
                      {
                        "id": "1:2",
                        "name": "Hero Chip",
                        "type": "INSTANCE",
                        "componentId": "50:1",
                        "absoluteBoundingBox": {"x": 24, "y": 32, "width": 80, "height": 28}
                      }
                    ]
                  }
                ]
              },
              {
                "id": "0:3",
                "name": "Feed",
                "type": "CANVAS",
                "children": [
                  {
                    "id": "2:1",
                    "name": "Feed Card",
                    "type": "RECTANGLE",
                    "absoluteBoundingBox": {"x": 10, "y": 20, "width": 360, "height": 180},
                    "fills": [{"type": "SOLID", "color": {"r": 0.3, "g": 0.8, "b": 0.5, "a": 1.0}}]
                  }
                ]
              },
              {
                "id": "0:4",
                "name": "Profile",
                "type": "CANVAS",
                "children": [
                  {
                    "id": "3:1",
                    "name": "Profile Heading",
                    "type": "TEXT",
                    "characters": "Miles",
                    "absoluteBoundingBox": {"x": 16, "y": 16, "width": 200, "height": 32},
                    "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1, "a": 1}}],
                    "style": {"fontFamily": "Inter", "fontSize": 24, "fontWeight": 700}
                  },
                  {
                    "id": "3:2",
                    "name": "Hidden Badge",
                    "type": "RECTANGLE",
                    "visible": false,
                    "absoluteBoundingBox": {"x": 16, "y": 60, "width": 48, "height": 20},
                    "fills": [{"type": "SOLID", "color": {"r": 1, "g": 0.2, "b": 0.2, "a": 1}}]
                  }
                ]
              }
            ]
          }
        }"#
        .to_owned()
    }

    struct ModalHarness {
        ctx: egui::Context,
        editor: Editor,
        out: egui::FullOutput,
        time: f64,
    }

    impl ModalHarness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut base = Project::blank();
            base.name = "StageHost".into();
            base.width = 960;
            base.height = 540;
            let mut editor = Editor::from_project(&ctx, base);
            editor.history.clear();
            Self {
                ctx,
                editor,
                out: egui::FullOutput::default(),
                time: 0.0,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.05;
            let editor = &mut self.editor;
            self.out = self.ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1920.0, 1080.0),
                    )),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ctx| editor.figma_import_dialog(ctx),
            );
        }

        fn settle(&mut self) {
            for _ in 0..3 {
                self.frame(vec![]);
            }
        }

        fn click(&mut self, label: &str) {
            let at = self
                .out
                .platform_output
                .accesskit_update
                .as_ref()
                .and_then(|update| {
                    update.nodes.iter().find_map(|(_, node)| {
                        (node.label() == Some(label))
                            .then(|| node.bounds())
                            .flatten()
                    })
                })
                .map(|b| egui::pos2((b.x0 + b.x1) as f32 * 0.5, (b.y0 + b.y1) as f32 * 0.5))
                .unwrap_or_else(|| panic!("control {label:?} is not shown"));
            self.frame(vec![
                egui::Event::PointerMoved(at),
                crate::tests::primary_button(at, true),
            ]);
            self.frame(vec![crate::tests::primary_button(at, false)]);
            self.settle();
        }

        fn wait_until_closed(&mut self) {
            for _ in 0..250 {
                if !has_figma_import_dialog(&self.ctx) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(4));
                self.frame(vec![]);
            }
            panic!("Import Figma dialog worker did not complete within timeout");
        }

        fn modal_rect(&mut self) -> egui::Rect {
            self.settle();
            self.ctx
                .memory(|m| m.area_rect(egui::Id::new("import-figma")))
                .expect("import-figma modal must be open")
        }
    }

    #[test]
    fn t391_stage_drop_opens_figma_dialog_and_imports_movieclip_or_direct_page_layers_with_undo() {
        let temp =
            std::env::temp_dir().join(format!("neoflash-t391-figma-dialog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let fig_path = temp.join("ProductScreens.figma.json");
        std::fs::write(&fig_path, sample_multipage_figma_json()).unwrap();

        let mut h = ModalHarness::new();
        let before = h.editor.project.clone();

        let mapping = crate::stage_mapping::StageMapping::identity();
        let origin = egui::pos2(40.0, 60.0);
        h.editor.zoom = 1.0;
        let drop_local = egui::pos2(300.0, 220.0);
        let drop_screen = mapping.edit_to_screen(origin, h.editor.zoom, drop_local);
        let stage_area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));

        // 1. Drop multi-page Figma file onto Stage -> opens interactive `Import Figma` dialog!
        let drop_input = egui::RawInput {
            screen_rect: Some(stage_area),
            events: vec![egui::Event::PointerMoved(drop_screen)],
            dropped_files: vec![egui::DroppedFile {
                path: Some(fig_path.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let editor_ref = &mut h.editor;
        let _ = h.ctx.run(drop_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor_ref.stage_file_drop(ui, stage_area, origin, mapping);
            });
        });

        assert!(has_figma_import_dialog(&h.ctx));
        assert!(h.editor.has_modal_dialog_in(&h.ctx));
        h.settle();

        // Default selection: 4 user pages (`Internal Only Canvas` excluded!), ONLY the first
        // visible non-empty page (`Onboarding`, index 1) is checked by default, `Create MovieClip symbol` is true!
        let initial = figma_import_draft(&h.ctx).expect("draft open");
        assert_eq!(initial.pages.len(), 4);
        assert_eq!(
            initial
                .pages
                .iter()
                .map(|p| (p.summary.name.as_str(), p.selected))
                .collect::<Vec<_>>(),
            vec![
                ("Empty Cover", false),
                ("Onboarding", true),
                ("Feed", false),
                ("Profile", false),
            ]
        );
        assert!(initial.create_movie_clip);
        assert!(!initial.import_hidden_layers);

        // Select `Feed` as well (`Onboarding` + `Feed` = 2 pages) and confirm with `Create MovieClip symbol = true`!
        h.click("Feed");
        h.click("Import");
        h.wait_until_closed();

        h.editor
            .project
            .validate()
            .expect("MovieClip Figma import validates");
        let mc_sym = h
            .editor
            .project
            .symbols
            .iter()
            .find(|s| s.name == "ProductScreens" && s.kind == SymbolKind::MovieClip)
            .expect("ProductScreens MovieClip created in Library");
        assert_eq!(
            mc_sym
                .timeline
                .layers
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Onboarding", "Feed"],
            "MovieClip timeline must have 1 Layer per selected Figma page"
        );
        let placed_inst = h
            .editor
            .symbol()
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .find(|el| {
                matches!(&el.kind, ElementKind::Instance { symbol_id, .. } if *symbol_id == mc_sym.id)
            })
            .expect("MovieClip instance placed on Stage at drop point");
        assert!((placed_inst.transform.x - (300.0 - 200.0)).abs() < 1e-4);
        assert!((placed_inst.transform.y - (220.0 - 150.0)).abs() < 1e-4);

        // Single Undo restores exact prior project!
        h.editor.undo();
        assert_eq!(h.editor.project, before);

        // 2. Drop again, pick 3 pages (`Onboarding`, `Feed`, `Profile`), UNCHECK `Create MovieClip symbol`
        //    -> imports directly onto active timeline as 3 separate Layers (`1 page = 1 layer`)!
        let drop_input_2 = egui::RawInput {
            screen_rect: Some(stage_area),
            events: vec![egui::Event::PointerMoved(drop_screen)],
            dropped_files: vec![egui::DroppedFile {
                path: Some(fig_path.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let editor_ref = &mut h.editor;
        let _ = h.ctx.run(drop_input_2, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor_ref.stage_file_drop(ui, stage_area, origin, mapping);
            });
        });
        h.settle();

        h.click("Feed");
        h.click("Profile");
        h.click("Create MovieClip symbol");
        let mid = figma_import_draft(&h.ctx).unwrap();
        assert!(!mid.create_movie_clip);

        h.click("Import");
        h.wait_until_closed();

        h.editor
            .project
            .validate()
            .expect("direct 1-page-per-layer timeline import validates");
        assert!(
            !h.editor
                .project
                .symbols
                .iter()
                .any(|s| s.name == "ProductScreens"),
            "unchecked Create MovieClip symbol must not wrap pages in an outer MovieClip"
        );
        let active_layers: Vec<&str> = h
            .editor
            .symbol()
            .timeline
            .layers
            .iter()
            .map(|l| l.name.as_str())
            .collect();
        assert_eq!(
            active_layers,
            vec!["Onboarding", "Feed", "Profile"],
            "each selected Figma page must become its own separate Layer on the active timeline"
        );
        // Verify Feed element is normalized to page origin `[0, 0]` and offset to the Stage drop
        // point (`center = [300, 220]`, stage `400x300` -> offset `[100, 70]`).
        let feed_layer = &h.editor.symbol().timeline.layers[1];
        assert_eq!(feed_layer.keyframes[0].elements.len(), 1);
        assert!((feed_layer.keyframes[0].elements[0].transform.x - 100.0).abs() < 1e-4);
        assert!((feed_layer.keyframes[0].elements[0].transform.y - 70.0).abs() < 1e-4);
        // Hidden Badge on Profile was excluded because `Include hidden layers` stayed false
        let profile_layer = &h.editor.symbol().timeline.layers[2];
        assert_eq!(profile_layer.keyframes[0].elements.len(), 1);

        // Single Undo restores exact prior project!
        h.editor.undo();
        assert_eq!(h.editor.project, before);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn t391_figma_import_dialog_keeps_one_fixed_outer_rect_across_states_and_densities() {
        let temp =
            std::env::temp_dir().join(format!("neoflash-t391-figma-rect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let fig_path = temp.join("RectCheck.figma.json");
        std::fs::write(&fig_path, sample_multipage_figma_json()).unwrap();

        for density in Density::ALL {
            metrics::set_density(density);
            let mut h = ModalHarness::new();
            h.editor.open_figma_import_dialog(
                &h.ctx,
                vec![fig_path.clone()],
                FigmaImportTarget::Stage { point: None },
            );

            let opened = h.modal_rect();
            assert!(
                (opened.width() - metrics::dialog_m() - 2.0).abs() <= 1.0,
                "Import Figma in {}: width {} does not match dialog_m {}",
                density.label(),
                opened.width(),
                metrics::dialog_m(),
            );

            // Step 1: 0 pages selected (Import disabled)
            h.click("Deselect all");
            assert_eq!(
                h.modal_rect(),
                opened,
                "Import Figma in {}: Deselect all resized/moved modal",
                density.label()
            );

            // Step 2: 30 pages scrolling inside `field_block`
            edit_figma_import_draft(&h.ctx, |d| {
                d.pages = (0..30)
                    .map(|i| FigmaPageChoice {
                        summary: FigmaPageSummary {
                            id: format!("0:{i}"),
                            name: format!("Design System Page #{i} — Extended Component Sheet"),
                            visible: i % 5 != 0,
                            top_frame_count: (i % 6) + 1,
                            node_count: i * 42 + 7,
                        },
                        selected: i % 2 == 0,
                    })
                    .collect();
            });
            assert_eq!(
                h.modal_rect(),
                opened,
                "Import Figma in {}: 30-page list resized/moved modal",
                density.label()
            );

            // Step 3: Error banner with empty page list
            edit_figma_import_draft(&h.ctx, |d| {
                d.pages.clear();
                d.error = Some("Corrupt container test error message".into());
            });
            assert_eq!(
                h.modal_rect(),
                opened,
                "Import Figma in {}: error state resized/moved modal",
                density.label()
            );

            // Step 4: Inspecting spinner state
            let (_tx, rx) = mpsc::channel::<InspectOutcome>();
            edit_figma_import_draft(&h.ctx, |d| {
                d.error = None;
                d.phase = FigmaModalPhase::Inspecting(Arc::new(Mutex::new(rx)));
            });
            assert_eq!(
                h.modal_rect(),
                opened,
                "Import Figma in {}: inspecting state resized/moved modal",
                density.label()
            );
        }

        metrics::set_density(Density::Comfortable);
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn t391_library_queue_and_open_document_routes_work_end_to_end() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-t391-figma-lib-open-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let fig_path = temp.join("LibAndOpen.figma.json");
        std::fs::write(&fig_path, sample_multipage_figma_json()).unwrap();

        let mut h = ModalHarness::new();
        let before = h.editor.project.clone();

        // 1. Queued `Import to Library…` (`enqueue_figma_import_dialog`) adds MovieClip to Library without Stage placement!
        h.editor
            .enqueue_figma_import_dialog(vec![fig_path.clone()], FigmaImportTarget::Library);
        assert!(has_figma_import_dialog(&h.ctx));
        h.settle();
        h.click("Feed");
        h.click("Import");
        h.wait_until_closed();

        h.editor.project.validate().unwrap();
        assert!(
            h.editor
                .project
                .symbols
                .iter()
                .any(|s| s.name == "LibAndOpen" && s.timeline.layers.len() == 2)
        );
        assert!(
            h.editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .is_empty(),
            "Import to Library must not place an instance on Stage"
        );
        h.editor.undo();
        assert_eq!(h.editor.project, before);

        // 2. Interactive `File ▸ Open` (`Pending::OpenPath`) opens `Import Figma` and creates a new session tab!
        h.editor
            .request(crate::app::Pending::OpenPath(fig_path.clone()), &h.ctx);
        assert!(has_figma_import_dialog(&h.ctx));
        h.settle();
        let draft = figma_import_draft(&h.ctx).unwrap();
        assert!(draft.pages.iter().all(|page| page.selected));
        assert!(draft.import_hidden_layers);
        h.click("Import");
        h.wait_until_closed();

        h.editor.project.validate().unwrap();
        assert_eq!(h.editor.project.name, "UI Screens Kit");
        assert_eq!(h.editor.project.width, 400);
        assert_eq!(h.editor.project.height, 300);
        let scenes: Vec<_> = h
            .editor
            .scene_list()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(scenes, ["Empty Cover", "Onboarding", "Feed", "Profile"]);
        let profile = h.editor.project.scenes.last().unwrap().clone();
        h.editor.switch_scene(&profile);
        assert!(
            h.editor
                .symbol()
                .timeline
                .layers
                .iter()
                .any(|layer| layer.name == "Hidden Badge" && !layer.visible)
        );

        let _ = std::fs::remove_dir_all(&temp);
    }

    fn tiny_png_wh_base64(w: u32, h: u32, rgba: [u8; 4]) -> String {
        let mut img = image::RgbaImage::new(w, h);
        for px in img.pixels_mut() {
            *px = image::Rgba(rgba);
        }
        let mut raw = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut raw), image::ImageFormat::Png)
            .unwrap();
        neoflash_core::ResourceUnit::from_bytes("tmp", "t.png", "image/png", &raw)
            .unwrap()
            .data_base64
            .to_string()
    }

    fn sample_multipage_bitmap_and_fill_figma_json() -> String {
        let hero_b64 = tiny_png_wh_base64(5, 3, [220, 60, 80, 255]);
        let avatar_b64 = tiny_png_wh_base64(2, 2, [40, 160, 240, 255]);
        format!(
            r#"{{
              "name": "Media Deck",
              "schemaVersion": 0,
              "images": {{
                "img_hero": "{hero_b64}",
                "img_avatar": "{avatar_b64}"
              }},
              "document": {{
                "id": "0:0",
                "name": "Document",
                "type": "DOCUMENT",
                "children": [
                  {{
                    "id": "0:1",
                    "name": "Cover",
                    "type": "CANVAS",
                    "children": [
                      {{
                        "id": "1:1",
                        "name": "Cover Banner",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 320, "height": 200}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 0.15, "g": 0.2, "b": 0.3, "a": 1.0}}}}]
                      }}
                    ]
                  }},
                  {{
                    "id": "0:2",
                    "name": "Gallery",
                    "type": "CANVAS",
                    "children": [
                      {{
                        "id": "2:1",
                        "name": "Hero Photo",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 200, "height": 120}},
                        "fills": [{{"type": "IMAGE", "imageRef": "img_hero", "scaleMode": "FILL"}}]
                      }},
                      {{
                        "id": "2:2",
                        "name": "Avatar Pill",
                        "type": "RECTANGLE",
                        "cornerRadius": 14,
                        "absoluteBoundingBox": {{"x": 220, "y": 16, "width": 64, "height": 64}},
                        "fills": [{{"type": "IMAGE", "imageRef": "img_avatar", "scaleMode": "FILL"}}]
                      }}
                    ]
                  }}
                ]
              }}
            }}"#
        )
    }

    fn assert_gallery_keyframe_has_valid_bitmap_and_fill(
        project: &Project,
        kf: &Keyframe,
    ) -> (String, String, String) {
        let mut bmp_res_id = None;
        let mut path_el_id = None;
        for el in &kf.elements {
            match &el.kind {
                ElementKind::Bitmap { resource_id, .. } => {
                    assert!(
                        project.resources.iter().any(|r| r.id == *resource_id),
                        "ElementKind::Bitmap resource_id {resource_id} missing from project.resources"
                    );
                    bmp_res_id = Some(resource_id.clone());
                }
                ElementKind::Path { .. } => {
                    path_el_id = Some(el.id.clone());
                }
                other => panic!("unexpected Gallery element kind: {other:?}"),
            }
        }
        let bmp_res = bmp_res_id.expect("Gallery page must contain ElementKind::Bitmap");
        let path_id = path_el_id.expect("Gallery page must contain rounded-rect ElementKind::Path");
        assert_eq!(
            kf.bitmap_fills.len(),
            1,
            "Gallery keyframe must carry 1 AuthoredBitmapFill"
        );
        let fill = &kf.bitmap_fills[0];
        assert_eq!(
            fill.element_id, path_id,
            "AuthoredBitmapFill.element_id must match rounded-rect Path element.id"
        );
        assert!(
            project.resources.iter().any(|r| r.id == fill.resource_id),
            "AuthoredBitmapFill.resource_id {} missing from project.resources",
            fill.resource_id
        );
        assert_ne!(bmp_res, fill.resource_id);
        (bmp_res, fill.resource_id.clone(), path_id)
    }

    #[test]
    fn t391_page2_bitmaps_and_bitmap_fills_survive_unwrapped_stage_and_library_imports() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-t391-figma-page2-bmp-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let fig_path = temp.join("MediaDeck.figma.json");
        std::fs::write(&fig_path, sample_multipage_bitmap_and_fill_figma_json()).unwrap();

        let mut h = ModalHarness::new();
        let before = h.editor.project.clone();

        // 1. Stage import (`Layers`, `Create MovieClip symbol = false`) with Page 1 + Page 2 (`Gallery`),
        //    followed by a second deduplicated re-import onto the same timeline!
        h.editor.open_figma_import_dialog(
            &h.ctx,
            vec![fig_path.clone()],
            FigmaImportTarget::Stage {
                point: Some([480.0, 270.0]),
            },
        );
        h.settle();
        h.click("Gallery");
        h.click("Create MovieClip symbol");
        h.click("Import");
        h.wait_until_closed();

        h.editor
            .project
            .validate()
            .expect("Stage unwrapped multi-page bitmap + bitmap_fill import validates");
        assert_eq!(h.editor.project.resources.len(), 2);
        let gallery_kf_1 = &h.editor.symbol().timeline.layers[1].keyframes[0];
        let (bmp_r1, fill_r1, path_id_1) =
            assert_gallery_keyframe_has_valid_bitmap_and_fill(&h.editor.project, gallery_kf_1);

        // Re-import onto Stage (`Create MovieClip symbol = false`) -> deduplicates resources & keeps IDs coherent!
        h.editor.open_figma_import_dialog(
            &h.ctx,
            vec![fig_path.clone()],
            FigmaImportTarget::Stage {
                point: Some([500.0, 290.0]),
            },
        );
        h.settle();
        h.click("Gallery");
        h.click("Create MovieClip symbol");
        h.click("Import");
        h.wait_until_closed();

        h.editor
            .project
            .validate()
            .expect("Stage deduplicated re-import validates");
        assert_eq!(h.editor.project.resources.len(), 2);
        assert_eq!(h.editor.symbol().timeline.layers.len(), 4);
        let gallery_kf_2 = &h.editor.symbol().timeline.layers[3].keyframes[0];
        let (bmp_r2, fill_r2, path_id_2) =
            assert_gallery_keyframe_has_valid_bitmap_and_fill(&h.editor.project, gallery_kf_2);
        assert_eq!(bmp_r2, bmp_r1);
        assert_eq!(fill_r2, fill_r1);
        assert_ne!(path_id_2, path_id_1);

        h.editor.undo();
        h.editor.undo();
        assert_eq!(h.editor.project, before);

        // 2. Library import (`FigmaImportTarget::Library`, `Create MovieClip symbol = false`) with Page 1 + Page 2,
        //    followed by a second deduplicated Library re-import!
        h.editor.open_figma_import_dialog(
            &h.ctx,
            vec![fig_path.clone()],
            FigmaImportTarget::Library,
        );
        h.settle();
        h.click("Gallery");
        h.click("Create MovieClip symbol");
        h.click("Import");
        h.wait_until_closed();

        h.editor
            .project
            .validate()
            .expect("Library unwrapped multi-page bitmap + bitmap_fill import validates");
        assert_eq!(h.editor.project.resources.len(), 2);
        let gallery_sym_1 = h
            .editor
            .project
            .symbols
            .iter()
            .find(|s| s.name == "Gallery")
            .expect("Gallery symbol imported into Library");
        let (lib_bmp_1, lib_fill_1, _) = assert_gallery_keyframe_has_valid_bitmap_and_fill(
            &h.editor.project,
            &gallery_sym_1.timeline.layers[0].keyframes[0],
        );

        // Second Library import (`Create MovieClip symbol = false`) -> deduplicates resources & remaps Page 2!
        h.editor.open_figma_import_dialog(
            &h.ctx,
            vec![fig_path.clone()],
            FigmaImportTarget::Library,
        );
        h.settle();
        h.click("Gallery");
        h.click("Create MovieClip symbol");
        h.click("Import");
        h.wait_until_closed();

        h.editor
            .project
            .validate()
            .expect("Library deduplicated re-import validates");
        assert_eq!(h.editor.project.resources.len(), 2);
        let gallery_sym_2 = h
            .editor
            .project
            .symbols
            .iter()
            .find(|s| s.name == "Gallery 2")
            .expect("Gallery 2 symbol imported into Library");
        let (lib_bmp_2, lib_fill_2, _) = assert_gallery_keyframe_has_valid_bitmap_and_fill(
            &h.editor.project,
            &gallery_sym_2.timeline.layers[0].keyframes[0],
        );
        assert_eq!(lib_bmp_2, lib_bmp_1);
        assert_eq!(lib_fill_2, lib_fill_1);

        h.editor.undo();
        h.editor.undo();
        assert_eq!(h.editor.project, before);

        let _ = std::fs::remove_dir_all(&temp);
    }
}
