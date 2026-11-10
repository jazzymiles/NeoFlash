//! Selection-aware Stage text editing (`T-202`): UTF-16 caret placement, drag range selection,
//! double-click word selection, keyboard/IME editing with coalesced Undo, per-range `TextFormatRun`
//! Inspector routing, and transformed/nested (`Edit in Place`) Stage overlay rendering.

use crate::{
    app::{Editor, Tool},
    selection::SelectionTarget,
    stage_mapping::StageMapping,
};
use eframe::egui::{
    self, Color32, FontFamily, FontId, Key, Modifiers, Pos2, Rect, Shape, Stroke, Vec2,
};
use neoflash_core::{
    AuthoredTextFieldOptions, DrawItem, ElementKind, Id, LayerRole, TextFieldState, TextFormat,
    TextFormatPatch, TextPatch, layer_effectively_locked, layer_effectively_visible, layout_text,
    sample_layer_in_at_fps, transform_matrix,
};

/// Preserve point-text RGBA opacity when rich formatting upgrades its storage to `TextBox`.
/// `TextBox` carries RGB in `TextFormat`, so the former color alpha belongs on the element.
pub(crate) fn preserve_point_text_alpha_on_upgrade(
    before: &ElementKind,
    after: &ElementKind,
    element_alpha: &mut f64,
) {
    if let (ElementKind::Text { color, .. }, ElementKind::TextBox { .. }) = (before, after) {
        *element_alpha *= f64::from(color[3]) / 255.0;
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StageTextEditState {
    pub(crate) active_target: Option<SelectionTarget>,
    pub(crate) symbol_id: Id,
    pub(crate) frame: u32,
    pub(crate) caret_utf16: usize,
    pub(crate) selection_anchor_utf16: Option<usize>,
    pub(crate) typing_format: Option<TextFormat>,
    pub(crate) pointer_dragging: bool,
    pub(crate) drag_press_pos: Option<Pos2>,
    pub(crate) coalesced_typing_active: bool,
    pub(crate) last_click_time: f64,
    pub(crate) last_click_pos: Option<Pos2>,
    pub(crate) preferred_x: Option<f64>,
}

impl StageTextEditState {
    pub(crate) fn is_editing_element(&self, element_id: &str) -> bool {
        self.active_target
            .as_ref()
            .is_some_and(|target| target.element == element_id)
    }

    pub(crate) fn is_active(&self, editor: &Editor) -> bool {
        let Some(target) = &self.active_target else {
            return false;
        };
        self.symbol_id == editor.symbol_id()
            && self.frame == editor.frame
            && editor.selection.single() == Some(target)
            && editor
                .selected_sampled()
                .is_some_and(|el| el.kind.is_text())
    }

    pub(crate) fn selection_range_utf16(&self) -> (usize, usize) {
        let anchor = self.selection_anchor_utf16.unwrap_or(self.caret_utf16);
        (anchor.min(self.caret_utf16), anchor.max(self.caret_utf16))
    }

    pub(crate) fn has_range_selection(&self) -> bool {
        let (lo, hi) = self.selection_range_utf16();
        lo < hi
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StageTextCharBox {
    pub(crate) start_utf16: usize,
    pub(crate) end_utf16: usize,
    pub(crate) x: f64,
    pub(crate) width: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct StageTextLineBox {
    pub(crate) start_utf16: usize,
    pub(crate) end_utf16: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) height: f64,
    pub(crate) chars: Vec<StageTextCharBox>,
}

#[derive(Clone, Debug)]
pub(crate) struct StageTextGeometry {
    pub(crate) lines: Vec<StageTextLineBox>,
    pub(crate) total_utf16: usize,
    pub(crate) box_width: f64,
    pub(crate) box_height: f64,
    #[cfg(test)]
    pub(crate) format_run_lookups: usize,
    #[cfg(test)]
    pub(crate) format_run_advances: usize,
}

impl StageTextGeometry {
    /// Map a point in the text element's local coordinate space to the nearest UTF-16 boundary.
    pub(crate) fn hit_test_utf16(&self, local_pt: Pos2) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let lx = f64::from(local_pt.x);
        let ly = f64::from(local_pt.y);
        let line = self
            .lines
            .iter()
            .find(|line| ly >= line.y && ly <= line.y + line.height.max(1.0))
            .unwrap_or_else(|| {
                self.lines
                    .iter()
                    .min_by(|a, b| {
                        let da = if ly < a.y {
                            a.y - ly
                        } else {
                            (ly - (a.y + a.height)).max(0.0)
                        };
                        let db = if ly < b.y {
                            b.y - ly
                        } else {
                            (ly - (b.y + b.height)).max(0.0)
                        };
                        da.total_cmp(&db)
                    })
                    .expect("non-empty lines")
            });
        Self::hit_test_line_x(line, lx)
    }

    fn hit_test_line_x(line: &StageTextLineBox, lx: f64) -> usize {
        if line.chars.is_empty() {
            return line.start_utf16;
        }
        for ch in &line.chars {
            let mid = ch.x + ch.width * 0.5;
            if lx <= mid {
                return ch.start_utf16;
            }
            if lx <= ch.x + ch.width {
                return ch.end_utf16;
            }
        }
        line.end_utf16
    }

    pub(crate) fn line_index_for_utf16(&self, offset_utf16: usize) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        for (idx, line) in self.lines.iter().enumerate() {
            if offset_utf16 >= line.start_utf16 && offset_utf16 <= line.end_utf16 {
                return idx;
            }
        }
        self.lines.len() - 1
    }

    /// Return `(top, bottom)` of the caret in element-local coordinates for `caret_utf16`.
    pub(crate) fn caret_segment_local(&self, caret_utf16: usize) -> (Pos2, Pos2) {
        if self.lines.is_empty() {
            return (Pos2::ZERO, Pos2::new(0.0, 16.0));
        }
        let clamped = caret_utf16.min(self.total_utf16);
        let line_idx = self.line_index_for_utf16(clamped);
        let line = &self.lines[line_idx];
        let x = if let Some(ch) = line.chars.iter().find(|c| c.start_utf16 == clamped) {
            ch.x
        } else if let Some(ch) = line.chars.iter().rev().find(|c| c.end_utf16 <= clamped) {
            ch.x + ch.width
        } else {
            line.x
        };
        let top = Pos2::new(x as f32, line.y as f32);
        let bottom = Pos2::new(x as f32, (line.y + line.height.max(12.0)) as f32);
        (top, bottom)
    }

    /// Return per-line highlight rectangles in element-local coordinates for `[start_utf16, end_utf16)`.
    pub(crate) fn selection_rects_local(&self, start_utf16: usize, end_utf16: usize) -> Vec<Rect> {
        let lo = start_utf16.min(end_utf16);
        let hi = start_utf16.max(end_utf16);
        if lo >= hi {
            return Vec::new();
        }
        let mut rects = Vec::new();
        for line in &self.lines {
            if line.end_utf16 < lo || line.start_utf16 > hi {
                continue;
            }
            let matching: Vec<&StageTextCharBox> = line
                .chars
                .iter()
                .filter(|c| c.end_utf16 > lo && c.start_utf16 < hi)
                .collect();
            if let (Some(first), Some(last)) = (matching.first(), matching.last()) {
                let min_x = first.x as f32;
                let max_x = (last.x + last.width).max(first.x + 2.0) as f32;
                let min_y = line.y as f32;
                let max_y = (line.y + line.height.max(12.0)) as f32;
                rects.push(Rect::from_min_max(
                    Pos2::new(min_x, min_y),
                    Pos2::new(max_x, max_y),
                ));
            } else if line.chars.is_empty() && line.start_utf16 >= lo && line.end_utf16 <= hi {
                let min_x = line.x as f32;
                let min_y = line.y as f32;
                let max_y = (line.y + line.height.max(12.0)) as f32;
                rects.push(Rect::from_min_max(
                    Pos2::new(min_x, min_y),
                    Pos2::new(min_x + 4.0, max_y),
                ));
            }
        }
        rects
    }

    pub(crate) fn line_home_end_utf16(&self, caret_utf16: usize) -> (usize, usize) {
        if self.lines.is_empty() {
            return (0, 0);
        }
        let idx = self.line_index_for_utf16(caret_utf16.min(self.total_utf16));
        let line = &self.lines[idx];
        (line.start_utf16, line.end_utf16)
    }

    pub(crate) fn move_vertical_utf16(
        &self,
        caret_utf16: usize,
        delta_lines: isize,
        preferred_x: &mut Option<f64>,
    ) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let clamped = caret_utf16.min(self.total_utf16);
        let cur_line_idx = self.line_index_for_utf16(clamped);
        let (cur_top, _) = self.caret_segment_local(clamped);
        let target_x = *preferred_x.get_or_insert(f64::from(cur_top.x));
        let next_idx =
            (cur_line_idx as isize + delta_lines).clamp(0, self.lines.len() as isize - 1) as usize;
        if next_idx == cur_line_idx {
            if delta_lines < 0 {
                return 0;
            }
            return self.total_utf16;
        }
        Self::hit_test_line_x(&self.lines[next_idx], target_x)
    }
}

pub(crate) fn prev_char_utf16(text: &str, offset_utf16: usize) -> usize {
    let mut prev = 0usize;
    let mut cur = 0usize;
    for ch in text.chars() {
        let next = cur + ch.len_utf16();
        if next >= offset_utf16 {
            return cur;
        }
        prev = cur;
        cur = next;
    }
    if offset_utf16 > cur { cur } else { prev }
}

pub(crate) fn next_char_utf16(text: &str, offset_utf16: usize) -> usize {
    let mut cur = 0usize;
    for ch in text.chars() {
        let next = cur + ch.len_utf16();
        if cur >= offset_utf16 {
            return next;
        }
        cur = next;
    }
    cur
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub(crate) fn prev_word_utf16(text: &str, offset_utf16: usize) -> usize {
    let mut chars: Vec<(usize, usize, char)> = Vec::new();
    let mut cur = 0usize;
    for ch in text.chars() {
        let next = cur + ch.len_utf16();
        if cur >= offset_utf16 {
            break;
        }
        chars.push((cur, next, ch));
        cur = next;
    }
    let mut idx = chars.len();
    while idx > 0 && chars[idx - 1].2.is_whitespace() {
        idx -= 1;
    }
    if idx == 0 {
        return 0;
    }
    let word_kind = is_word_char(chars[idx - 1].2);
    while idx > 0
        && !chars[idx - 1].2.is_whitespace()
        && is_word_char(chars[idx - 1].2) == word_kind
    {
        idx -= 1;
    }
    chars.get(idx).map_or(0, |(start, _, _)| *start)
}

pub(crate) fn next_word_utf16(text: &str, offset_utf16: usize) -> usize {
    let mut chars: Vec<(usize, usize, char)> = Vec::new();
    let mut cur = 0usize;
    for ch in text.chars() {
        let next = cur + ch.len_utf16();
        if next > offset_utf16 {
            chars.push((cur, next, ch));
        }
        cur = next;
    }
    let total = cur;
    let mut idx = 0usize;
    if idx < chars.len() && !chars[idx].2.is_whitespace() {
        let word_kind = is_word_char(chars[idx].2);
        while idx < chars.len()
            && !chars[idx].2.is_whitespace()
            && is_word_char(chars[idx].2) == word_kind
        {
            idx += 1;
        }
    } else {
        while idx < chars.len() && chars[idx].2.is_whitespace() {
            idx += 1;
        }
        if idx < chars.len() {
            let word_kind = is_word_char(chars[idx].2);
            while idx < chars.len()
                && !chars[idx].2.is_whitespace()
                && is_word_char(chars[idx].2) == word_kind
            {
                idx += 1;
            }
        }
    }
    if idx == 0 { total } else { chars[idx - 1].1 }
}

pub(crate) fn word_utf16_bounds_at(text: &str, offset_utf16: usize) -> (usize, usize) {
    let mut chars: Vec<(usize, usize, char)> = Vec::new();
    let mut cur = 0usize;
    for ch in text.chars() {
        let next = cur + ch.len_utf16();
        chars.push((cur, next, ch));
        cur = next;
    }
    if chars.is_empty() {
        return (0, 0);
    }
    let pos = chars
        .iter()
        .position(|(start, end, _)| offset_utf16 >= *start && offset_utf16 < *end)
        .unwrap_or(chars.len() - 1);
    let ch = chars[pos].2;
    if ch == '\n' {
        return (chars[pos].0, chars[pos].1);
    }
    let is_ws = ch.is_whitespace();
    let is_wc = is_word_char(ch);
    let matches_group = |c: char| {
        if c == '\n' {
            false
        } else if is_ws {
            c.is_whitespace()
        } else {
            !c.is_whitespace() && is_word_char(c) == is_wc
        }
    };
    let mut start_idx = pos;
    while start_idx > 0 && matches_group(chars[start_idx - 1].2) {
        start_idx -= 1;
    }
    let mut end_idx = pos;
    while end_idx + 1 < chars.len() && matches_group(chars[end_idx + 1].2) {
        end_idx += 1;
    }
    (chars[start_idx].0, chars[end_idx].1)
}

pub(crate) fn utf16_slice(text: &str, start_utf16: usize, end_utf16: usize) -> &str {
    let lo = start_utf16.min(end_utf16);
    let hi = start_utf16.max(end_utf16);
    let mut byte_start = text.len();
    let mut byte_end = text.len();
    let mut cur = 0usize;
    let mut found_start = false;
    for (byte_idx, ch) in text.char_indices() {
        if !found_start && cur >= lo {
            byte_start = byte_idx;
            found_start = true;
        }
        if cur >= hi {
            byte_end = byte_idx;
            return &text[byte_start..byte_end];
        }
        cur += ch.len_utf16();
    }
    if !found_start {
        byte_start = text.len();
    }
    &text[byte_start..byte_end]
}

/// Compute the minimal UTF-16 range `[start_utf16, end_utf16)` in `before` that was replaced by
/// `inserted` in `after`.
pub(crate) fn diff_utf16_replacement<'a>(before: &str, after: &'a str) -> (usize, usize, &'a str) {
    let before_chars: Vec<(usize, char)> = before.char_indices().collect();
    let after_chars: Vec<(usize, char)> = after.char_indices().collect();
    let mut prefix_len = 0usize;
    while prefix_len < before_chars.len()
        && prefix_len < after_chars.len()
        && before_chars[prefix_len].1 == after_chars[prefix_len].1
    {
        prefix_len += 1;
    }
    let mut suffix_len = 0usize;
    while suffix_len < (before_chars.len() - prefix_len)
        && suffix_len < (after_chars.len() - prefix_len)
        && before_chars[before_chars.len() - 1 - suffix_len].1
            == after_chars[after_chars.len() - 1 - suffix_len].1
    {
        suffix_len += 1;
    }
    let start_utf16: usize = before_chars[..prefix_len]
        .iter()
        .map(|(_, ch)| ch.len_utf16())
        .sum();
    let end_utf16: usize = before_chars[..before_chars.len() - suffix_len]
        .iter()
        .map(|(_, ch)| ch.len_utf16())
        .sum();
    let after_byte_start = after_chars.get(prefix_len).map_or(after.len(), |(b, _)| *b);
    let after_byte_end = after_chars
        .get(after_chars.len() - suffix_len)
        .map_or(after.len(), |(b, _)| *b);
    (
        start_utf16,
        end_utf16,
        &after[after_byte_start..after_byte_end],
    )
}

impl Editor {
    pub(crate) fn enter_stage_text_edit(
        &mut self,
        target: SelectionTarget,
        caret_utf16: usize,
        anchor_utf16: Option<usize>,
    ) {
        self.finish_edits();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        if self.selection.single() != Some(&target) {
            self.set_selection(vec![target.clone()]);
        }
        let max_u16 = self
            .selected_sampled()
            .and_then(|e| e.kind.text_utf16_len())
            .unwrap_or(0);
        let clamped_caret = caret_utf16.min(max_u16);
        let clamped_anchor = anchor_utf16
            .map(|a| a.min(max_u16))
            .filter(|&a| a != clamped_caret);
        self.stage_text_edit.active_target = Some(target);
        self.stage_text_edit.symbol_id = sym_id;
        self.stage_text_edit.frame = frame;
        self.stage_text_edit.caret_utf16 = clamped_caret;
        self.stage_text_edit.selection_anchor_utf16 = clamped_anchor;
        self.stage_text_edit.typing_format = None;
        self.stage_text_edit.pointer_dragging = false;
        self.stage_text_edit.drag_press_pos = None;
        self.stage_text_edit.coalesced_typing_active = false;
        self.stage_text_edit.preferred_x = None;
    }

    pub(crate) fn exit_stage_text_edit(&mut self) {
        self.stage_text_edit.active_target = None;
        self.stage_text_edit.selection_anchor_utf16 = None;
        self.stage_text_edit.typing_format = None;
        self.stage_text_edit.pointer_dragging = false;
        self.stage_text_edit.drag_press_pos = None;
        self.stage_text_edit.coalesced_typing_active = false;
        self.stage_text_edit.preferred_x = None;
    }

    pub(crate) fn finish_stage_text_typing(&mut self) {
        self.stage_text_edit.coalesced_typing_active = false;
        self.stage_text_edit.pointer_dragging = false;
        self.stage_text_edit.drag_press_pos = None;
    }

    pub(crate) fn sync_stage_text_edit(&mut self) {
        if !self.stage_text_edit.is_active(self) {
            if self.stage_text_edit.active_target.is_some() {
                self.exit_stage_text_edit();
            }
            return;
        }
        let max_u16 = self
            .selected_sampled()
            .and_then(|e| e.kind.text_utf16_len())
            .unwrap_or(0);
        self.stage_text_edit.caret_utf16 = self.stage_text_edit.caret_utf16.min(max_u16);
        self.stage_text_edit.selection_anchor_utf16 = self
            .stage_text_edit
            .selection_anchor_utf16
            .map(|a| a.min(max_u16))
            .filter(|&a| a != self.stage_text_edit.caret_utf16);
    }

    /// Select a UTF-16 character range `[start_utf16, end_utf16)` on the currently selected text element,
    /// entering Stage text editing if not already active.
    #[allow(dead_code)]
    pub fn select_stage_text_utf16_range(&mut self, start_utf16: usize, end_utf16: usize) -> bool {
        let Some(target) = self.selection.single().cloned() else {
            return false;
        };
        if !self.selected_sampled().is_some_and(|el| el.kind.is_text()) {
            return false;
        }
        self.enter_stage_text_edit(target, end_utf16, Some(start_utf16));
        true
    }

    /// Place a collapsed caret at `caret_utf16` on the currently selected text element,
    /// entering Stage text editing if not already active.
    #[allow(dead_code)]
    pub fn place_stage_text_caret_utf16(&mut self, caret_utf16: usize) -> bool {
        let Some(target) = self.selection.single().cloned() else {
            return false;
        };
        if !self.selected_sampled().is_some_and(|el| el.kind.is_text()) {
            return false;
        }
        self.enter_stage_text_edit(target, caret_utf16, None);
        true
    }

    pub(crate) fn stage_text_geometry(
        &mut self,
        painter: Option<&egui::Painter>,
        kind: &ElementKind,
    ) -> Option<StageTextGeometry> {
        let state = TextFieldState::from_kind(kind)?;
        let total_utf16 = state.utf16_len();
        let layout = if let Some(p) = painter {
            layout_text(&state, |segments, seg_format| {
                let named = crate::fonts::styled_family(
                    &seg_format.font,
                    seg_format.bold,
                    seg_format.italic,
                );
                let family = if p.ctx().fonts(|f| f.families().contains(&named)) {
                    named
                } else {
                    FontFamily::Proportional
                };
                Ok(segments
                    .iter()
                    .map(|text| {
                        p.layout_no_wrap(
                            text.clone(),
                            FontId::new(seg_format.size as f32, family.clone()),
                            Color32::WHITE,
                        )
                        .size()
                        .x as f64
                    })
                    .collect())
            })
            .ok()?
        } else {
            layout_text(&state, |segments, seg_format| {
                Ok(segments
                    .iter()
                    .map(|text| {
                        self.fonts.measure_width(
                            &seg_format.font,
                            seg_format.bold,
                            seg_format.italic,
                            seg_format.size,
                            text,
                        )
                    })
                    .collect())
            })
            .ok()?
        };

        // `materialized_runs` is sorted and covers the complete UTF-16 range. Layout lines and
        // their characters are likewise emitted in increasing UTF-16 order, so a single cursor
        // resolves every character's format in O(chars + runs). Calling
        // `format_at_utf16_offset` here would rescan the full string and the run list per
        // character, making large editable text fields quadratic on every Stage redraw.
        let materialized_runs = state.materialized_runs(total_utf16 as u32);
        let mut format_run_index = 0usize;
        #[cfg(test)]
        let mut format_run_lookups = 0usize;
        #[cfg(test)]
        let mut format_run_advances = 0usize;
        let mut lines = Vec::with_capacity(layout.lines.len().max(1));
        for line in &layout.lines {
            let line_start = line.start as usize;
            let line_end = line.end as usize;
            let slice = utf16_slice(&state.text, line_start, line_end);
            let mut chars = Vec::new();
            let mut cursor_u16 = line_start;
            let mut cursor_x = line.x;
            for ch in slice.chars() {
                let ch_u16 = ch.len_utf16();
                let cursor = cursor_u16 as u32;
                while materialized_runs
                    .get(format_run_index)
                    .is_some_and(|run| cursor >= run.end)
                {
                    format_run_index += 1;
                    #[cfg(test)]
                    {
                        format_run_advances += 1;
                    }
                }
                #[cfg(test)]
                {
                    format_run_lookups += 1;
                }
                let fmt = materialized_runs
                    .get(format_run_index)
                    .filter(|run| run.start <= cursor && cursor < run.end)
                    .map_or(&state.format, |run| &run.format);
                let ch_str = ch.to_string();
                let base_adv = if let Some(p) = painter {
                    let named = crate::fonts::styled_family(&fmt.font, fmt.bold, fmt.italic);
                    let family = if p.ctx().fonts(|f| f.families().contains(&named)) {
                        named
                    } else {
                        FontFamily::Proportional
                    };
                    p.layout_no_wrap(ch_str, FontId::new(fmt.size as f32, family), Color32::WHITE)
                        .size()
                        .x as f64
                } else {
                    self.fonts
                        .measure_width(&fmt.font, fmt.bold, fmt.italic, fmt.size, &ch_str)
                };
                let width = (base_adv + fmt.letter_spacing).max(1.0);
                chars.push(StageTextCharBox {
                    start_utf16: cursor_u16,
                    end_utf16: cursor_u16 + ch_u16,
                    x: cursor_x,
                    width,
                });
                cursor_x += width;
                cursor_u16 += ch_u16;
            }
            lines.push(StageTextLineBox {
                start_utf16: line_start,
                end_utf16: line_end,
                x: line.x,
                y: line.y,
                height: line.height.max(state.format.size * 1.2),
                chars,
            });
        }
        if lines.is_empty() {
            lines.push(StageTextLineBox {
                start_utf16: 0,
                end_utf16: 0,
                x: 0.0,
                y: 0.0,
                height: (state.format.size * 1.2).max(12.0),
                chars: Vec::new(),
            });
        }
        let (box_width, box_height) = match kind {
            ElementKind::TextBox { width, height, .. } => (*width, *height),
            _ => (
                layout.text_width.max(12.0),
                layout.text_height.max(state.format.size * 1.2),
            ),
        };
        Some(StageTextGeometry {
            lines,
            total_utf16,
            box_width,
            box_height,
            #[cfg(test)]
            format_run_lookups,
            #[cfg(test)]
            format_run_advances,
        })
    }

    /// Update a non-wrapping `TextBox` (such as point text upgraded to a `TextBox` when given mixed
    /// formatting) so its bounding box expands to fit all laid-out runs without clipping.
    pub(crate) fn refresh_non_wrapping_text_box_bounds(&mut self, kind: &mut ElementKind) {
        let ElementKind::TextBox {
            word_wrap, format, ..
        } = kind
        else {
            return;
        };
        let auto_size_active = format
            .field_options()
            .is_some_and(|o| !matches!(o.auto_size.as_str(), "" | "none"));
        if *word_wrap && !auto_size_active {
            return;
        }
        let Some(mut state) = TextFieldState::from_kind(kind) else {
            return;
        };
        state.box_size = None;
        state.word_wrap = false;
        if let Ok(layout) = layout_text(&state, |segments, seg_format| {
            Ok(segments
                .iter()
                .map(|text| {
                    self.fonts.measure_width(
                        &seg_format.font,
                        seg_format.bold,
                        seg_format.italic,
                        seg_format.size,
                        text,
                    )
                })
                .collect())
        }) {
            let max_font_size = state
                .format_runs
                .iter()
                .map(|r| r.format.size)
                .fold(state.format.size, f64::max)
                .max(4.0);
            let nat_w = layout
                .lines
                .iter()
                .map(|l| l.x.max(0.0) + l.width)
                .fold(0.0_f64, f64::max);
            let nat_h = layout
                .lines
                .last()
                .map(|l| l.y.max(0.0) + l.height)
                .unwrap_or(max_font_size * 1.2);
            let min_w = (nat_w + max_font_size * 0.4 + 8.0).clamp(16.0, 1_000_000.0);
            let min_h = (nat_h + 6.0).clamp(max_font_size * 1.2 + 4.0, 1_000_000.0);
            if let ElementKind::TextBox { width, height, .. } = kind {
                if auto_size_active {
                    *width = min_w;
                    *height = min_h;
                } else {
                    *width = (*width).max(min_w);
                    *height = (*height).max(min_h);
                }
            }
        }
    }

    /// Replace the active UTF-16 selection (or insert at the collapsed caret) in the currently edited
    /// Stage text element, coalescing consecutive typing/deletion edits into one `"Edit text"` Undo step.
    pub fn stage_text_insert_or_replace(&mut self, inserted: &str) -> bool {
        self.sync_stage_text_edit();
        if !self.stage_text_edit.is_active(self) || !self.selected_editable() {
            return false;
        }
        let (sel_start, sel_end) = self.stage_text_edit.selection_range_utf16();
        let typing_style = self.stage_text_edit.typing_format.clone();
        let Some(sampled) = self.selected_sampled() else {
            return false;
        };
        let Some(mut state) = TextFieldState::from_kind(&sampled.kind) else {
            return false;
        };
        let state_before = state.clone();
        let Ok(new_caret) =
            state.replace_utf16_range(sel_start, sel_end, inserted, typing_style.as_ref())
        else {
            return false;
        };
        if !inserted.is_empty() && state == state_before {
            // Restrict/maxChars rejected the complete insertion. Keep the active selection and
            // avoid manufacturing an empty Undo step.
            return true;
        }
        let mut next_kind = sampled.kind.clone();
        state.write_to_kind(&mut next_kind);
        self.refresh_non_wrapping_text_box_bounds(&mut next_kind);

        if self.stage_text_edit.coalesced_typing_active && self.history.can_undo() {
            self.ensure_selected_key();
            if let Some(el) = self.selected_element_mut() {
                preserve_point_text_alpha_on_upgrade(&el.kind, &next_kind, &mut el.transform.alpha);
                el.kind = next_kind;
            }
            self.project.upgrade_format_for_features();
        } else {
            self.finish_property_edit();
            self.transact_named("Edit text", |s| {
                s.ensure_selected_key();
                if let Some(el) = s.selected_element_mut() {
                    preserve_point_text_alpha_on_upgrade(
                        &el.kind,
                        &next_kind,
                        &mut el.transform.alpha,
                    );
                    el.kind = next_kind;
                }
            });
            self.stage_text_edit.coalesced_typing_active = true;
        }
        self.stage_text_edit.caret_utf16 = new_caret;
        self.stage_text_edit.selection_anchor_utf16 = None;
        self.stage_text_edit.preferred_x = None;
        true
    }

    pub fn stage_text_backspace(&mut self, word: bool) -> bool {
        self.sync_stage_text_edit();
        if !self.stage_text_edit.is_active(self) || !self.selected_editable() {
            return false;
        }
        let (sel_start, sel_end) = self.stage_text_edit.selection_range_utf16();
        if sel_start == sel_end {
            if sel_start == 0 {
                return true;
            }
            let Some(sampled) = self.selected_sampled() else {
                return false;
            };
            let Some(state) = TextFieldState::from_kind(&sampled.kind) else {
                return false;
            };
            let prev = if word {
                prev_word_utf16(&state.text, sel_start)
            } else {
                prev_char_utf16(&state.text, sel_start)
            };
            self.stage_text_edit.selection_anchor_utf16 = Some(prev);
            self.stage_text_edit.caret_utf16 = sel_start;
        }
        self.stage_text_edit.typing_format = None;
        self.stage_text_insert_or_replace("")
    }

    pub fn stage_text_delete_forward(&mut self, word: bool) -> bool {
        self.sync_stage_text_edit();
        if !self.stage_text_edit.is_active(self) || !self.selected_editable() {
            return false;
        }
        let (sel_start, sel_end) = self.stage_text_edit.selection_range_utf16();
        if sel_start == sel_end {
            let Some(sampled) = self.selected_sampled() else {
                return false;
            };
            let Some(state) = TextFieldState::from_kind(&sampled.kind) else {
                return false;
            };
            let total = state.utf16_len();
            if sel_start >= total {
                return true;
            }
            let next = if word {
                next_word_utf16(&state.text, sel_start)
            } else {
                next_char_utf16(&state.text, sel_start)
            };
            self.stage_text_edit.selection_anchor_utf16 = Some(sel_start);
            self.stage_text_edit.caret_utf16 = next;
        }
        self.stage_text_edit.typing_format = None;
        self.stage_text_insert_or_replace("")
    }

    /// Apply a `TextFormatPatch` using selection-aware Stage text routing (`T-202`):
    /// - When inline Stage text editing is active with a non-empty UTF-16 range `(start < end)`,
    ///   splits overlapping `TextFormatRun`s and applies `patch` only to `[start, end)` in one Undo step.
    /// - When inline Stage text editing is active with a collapsed caret (`start == end`),
    ///   updates `typing_format` for Character attributes and applies Paragraph attributes to the
    ///   enclosing paragraph.
    /// - When the text element is selected as an object outside inline Stage text editing,
    ///   applies `patch` across the whole text element in one Undo step.
    pub fn apply_stage_text_format_patch(
        &mut self,
        patch: &TextFormatPatch,
    ) -> Result<bool, String> {
        if patch.is_empty() {
            return Ok(false);
        }
        self.finish_stage_text_typing();
        self.sync_stage_text_edit();
        let Some(sampled) = self.selected_sampled() else {
            return Err("Select a text element first.".into());
        };
        let Some(mut state) = TextFieldState::from_kind(&sampled.kind) else {
            return Err("Selected element is not a text field.".into());
        };
        let inline_active = self.stage_text_edit.is_active(self);
        if inline_active {
            let (sel_start, sel_end) = self.stage_text_edit.selection_range_utf16();
            if sel_start < sel_end {
                let char_patch = patch.character_only();
                let para_patch = patch.paragraph_only();
                if !char_patch.is_empty() {
                    state
                        .apply_format_to_utf16_range(sel_start, sel_end, &char_patch)
                        .map_err(|e| e.to_string())?;
                }
                if !para_patch.is_empty() {
                    let (p_start, p_end) =
                        state.paragraph_utf16_bounds_for_range(sel_start, sel_end);
                    state
                        .apply_format_to_utf16_range(p_start, p_end, &para_patch)
                        .map_err(|e| e.to_string())?;
                }
            } else {
                let char_patch = patch.character_only();
                let para_patch = patch.paragraph_only();
                if !char_patch.is_empty() {
                    let mut next_typing = self
                        .stage_text_edit
                        .typing_format
                        .clone()
                        .unwrap_or_else(|| state.format_at_utf16_caret(sel_start));
                    char_patch.apply(&mut next_typing);
                    next_typing.set_field_options(None);
                    self.stage_text_edit.typing_format = Some(next_typing);
                    if state.utf16_len() == 0 {
                        char_patch.apply(&mut state.format);
                        char_patch.apply(&mut state.default_format);
                    }
                }
                if !para_patch.is_empty() {
                    let (p_start, p_end) =
                        state.paragraph_utf16_bounds_for_range(sel_start, sel_end);
                    if p_start < p_end {
                        state
                            .apply_format_to_utf16_range(p_start, p_end, &para_patch)
                            .map_err(|e| e.to_string())?;
                    } else {
                        state
                            .apply(&TextPatch {
                                format: Some(para_patch),
                                ..TextPatch::default()
                            })
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        } else {
            state
                .apply(&TextPatch {
                    format: Some(patch.clone()),
                    ..TextPatch::default()
                })
                .map_err(|e| e.to_string())?;
        }
        state.validate().map_err(|e| e.to_string())?;
        let mut next_kind = sampled.kind.clone();
        state.write_to_kind(&mut next_kind);
        self.refresh_non_wrapping_text_box_bounds(&mut next_kind);
        if next_kind == sampled.kind {
            return Ok(false);
        }
        self.finish_property_edit();
        self.transact_named("Text format", |s| {
            s.ensure_selected_key();
            if let Some(el) = s.selected_element_mut() {
                preserve_point_text_alpha_on_upgrade(&el.kind, &next_kind, &mut el.transform.alpha);
                el.kind = next_kind;
            }
        });
        Ok(true)
    }

    /// Resolve the `TextFormat` to display in `Properties — Text` for `element`, taking into account
    /// the active UTF-16 selection range or collapsed `typing_format` when inline Stage text editing
    /// is active.
    pub(crate) fn resolved_inspector_text_format(
        &self,
        element_id: &str,
        state: &TextFieldState,
    ) -> TextFormat {
        if self.stage_text_edit.is_editing_element(element_id)
            && self.stage_text_edit.symbol_id == self.symbol_id()
            && self.stage_text_edit.frame == self.frame
        {
            let (sel_start, sel_end) = self.stage_text_edit.selection_range_utf16();
            if sel_start < sel_end {
                state.format_for_utf16_selection(sel_start, sel_end)
            } else {
                self.stage_text_edit
                    .typing_format
                    .clone()
                    .unwrap_or_else(|| state.format_at_utf16_caret(sel_start))
            }
        } else {
            state.format.clone()
        }
    }

    /// Reconcile mutations produced by the `Properties — Text` UI controls (`after_kind`) against
    /// `original_kind` using selection-aware UTF-16 range formatting and `typing_format`.
    pub(crate) fn reconcile_inspector_text_edit(
        &mut self,
        element_id: &str,
        original_kind: &ElementKind,
        shown_format: &TextFormat,
        after_kind: &mut ElementKind,
        alpha: &mut f64,
    ) {
        let Some(mut next_state) = TextFieldState::from_kind(original_kind) else {
            return;
        };
        let (after_text, mut after_format, after_opts_opt, after_wrap, after_multiline, after_box) =
            match after_kind {
                ElementKind::Text {
                    text,
                    font_family,
                    font_size,
                    color,
                } => {
                    let mut fmt = shown_format.clone();
                    fmt.font = font_family.clone();
                    fmt.size = *font_size;
                    fmt.color = u32::from_be_bytes([0, color[0], color[1], color[2]]);
                    next_state.alpha = color[3];
                    (
                        text.clone(),
                        fmt,
                        None,
                        next_state.word_wrap,
                        next_state.multiline,
                        next_state.box_size,
                    )
                }
                ElementKind::TextBox {
                    text,
                    width,
                    height,
                    format,
                    word_wrap,
                    multiline,
                } => {
                    let opts = format.field_options();
                    let mut clean = format.clone();
                    clean.set_field_options(None);
                    (
                        text.clone(),
                        clean,
                        opts,
                        *word_wrap,
                        *multiline,
                        Some(neoflash_core::TextBoxSize {
                            width: *width,
                            height: *height,
                        }),
                    )
                }
                _ => return,
            };

        let mut clean_shown = shown_format.clone();
        clean_shown.set_field_options(None);
        after_format.set_field_options(None);

        // 1. If the user edited the text string in the Properties panel's TextEdit, apply a UTF-16 diff
        // replacement so existing format_runs shift accurately instead of failing validation.
        let normalized_after = after_text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized_after != next_state.text {
            let (diff_start, diff_end, inserted) =
                diff_utf16_replacement(&next_state.text, &normalized_after);
            let typing = self.stage_text_edit.typing_format.clone();
            if let Ok(new_caret) =
                next_state.replace_utf16_range(diff_start, diff_end, inserted, typing.as_ref())
                && self.stage_text_edit.is_editing_element(element_id)
            {
                self.stage_text_edit.caret_utf16 = new_caret;
                self.stage_text_edit.selection_anchor_utf16 = None;
            }
        }

        // 2. Apply any field-level AuthoredTextFieldOptions changes while preserving `next_state.format_runs`.
        if let Some(mut opts) = after_opts_opt {
            let preserved_runs = next_state.format_runs.clone();
            let preserved_fmt = next_state.format.clone();
            opts.format_runs = preserved_runs;
            opts.apply_to_state(&mut next_state);
            if next_state.format_runs.is_empty() {
                next_state.format = preserved_fmt;
            }
        } else if matches!(after_kind, ElementKind::TextBox { .. }) {
            let default_opts = AuthoredTextFieldOptions {
                format_runs: next_state.format_runs.clone(),
                ..Default::default()
            };
            let preserved_fmt = next_state.format.clone();
            default_opts.apply_to_state(&mut next_state);
            if next_state.format_runs.is_empty() {
                next_state.format = preserved_fmt;
            }
        }
        if matches!(after_kind, ElementKind::TextBox { .. }) {
            next_state.word_wrap = after_wrap;
            next_state.multiline = after_multiline;
            next_state.box_size = after_box;
        }

        // 3. Apply Character / Paragraph `TextFormatPatch` changes according to active selection state.
        let fmt_patch = TextFormatPatch::diff_from(&clean_shown, &after_format);
        if !fmt_patch.is_empty() {
            let inline_active = self.stage_text_edit.is_editing_element(element_id)
                && self.stage_text_edit.symbol_id == self.symbol_id()
                && self.stage_text_edit.frame == self.frame;
            if inline_active {
                let (sel_start, sel_end) = self.stage_text_edit.selection_range_utf16();
                if sel_start < sel_end {
                    let char_patch = fmt_patch.character_only();
                    let para_patch = fmt_patch.paragraph_only();
                    if !char_patch.is_empty() {
                        let _ =
                            next_state.apply_format_to_utf16_range(sel_start, sel_end, &char_patch);
                    }
                    if !para_patch.is_empty() {
                        let (p_start, p_end) =
                            next_state.paragraph_utf16_bounds_for_range(sel_start, sel_end);
                        let _ = next_state.apply_format_to_utf16_range(p_start, p_end, &para_patch);
                    }
                } else {
                    let char_patch = fmt_patch.character_only();
                    let para_patch = fmt_patch.paragraph_only();
                    if !char_patch.is_empty() {
                        let mut next_typing = self
                            .stage_text_edit
                            .typing_format
                            .clone()
                            .unwrap_or_else(|| next_state.format_at_utf16_caret(sel_start));
                        char_patch.apply(&mut next_typing);
                        next_typing.set_field_options(None);
                        self.stage_text_edit.typing_format = Some(next_typing);
                        if next_state.utf16_len() == 0 {
                            char_patch.apply(&mut next_state.format);
                            char_patch.apply(&mut next_state.default_format);
                        }
                    }
                    if !para_patch.is_empty() {
                        let (p_start, p_end) =
                            next_state.paragraph_utf16_bounds_for_range(sel_start, sel_end);
                        if p_start < p_end {
                            let _ =
                                next_state.apply_format_to_utf16_range(p_start, p_end, &para_patch);
                        } else {
                            let _ = next_state.apply(&TextPatch {
                                format: Some(para_patch),
                                ..TextPatch::default()
                            });
                        }
                    }
                }
            } else {
                let _ = next_state.apply(&TextPatch {
                    format: Some(fmt_patch),
                    ..TextPatch::default()
                });
            }
        }

        // Write back to `after_kind` (preserving `ElementKind::Text` when plain, or upgrading to `TextBox` when rich).
        let was_point_text = matches!(original_kind, ElementKind::Text { .. });
        let explicitly_converted_to_box =
            was_point_text && matches!(after_kind, ElementKind::TextBox { .. });
        if !explicitly_converted_to_box {
            *after_kind = original_kind.clone();
        }
        next_state.write_to_kind(after_kind);
        if was_point_text {
            preserve_point_text_alpha_on_upgrade(original_kind, after_kind, alpha);
        }
        self.refresh_non_wrapping_text_box_bounds(after_kind);
    }

    /// Consume keyboard and IME events when inline Stage text editing is active and no panel widget
    /// owns egui focus.
    pub(crate) fn handle_stage_text_edit_keys(&mut self, ctx: &egui::Context) -> bool {
        self.sync_stage_text_edit();
        if !self.stage_text_edit.is_active(self) {
            return false;
        }
        if ctx.memory(|m| m.focused().is_some()) {
            return false;
        }

        let mut handled = false;
        let geom = self
            .selected_sampled()
            .and_then(|el| self.stage_text_geometry(None, &el.kind));
        let text_content = self
            .selected_sampled()
            .and_then(|el| TextFieldState::from_kind(&el.kind))
            .map(|s| s.text)
            .unwrap_or_default();
        let total_u16 = text_content.encode_utf16().count();
        let is_multiline = self
            .selected_sampled()
            .and_then(|el| TextFieldState::from_kind(&el.kind))
            .is_none_or(|s| s.multiline);

        enum Action {
            Insert(String),
            Backspace(bool),
            Delete(bool),
            MoveLeft { shift: bool, word: bool, line: bool },
            MoveRight { shift: bool, word: bool, line: bool },
            MoveUp { shift: bool },
            MoveDown { shift: bool },
            Home { shift: bool },
            End { shift: bool },
            SelectAll,
            ToggleBold,
            ToggleItalic,
            ToggleUnderline,
            Copy,
            Cut,
            Exit,
        }

        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            let mut retained = Vec::with_capacity(i.events.len());
            for event in i.events.drain(..) {
                match &event {
                    egui::Event::Text(text)
                        if !text.is_empty() && !text.chars().all(char::is_control) =>
                    {
                        actions.push(Action::Insert(text.clone()));
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty() => {
                        actions.push(Action::Insert(text.clone()));
                    }
                    egui::Event::Copy if self.stage_text_edit.has_range_selection() => {
                        actions.push(Action::Copy);
                    }
                    egui::Event::Cut if self.stage_text_edit.has_range_selection() => {
                        actions.push(Action::Cut);
                    }
                    egui::Event::Key {
                        key: Key::Escape,
                        pressed: true,
                        modifiers,
                        ..
                    } if *modifiers == Modifiers::NONE => {
                        actions.push(Action::Exit);
                    }
                    egui::Event::Key {
                        key: Key::A,
                        pressed: true,
                        modifiers,
                        ..
                    } if (modifiers.command || modifiers.ctrl)
                        && !modifiers.alt
                        && !modifiers.shift =>
                    {
                        actions.push(Action::SelectAll);
                    }
                    egui::Event::Key {
                        key: Key::B,
                        pressed: true,
                        modifiers,
                        ..
                    } if (modifiers.command || modifiers.ctrl)
                        && !modifiers.alt
                        && !modifiers.shift =>
                    {
                        actions.push(Action::ToggleBold);
                    }
                    egui::Event::Key {
                        key: Key::I,
                        pressed: true,
                        modifiers,
                        ..
                    } if (modifiers.command || modifiers.ctrl)
                        && !modifiers.alt
                        && !modifiers.shift =>
                    {
                        actions.push(Action::ToggleItalic);
                    }
                    egui::Event::Key {
                        key: Key::U,
                        pressed: true,
                        modifiers,
                        ..
                    } if (modifiers.command || modifiers.ctrl)
                        && !modifiers.alt
                        && !modifiers.shift =>
                    {
                        actions.push(Action::ToggleUnderline);
                    }
                    egui::Event::Key {
                        key: Key::Backspace,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.command => {
                        actions.push(Action::Backspace(modifiers.alt || modifiers.ctrl));
                    }
                    egui::Event::Key {
                        key: Key::Delete,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.command => {
                        actions.push(Action::Delete(modifiers.alt || modifiers.ctrl));
                    }
                    egui::Event::Key {
                        key: Key::Enter,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.command && !modifiers.ctrl && !modifiers.alt => {
                        if is_multiline {
                            actions.push(Action::Insert("\n".into()));
                        } else {
                            actions.push(Action::Exit);
                        }
                    }
                    egui::Event::Key {
                        key: Key::ArrowLeft,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        actions.push(Action::MoveLeft {
                            shift: modifiers.shift,
                            word: modifiers.alt || modifiers.ctrl,
                            line: modifiers.command,
                        });
                    }
                    egui::Event::Key {
                        key: Key::ArrowRight,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        actions.push(Action::MoveRight {
                            shift: modifiers.shift,
                            word: modifiers.alt || modifiers.ctrl,
                            line: modifiers.command,
                        });
                    }
                    egui::Event::Key {
                        key: Key::ArrowUp,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.command && !modifiers.ctrl && !modifiers.alt => {
                        actions.push(Action::MoveUp {
                            shift: modifiers.shift,
                        });
                    }
                    egui::Event::Key {
                        key: Key::ArrowDown,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.command && !modifiers.ctrl && !modifiers.alt => {
                        actions.push(Action::MoveDown {
                            shift: modifiers.shift,
                        });
                    }
                    egui::Event::Key {
                        key: Key::Home,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        actions.push(Action::Home {
                            shift: modifiers.shift,
                        });
                    }
                    egui::Event::Key {
                        key: Key::End,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        actions.push(Action::End {
                            shift: modifiers.shift,
                        });
                    }
                    _ => retained.push(event),
                }
            }
            // If a text insertion event was consumed this frame, also strip any plain unmodified
            // single-letter Key event so tool shortcuts (V, T, R, B, etc.) don't fire while typing.
            if actions.iter().any(|a| matches!(a, Action::Insert(_))) {
                retained.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            pressed: true,
                            modifiers,
                            ..
                        } if !modifiers.command && !modifiers.ctrl && !modifiers.alt
                    )
                });
            }
            i.events = retained;
        });

        for action in actions {
            handled = true;
            match action {
                Action::Insert(text) => {
                    self.stage_text_insert_or_replace(&text);
                }
                Action::Backspace(word) => {
                    self.stage_text_backspace(word);
                }
                Action::Delete(word) => {
                    self.stage_text_delete_forward(word);
                }
                Action::SelectAll => {
                    self.finish_stage_text_typing();
                    self.stage_text_edit.selection_anchor_utf16 = Some(0);
                    self.stage_text_edit.caret_utf16 = total_u16;
                    self.stage_text_edit.typing_format = None;
                    self.stage_text_edit.preferred_x = None;
                }
                Action::ToggleBold => {
                    if let Some(sampled) = self.selected_sampled()
                        && let Some(state) = TextFieldState::from_kind(&sampled.kind)
                    {
                        let cur = self.resolved_inspector_text_format(&sampled.id, &state);
                        let _ = self.apply_stage_text_format_patch(&TextFormatPatch {
                            bold: Some(!cur.bold),
                            ..TextFormatPatch::default()
                        });
                    }
                }
                Action::ToggleItalic => {
                    if let Some(sampled) = self.selected_sampled()
                        && let Some(state) = TextFieldState::from_kind(&sampled.kind)
                    {
                        let cur = self.resolved_inspector_text_format(&sampled.id, &state);
                        let _ = self.apply_stage_text_format_patch(&TextFormatPatch {
                            italic: Some(!cur.italic),
                            ..TextFormatPatch::default()
                        });
                    }
                }
                Action::ToggleUnderline => {
                    if let Some(sampled) = self.selected_sampled()
                        && let Some(state) = TextFieldState::from_kind(&sampled.kind)
                    {
                        let cur = self.resolved_inspector_text_format(&sampled.id, &state);
                        let _ = self.apply_stage_text_format_patch(&TextFormatPatch {
                            underline: Some(!cur.underline),
                            ..TextFormatPatch::default()
                        });
                    }
                }
                Action::Copy => {
                    let (lo, hi) = self.stage_text_edit.selection_range_utf16();
                    let copied = utf16_slice(&text_content, lo, hi).to_owned();
                    ctx.copy_text(copied);
                }
                Action::Cut => {
                    let (lo, hi) = self.stage_text_edit.selection_range_utf16();
                    let copied = utf16_slice(&text_content, lo, hi).to_owned();
                    ctx.copy_text(copied);
                    self.stage_text_edit.typing_format = None;
                    self.stage_text_insert_or_replace("");
                }
                Action::Exit => {
                    self.exit_stage_text_edit();
                }
                Action::MoveLeft { shift, word, line } => {
                    self.finish_stage_text_typing();
                    self.stage_text_edit.typing_format = None;
                    self.stage_text_edit.preferred_x = None;
                    let (lo, _hi) = self.stage_text_edit.selection_range_utf16();
                    if !shift && self.stage_text_edit.has_range_selection() {
                        self.stage_text_edit.caret_utf16 = lo;
                        self.stage_text_edit.selection_anchor_utf16 = None;
                    } else {
                        let cur = self.stage_text_edit.caret_utf16;
                        let next = if line {
                            geom.as_ref().map_or(0, |g| g.line_home_end_utf16(cur).0)
                        } else if word {
                            prev_word_utf16(&text_content, cur)
                        } else {
                            prev_char_utf16(&text_content, cur)
                        };
                        self.move_caret_with_shift(next, shift);
                    }
                }
                Action::MoveRight { shift, word, line } => {
                    self.finish_stage_text_typing();
                    self.stage_text_edit.typing_format = None;
                    self.stage_text_edit.preferred_x = None;
                    let (_lo, hi) = self.stage_text_edit.selection_range_utf16();
                    if !shift && self.stage_text_edit.has_range_selection() {
                        self.stage_text_edit.caret_utf16 = hi;
                        self.stage_text_edit.selection_anchor_utf16 = None;
                    } else {
                        let cur = self.stage_text_edit.caret_utf16;
                        let next = if line {
                            geom.as_ref()
                                .map_or(total_u16, |g| g.line_home_end_utf16(cur).1)
                        } else if word {
                            next_word_utf16(&text_content, cur)
                        } else {
                            next_char_utf16(&text_content, cur)
                        };
                        self.move_caret_with_shift(next, shift);
                    }
                }
                Action::MoveUp { shift } => {
                    self.finish_stage_text_typing();
                    self.stage_text_edit.typing_format = None;
                    let cur = self.stage_text_edit.caret_utf16;
                    let mut pref_x = self.stage_text_edit.preferred_x;
                    let next = geom
                        .as_ref()
                        .map_or(0, |g| g.move_vertical_utf16(cur, -1, &mut pref_x));
                    self.stage_text_edit.preferred_x = pref_x;
                    self.move_caret_with_shift(next, shift);
                }
                Action::MoveDown { shift } => {
                    self.finish_stage_text_typing();
                    self.stage_text_edit.typing_format = None;
                    let cur = self.stage_text_edit.caret_utf16;
                    let mut pref_x = self.stage_text_edit.preferred_x;
                    let next = geom
                        .as_ref()
                        .map_or(total_u16, |g| g.move_vertical_utf16(cur, 1, &mut pref_x));
                    self.stage_text_edit.preferred_x = pref_x;
                    self.move_caret_with_shift(next, shift);
                }
                Action::Home { shift } => {
                    self.finish_stage_text_typing();
                    self.stage_text_edit.typing_format = None;
                    self.stage_text_edit.preferred_x = None;
                    let cur = self.stage_text_edit.caret_utf16;
                    let next = geom.as_ref().map_or(0, |g| g.line_home_end_utf16(cur).0);
                    self.move_caret_with_shift(next, shift);
                }
                Action::End { shift } => {
                    self.finish_stage_text_typing();
                    self.stage_text_edit.typing_format = None;
                    self.stage_text_edit.preferred_x = None;
                    let cur = self.stage_text_edit.caret_utf16;
                    let next = geom
                        .as_ref()
                        .map_or(total_u16, |g| g.line_home_end_utf16(cur).1);
                    self.move_caret_with_shift(next, shift);
                }
            }
        }
        handled
    }

    fn move_caret_with_shift(&mut self, next_utf16: usize, shift: bool) {
        if shift {
            let anchor = self
                .stage_text_edit
                .selection_anchor_utf16
                .unwrap_or(self.stage_text_edit.caret_utf16);
            self.stage_text_edit.caret_utf16 = next_utf16;
            self.stage_text_edit.selection_anchor_utf16 = (anchor != next_utf16).then_some(anchor);
        } else {
            self.stage_text_edit.caret_utf16 = next_utf16;
            self.stage_text_edit.selection_anchor_utf16 = None;
        }
    }

    fn pick_editable_text_placement(
        &self,
        scene: &[DrawItem],
        painter: &egui::Painter,
        stage_point: Pos2,
        mapping: StageMapping,
    ) -> Option<SelectionTarget> {
        let hit = self.pick_placement(scene, painter, stage_point, mapping)?;
        if !self.target_editable(&hit) {
            return None;
        }
        let timeline = &self.symbol().timeline;
        for layer in &timeline.layers {
            if layer.id != hit.layer
                || layer.properties.role != LayerRole::Normal
                || layer_effectively_locked(timeline, layer)
                || !layer_effectively_visible(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                continue;
            }
            for el in
                sample_layer_in_at_fps(timeline, layer, self.frame, f64::from(self.project.fps))
            {
                if el.id == hit.element {
                    return el.kind.is_text().then_some(hit);
                }
            }
        }
        None
    }

    /// Handle pointer press, drag range selection, double-click word selection, and keyboard input
    /// on the Stage for `Tool::Text` and active inline text editing.
    /// Returns `true` when the current pointer gesture is owned by inline Stage text editing so
    /// `shape_stage` and `create_stage_text` do not create a new text element over the edited one.
    pub(crate) fn stage_text_edit_stage(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        mapping: StageMapping,
        origin: Pos2,
        rect: Rect,
        scene: &[DrawItem],
    ) -> bool {
        self.handle_stage_text_edit_keys(ui.ctx());
        self.sync_stage_text_edit();

        let mut owns_pointer = self.stage_text_edit.pointer_dragging;
        if self.tool != Tool::Text && !self.stage_text_edit.is_active(self) {
            return false;
        }

        let now = ui.input(|i| i.time);
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                } if rect.contains(pos)
                    && !self.stage_view.guide_pointer
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == ui.layer_id()) =>
                {
                    let stage_pt = mapping.screen_to_edit(origin, self.zoom, pos);
                    let hit_target =
                        self.pick_editable_text_placement(scene, painter, stage_pt, mapping);
                    if let Some(target) = hit_target {
                        owns_pointer = true;
                        self.playing = false;
                        // Surrender any panel TextEdit focus so subsequent Stage typing / arrows
                        // go directly to the Stage inline text editor.
                        if let Some(focused) = ui.ctx().memory(|m| m.focused()) {
                            ui.ctx().memory_mut(|m| m.surrender_focus(focused));
                        }
                        let already_editing_this = self.stage_text_edit.active_target.as_ref()
                            == Some(&target)
                            && self.stage_text_edit.symbol_id == self.symbol_id()
                            && self.stage_text_edit.frame == self.frame;
                        if !already_editing_this {
                            self.enter_stage_text_edit(target.clone(), 0, None);
                        } else {
                            self.finish_stage_text_typing();
                        }
                        if let Some(sampled) = self.selected_sampled()
                            && let Some(local_pt) =
                                crate::stage::inverse(transform_matrix(sampled.transform), stage_pt)
                            && let Some(geom) =
                                self.stage_text_geometry(Some(painter), &sampled.kind)
                        {
                            let hit_u16 = geom.hit_test_utf16(local_pt);
                            let is_double = already_editing_this
                                && now >= self.stage_text_edit.last_click_time
                                && now - self.stage_text_edit.last_click_time <= 0.45
                                && self
                                    .stage_text_edit
                                    .last_click_pos
                                    .is_some_and(|p| p.distance(pos) <= 8.0);
                            if is_double {
                                if let Some(state) = TextFieldState::from_kind(&sampled.kind) {
                                    let (w_start, w_end) =
                                        word_utf16_bounds_at(&state.text, hit_u16);
                                    self.stage_text_edit.selection_anchor_utf16 = Some(w_start);
                                    self.stage_text_edit.caret_utf16 = w_end;
                                    self.stage_text_edit.typing_format = None;
                                    self.stage_text_edit.pointer_dragging = false;
                                    self.stage_text_edit.drag_press_pos = None;
                                }
                                self.stage_text_edit.last_click_time = 0.0;
                                self.stage_text_edit.last_click_pos = None;
                            } else if modifiers.shift && already_editing_this {
                                let anchor = self
                                    .stage_text_edit
                                    .selection_anchor_utf16
                                    .unwrap_or(self.stage_text_edit.caret_utf16);
                                self.stage_text_edit.selection_anchor_utf16 = Some(anchor);
                                self.stage_text_edit.caret_utf16 = hit_u16;
                                self.stage_text_edit.typing_format = None;
                                self.stage_text_edit.pointer_dragging = true;
                                self.stage_text_edit.drag_press_pos = Some(pos);
                                self.stage_text_edit.last_click_time = now;
                                self.stage_text_edit.last_click_pos = Some(pos);
                            } else {
                                self.stage_text_edit.caret_utf16 = hit_u16;
                                self.stage_text_edit.selection_anchor_utf16 = Some(hit_u16);
                                self.stage_text_edit.typing_format = None;
                                self.stage_text_edit.pointer_dragging = true;
                                self.stage_text_edit.drag_press_pos = Some(pos);
                                self.stage_text_edit.last_click_time = now;
                                self.stage_text_edit.last_click_pos = Some(pos);
                            }
                            self.stage_text_edit.preferred_x = None;
                        }
                    } else if self.stage_text_edit.is_active(self) {
                        self.exit_stage_text_edit();
                    }
                }
                egui::Event::PointerMoved(pos) if self.stage_text_edit.pointer_dragging => {
                    owns_pointer = true;
                    let stage_pt = mapping.screen_to_edit(origin, self.zoom, pos);
                    if let Some(sampled) = self.selected_sampled()
                        && let Some(local_pt) =
                            crate::stage::inverse(transform_matrix(sampled.transform), stage_pt)
                        && let Some(geom) = self.stage_text_geometry(Some(painter), &sampled.kind)
                    {
                        let hit_u16 = geom.hit_test_utf16(local_pt);
                        self.stage_text_edit.caret_utf16 = hit_u16;
                        if self.stage_text_edit.selection_anchor_utf16 != Some(hit_u16) {
                            self.stage_text_edit.typing_format = None;
                        }
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } if self.stage_text_edit.pointer_dragging => {
                    owns_pointer = true;
                    if self
                        .stage_text_edit
                        .drag_press_pos
                        .is_some_and(|press| press.distance(pos) > 1.5)
                    {
                        let stage_pt = mapping.screen_to_edit(origin, self.zoom, pos);
                        if let Some(sampled) = self.selected_sampled()
                            && let Some(local_pt) =
                                crate::stage::inverse(transform_matrix(sampled.transform), stage_pt)
                            && let Some(geom) =
                                self.stage_text_geometry(Some(painter), &sampled.kind)
                        {
                            self.stage_text_edit.caret_utf16 = geom.hit_test_utf16(local_pt);
                        }
                    }
                    if self.stage_text_edit.selection_anchor_utf16
                        == Some(self.stage_text_edit.caret_utf16)
                    {
                        self.stage_text_edit.selection_anchor_utf16 = None;
                    }
                    self.stage_text_edit.pointer_dragging = false;
                    self.stage_text_edit.drag_press_pos = None;
                }
                _ => {}
            }
        }
        owns_pointer
    }

    /// Paint the active Stage text editing overlay (transformed container frame, UTF-16 selection
    /// range highlight quads, and caret line).
    pub(crate) fn paint_stage_text_edit_overlay(
        &mut self,
        painter: &egui::Painter,
        mapping: StageMapping,
        origin: Pos2,
    ) {
        self.sync_stage_text_edit();
        if !self.stage_text_edit.is_active(self) {
            return;
        }
        let Some(sampled) = self.selected_sampled() else {
            return;
        };
        let Some(geom) = self.stage_text_geometry(Some(painter), &sampled.kind) else {
            return;
        };
        let matrix = transform_matrix(sampled.transform);
        let zoom = self.zoom;
        let to_screen = |local_pt: Pos2| {
            let stage_pt = crate::stage::affine(matrix, local_pt);
            mapping.edit_to_screen(origin, zoom, stage_pt)
        };
        let palette = neoflash_ui::palette(painter.ctx());

        // 1. Subtle active editing frame around the text bounds.
        let box_rect = Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(geom.box_width as f32, geom.box_height as f32),
        );
        let frame_pts = [
            box_rect.left_top(),
            box_rect.right_top(),
            box_rect.right_bottom(),
            box_rect.left_bottom(),
            box_rect.left_top(),
        ]
        .map(to_screen);
        painter.add(Shape::line(
            frame_pts.to_vec(),
            Stroke::new(1.0_f32, palette.accent_default),
        ));

        // 2. Selection range highlight quads, or caret line when collapsed.
        let (sel_start, sel_end) = self.stage_text_edit.selection_range_utf16();
        let fill_color = neoflash_ui::Palette::tint(palette.accent_default, 0.3);
        for local_rect in geom.selection_rects_local(sel_start, sel_end) {
            let corners = [
                to_screen(local_rect.left_top()),
                to_screen(local_rect.right_top()),
                to_screen(local_rect.right_bottom()),
                to_screen(local_rect.left_bottom()),
            ];
            painter.add(Shape::convex_polygon(
                corners.to_vec(),
                fill_color,
                Stroke::NONE,
            ));
        }

        let (caret_top, caret_bottom) = geom.caret_segment_local(self.stage_text_edit.caret_utf16);
        let screen_top = to_screen(caret_top);
        let screen_bottom = to_screen(caret_bottom);
        painter.line_segment(
            [screen_top, screen_bottom],
            Stroke::new(1.5_f32, palette.text_primary),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Editor;
    use eframe::egui::{self, Key, Modifiers, Pos2, Rect, Vec2};
    use neoflash_core::{
        Element, ElementKind, InstancePlayback, Keyframe, Layer, Project, Symbol, SymbolKind,
        TextAlign, TextFieldState, TextFormat, TextFormatPatch, TextFormatRun, Timeline, Transform,
        new_id,
    };

    fn stage_frame(editor: &mut Editor, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 900.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                editor.shortcuts(ctx);
                editor.stage(ctx);
            },
        );
    }

    #[test]
    fn stage_text_range_formatting_updates_only_selected_utf16_substring_and_undoes_in_one_step() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.add_element(
            ElementKind::TextBox {
                text: "Hello brave world".into(),
                width: 260.0,
                height: 60.0,
                format: TextFormat {
                    font: "Inter".into(),
                    size: 16.0,
                    color: 0x112233,
                    ..TextFormat::default()
                },
                word_wrap: true,
                multiline: true,
            },
            40.0,
            50.0,
        );
        let baseline = editor.project.clone();

        // Select "brave" (UTF-16 6..11) and apply Character formatting in one Undo step.
        assert!(editor.select_stage_text_utf16_range(6, 11));
        assert_eq!(editor.stage_text_edit.selection_range_utf16(), (6, 11));
        assert!(
            editor
                .apply_stage_text_format_patch(&TextFormatPatch {
                    font: Some("Georgia".into()),
                    size: Some(28.0),
                    color: Some(0xe53935),
                    bold: Some(true),
                    italic: Some(true),
                    letter_spacing: Some(1.5),
                    ..TextFormatPatch::default()
                })
                .unwrap()
        );

        let sampled = editor.selected_sampled().unwrap();
        let state = TextFieldState::from_kind(&sampled.kind).unwrap();
        assert_eq!(state.format_runs.len(), 3);
        assert_eq!(
            (state.format_runs[0].start, state.format_runs[0].end),
            (0, 6)
        );
        assert_eq!(state.format_runs[0].format.font, "Inter");
        assert_eq!(state.format_runs[0].format.size, 16.0);
        assert_eq!(state.format_runs[0].format.color, 0x112233);
        assert!(!state.format_runs[0].format.bold);

        assert_eq!(
            (state.format_runs[1].start, state.format_runs[1].end),
            (6, 11)
        );
        assert_eq!(state.format_runs[1].format.font, "Georgia");
        assert_eq!(state.format_runs[1].format.size, 28.0);
        assert_eq!(state.format_runs[1].format.color, 0xe53935);
        assert!(state.format_runs[1].format.bold);
        assert!(state.format_runs[1].format.italic);
        assert_eq!(state.format_runs[1].format.letter_spacing, 1.5);

        assert_eq!(
            (state.format_runs[2].start, state.format_runs[2].end),
            (11, 17)
        );
        assert_eq!(state.format_runs[2].format.font, "Inter");
        assert_eq!(state.format_runs[2].format.size, 16.0);
        assert_eq!(state.format_runs[2].format.color, 0x112233);

        // Inspector reconciliation also routes range changes only to the selected UTF-16 range.
        assert!(editor.select_stage_text_utf16_range(12, 17));
        let orig = editor.selected_sampled().unwrap();
        let orig_state = TextFieldState::from_kind(&orig.kind).unwrap();
        let shown = editor.resolved_inspector_text_format(&orig.id, &orig_state);
        assert_eq!(shown.font, "Inter");
        assert!(!shown.bold);

        let mut after_kind = orig.kind.clone();
        if let ElementKind::TextBox { format, .. } = &mut after_kind {
            let opts = format.field_options();
            *format = shown.clone();
            format.color = 0x1e63d6;
            format.underline = true;
            format.set_field_options(opts);
        }
        let mut alpha = orig.transform.alpha;
        editor.reconcile_inspector_text_edit(
            &orig.id,
            &orig.kind,
            &shown,
            &mut after_kind,
            &mut alpha,
        );
        let reconciled = TextFieldState::from_kind(&after_kind).unwrap();
        assert_eq!(reconciled.format_runs.len(), 4);
        assert_eq!(reconciled.format_runs[1].format.font, "Georgia");
        assert_eq!(reconciled.format_runs[1].format.color, 0xe53935);
        assert_eq!(
            (
                reconciled.format_runs[3].start,
                reconciled.format_runs[3].end
            ),
            (12, 17)
        );
        assert_eq!(reconciled.format_runs[3].format.color, 0x1e63d6);
        assert!(reconciled.format_runs[3].format.underline);

        // One Undo step reverts the applied range format change.
        editor.undo();
        assert_eq!(editor.project, baseline);
        editor.project.validate().unwrap();
    }

    #[test]
    fn stage_text_collapsed_caret_typing_format_coalesced_undo_and_keyboard_navigation_work() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.add_element(
            ElementKind::TextBox {
                text: "Hello world".into(),
                width: 240.0,
                height: 60.0,
                format: TextFormat {
                    font: "Inter".into(),
                    size: 16.0,
                    color: 0x222222,
                    ..TextFormat::default()
                },
                word_wrap: true,
                multiline: true,
            },
            30.0,
            40.0,
        );
        let baseline = editor.project.clone();
        let undo_before_typing = editor.history.undo_len();

        // Place collapsed caret at UTF-16 offset 6 (before "world") and set typing_format via patch.
        assert!(editor.place_stage_text_caret_utf16(6));
        assert_eq!(editor.stage_text_edit.selection_range_utf16(), (6, 6));
        assert!(
            !editor
                .apply_stage_text_format_patch(&TextFormatPatch {
                    font: Some("Courier".into()),
                    size: Some(22.0),
                    color: Some(0x00aa55),
                    bold: Some(true),
                    ..TextFormatPatch::default()
                })
                .unwrap(),
            "changing Character format at a collapsed caret updates typing_format without mutating existing text"
        );
        assert_eq!(editor.project, baseline);
        assert_eq!(editor.history.undo_len(), undo_before_typing);

        // Type "big " character-by-character; all keystrokes coalesce into one "Edit text" Undo step.
        for (idx, ch) in ['b', 'i', 'g', ' '].into_iter().enumerate() {
            stage_frame(
                &mut editor,
                &ctx,
                1.0 + idx as f64 * 0.05,
                vec![egui::Event::Text(ch.to_string())],
            );
        }
        assert_eq!(editor.history.undo_len(), undo_before_typing + 1);
        let sampled = editor.selected_sampled().unwrap();
        let state = TextFieldState::from_kind(&sampled.kind).unwrap();
        assert_eq!(state.text, "Hello big world");
        assert_eq!(state.format_runs.len(), 3);
        assert_eq!(
            (state.format_runs[0].start, state.format_runs[0].end),
            (0, 6)
        );
        assert_eq!(state.format_runs[0].format.font, "Inter");
        assert_eq!(
            (state.format_runs[1].start, state.format_runs[1].end),
            (6, 10)
        );
        assert_eq!(state.format_runs[1].format.font, "Courier");
        assert_eq!(state.format_runs[1].format.size, 22.0);
        assert_eq!(state.format_runs[1].format.color, 0x00aa55);
        assert!(state.format_runs[1].format.bold);
        assert_eq!(
            (state.format_runs[2].start, state.format_runs[2].end),
            (10, 15)
        );
        assert_eq!(state.format_runs[2].format.font, "Inter");

        // Test Home, Shift+ArrowRight, Command+A, and Escape.
        let key_ev = |key: Key, modifiers: Modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        stage_frame(
            &mut editor,
            &ctx,
            2.0,
            vec![key_ev(Key::Home, Modifiers::NONE)],
        );
        assert_eq!(editor.stage_text_edit.selection_range_utf16(), (0, 0));

        stage_frame(
            &mut editor,
            &ctx,
            2.1,
            vec![
                key_ev(Key::ArrowRight, Modifiers::SHIFT),
                key_ev(Key::ArrowRight, Modifiers::SHIFT),
                key_ev(Key::ArrowRight, Modifiers::SHIFT),
                key_ev(Key::ArrowRight, Modifiers::SHIFT),
                key_ev(Key::ArrowRight, Modifiers::SHIFT),
            ],
        );
        assert_eq!(editor.stage_text_edit.selection_range_utf16(), (0, 5));

        stage_frame(
            &mut editor,
            &ctx,
            2.2,
            vec![key_ev(Key::A, Modifiers::COMMAND)],
        );
        assert_eq!(editor.stage_text_edit.selection_range_utf16(), (0, 15));

        stage_frame(
            &mut editor,
            &ctx,
            2.3,
            vec![key_ev(Key::Escape, Modifiers::NONE)],
        );
        assert!(!editor.stage_text_edit.is_active(&editor));
        assert!(
            editor.selected_element().is_some(),
            "Escape exits inline text editing while keeping the text element selected"
        );

        // One Undo step reverts the entire coalesced "big " typing sequence.
        editor.undo();
        assert_eq!(editor.project, baseline);
    }

    #[test]
    fn stage_text_upgrade_preserves_point_alpha_and_rejected_typing_preserves_selection() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.add_element(
            ElementKind::Text {
                text: "AB".into(),
                font_family: "Inter".into(),
                font_size: 16.0,
                color: [20, 40, 60, 128],
            },
            20.0,
            30.0,
        );
        let baseline = editor.project.clone();
        assert!(editor.place_stage_text_caret_utf16(1));
        assert!(
            !editor
                .apply_stage_text_format_patch(&TextFormatPatch {
                    bold: Some(true),
                    ..TextFormatPatch::default()
                })
                .unwrap()
        );
        assert!(editor.stage_text_insert_or_replace("X"));
        let upgraded = editor.selected_element().unwrap();
        assert!(matches!(upgraded.kind, ElementKind::TextBox { .. }));
        assert!(
            (upgraded.transform.alpha - 128.0 / 255.0).abs() < 1.0e-9,
            "point-text alpha must move to the element when rich typing creates a TextBox"
        );
        editor.undo();
        assert_eq!(editor.project, baseline);

        let original = editor.selected_sampled().unwrap();
        let shown = TextFieldState::from_kind(&original.kind).unwrap().format;
        let mut inspector_box = ElementKind::TextBox {
            text: "AB".into(),
            width: 80.0,
            height: 24.0,
            format: shown.clone(),
            word_wrap: false,
            multiline: false,
        };
        let mut inspector_alpha = original.transform.alpha;
        editor.reconcile_inspector_text_edit(
            &original.id,
            &original.kind,
            &shown,
            &mut inspector_box,
            &mut inspector_alpha,
        );
        assert!(matches!(inspector_box, ElementKind::TextBox { .. }));
        assert!((inspector_alpha - 128.0 / 255.0).abs() < 1.0e-9);

        let mut format = TextFormat {
            font: "Inter".into(),
            size: 16.0,
            ..TextFormat::default()
        };
        format.set_field_options(Some(AuthoredTextFieldOptions {
            restrict: Some("0-9".into()),
            ..AuthoredTextFieldOptions::default()
        }));
        let mut restricted = Editor::from_project(&ctx, Project::blank());
        restricted.add_element(
            ElementKind::TextBox {
                text: "A12B".into(),
                width: 160.0,
                height: 40.0,
                format,
                word_wrap: false,
                multiline: false,
            },
            10.0,
            10.0,
        );
        assert!(restricted.select_stage_text_utf16_range(1, 3));
        let before = restricted.project.clone();
        let undo_before = restricted.history.undo_len();
        assert!(restricted.stage_text_insert_or_replace("x"));
        assert_eq!(restricted.project, before);
        assert_eq!(restricted.history.undo_len(), undo_before);
        assert_eq!(restricted.stage_text_edit.selection_range_utf16(), (1, 3));
    }

    #[test]
    fn stage_text_geometry_matches_reference_formats_across_mixed_utf16_runs() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let base = TextFormat {
            font: "Inter".into(),
            size: 11.0,
            color: 0x112233,
            ..TextFormat::default()
        };
        let mut large = base.clone();
        large.size = 23.0;
        large.bold = true;
        large.letter_spacing = 2.5;
        let mut trailing = base.clone();
        trailing.size = 17.0;
        trailing.italic = true;
        trailing.letter_spacing = -0.25;
        let mut kind = ElementKind::TextBox {
            text: "A😀BCé".into(),
            width: 480.0,
            height: 80.0,
            format: base.clone(),
            word_wrap: false,
            multiline: false,
        };
        let mut state = TextFieldState::from_kind(&kind).unwrap();
        state.format_runs = vec![
            TextFormatRun {
                start: 0,
                end: 1,
                format: base,
            },
            TextFormatRun {
                start: 1,
                end: 4,
                format: large,
            },
            TextFormatRun {
                start: 4,
                end: 6,
                format: trailing,
            },
        ];
        state.validate().unwrap();
        state.write_to_kind(&mut kind);

        let geom = editor.stage_text_geometry(None, &kind).unwrap();
        let boxes: Vec<_> = geom.lines.iter().flat_map(|line| &line.chars).collect();
        assert_eq!(
            boxes
                .iter()
                .map(|ch| (ch.start_utf16, ch.end_utf16))
                .collect::<Vec<_>>(),
            vec![(0, 1), (1, 3), (3, 4), (4, 5), (5, 6)]
        );
        assert_eq!(geom.format_run_lookups, boxes.len());
        assert_eq!(geom.format_run_advances, 2);

        let mut expected_x = geom.lines[0].x;
        for (char_box, ch) in boxes.iter().zip(state.text.chars()) {
            let format = state.format_at_utf16_offset(char_box.start_utf16);
            let text = ch.to_string();
            let expected_width = (editor.fonts.measure_width(
                &format.font,
                format.bold,
                format.italic,
                format.size,
                &text,
            ) + format.letter_spacing)
                .max(1.0);
            assert!((char_box.x - expected_x).abs() <= f64::EPSILON);
            assert!((char_box.width - expected_width).abs() <= f64::EPSILON);
            expected_x += expected_width;
        }
    }

    #[test]
    fn stage_text_geometry_long_mixed_text_stays_within_linear_format_lookup_budget() {
        const RUN_COUNT: usize = 256;
        const CHARS_PER_RUN: usize = 64;
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let char_count = RUN_COUNT * CHARS_PER_RUN;
        let mut kind = ElementKind::TextBox {
            text: "x".repeat(char_count),
            width: 1_000_000.0,
            height: 80.0,
            format: TextFormat {
                font: "Inter".into(),
                size: 12.0,
                ..TextFormat::default()
            },
            word_wrap: false,
            multiline: false,
        };
        let mut state = TextFieldState::from_kind(&kind).unwrap();
        state.format_runs = (0..RUN_COUNT)
            .map(|index| {
                let mut format = state.format.clone();
                format.size += (index % 2) as f64;
                format.letter_spacing = if index % 2 == 0 { 0.25 } else { 1.25 };
                TextFormatRun {
                    start: (index * CHARS_PER_RUN) as u32,
                    end: ((index + 1) * CHARS_PER_RUN) as u32,
                    format,
                }
            })
            .collect();
        state.validate().unwrap();
        state.write_to_kind(&mut kind);

        let geom = editor.stage_text_geometry(None, &kind).unwrap();
        let materialized_run_count = state.materialized_runs(char_count as u32).len();
        let geometry_char_count: usize = geom.lines.iter().map(|line| line.chars.len()).sum();
        assert_eq!(geometry_char_count, char_count);
        assert_eq!(geom.format_run_lookups, char_count);
        assert_eq!(
            geom.format_run_advances,
            materialized_run_count.saturating_sub(1)
        );
        assert!(
            geom.format_run_lookups + geom.format_run_advances
                <= char_count + materialized_run_count,
            "format resolution must stay within one lookup per character plus one advance per run"
        );
    }

    #[test]
    fn stage_text_hit_testing_works_for_transformed_and_nested_edit_in_place_text() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());

        // Create a child MovieClip symbol with a rotated/scaled TextBox and edit it in place.
        let child_id = new_id("sym");
        let text_id = new_id("el");
        let child_sym = Symbol {
            id: child_id.clone(),
            name: "NestedBadge".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: new_id("layer"),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: text_id.clone(),
                            name: String::new(),
                            transform: Transform {
                                x: 20.0,
                                y: 15.0,
                                scale_x: 1.25,
                                scale_y: 1.25,
                                rotation: 18.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::TextBox {
                                text: "Alpha Beta Gamma".into(),
                                width: 240.0,
                                height: 60.0,
                                format: TextFormat {
                                    font: "Inter".into(),
                                    size: 20.0,
                                    color: 0xffffff,
                                    ..TextFormat::default()
                                },
                                word_wrap: false,
                                multiline: false,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: vec![],
                guides: vec![],
            },
            class_name: None,
            linkage_id: None,
        };
        editor.project.symbols.push(child_sym);
        editor.add_element_transformed(
            ElementKind::Instance {
                symbol_id: child_id.clone(),
                playback: InstancePlayback::Loop,
            },
            Transform {
                x: 120.0,
                y: 90.0,
                rotation: -12.0,
                ..Transform::default()
            },
        );
        editor.enter_selected_symbol_in_place();
        assert_eq!(editor.symbol_id(), child_id);

        editor.select_single_id(text_id.clone());
        let sampled = editor.selected_sampled().unwrap();
        let geom = editor.stage_text_geometry(None, &sampled.kind).unwrap();
        assert_eq!(geom.total_utf16, 16);

        // Verify local caret coordinates map monotonically across characters and hit-test back to the exact UTF-16 offset.
        let (caret_0, _) = geom.caret_segment_local(0);
        let (caret_6, _) = geom.caret_segment_local(6);
        let (caret_11, _) = geom.caret_segment_local(11);
        assert!(caret_0.x < caret_6.x && caret_6.x < caret_11.x);
        assert_eq!(geom.hit_test_utf16(caret_0 + Vec2::new(1.0, 4.0)), 0);
        assert_eq!(geom.hit_test_utf16(caret_6 + Vec2::new(1.0, 4.0)), 6);
        assert_eq!(geom.hit_test_utf16(caret_11 + Vec2::new(1.0, 4.0)), 11);

        // Double-click word bounds on "Beta" (offset 7) selects 6..10 ("Beta").
        assert_eq!(word_utf16_bounds_at("Alpha Beta Gamma", 7), (6, 10));

        // Format "Beta" inside nested Edit in Place and verify Paragraph alignment on the paragraph.
        assert!(editor.select_stage_text_utf16_range(6, 10));
        assert!(
            editor
                .apply_stage_text_format_patch(&TextFormatPatch {
                    bold: Some(true),
                    color: Some(0xffcc00),
                    align: Some(TextAlign::Center),
                    ..TextFormatPatch::default()
                })
                .unwrap()
        );
        let updated = TextFieldState::from_kind(&editor.selected_sampled().unwrap().kind).unwrap();
        assert_eq!(updated.format_runs.len(), 3);
        assert!(updated.format_runs[1].format.bold);
        assert_eq!(updated.format_runs[1].format.color, 0xffcc00);
        assert_eq!(updated.format_runs[0].format.align, TextAlign::Center);
        assert_eq!(updated.format_runs[1].format.align, TextAlign::Center);
        assert_eq!(updated.format_runs[2].format.align, TextAlign::Center);
    }

    #[test]
    fn stage_text_selection_expands_paragraph_format_only_to_intersected_paragraphs() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.add_element(
            ElementKind::TextBox {
                text: "One\nTwo\nThree".into(),
                width: 240.0,
                height: 120.0,
                format: TextFormat::default(),
                word_wrap: false,
                multiline: true,
            },
            20.0,
            20.0,
        );

        // Exact character selection intersects the first two paragraphs only.
        assert!(editor.select_stage_text_utf16_range(2, 6));
        assert!(
            editor
                .apply_stage_text_format_patch(&TextFormatPatch {
                    bold: Some(true),
                    align: Some(TextAlign::Right),
                    ..TextFormatPatch::default()
                })
                .unwrap()
        );

        let state = TextFieldState::from_kind(&editor.selected_sampled().unwrap().kind).unwrap();
        let first = state.format_at_utf16_offset(1);
        let selected_first = state.format_at_utf16_offset(2);
        let selected_second = state.format_at_utf16_offset(5);
        let third = state.format_at_utf16_offset(10);
        assert!(
            !first.bold,
            "character formatting must remain selection-scoped"
        );
        assert!(selected_first.bold);
        assert!(selected_second.bold);
        assert_eq!(first.align, TextAlign::Right);
        assert_eq!(selected_second.align, TextAlign::Right);
        assert_eq!(third.align, TextAlign::Left);
        assert!(!third.bold);
    }

    #[test]
    fn stage_text_mixed_runs_survive_save_reopen_break_apart_and_html5_svg_export() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.add_element(
            ElementKind::Text {
                text: "RedBlue".into(),
                font_family: "Inter".into(),
                font_size: 20.0,
                color: [220, 20, 20, 255],
            },
            50.0,
            60.0,
        );

        // Selecting "Blue" (3..7) on point text and formatting it upgrades to a non-wrapping TextBox with 2 runs.
        assert!(editor.select_stage_text_utf16_range(3, 7));
        assert!(
            editor
                .apply_stage_text_format_patch(&TextFormatPatch {
                    font: Some("Georgia".into()),
                    size: Some(30.0),
                    color: Some(0x1e63d6),
                    bold: Some(true),
                    italic: Some(true),
                    ..TextFormatPatch::default()
                })
                .unwrap()
        );

        // 1. Save / reopen round-trip.
        let json = editor.project.to_json().unwrap();
        let reopened = Project::from_json(&json).unwrap();
        reopened.validate().unwrap();
        let reopened_el = &reopened.symbols[0].timeline.layers[0].keyframes[0].elements[0];
        let reopened_state = TextFieldState::from_kind(&reopened_el.kind).unwrap();
        assert_eq!(reopened_state.format_runs.len(), 2);
        assert_eq!(reopened_state.format_runs[0].format.font, "Inter");
        assert_eq!(reopened_state.format_runs[0].format.color, 0xdc1414);
        assert_eq!(reopened_state.format_runs[1].format.font, "Georgia");
        assert_eq!(reopened_state.format_runs[1].format.size, 30.0);
        assert_eq!(reopened_state.format_runs[1].format.color, 0x1e63d6);
        assert!(reopened_state.format_runs[1].format.bold);
        assert!(reopened_state.format_runs[1].format.italic);

        // 2. SVG and HTML5 export preserve per-run formatting.
        let (svg, _, _) = neoflash_export::render_frame_svg(
            &editor.project,
            &neoflash_export::SvgOptions::default(),
        )
        .unwrap();
        assert!(
            svg.contains("<tspan") && svg.contains("Georgia") && svg.contains("#1e63d6"),
            "SVG export must emit per-run <tspan> attributes: {svg}"
        );
        let export_dir =
            std::env::temp_dir().join(format!("neoflash-t202-export-{}", std::process::id()));
        let runtime_dir =
            std::env::temp_dir().join(format!("neoflash-t202-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&export_dir);
        let _ = std::fs::remove_dir_all(&runtime_dir);
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("neoflash_web.js"),
            b"let wasm_bindgen = (function() {\n    class WebPlayer {}\n    return Object.assign(init, { WebPlayer });\n})();\nNeoFlashWasm = wasm_bindgen;\n",
        )
        .unwrap();
        std::fs::write(
            runtime_dir.join("neoflash_web_bg.wasm"),
            b"\0asm\x01\0\0\0\x01\x04\x01\x60\x00\x00\x03\x02\x01\x00\x07\x0d\x01\x09webplayer\x00\x00\x0a\x04\x01\x02\x00\x0b",
        )
        .unwrap();
        neoflash_export::export_project(&editor.project, &export_dir, &runtime_dir).unwrap();
        let project_json = std::fs::read_to_string(export_dir.join("project.json"))
            .or_else(|_| std::fs::read_to_string(export_dir.join("bundle.js")))
            .unwrap();
        let _ = std::fs::remove_dir_all(&export_dir);
        let _ = std::fs::remove_dir_all(&runtime_dir);
        assert!(
            project_json.contains("format_runs") && project_json.contains("Georgia"),
            "HTML5 export bundle must preserve per-run TextFormatRun data"
        );

        // 3. First Break Apart splits "RedBlue" into 7 character text blocks preserving each run's attributes.
        editor.exit_stage_text_edit();
        editor.break_apart_selection();
        let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        assert_eq!(elements.len(), 7);
        let first_char_state = TextFieldState::from_kind(&elements[0].kind).unwrap();
        assert_eq!(first_char_state.text, "R");
        assert_eq!(first_char_state.format.font, "Inter");
        assert_eq!(first_char_state.format.size, 20.0);
        assert_eq!(first_char_state.format.color, 0xdc1414);
        assert!(!first_char_state.format.bold);

        let fourth_char_state = TextFieldState::from_kind(&elements[3].kind).unwrap();
        assert_eq!(fourth_char_state.text, "B");
        assert_eq!(fourth_char_state.format.font, "Georgia");
        assert_eq!(fourth_char_state.format.size, 30.0);
        assert_eq!(fourth_char_state.format.color, 0x1e63d6);
        assert!(fourth_char_state.format.bold);
        assert!(fourth_char_state.format.italic);
    }
}
