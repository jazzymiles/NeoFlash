//! Publish weight (R-032, Figma `124:18239` in “Answers — Codex batch”): the stacked
//! [`WeightBar`], its wrapped [`weight_legend`] and the [`weight_totals`] row of Publish settings ▸
//! HTML5 ▸ Runtime.
//!
//! The bar shows **on-disk** bytes per part; per-part gzip is never shown (it is an attribution, not
//! additive). There is no budget marker. While an estimate runs, show `NF2/Progress` instead of the bar
//! and `—` in the totals.
use super::{galley, text_at};
use crate::theme::{cr, palette};
use crate::tokens::{Palette, TextStyle, radius, space};
use eframe::egui::{self, Color32, Rect, Response, Sense, Ui};

/// Bar height.
pub const WEIGHT_BAR: f32 = 8.0;
/// Gap between bar segments.
pub const WEIGHT_GAP: f32 = 2.0;
/// Narrowest drawn segment, so a tiny non-empty part (Page) stays visible.
pub const WEIGHT_MIN_SEGMENT: f32 = 2.0;

/// The parts of a published HTML5 bundle, in bar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeightPart {
    WebAssembly,
    RuntimeScripts,
    ProjectScripts,
    Content,
    Resources,
    Fonts,
    Page,
}

impl WeightPart {
    pub const ALL: [Self; 7] = [
        Self::WebAssembly,
        Self::RuntimeScripts,
        Self::ProjectScripts,
        Self::Content,
        Self::Resources,
        Self::Fonts,
        Self::Page,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::WebAssembly => "WebAssembly",
            Self::RuntimeScripts => "Runtime scripts",
            Self::ProjectScripts => "Project scripts",
            Self::Content => "Content",
            Self::Resources => "Resources",
            Self::Fonts => "Fonts",
            Self::Page => "Page",
        }
    }
    /// Segment / legend swatch colour (R-032 answer).
    pub fn color(self, p: &Palette) -> Color32 {
        match self {
            Self::WebAssembly => p.accent_default,
            Self::RuntimeScripts => p.syntax_keyword,
            Self::ProjectScripts => p.syntax_function,
            Self::Content => p.layer_green,
            Self::Resources => p.warning_default,
            Self::Fonts => p.syntax_type,
            Self::Page => p.text_tertiary,
        }
    }
}

/// One part of the estimate: on-disk `bytes` (segment length) and the caller-formatted `value`
/// ("1.62 MB") shown in the legend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightSegment<'a> {
    pub part: WeightPart,
    pub bytes: u64,
    pub value: &'a str,
}

/// Horizontal spans `(x0, x1)` relative to the bar start for each entry of `bytes`. Zero-byte parts
/// get an empty span and no gap; every non-empty part is at least [`WEIGHT_MIN_SEGMENT`] wide and
/// segments are separated by [`WEIGHT_GAP`]. The last non-empty span ends exactly at `width`.
pub fn weight_spans(bytes: &[u64], width: f32) -> Vec<(f32, f32)> {
    let shown: Vec<usize> = (0..bytes.len()).filter(|&i| bytes[i] > 0).collect();
    let mut spans = vec![(0.0, 0.0); bytes.len()];
    if shown.is_empty() || width <= 0.0 {
        return spans;
    }
    let gaps = WEIGHT_GAP * (shown.len() - 1) as f32;
    let room = (width - gaps).max(0.0);
    let total: f64 = shown.iter().map(|&i| bytes[i] as f64).sum();
    // Raise tiny parts to the minimum, taking the room from the others proportionally.
    let raw: Vec<f32> = shown
        .iter()
        .map(|&i| (bytes[i] as f64 / total) as f32 * room)
        .collect();
    let small: f32 = raw.iter().filter(|w| **w < WEIGHT_MIN_SEGMENT).count() as f32;
    let big_sum: f32 = raw.iter().filter(|w| **w >= WEIGHT_MIN_SEGMENT).sum();
    let scale = if big_sum > 0.0 {
        ((room - small * WEIGHT_MIN_SEGMENT) / big_sum).max(0.0)
    } else {
        0.0
    };
    let mut x = 0.0;
    for (k, &i) in shown.iter().enumerate() {
        let w = if raw[k] < WEIGHT_MIN_SEGMENT {
            WEIGHT_MIN_SEGMENT.min(room)
        } else {
            raw[k] * scale
        };
        let end = if k + 1 == shown.len() { width } else { x + w };
        spans[i] = (x, end);
        x = end + WEIGHT_GAP;
    }
    spans
}

/// Stacked weight bar: 8 pt high, 2 pt gaps, `radius/sm` at both ends of the bar.
#[must_use = "call .show(ui, width)"]
pub struct WeightBar<'a> {
    segments: &'a [WeightSegment<'a>],
}

impl<'a> WeightBar<'a> {
    pub fn new(segments: &'a [WeightSegment<'a>]) -> Self {
        Self { segments }
    }

    pub fn show(self, ui: &mut Ui, width: f32) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, WEIGHT_BAR), Sense::hover());
        let bytes: Vec<u64> = self.segments.iter().map(|s| s.bytes).collect();
        let spans = weight_spans(&bytes, width);
        let first = spans.iter().position(|s| s.1 > s.0);
        let last = spans.iter().rposition(|s| s.1 > s.0);
        if first.is_none() {
            ui.painter().rect_filled(rect, cr(radius::SM), p.bg_hover);
        }
        let r = radius::SM.min(WEIGHT_BAR / 2.0) as u8;
        for (i, (seg, (x0, x1))) in self.segments.iter().zip(spans).enumerate() {
            if x1 <= x0 {
                continue;
            }
            let span = Rect::from_min_max(
                egui::pos2(rect.left() + x0, rect.top()),
                egui::pos2(rect.left() + x1, rect.bottom()),
            );
            let corner = egui::CornerRadius {
                nw: if Some(i) == first { r } else { 0 },
                sw: if Some(i) == first { r } else { 0 },
                ne: if Some(i) == last { r } else { 0 },
                se: if Some(i) == last { r } else { 0 },
            };
            ui.painter().rect_filled(span, corner, seg.part.color(p));
        }
        let summary = self
            .segments
            .iter()
            .map(|s| format!("{} {}", s.part.label(), s.value))
            .collect::<Vec<_>>()
            .join(", ");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &summary));
        response
    }
}

/// Wrapped legend under the bar: 8 pt swatch (radius 2), `NF2/Small` label in `text/secondary` and the
/// value in `text/primary`; items 16 pt apart, rows 6 pt apart, wrapping at `width`.
pub fn weight_legend(ui: &mut Ui, segments: &[WeightSegment<'_>], width: f32) -> Response {
    let p = palette(ui.ctx());
    let items: Vec<_> = segments
        .iter()
        .map(|s| {
            let label = galley(ui, s.part.label(), TextStyle::Small, p.text_secondary);
            let value = galley(ui, s.value, TextStyle::Small, p.text_primary);
            let w = 8.0 + 6.0 + label.size().x + 6.0 + value.size().x;
            (s.part, label, value, w)
        })
        .collect();
    let row_h = 16.0;
    let mut rows = 1;
    let mut x = 0.0;
    let mut placed = Vec::with_capacity(items.len());
    for (_, _, _, w) in &items {
        if x > 0.0 && x + w > width {
            rows += 1;
            x = 0.0;
        }
        placed.push((x, rows - 1));
        x += w + space::S4;
    }
    let height = rows as f32 * row_h + (rows - 1) as f32 * 6.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        for ((part, label, value, _), (x, row)) in items.into_iter().zip(placed) {
            let top = rect.top() + row as f32 * (row_h + 6.0);
            let line =
                Rect::from_min_size(egui::pos2(rect.left() + x, top), egui::vec2(0.0, row_h));
            let swatch = Rect::from_center_size(
                egui::pos2(line.left() + 4.0, line.center().y),
                egui::vec2(8.0, 8.0),
            );
            ui.painter().rect_filled(swatch, cr(2.0), part.color(p));
            let lx = swatch.right() + 6.0;
            let lw = label.size().x;
            text_at(ui, lx, line, label, p.text_secondary);
            text_at(ui, lx + lw + 6.0, line, value, p.text_primary);
        }
    }
    response
}

/// Totals row: pairs of `text/tertiary` caption and `text/primary` value, 16 pt apart
/// (`On disk 2.47 MB · Transfer (gzip) 864 KB · Files 7`; `—` while computing).
pub fn weight_totals(ui: &mut Ui, totals: &[(&str, &str)]) -> Response {
    let p = palette(ui.ctx());
    let pairs: Vec<_> = totals
        .iter()
        .map(|(k, v)| {
            (
                galley(ui, k, TextStyle::Small, p.text_tertiary),
                galley(ui, v, TextStyle::Small, p.text_primary),
            )
        })
        .collect();
    let width: f32 = pairs
        .iter()
        .map(|(k, v)| k.size().x + 6.0 + v.size().x)
        .sum::<f32>()
        + space::S4 * pairs.len().saturating_sub(1) as f32;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 16.0), Sense::hover());
    let mut x = rect.left();
    for (k, v) in pairs {
        let (kw, vw) = (k.size().x, v.size().x);
        text_at(ui, x, rect, k, p.text_tertiary);
        text_at(ui, x + kw + 6.0, rect, v, p.text_primary);
        x += kw + 6.0 + vw + space::S4;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_fill_the_bar_with_gaps_and_keep_tiny_parts_visible() {
        // Figma ready state: 566 pt bar, seven parts, Page is tiny.
        let bytes = [1_620_000, 410_000, 38_000, 120_000, 196_000, 84_000, 6_000];
        let spans = weight_spans(&bytes, 566.0);
        assert_eq!(spans.first().unwrap().0, 0.0);
        assert_eq!(spans.last().unwrap().1, 566.0);
        for pair in spans.windows(2) {
            assert!((pair[1].0 - pair[0].1 - WEIGHT_GAP).abs() < 1e-3);
        }
        assert!(
            spans
                .iter()
                .all(|(a, b)| b - a >= WEIGHT_MIN_SEGMENT - 1e-3)
        );
        // WebAssembly dominates like the reference (363 of 566).
        let wasm = spans[0].1 - spans[0].0;
        assert!((340.0..380.0).contains(&wasm), "{wasm}");
    }

    #[test]
    fn empty_parts_take_no_room_and_no_gap() {
        let spans = weight_spans(&[100, 0, 100], 102.0);
        assert_eq!(spans[1], (0.0, 0.0));
        assert_eq!(spans[0], (0.0, 50.0));
        assert_eq!(spans[2], (52.0, 102.0));
        assert!(weight_spans(&[0, 0], 50.0).iter().all(|s| *s == (0.0, 0.0)));
    }
}
