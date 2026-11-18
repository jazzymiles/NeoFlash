//! Virtualized Output panel helpers (`T-227`, `T-310`): keeps rendering `O(visible_rows)` and cache
//! validation `O(1)` via `(instance_id, document_id, revision, filter)` when the structured log
//! buffer grows to thousands of trace/compiler/debugger/import/export events.
use crate::app::Editor;
use crate::output_pipeline::{OutputLocation, OutputPipeline, ParsedOutputSearch};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use eframe::egui::{self, Sense, Vec2};
use neoflash_ui::components::{self as nf, code::Severity};
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, radius, space};
use std::sync::Arc;

/// Line count above which Output switches to viewport row virtualization (`T-227`).
pub const OUTPUT_VIRTUALIZATION_THRESHOLD: usize = 64;
/// Overscan rows above and below the visible Output viewport.
pub const OUTPUT_OVERSCAN_ROWS: usize = 8;

/// Deterministic virtualization statistics recorded on each Output paint pass (`T-227`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputVirtualStats {
    pub total_lines: usize,
    pub first_row: usize,
    pub last_row: usize,
    pub drawn_rows: usize,
}

/// Pre-expanded logical Output row cached across unchanged frames (`T-310`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedOutputRow {
    pub severity: Severity,
    pub text: String,
    pub location: Option<OutputLocation>,
}

#[derive(Clone, Default)]
struct CachedOutputLines {
    pipeline_instance_id: u64,
    document_id: String,
    pipeline_revision: u64,
    filter_key: String,
    build_generation: u64,
    rows: Vec<CachedOutputRow>,
    visible_lines: Vec<String>,
}

/// Drop the cached expanded Output row vector for `session` when `Output` is cleared.
pub(crate) fn invalidate_output_panel_cache(ctx: &egui::Context, session: &str) {
    let cache_id = egui::Id::new(("neoflash-output-lines-cache", session));
    ctx.data_mut(|d| d.remove::<Arc<CachedOutputLines>>(cache_id));
}

fn cached_output_lines(
    ctx: &egui::Context,
    cache_id: egui::Id,
    output: &OutputPipeline,
    filter: &str,
) -> Arc<CachedOutputLines> {
    let filter_trimmed = filter.trim();
    let cached = ctx.data(|data| data.get_temp::<Arc<CachedOutputLines>>(cache_id));
    let previous_generation = match cached {
        Some(cached)
            if cached.pipeline_instance_id == output.instance_id()
                && cached.document_id == output.document_id()
                && cached.pipeline_revision == output.revision()
                && cached.filter_key == filter_trimmed =>
        {
            return cached;
        }
        Some(cached) => cached.build_generation,
        None => 0,
    };

    let parsed = ParsedOutputSearch::parse(filter_trimmed);
    let mut rows = Vec::new();
    let mut visible_lines = Vec::new();
    for event in output.events() {
        if !parsed.matches_event_metadata(event) {
            continue;
        }
        let loc_summary = if parsed.text_needle_lower.is_empty() {
            String::new()
        } else {
            event
                .location
                .as_ref()
                .map(OutputLocation::search_summary_lower)
                .unwrap_or_default()
        };
        let ui_severity = event.severity.ui_severity();
        let raw_lines: Vec<&str> = if event.message.is_empty() {
            vec![""]
        } else {
            let collected: Vec<&str> = event.message.lines().collect();
            if collected.is_empty() {
                vec![""]
            } else {
                collected
            }
        };
        let event_full_text = if event.repeat_count > 1 {
            format!("{} (×{})", event.message, event.repeat_count)
        } else {
            event.message.clone()
        };
        if !parsed.matches_line(&event_full_text, &loc_summary) {
            continue;
        }
        let last_idx = raw_lines.len().saturating_sub(1);
        for (idx, raw_line) in raw_lines.into_iter().enumerate() {
            let text = if idx == last_idx && event.repeat_count > 1 {
                format!("{raw_line} (×{})", event.repeat_count)
            } else {
                raw_line.to_owned()
            };
            visible_lines.push(text.clone());
            rows.push(CachedOutputRow {
                severity: ui_severity,
                text,
                location: event.location.clone(),
            });
        }
    }

    let cached = Arc::new(CachedOutputLines {
        pipeline_instance_id: output.instance_id(),
        document_id: output.document_id().to_owned(),
        pipeline_revision: output.revision(),
        filter_key: filter_trimmed.to_owned(),
        build_generation: previous_generation.wrapping_add(1),
        rows,
        visible_lines,
    });
    ctx.data_mut(|data| data.insert_temp(cache_id, Arc::clone(&cached)));
    cached
}

/// Nominal height of a single-line `NF2/LogLine` row (`Mono` line height + `S1` vertical padding).
pub fn output_row_height() -> f32 {
    TextStyle::Mono.spec().line_height + space::S1
}

/// Compute the visible Output row index range (`start..end`) inside `clip_top..=clip_bottom`
/// with `OUTPUT_OVERSCAN_ROWS` overscan margin.
pub fn visible_output_row_range(
    clip_top: f32,
    clip_bottom: f32,
    list_top: f32,
    row_height: f32,
    total_rows: usize,
) -> std::ops::Range<usize> {
    if total_rows == 0 || row_height <= 0.0 || !clip_top.is_finite() || !clip_bottom.is_finite() {
        return 0..total_rows;
    }
    let first = ((clip_top - list_top) / row_height).floor().max(0.0) as usize;
    let last = ((clip_bottom - list_top) / row_height).ceil().max(0.0) as usize;
    let start = first.saturating_sub(OUTPUT_OVERSCAN_ROWS).min(total_rows);
    let end = (last + OUTPUT_OVERSCAN_ROWS).min(total_rows).max(start);
    start..end
}

fn show_virtualized_cached_rows(
    ui: &mut egui::Ui,
    rows: &[CachedOutputRow],
) -> (OutputVirtualStats, Option<OutputLocation>) {
    let total_lines = rows.len();
    let row_h = output_row_height();
    let list_top = ui.cursor().top();
    let clip = ui.clip_rect();
    let range = if total_lines > OUTPUT_VIRTUALIZATION_THRESHOLD {
        visible_output_row_range(clip.top(), clip.bottom(), list_top, row_h, total_lines)
    } else {
        0..total_lines
    };
    let width = ui.available_width();
    if range.start > 0 {
        ui.allocate_exact_size(Vec2::new(width, range.start as f32 * row_h), Sense::hover());
    }
    let mut drawn_rows = 0usize;
    let mut clicked_location = None;
    for (offset, row) in rows[range.clone()].iter().enumerate() {
        let row_index = range.start + offset;
        let resp = ui.add(nf::LogLine {
            severity: row.severity,
            time: "",
            text: &row.text,
        });
        if let Some(loc) = row.location.as_ref().filter(|loc| loc.is_navigable())
            && ui
                .interact(
                    resp.rect,
                    ui.id().with(("output-row-jump", row_index)),
                    Sense::click(),
                )
                .clicked()
        {
            clicked_location = Some(loc.clone());
        }
        drawn_rows += 1;
    }
    if range.end < total_lines {
        let remaining = total_lines - range.end;
        ui.allocate_exact_size(Vec2::new(width, remaining as f32 * row_h), Sense::hover());
    }
    let stats = OutputVirtualStats {
        total_lines,
        first_row: range.start,
        last_row: range.end.saturating_sub(1),
        drawn_rows,
    };
    ui.ctx().data_mut(|d| {
        d.insert_temp(egui::Id::new("neoflash-output-virtual-stats"), stats);
    });
    (stats, clicked_location)
}

/// Render `visible_lines` into the current vertical `ScrollArea` using exact top/bottom spacer
/// rects once `visible_lines.len() > OUTPUT_VIRTUALIZATION_THRESHOLD`.
#[allow(dead_code)]
pub fn show_virtualized_output_rows(
    ui: &mut egui::Ui,
    visible_lines: &[String],
) -> OutputVirtualStats {
    let rows: Vec<CachedOutputRow> = visible_lines
        .iter()
        .map(|line| CachedOutputRow {
            severity: Severity::Info,
            text: line.clone(),
            location: None,
        })
        .collect();
    show_virtualized_cached_rows(ui, &rows).0
}

impl Editor {
    /// Return the most recent `OutputVirtualStats` recorded by the Output panel.
    #[cfg(test)]
    pub fn last_output_virtual_stats(&self, ctx: &egui::Context) -> Option<OutputVirtualStats> {
        ctx.data(|d| d.get_temp(egui::Id::new("neoflash-output-virtual-stats")))
    }

    /// Return the build generation of the active session's cached Output expansion (`O(1)` cache test hook).
    #[cfg(test)]
    pub fn last_output_cache_generation(&self, ctx: &egui::Context) -> Option<u64> {
        let cache_id = egui::Id::new(("neoflash-output-lines-cache", &self.clipboard.session));
        ctx.data(|d| {
            d.get_temp::<Arc<CachedOutputLines>>(cache_id)
                .map(|c| c.build_generation)
        })
    }

    /// Output: filter, copy and clear, then the log in `NF2/Mono`. Filtered line expansion is cached
    /// in `O(1)` via `(instance_id, document_id, revision, filter)` across unchanged frames and only
    /// the visible `LogLine` slice is painted inside the viewport.
    pub(crate) fn output_panel(&mut self, ui: &mut egui::Ui) {
        self.sync_output_document_id();
        let filter_id = egui::Id::new(("neoflash-output-filter", &self.clipboard.session));
        let cache_id = egui::Id::new(("neoflash-output-lines-cache", &self.clipboard.session));
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();

        let mut clear_clicked = false;
        let mut visible_cache: Option<Arc<CachedOutputLines>> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let search = (ui.available_width()
                - 2. * (neoflash_ui::metrics::control() + space::S2))
                .max(80.);
            let search_resp = ui.add(nf::Search::new(&mut filter, "Filter output").width(search));
            if search_resp.changed() {
                ui.ctx().request_repaint();
            }
            let cache = cached_output_lines(ui.ctx(), cache_id, &self.output, &filter);
            let copy_sc = self.tooltip_sc(crate::commands::CommandId::EditCopy);
            let mut copy_btn = nf::IconButton::new(i16::COPY, "Copy output");
            if let Some(sc) = copy_sc.as_deref() {
                copy_btn = copy_btn.shortcut(sc);
            }
            let copy = ui.add_enabled_semantic(!cache.visible_lines.is_empty(), copy_btn);
            if copy.clicked() {
                ui.ctx().copy_text(cache.visible_lines.join("\n"));
            }
            let clear = ui.add_enabled_semantic(
                !self.output.is_empty(),
                nf::IconButton::new(i16::TRASH, "Clear output"),
            );
            if clear.clicked() {
                clear_clicked = true;
            }
            visible_cache = Some(cache);
        });
        if clear_clicked {
            let ctx = ui.ctx().clone();
            self.clear_output(Some(&ctx));
        }
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        ui.add_space(space::S2);
        if self.output.is_empty() {
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("neoflash-output-virtual-stats"),
                    OutputVirtualStats::default(),
                );
            });
            nf::EmptyState {
                icon: i16::TERMINAL,
                title: "No output yet",
                body: "Status messages, script checks and export results appear here.",
                action: None,
            }
            .show(ui);
            return;
        }
        let visible_cache = visible_cache
            .unwrap_or_else(|| cached_output_lines(ui.ctx(), cache_id, &self.output, &filter));
        let rows = &visible_cache.rows;
        let visible_lines = &visible_cache.visible_lines;
        let needle_active = !visible_cache.filter_key.is_empty();
        let p = neoflash_ui::palette(ui.ctx());
        let mut clicked_location = None;
        egui::Frame::new()
            .fill(p.bg_field)
            .corner_radius(radius::MD)
            .inner_margin(egui::Margin::same(space::S2 as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt(("output_contents", &self.clipboard.session))
                    .stick_to_bottom(!needle_active)
                    .themed()
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        let (_, jumped) = show_virtualized_cached_rows(ui, rows);
                        clicked_location = jumped;
                        if visible_lines.is_empty() {
                            ui.label(
                                neoflash_ui::typography::rich(
                                    format!("No output matches “{}”.", filter.trim()),
                                    TextStyle::Small,
                                )
                                .color(p.text_tertiary),
                            );
                        }
                    });
            });
        if let Some(loc) = clicked_location {
            let ctx = ui.ctx().clone();
            self.navigate_to_output_location(&ctx, &loc);
        }
    }

    /// Compatibility entry point for focused virtualization tests. Production dock routing calls
    /// [`Self::output_panel`] directly, so there is only one implementation.
    #[cfg(test)]
    pub fn output_panel_virtualized(&mut self, ui: &mut egui::Ui) {
        self.output_panel(ui);
    }
}
