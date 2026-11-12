//! Virtualized Output panel helpers (`T-227`): keeps rendering `O(visible_rows)` when the log buffer
//! grows to thousands of trace/compiler/export lines.
#![allow(dead_code)]
use crate::app::Editor;
use crate::theme::{DesignUi as _, ScrollTheme as _};
use eframe::egui::{self, Sense, Vec2};
use neoflash_ui::components::{self as nf, code::Severity};
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, radius, space};

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

#[derive(Clone, Default)]
struct CachedOutputLines {
    entry_count: usize,
    first_entry: Option<String>,
    last_entry: Option<String>,
    filter_lower: String,
    visible_lines: Vec<String>,
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

/// Render `visible_lines` into the current vertical `ScrollArea` using exact top/bottom spacer
/// rects once `visible_lines.len() > OUTPUT_VIRTUALIZATION_THRESHOLD`.
pub fn show_virtualized_output_rows(
    ui: &mut egui::Ui,
    visible_lines: &[String],
) -> OutputVirtualStats {
    let total_lines = visible_lines.len();
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
    for line in &visible_lines[range.clone()] {
        ui.add(nf::LogLine {
            severity: Severity::Info,
            time: "",
            text: line,
        });
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
    stats
}

impl Editor {
    /// Return the most recent `OutputVirtualStats` recorded by the Output panel.
    pub fn last_output_virtual_stats(&self, ctx: &egui::Context) -> Option<OutputVirtualStats> {
        ctx.data(|d| d.get_temp(egui::Id::new("neoflash-output-virtual-stats")))
    }

    /// Virtualized Output panel (`T-227`): caches filtered line expansion across unchanged frames
    /// and renders only the visible `LogLine` slice inside the scroll viewport.
    pub fn output_panel_virtualized(&mut self, ui: &mut egui::Ui) {
        let filter_id = egui::Id::new(("neoflash-output-filter", &self.clipboard.session));
        let cache_id = egui::Id::new(("neoflash-output-lines-cache", &self.clipboard.session));
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();

        let mut clear_clicked = false;
        let mut copy_clicked = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let search = (ui.available_width()
                - 2. * (neoflash_ui::metrics::control() + space::S2))
                .max(80.);
            let search_resp = ui.add(nf::Search::new(&mut filter, "Filter output").width(search));
            if search_resp.changed() {
                ui.ctx().request_repaint();
            }
            let copy_sc = self.tooltip_sc(crate::commands::CommandId::EditCopy);
            let mut copy_btn = nf::IconButton::new(i16::COPY, "Copy output");
            if let Some(sc) = copy_sc.as_deref() {
                copy_btn = copy_btn.shortcut(sc);
            }
            let copy = ui.add_enabled_semantic(!self.output.is_empty(), copy_btn);
            if copy.clicked() {
                copy_clicked = true;
            }
            let clear = ui.add_enabled_semantic(
                !self.output.is_empty(),
                nf::IconButton::new(i16::TRASH, "Clear output"),
            );
            if clear.clicked() {
                clear_clicked = true;
            }
        });
        if clear_clicked {
            self.output.clear();
            ui.data_mut(|d| d.remove::<CachedOutputLines>(cache_id));
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

        let needle = filter.trim().to_lowercase();
        let first_entry = self.output.first().cloned();
        let last_entry = self.output.last().cloned();
        let entry_count = self.output.len();
        let cached: Option<CachedOutputLines> = ui.data(|d| d.get_temp(cache_id));
        let visible_lines = match cached {
            Some(c)
                if c.entry_count == entry_count
                    && c.first_entry == first_entry
                    && c.last_entry == last_entry
                    && c.filter_lower == needle =>
            {
                c.visible_lines
            }
            _ => {
                let lines: Vec<String> = self
                    .output
                    .iter()
                    .flat_map(|entry| {
                        if entry.is_empty() {
                            vec![String::new()]
                        } else {
                            entry.lines().map(str::to_owned).collect::<Vec<_>>()
                        }
                    })
                    .filter(|line| needle.is_empty() || line.to_lowercase().contains(&needle))
                    .collect();
                ui.data_mut(|d| {
                    d.insert_temp(
                        cache_id,
                        CachedOutputLines {
                            entry_count,
                            first_entry,
                            last_entry,
                            filter_lower: needle.clone(),
                            visible_lines: lines.clone(),
                        },
                    );
                });
                lines
            }
        };

        if copy_clicked && !visible_lines.is_empty() {
            ui.ctx().copy_text(visible_lines.join("\n"));
        }

        let needle_active = !needle.is_empty();
        let p = neoflash_ui::palette(ui.ctx());
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
                        show_virtualized_output_rows(ui, &visible_lines);
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
    }
}
