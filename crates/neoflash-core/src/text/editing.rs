use crate::ElementKind;
use serde::{Deserialize, Serialize};

use super::graphemes::{Restriction, clamp_utf16, utf16_byte};
use super::*;

impl TextFieldState {
    pub fn from_kind(kind: &ElementKind) -> Option<Self> {
        match kind {
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => {
                let format = TextFormat {
                    font: font_family.clone(),
                    size: *font_size,
                    color: u32::from(color[0]) << 16
                        | u32::from(color[1]) << 8
                        | u32::from(color[2]),
                    ..TextFormat::default()
                };
                Some(Self {
                    text: normalize_text(text),
                    box_size: None,
                    default_format: format.clone(),
                    format,
                    multiline: true,
                    word_wrap: false,
                    alpha: color[3],
                    revision: "1".into(),
                    ..Self::default()
                })
            }
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                word_wrap,
                multiline,
            } => {
                let mut clean_format = format.clone();
                let opts = clean_format.field_options();
                clean_format.set_field_options(None);
                let mut state = Self {
                    text: normalize_text(text),
                    box_size: Some(TextBoxSize {
                        width: *width,
                        height: *height,
                    }),
                    format: clean_format.clone(),
                    default_format: clean_format,
                    multiline: *multiline,
                    word_wrap: *word_wrap,
                    alpha: 255,
                    revision: "1".into(),
                    ..Self::default()
                };
                if let Some(opts) = opts {
                    opts.apply_to_state(&mut state);
                }
                Some(state)
            }
            _ => None,
        }
    }

    pub fn write_to_kind(&self, kind: &mut ElementKind) {
        match kind {
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                word_wrap,
                multiline,
            } => {
                let prev_opts = format.field_options();
                let prev_var = prev_opts.as_ref().and_then(|o| o.variable.clone());
                let prev_html = prev_opts.as_ref().is_some_and(|o| o.html);
                *text = self.text.clone();
                if let Some(size) = self.box_size {
                    *width = size.width;
                    *height = size.height;
                }
                *word_wrap = self.word_wrap;
                *multiline = self.multiline;
                let mut next_fmt = self.format.clone();
                let opts = AuthoredTextFieldOptions::from_state(self, prev_var, prev_html);
                next_fmt.set_field_options((!opts.is_default()).then_some(opts));
                *format = next_fmt;
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => {
                let opts = AuthoredTextFieldOptions::from_state(self, None, false);
                if self.format_runs.is_empty()
                    && self.format.is_plain_point_text_compatible()
                    && opts.is_default()
                {
                    *text = self.text.clone();
                    *font_family = self.format.font.clone();
                    *font_size = self.format.size;
                    *color = [
                        ((self.format.color >> 16) & 0xff) as u8,
                        ((self.format.color >> 8) & 0xff) as u8,
                        (self.format.color & 0xff) as u8,
                        self.alpha,
                    ];
                } else {
                    let (nat_w, nat_h) = self.natural_box_size();
                    let size = self.box_size.unwrap_or(TextBoxSize {
                        width: nat_w,
                        height: nat_h,
                    });
                    let mut next_fmt = self.format.clone();
                    next_fmt.set_field_options((!opts.is_default()).then_some(opts));
                    *kind = ElementKind::TextBox {
                        text: self.text.clone(),
                        width: size.width,
                        height: size.height,
                        format: next_fmt,
                        word_wrap: self.word_wrap,
                        multiline: self.multiline,
                    };
                }
            }
            _ => {}
        }
    }

    pub fn natural_box_size(&self) -> (f64, f64) {
        let mut probe = self.clone();
        probe.box_size = None;
        probe.word_wrap = false;
        let max_font_size = self
            .format_runs
            .iter()
            .map(|r| r.format.size)
            .fold(self.format.size, f64::max)
            .max(4.0);
        if let Ok(layout) = layout_text(&probe, |segments, seg_format| {
            Ok(segments
                .iter()
                .map(|s| s.chars().count() as f64 * seg_format.size * 0.62)
                .collect())
        }) {
            let w = layout
                .lines
                .iter()
                .map(|l| l.x.max(0.0) + l.width)
                .fold(0.0_f64, f64::max);
            let h = layout
                .lines
                .last()
                .map(|l| l.y.max(0.0) + l.height)
                .unwrap_or(max_font_size * 1.2);
            (
                (w + max_font_size * 0.5 + 8.0).clamp(24.0, 1_000_000.0),
                (h + 8.0).clamp(max_font_size * 1.2 + 4.0, 1_000_000.0),
            )
        } else {
            (180.0, (max_font_size * 1.4).max(24.0))
        }
    }

    pub(crate) fn storage_bytes(&self) -> usize {
        self.text.len() + self.restrict.as_ref().map_or(0, String::len)
    }
    pub fn validate(&self) -> Result<(), TextError> {
        if !matches!(
            self.as2_viewport.auto_size.as_str(),
            "none" | "left" | "center" | "right"
        ) {
            return Err(error("autoSize requires none, left, center or right"));
        }
        if !matches!(self.anti_alias_type.as_str(), "normal" | "advanced") {
            return Err(error("antiAliasType requires normal or advanced"));
        }
        if !matches!(self.grid_fit_type.as_str(), "none" | "pixel" | "subpixel") {
            return Err(error("gridFitType requires none, pixel or subpixel"));
        }
        if !matches!(self.interaction_mode.as_str(), "normal" | "selection") {
            return Err(error("interactionMode requires normal or selection"));
        }
        if !self.sharpness.is_finite() || !(-400.0..=400.0).contains(&self.sharpness) {
            return Err(error("sharpness requires a finite number in -400..=400"));
        }
        if !self.thickness.is_finite() || !(-200.0..=200.0).contains(&self.thickness) {
            return Err(error("thickness requires a finite number in -200..=200"));
        }
        if self.as2_viewport.background_color > 0xffffff
            || self.as2_viewport.border_color > 0xffffff
            || self.as2_viewport.scroll == 0
            || self.as2_viewport.scroll > 1_000_000
            || self.as2_viewport.hscroll > 1_000_000
        {
            return Err(error(
                "AS2 viewport colors require RGB; scroll requires 1..=1000000; hscroll requires 0..=1000000",
            ));
        }
        if self.text.len() > MAX_TEXT_BYTES {
            return Err(error("text exceeds 1048576 UTF-8 bytes"));
        }
        if self.max_chars > MAX_TEXT_BYTES as u32 {
            return Err(error("maxChars must be in 0..=1048576 UTF-16 units"));
        }
        if let Some(pattern) = &self.restrict {
            Restriction::compile(pattern)?;
        }
        if self.selection_begin > self.selection_end
            || (self.caret != self.selection_begin && self.caret != self.selection_end)
        {
            return Err(error("invalid selection endpoints/caret"));
        }
        utf16_byte(&self.text, self.selection_begin)?;
        utf16_byte(&self.text, self.selection_end)?;
        if self.field_type == TextFieldType::Input && self.format.bullet {
            return Err(error("bullet currently requires a non-input TextField"));
        }
        self.format.validate()?;
        self.default_format.validate()?;
        if !self.format_runs.is_empty() {
            let total_utf16 = self.text.encode_utf16().count() as u32;
            let mut cursor = 0u32;
            for run in &self.format_runs {
                if run.start != cursor || run.end <= run.start || run.end > total_utf16 {
                    return Err(error("format_runs must partition the UTF-16 text range"));
                }
                utf16_byte(&self.text, run.start)?;
                utf16_byte(&self.text, run.end)?;
                run.format.validate()?;
                cursor = run.end;
            }
            if cursor != total_utf16 {
                return Err(error("format_runs must cover the entire UTF-16 text range"));
            }
        }
        if let Some(size) = self.box_size {
            size.validate()?
        }
        if self.revision.parse::<u64>().is_err() {
            return Err(error("invalid text revision"));
        }
        Ok(())
    }
    pub fn apply(&mut self, patch: &TextPatch) -> Result<(), TextError> {
        if usize::from(patch.text.is_some())
            + usize::from(patch.append.is_some())
            + usize::from(patch.replace.is_some())
            + usize::from(patch.replace_selected.is_some())
            > 1
        {
            return Err(error(
                "a text update must contain at most one replacement or append operation",
            ));
        }
        let selected_replacement = patch.replace_selected.as_ref().map(|text| TextReplacement {
            begin: self.selection_begin,
            end: self.selection_end,
            text: text.clone(),
        });
        let mut next = self.clone();
        if let Some(viewport) = &patch.as2_viewport {
            next.as2_viewport = viewport.clone();
        }
        if let Some(value) = patch.field_type {
            next.field_type = value;
        }
        if let Some(value) = patch.selectable {
            next.selectable = value;
        }
        if let Some(value) = patch.display_as_password {
            next.display_as_password = value;
        }
        if let Some(value) = patch.max_chars {
            next.max_chars = value;
        }
        if let Some(value) = &patch.restrict {
            next.restrict = value.value.clone();
        }
        if let Some(value) = &patch.anti_alias_type {
            next.anti_alias_type = value.clone();
        }
        if let Some(value) = &patch.grid_fit_type {
            next.grid_fit_type = value.clone();
        }
        if let Some(value) = patch.sharpness {
            if !value.is_finite() {
                return Err(error("sharpness requires a finite number"));
            }
            next.sharpness = value.clamp(-400.0, 400.0);
        }
        if let Some(value) = patch.thickness {
            if !value.is_finite() {
                return Err(error("thickness requires a finite number"));
            }
            next.thickness = value.clamp(-200.0, 200.0);
        }
        if let Some(value) = patch.embed_fonts {
            next.embed_fonts = value;
        }
        if let Some(value) = patch.condense_white {
            next.condense_white = value;
        }
        if let Some(value) = patch.always_show_selection {
            next.always_show_selection = value;
        }
        if let Some(value) = &patch.interaction_mode {
            next.interaction_mode = value.clone();
        }

        if let Some(format) = &patch.default_format {
            format.apply(&mut next.default_format)
        }
        if let Some(value) = patch.width {
            next.box_size
                .get_or_insert(TextBoxSize {
                    width: 100.,
                    height: 100.,
                })
                .width = value
        }
        if let Some(value) = patch.height {
            next.box_size
                .get_or_insert(TextBoxSize {
                    width: 100.,
                    height: 100.,
                })
                .height = value
        }
        if let Some(value) = patch.multiline {
            next.multiline = value
        }
        if let Some(value) = patch.word_wrap {
            next.word_wrap = value
        }
        if let Some(text) = &patch.text {
            if text.len() > MAX_TEXT_BYTES {
                return Err(error("text exceeds 1048576 UTF-8 bytes"));
            }
            next.text = normalize_text(text);
            next.format = next.default_format.clone();
            next.format_runs.clear();
        }
        if let Some(text) = &patch.append {
            if next.text.len().saturating_add(text.len()) > MAX_TEXT_BYTES {
                return Err(error("append exceeds 1048576 UTF-8 bytes"));
            }
            let normalized = normalize_text(text);
            if !normalized.is_empty() {
                let old_units = next.text.encode_utf16().count() as u32;
                let added_units = normalized.encode_utf16().count() as u32;
                if old_units == 0 {
                    next.format = next.default_format.clone();
                    next.format_runs.clear();
                } else if !next.format_runs.is_empty() || next.format != next.default_format {
                    let mut runs = next.materialized_runs(old_units);
                    runs.push(TextFormatRun {
                        start: old_units,
                        end: old_units + added_units,
                        format: next.default_format.clone(),
                    });
                    next.format_runs = runs;
                }
                next.text.push_str(&normalized);
                next.normalize_runs();
            }
        }
        let mut applied_selected_caret: Option<u32> = None;
        if let Some(replacement) = patch.replace.as_ref().or(selected_replacement.as_ref()) {
            utf16_byte(&next.text, replacement.begin)?;
            utf16_byte(&next.text, replacement.end)?;
            if replacement.begin > replacement.end {
                return Err(error("replacement range must be ordered"));
            }
            let rep_begin = snap_utf16_to_grapheme_floor(&next.text, i64::from(replacement.begin));
            let rep_end = if replacement.end > replacement.begin {
                snap_utf16_to_grapheme_ceil(&next.text, i64::from(replacement.end))
            } else {
                rep_begin
            };
            let start = utf16_byte(&next.text, rep_begin)?;
            let end = utf16_byte(&next.text, rep_end)?;
            if next.text.len() - (end - start) + replacement.text.len() > MAX_TEXT_BYTES {
                return Err(error("replacement exceeds 1048576 UTF-8 bytes"));
            }
            let normalized = normalize_text(&replacement.text);
            let old_units = next.text.encode_utf16().count() as u32;
            let insert_units = normalized.encode_utf16().count() as u32;
            let retained = start > 0 || end < next.text.len();
            if !retained {
                if selected_replacement.is_some() || old_units == 0 {
                    next.format = next.default_format.clone();
                }
                next.format_runs.clear();
            } else {
                let insert_fmt = if selected_replacement.is_some() {
                    next.default_format.clone()
                } else if rep_begin < rep_end {
                    next.format_at_utf16(rep_begin, old_units)
                } else {
                    next.format_at_utf16_caret(rep_begin as usize)
                };
                if !next.format_runs.is_empty() || insert_fmt != next.format {
                    let old_runs = next.materialized_runs(old_units);
                    let removed = rep_end - rep_begin;
                    let mut runs = Vec::new();
                    for r in &old_runs {
                        if r.end <= rep_begin {
                            runs.push(r.clone());
                        } else if r.start < rep_begin {
                            runs.push(TextFormatRun {
                                start: r.start,
                                end: rep_begin,
                                format: r.format.clone(),
                            });
                        }
                    }
                    if insert_units > 0 {
                        runs.push(TextFormatRun {
                            start: rep_begin,
                            end: rep_begin + insert_units,
                            format: insert_fmt,
                        });
                    }
                    for r in &old_runs {
                        if r.start >= rep_end {
                            runs.push(TextFormatRun {
                                start: r.start - removed + insert_units,
                                end: r.end - removed + insert_units,
                                format: r.format.clone(),
                            });
                        } else if r.end > rep_end {
                            runs.push(TextFormatRun {
                                start: rep_begin + insert_units,
                                end: r.end - removed + insert_units,
                                format: r.format.clone(),
                            });
                        }
                    }
                    next.format_runs = runs;
                }
            }
            next.text.replace_range(start..end, &normalized);
            next.normalize_runs();
            if selected_replacement.is_some() {
                applied_selected_caret = Some(rep_begin + insert_units);
            }
        }
        if let Some(format) = &patch.format {
            format.apply(&mut next.format);
            for run in &mut next.format_runs {
                format.apply(&mut run.format);
            }
            next.normalize_runs();
        }
        if let Some(runs) = &patch.format_runs {
            next.format_runs = runs.clone();
            next.normalize_runs();
        }
        if let Some(range_patch) = &patch.format_range {
            utf16_byte(&next.text, range_patch.begin)?;
            utf16_byte(&next.text, range_patch.end)?;
            if range_patch.begin > range_patch.end {
                return Err(error("format_range endpoints must be ordered"));
            }
            let total_units = next.text.encode_utf16().count() as u32;
            if range_patch.begin < range_patch.end && total_units > 0 {
                let old_runs = next.materialized_runs(total_units);
                let mut runs = Vec::new();
                for r in old_runs {
                    if r.end <= range_patch.begin || r.start >= range_patch.end {
                        runs.push(r);
                    } else {
                        if r.start < range_patch.begin {
                            runs.push(TextFormatRun {
                                start: r.start,
                                end: range_patch.begin,
                                format: r.format.clone(),
                            });
                        }
                        let mid_start = r.start.max(range_patch.begin);
                        let mid_end = r.end.min(range_patch.end);
                        let mut mid_fmt = r.format.clone();
                        range_patch.format.apply(&mut mid_fmt);
                        runs.push(TextFormatRun {
                            start: mid_start,
                            end: mid_end,
                            format: mid_fmt,
                        });
                        if r.end > range_patch.end {
                            runs.push(TextFormatRun {
                                start: range_patch.end,
                                end: r.end,
                                format: r.format,
                            });
                        }
                    }
                }
                next.format_runs = runs;
                next.normalize_runs();
            }
        }
        if patch.text.is_some() {
            next.selection_begin = 0;
            next.selection_end = 0;
            next.caret = 0;
        }
        if let Some(caret) = applied_selected_caret {
            next.selection_begin = caret;
            next.selection_end = caret;
            next.caret = caret;
        }
        next.selection_begin = clamp_utf16(&next.text, i64::from(next.selection_begin));
        next.selection_end = clamp_utf16(&next.text, i64::from(next.selection_end));
        next.caret = clamp_utf16(&next.text, i64::from(next.caret));
        if let Some(selection) = patch.selection {
            let begin = clamp_utf16(&next.text, selection.begin);
            let end = clamp_utf16(&next.text, selection.end);
            next.selection_begin = begin.min(end);
            next.selection_end = begin.max(end);
            next.caret = end;
        }
        next.validate()?;
        if next != *self {
            next.revision = self
                .revision
                .parse::<u64>()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| error("text revision exhausted"))?
                .to_string();
            *self = next;
        }
        Ok(())
    }
    /// Commit a user replacement against the exact revision observed by the
    /// DOM composition transaction. Programmatic mutations use apply instead.
    pub fn apply_input(&mut self, edit: &TextInputEdit) -> Result<(), TextError> {
        if self.field_type != TextFieldType::Input {
            return Err(error("user editing requires an input TextField"));
        }
        if edit.expected_revision != self.revision {
            return Err(error(
                "stale text input revision; refresh the editor before committing",
            ));
        }
        if edit.text.len() > MAX_TEXT_BYTES {
            return Err(error("input insertion exceeds 1048576 UTF-8 bytes"));
        }
        utf16_byte(&self.text, edit.begin)?;
        utf16_byte(&self.text, edit.end)?;
        if edit.begin > edit.end {
            return Err(error("input replacement range must be ordered"));
        }
        let floor_begin = snap_utf16_to_grapheme_floor(&self.text, i64::from(edit.begin));
        let ceil_end = if edit.end > edit.begin {
            snap_utf16_to_grapheme_ceil(&self.text, i64::from(edit.end))
        } else {
            floor_begin
        };
        let (eff_begin, eff_end, candidate_text) = if edit.text.is_empty() {
            (floor_begin, ceil_end, String::new())
        } else if floor_begin < edit.begin || ceil_end > edit.end {
            (
                floor_begin,
                ceil_end,
                format!(
                    "{}{}{}",
                    utf16_slice_safe(&self.text, floor_begin as usize, edit.begin as usize),
                    normalize_text(&edit.text),
                    utf16_slice_safe(&self.text, edit.end as usize, ceil_end as usize),
                ),
            )
        } else {
            (edit.begin, edit.end, normalize_text(&edit.text))
        };
        let restriction = self
            .restrict
            .as_deref()
            .map(Restriction::compile)
            .transpose()?;
        let total_units = self.text.encode_utf16().count();
        let retained = total_units.saturating_sub((eff_end - eff_begin) as usize);
        let capacity = if self.max_chars == 0 {
            MAX_TEXT_BYTES
        } else {
            (self.max_chars as usize).saturating_sub(retained)
        };
        let mut inserted = String::new();
        let mut units = 0usize;
        for (_, _, cluster) in grapheme_clusters_utf16(&candidate_text) {
            if cluster == "\n" {
                if !self.multiline {
                    continue;
                }
            } else if let Some(rule) = &restriction
                && !cluster.chars().all(|ch| rule.allows(ch))
            {
                continue;
            }
            let cluster_units = cluster.encode_utf16().count();
            if units + cluster_units > capacity {
                break;
            }
            inserted.push_str(cluster);
            units += cluster_units;
        }
        // Rejected typing must not erase an existing selection.
        if !edit.text.is_empty() && inserted.is_empty() {
            return Ok(());
        }
        let byte_start = utf16_byte(&self.text, eff_begin)?;
        let byte_end = utf16_byte(&self.text, eff_end)?;
        if self.text.len() - (byte_end - byte_start) + inserted.len() > MAX_TEXT_BYTES {
            return Err(error("replacement exceeds 1048576 UTF-8 bytes"));
        }
        let old_units = total_units as u32;
        let insert_units = units as u32;
        let removed_units = eff_end - eff_begin;
        let mut insert_fmt = self.format_for_utf16_selection(eff_begin as usize, eff_end as usize);
        insert_fmt.set_field_options(None);

        let mut next = self.clone();
        let has_surrounding = eff_begin > 0 || eff_end < old_units;
        if !has_surrounding {
            if old_units == 0 {
                next.format = next.default_format.clone();
            } else {
                next.format = insert_fmt;
            }
            next.format_runs.clear();
        } else if !next.format_runs.is_empty() || insert_fmt != next.format {
            let old_runs = next.materialized_runs(old_units);
            let mut runs = Vec::new();
            for r in &old_runs {
                if r.end <= eff_begin {
                    runs.push(r.clone());
                } else if r.start < eff_begin {
                    runs.push(TextFormatRun {
                        start: r.start,
                        end: eff_begin,
                        format: r.format.clone(),
                    });
                }
            }
            if insert_units > 0 {
                runs.push(TextFormatRun {
                    start: eff_begin,
                    end: eff_begin + insert_units,
                    format: insert_fmt,
                });
            }
            for r in &old_runs {
                if r.start >= eff_end {
                    runs.push(TextFormatRun {
                        start: r.start - removed_units + insert_units,
                        end: r.end - removed_units + insert_units,
                        format: r.format.clone(),
                    });
                } else if r.end > eff_end {
                    runs.push(TextFormatRun {
                        start: eff_begin + insert_units,
                        end: r.end - removed_units + insert_units,
                        format: r.format.clone(),
                    });
                }
            }
            next.format_runs = runs;
        }
        next.text.replace_range(byte_start..byte_end, &inserted);
        next.normalize_runs();
        let new_caret = eff_begin + insert_units;
        next.selection_begin = new_caret;
        next.selection_end = new_caret;
        next.caret = new_caret;
        next.validate()?;
        if next != *self {
            next.revision = self
                .revision
                .parse::<u64>()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| error("text revision exhausted"))?
                .to_string();
            *self = next;
        }
        Ok(())
    }

    pub fn materialized_runs(&self, total_units: u32) -> Vec<TextFormatRun> {
        if total_units == 0 {
            return Vec::new();
        }
        if self.format_runs.is_empty() {
            return vec![TextFormatRun {
                start: 0,
                end: total_units,
                format: self.format.clone(),
            }];
        }
        let mut sorted = self.format_runs.clone();
        sorted.sort_by_key(|r| (r.start, r.end));
        let mut out: Vec<TextFormatRun> = Vec::new();
        let mut cursor = 0u32;
        for run in sorted {
            let start = run.start.min(total_units).max(cursor);
            let end = run.end.min(total_units);
            if start > cursor {
                Self::push_merged_run(
                    &mut out,
                    TextFormatRun {
                        start: cursor,
                        end: start,
                        format: self.format.clone(),
                    },
                );
            }
            if end > start {
                Self::push_merged_run(
                    &mut out,
                    TextFormatRun {
                        start,
                        end,
                        format: run.format,
                    },
                );
                cursor = end;
            }
        }
        if cursor < total_units {
            Self::push_merged_run(
                &mut out,
                TextFormatRun {
                    start: cursor,
                    end: total_units,
                    format: self.format.clone(),
                },
            );
        }
        out
    }

    fn push_merged_run(out: &mut Vec<TextFormatRun>, run: TextFormatRun) {
        if run.start >= run.end {
            return;
        }
        if let Some(last) = out.last_mut()
            && last.end == run.start
            && last.format == run.format
        {
            last.end = run.end;
            return;
        }
        out.push(run);
    }

    pub fn format_at_utf16(&self, pos: u32, total_units: u32) -> TextFormat {
        if total_units == 0 || self.format_runs.is_empty() {
            return self.format.clone();
        }
        for r in &self.format_runs {
            if r.start <= pos && pos < r.end {
                return r.format.clone();
            }
        }
        if pos > 0 {
            for r in &self.format_runs {
                if r.end == pos {
                    return r.format.clone();
                }
            }
        }
        self.format.clone()
    }

    pub fn normalize_runs(&mut self) {
        let total_units = self.text.encode_utf16().count() as u32;
        if total_units == 0 {
            self.format_runs.clear();
            return;
        }
        if self.format_runs.is_empty() {
            return;
        }
        let runs = self.materialized_runs(total_units);
        if runs.len() <= 1 {
            if let Some(only) = runs.into_iter().next() {
                self.format = only.format;
            }
            self.format_runs.clear();
        } else {
            self.format = runs[0].format.clone();
            self.format_runs = runs;
        }
    }

    pub fn utf16_len(&self) -> usize {
        self.text.encode_utf16().count()
    }

    pub fn clamp_utf16_offset(&self, offset_utf16: usize) -> usize {
        clamp_utf16(&self.text, offset_utf16 as i64) as usize
    }

    pub fn format_at_utf16_offset(&self, offset_utf16: usize) -> TextFormat {
        let total = self.utf16_len() as u32;
        let clamped = self.clamp_utf16_offset(offset_utf16) as u32;
        self.format_at_utf16(clamped, total)
    }

    pub fn format_at_utf16_caret(&self, caret_utf16: usize) -> TextFormat {
        let total = self.utf16_len() as u32;
        if total == 0 || self.format_runs.is_empty() {
            return self.format.clone();
        }
        let clamped = self.clamp_utf16_offset(caret_utf16) as u32;
        if clamped > 0 {
            for r in &self.format_runs {
                if r.start < clamped && clamped <= r.end {
                    return r.format.clone();
                }
            }
        }
        self.format_at_utf16(clamped, total)
    }

    pub fn format_for_utf16_selection(&self, start_utf16: usize, end_utf16: usize) -> TextFormat {
        let lo = start_utf16.min(end_utf16);
        let hi = start_utf16.max(end_utf16);
        if lo < hi {
            self.format_at_utf16_offset(lo)
        } else {
            self.format_at_utf16_caret(lo)
        }
    }

    pub fn paragraph_utf16_bounds_for_range(
        &self,
        start_utf16: usize,
        end_utf16: usize,
    ) -> (usize, usize) {
        let lo = self.clamp_utf16_offset(start_utf16.min(end_utf16));
        let hi = self.clamp_utf16_offset(start_utf16.max(end_utf16));
        let mut para_start = 0usize;
        let mut para_end = self.utf16_len();
        let mut cursor = 0usize;
        for ch in self.text.chars() {
            let next = cursor + ch.len_utf16();
            if ch == '\n' {
                if next <= lo {
                    para_start = next;
                }
                if cursor >= hi && (cursor > lo || lo == hi) {
                    para_end = cursor;
                    break;
                }
            }
            cursor = next;
        }
        if para_end < para_start {
            (lo, hi)
        } else {
            (para_start, para_end)
        }
    }

    pub fn apply_format_to_utf16_range(
        &mut self,
        start_utf16: usize,
        end_utf16: usize,
        patch: &TextFormatPatch,
    ) -> Result<(), TextError> {
        if patch.is_empty() {
            return Ok(());
        }
        let total = self.utf16_len();
        if total == 0 {
            let mut next = self.clone();
            patch.apply(&mut next.format);
            patch.apply(&mut next.default_format);
            next.validate()?;
            *self = next;
            return Ok(());
        }
        let lo = self.clamp_utf16_offset(start_utf16.min(end_utf16)) as u32;
        let hi = self.clamp_utf16_offset(start_utf16.max(end_utf16)) as u32;
        if lo >= hi {
            return Ok(());
        }
        self.apply(&TextPatch {
            format_range: Some(TextFormatRangePatch {
                begin: lo,
                end: hi,
                format: patch.clone(),
            }),
            ..TextPatch::default()
        })
    }

    pub fn slice_runs_utf16(&self, start_utf16: usize, end_utf16: usize) -> Vec<TextFormatRun> {
        let lo = snap_utf16_to_grapheme_floor(&self.text, start_utf16.min(end_utf16) as i64);
        let hi = snap_utf16_to_grapheme_ceil(&self.text, start_utf16.max(end_utf16) as i64);
        if lo >= hi {
            return Vec::new();
        }
        let total = self.text.encode_utf16().count() as u32;
        let mut out: Vec<TextFormatRun> = Vec::new();
        for r in self.materialized_runs(total) {
            let s = r.start.max(lo);
            let e = r.end.min(hi);
            if s < e {
                let mut fmt = r.format;
                fmt.set_field_options(None);
                let rel_start = s - lo;
                let rel_end = e - lo;
                if let Some(last) = out.last_mut()
                    && last.end == rel_start
                    && last.format == fmt
                {
                    last.end = rel_end;
                    continue;
                }
                out.push(TextFormatRun {
                    start: rel_start,
                    end: rel_end,
                    format: fmt,
                });
            }
        }
        out
    }

    pub fn replace_utf16_range(
        &mut self,
        start_utf16: usize,
        end_utf16: usize,
        inserted: &str,
        typing_style: Option<&TextFormat>,
    ) -> Result<usize, TextError> {
        self.replace_utf16_range_with_runs(start_utf16, end_utf16, inserted, &[], typing_style)
    }

    pub fn replace_utf16_range_with_runs(
        &mut self,
        start_utf16: usize,
        end_utf16: usize,
        inserted: &str,
        inserted_runs: &[TextFormatRun],
        typing_style: Option<&TextFormat>,
    ) -> Result<usize, TextError> {
        let old_units = self.text.encode_utf16().count() as u32;
        let raw_begin = start_utf16.min(end_utf16).min(old_units as usize) as u32;
        let raw_end = start_utf16.max(end_utf16).min(old_units as usize) as u32;
        utf16_byte(&self.text, raw_begin)?;
        utf16_byte(&self.text, raw_end)?;
        let begin = snap_utf16_to_grapheme_floor(&self.text, raw_begin as i64);
        let end = if raw_end > raw_begin {
            snap_utf16_to_grapheme_ceil(&self.text, raw_end as i64)
        } else {
            begin
        };
        let byte_start = utf16_byte(&self.text, begin)?;
        let byte_end = utf16_byte(&self.text, end)?;

        // Normalize \r\n / \r to \n while remapping inserted_runs offsets if \r\n was present.
        let (normalized_source, mapped_runs) = normalize_text_and_runs(inserted, inserted_runs);
        let compiled_rule = if let Some(pattern) = &self.restrict {
            Some(Restriction::compile(pattern)?)
        } else {
            None
        };
        let removed_units = end - begin;
        let capacity = if self.max_chars > 0 {
            let retained_units = old_units.saturating_sub(removed_units) as usize;
            Some((self.max_chars as usize).saturating_sub(retained_units))
        } else {
            None
        };

        let mut base_insert_fmt = typing_style
            .cloned()
            .unwrap_or_else(|| self.format_for_utf16_selection(begin as usize, end as usize));
        base_insert_fmt.set_field_options(None);

        let mut normalized = String::new();
        let mut filtered_runs: Vec<TextFormatRun> = Vec::new();
        let mut kept_units = 0u32;
        for (c_start, _c_end, cluster) in grapheme_clusters_utf16(&normalized_source) {
            let effective_cluster: &str = if !self.multiline && cluster == "\n" {
                " "
            } else {
                cluster
            };
            if let Some(rule) = &compiled_rule {
                let allowed = effective_cluster == "\n"
                    || effective_cluster.chars().all(|ch| rule.allows(ch));
                if !allowed {
                    continue;
                }
            }
            let c_units = effective_cluster.encode_utf16().count() as u32;
            if let Some(cap) = capacity
                && (kept_units as usize) + (c_units as usize) > cap
            {
                break;
            }
            let mut cluster_fmt = mapped_runs
                .iter()
                .find(|r| r.start <= c_start && c_start < r.end)
                .map(|r| r.format.clone())
                .unwrap_or_else(|| base_insert_fmt.clone());
            cluster_fmt.set_field_options(None);

            let r_start = kept_units;
            let r_end = kept_units + c_units;
            if let Some(last) = filtered_runs.last_mut() {
                if last.end == r_start && last.format == cluster_fmt {
                    last.end = r_end;
                } else {
                    filtered_runs.push(TextFormatRun {
                        start: r_start,
                        end: r_end,
                        format: cluster_fmt,
                    });
                }
            } else {
                filtered_runs.push(TextFormatRun {
                    start: r_start,
                    end: r_end,
                    format: cluster_fmt,
                });
            }
            normalized.push_str(effective_cluster);
            kept_units = r_end;
        }

        // A rejected typed/pasted value is a no-op, not a deletion of the active selection.
        // Intentional deletion is represented by an actually empty `inserted` string.
        if !inserted.is_empty() && normalized.is_empty() {
            return Ok(begin as usize);
        }
        if self.text.len() - (byte_end - byte_start) + normalized.len() > MAX_TEXT_BYTES {
            return Err(error("replacement exceeds 1048576 UTF-8 bytes"));
        }
        let insert_units = kept_units;

        let mut next = self.clone();
        let retained = begin > 0 || end < old_units;
        if !retained {
            if filtered_runs.is_empty() {
                next.format = base_insert_fmt.clone();
                next.default_format = base_insert_fmt;
                next.format_runs.clear();
            } else {
                let first_fmt = filtered_runs[0].format.clone();
                next.format = first_fmt.clone();
                next.default_format = first_fmt;
                next.format_runs = filtered_runs;
            }
        } else if !next.format_runs.is_empty()
            || base_insert_fmt != next.format
            || filtered_runs.iter().any(|r| r.format != next.format)
        {
            let old_runs = next.materialized_runs(old_units);
            let mut runs = Vec::new();
            for r in &old_runs {
                if r.end <= begin {
                    runs.push(r.clone());
                } else if r.start < begin {
                    runs.push(TextFormatRun {
                        start: r.start,
                        end: begin,
                        format: r.format.clone(),
                    });
                }
            }
            if insert_units > 0 {
                for r in filtered_runs {
                    runs.push(TextFormatRun {
                        start: begin + r.start,
                        end: begin + r.end,
                        format: r.format,
                    });
                }
            }
            for r in &old_runs {
                if r.start >= end {
                    runs.push(TextFormatRun {
                        start: r.start - removed_units + insert_units,
                        end: r.end - removed_units + insert_units,
                        format: r.format.clone(),
                    });
                } else if r.end > end {
                    runs.push(TextFormatRun {
                        start: begin + insert_units,
                        end: r.end - removed_units + insert_units,
                        format: r.format.clone(),
                    });
                }
            }
            next.format_runs = runs;
        }
        next.text.replace_range(byte_start..byte_end, &normalized);
        next.normalize_runs();
        let new_caret = begin + insert_units;
        next.selection_begin = new_caret;
        next.selection_end = new_caret;
        next.caret = new_caret;
        next.validate()?;
        if next != *self {
            next.revision = self
                .revision
                .parse::<u64>()
                .unwrap_or(0)
                .saturating_add(1)
                .to_string();
            *self = next;
        }
        Ok(new_caret as usize)
    }

    pub fn merged_format_json(&self, begin: i64, end: i64) -> Result<serde_json::Value, TextError> {
        let total = self.text.encode_utf16().count() as i64;
        let format_to_json = |f: &TextFormat| {
            serde_json::json!({
                "font": f.font,
                "size": f.size,
                "color": f.color,
                "bold": f.bold,
                "italic": f.italic,
                "underline": f.underline,
                "align": f.effective_align_mode(),
                "leading": f.leading,
                "letter_spacing": f.letter_spacing,
                "left_margin": f.left_margin,
                "right_margin": f.right_margin,
                "indent": f.indent,
                "block_indent": f.block_indent,
                "bullet": f.bullet,
                "kerning": f.kerning,
                "url": f.url,
                "target": f.target_str(),
                "tab_stops": f.tab_stops,
                "display": f.display().unwrap_or("block"),
            })
        };
        if total == 0 {
            if begin == -1 && end == -1 {
                return Ok(format_to_json(&self.format));
            }
            return Err(error("TextField.getTextFormat range is out of bounds"));
        }
        let (r_begin, r_end) = if begin == -1 && end == -1 {
            (0u32, total as u32)
        } else if begin >= 0 && end == -1 {
            if begin >= total {
                return Err(error("TextField.getTextFormat beginIndex is out of bounds"));
            }
            (begin as u32, (begin + 1) as u32)
        } else {
            if begin < 0 || end < begin || begin >= total || end > total {
                return Err(error("TextField.getTextFormat range is out of bounds"));
            }
            if begin == end {
                let fmt = self.format_at_utf16(begin as u32, total as u32);
                return Ok(format_to_json(&fmt));
            }
            (begin as u32, end as u32)
        };
        let runs: Vec<TextFormatRun> = self
            .materialized_runs(total as u32)
            .into_iter()
            .filter(|r| r.end > r_begin && r.start < r_end)
            .collect();
        let Some(first) = runs.first() else {
            return Ok(format_to_json(&self.format));
        };
        if runs.len() == 1 {
            return Ok(format_to_json(&first.format));
        }
        let f0 = &first.format;
        let same = |pred: &dyn Fn(&TextFormat) -> bool| runs.iter().all(|r| pred(&r.format));
        Ok(serde_json::json!({
            "font": if same(&|f| f.font == f0.font) { serde_json::json!(f0.font) } else { serde_json::Value::Null },
            "size": if same(&|f| (f.size - f0.size).abs() < f64::EPSILON) { serde_json::json!(f0.size) } else { serde_json::Value::Null },
            "color": if same(&|f| f.color == f0.color) { serde_json::json!(f0.color) } else { serde_json::Value::Null },
            "bold": if same(&|f| f.bold == f0.bold) { serde_json::json!(f0.bold) } else { serde_json::Value::Null },
            "italic": if same(&|f| f.italic == f0.italic) { serde_json::json!(f0.italic) } else { serde_json::Value::Null },
            "underline": if same(&|f| f.underline == f0.underline) { serde_json::json!(f0.underline) } else { serde_json::Value::Null },
            "align": if same(&|f| f.effective_align_mode() == f0.effective_align_mode()) { serde_json::json!(f0.effective_align_mode()) } else { serde_json::Value::Null },
            "leading": if same(&|f| (f.leading - f0.leading).abs() < f64::EPSILON) { serde_json::json!(f0.leading) } else { serde_json::Value::Null },
            "letter_spacing": if same(&|f| (f.letter_spacing - f0.letter_spacing).abs() < f64::EPSILON) { serde_json::json!(f0.letter_spacing) } else { serde_json::Value::Null },
            "left_margin": if same(&|f| (f.left_margin - f0.left_margin).abs() < f64::EPSILON) { serde_json::json!(f0.left_margin) } else { serde_json::Value::Null },
            "right_margin": if same(&|f| (f.right_margin - f0.right_margin).abs() < f64::EPSILON) { serde_json::json!(f0.right_margin) } else { serde_json::Value::Null },
            "indent": if same(&|f| (f.indent - f0.indent).abs() < f64::EPSILON) { serde_json::json!(f0.indent) } else { serde_json::Value::Null },
            "block_indent": if same(&|f| (f.block_indent - f0.block_indent).abs() < f64::EPSILON) { serde_json::json!(f0.block_indent) } else { serde_json::Value::Null },
            "bullet": if same(&|f| f.bullet == f0.bullet) { serde_json::json!(f0.bullet) } else { serde_json::Value::Null },
            "kerning": if same(&|f| f.kerning == f0.kerning) { serde_json::json!(f0.kerning) } else { serde_json::Value::Null },
            "url": if same(&|f| f.url == f0.url) { serde_json::json!(f0.url) } else { serde_json::Value::Null },
            "target": if same(&|f| f.target_str() == f0.target_str()) { serde_json::json!(f0.target_str()) } else { serde_json::Value::Null },
            "tab_stops": if same(&|f| f.tab_stops == f0.tab_stops) { serde_json::json!(f0.tab_stops) } else { serde_json::Value::Null },
            "display": if same(&|f| f.display().unwrap_or("block") == f0.display().unwrap_or("block")) { serde_json::json!(f0.display().unwrap_or("block")) } else { serde_json::Value::Null },
        }))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPatch {
    pub as2_viewport: Option<As2TextViewport>,
    pub text: Option<String>,
    pub append: Option<String>,
    pub replace: Option<TextReplacement>,
    pub replace_selected: Option<String>,
    pub selection: Option<TextSelection>,
    pub field_type: Option<TextFieldType>,
    pub selectable: Option<bool>,
    #[serde(default)]
    pub interaction_mode: Option<String>,
    pub display_as_password: Option<bool>,
    pub max_chars: Option<u32>,
    pub restrict: Option<TextRestrictPatch>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub multiline: Option<bool>,
    pub word_wrap: Option<bool>,
    pub format: Option<TextFormatPatch>,
    pub default_format: Option<TextFormatPatch>,
    #[serde(default)]
    pub format_range: Option<TextFormatRangePatch>,
    #[serde(default)]
    pub format_runs: Option<Vec<TextFormatRun>>,
    pub anti_alias_type: Option<String>,
    pub grid_fit_type: Option<String>,
    pub sharpness: Option<f64>,
    pub thickness: Option<f64>,
    pub embed_fonts: Option<bool>,
    pub condense_white: Option<bool>,
    pub always_show_selection: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextReplacement {
    pub begin: u32,
    pub end: u32,
    pub text: String,
}
