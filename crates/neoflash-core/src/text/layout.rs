use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLine {
    pub text: String,
    pub start: u32,
    pub end: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextDecoration {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextViewportMetrics {
    pub offset_x: f64,
    pub offset_y: f64,
    pub scroll: u32,
    pub hscroll: u32,
    pub maxscroll: u32,
    pub maxhscroll: u32,
    pub bottom_scroll: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSurface {
    pub width: f64,
    pub height: f64,
    pub background: Option<u32>,
    pub border: Option<u32>,
    #[serde(default = "default_one_f64", skip_serializing_if = "is_one_f64")]
    pub border_width: f64,
    #[serde(default = "default_one_u32", skip_serializing_if = "is_one_u32")]
    pub column_count: u32,
    #[serde(
        default = "default_gutter_f64",
        skip_serializing_if = "is_default_gutter"
    )]
    pub column_gutter: f64,
    #[serde(default = "default_common_str", skip_serializing_if = "is_common_str")]
    pub ligatures: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub digit_width: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub digit_case: String,
    #[serde(
        default = "default_locale_str",
        skip_serializing_if = "is_default_locale_str"
    )]
    pub locale: String,
    #[serde(default = "default_auto_str", skip_serializing_if = "is_auto_str")]
    pub glyph_rotation: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub rotation: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub text_width: f64,
    pub text_height: f64,
    pub line_height: f64,
    pub decorations: Vec<TextDecoration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<TextDecoration>,
    pub runs: Vec<TextRun>,
    pub bullets: Vec<[f64; 3]>,
    pub viewport: TextViewportMetrics,
    pub surface: TextSurface,
}
struct Part {
    text: String,
    start: u32,
    end: u32,
    newline: bool,
}
fn locale_primary_lang(locale: &str) -> String {
    locale
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase()
}
fn is_east_asian_locale(locale: &str) -> bool {
    matches!(locale_primary_lang(locale).as_str(), "ja" | "zh" | "ko")
}
fn is_cjk_or_fullwidth(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x2E80..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF01..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x2FA1F
    )
}
fn is_kinsoku_leading_prohibited(ch: char) -> bool {
    matches!(
        ch,
        '。' | '、'
            | '，'
            | '．'
            | '！'
            | '？'
            | '：'
            | '；'
            | '）'
            | '］'
            | '｝'
            | '」'
            | '』'
            | '】'
            | '》'
            | '〉'
            | '≫'
            | 'ゝ'
            | 'ゞ'
            | 'ー'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
    )
}
fn is_kinsoku_trailing_prohibited(ch: char) -> bool {
    matches!(
        ch,
        '（' | '［' | '｛' | '「' | '『' | '【' | '《' | '〈' | '≪' | '¥' | '＄'
    )
}
pub fn apply_ligatures(text: &str, level: &str) -> String {
    match level {
        // Explicit Unicode presentation characters are authored content, not OpenType
        // substitutions. Disabling ligatures must not decompose them or change UTF-16 indices.
        "none" => text.to_owned(),
        "minimum" => text.replace("fi", "\u{FB01}").replace("fl", "\u{FB02}"),
        "uncommon" | "discretionary" | "exotic" => text
            .replace("ffi", "\u{FB03}")
            .replace("ffl", "\u{FB04}")
            .replace("ff", "\u{FB00}")
            .replace("fi", "\u{FB01}")
            .replace("fl", "\u{FB02}")
            .replace("st", "\u{FB06}")
            .replace("ft", "\u{FB05}")
            .replace("IJ", "\u{0132}")
            .replace("ij", "\u{0133}"),
        _ => text
            .replace("ffi", "\u{FB03}")
            .replace("ffl", "\u{FB04}")
            .replace("ff", "\u{FB00}")
            .replace("fi", "\u{FB01}")
            .replace("fl", "\u{FB02}"),
    }
}
pub fn glyph_rotation_degrees(ch: char, glyph_rotation: &str, is_vertical: bool) -> f64 {
    match glyph_rotation {
        "0" | "rotate0" => 0.0,
        "90" | "rotate90" => 90.0,
        "180" | "rotate180" => 180.0,
        "270" | "rotate270" => 270.0,
        _ => {
            if is_vertical && !ch.is_whitespace() && !is_cjk_or_fullwidth(ch) {
                90.0
            } else {
                0.0
            }
        }
    }
}
fn apply_typographic_case_for_locale(text: &str, mode: &str, locale: &str) -> String {
    let lang = locale_primary_lang(locale);
    let is_turkic = matches!(lang.as_str(), "tr" | "az");
    let is_greek = lang == "el";
    match mode {
        "uppercase" | "lowercase_to_small_caps" => text
            .chars()
            .map(|ch| {
                if is_turkic {
                    match ch {
                        'i' => return "İ".to_owned(),
                        'ı' => return "I".to_owned(),
                        _ => {}
                    }
                }
                if is_greek {
                    match ch {
                        'ά' | 'Ά' => return "Α".to_owned(),
                        'έ' | 'Έ' => return "Ε".to_owned(),
                        'ή' | 'Ή' => return "Η".to_owned(),
                        'ί' | 'Ί' => return "Ι".to_owned(),
                        'ό' | 'Ό' => return "Ο".to_owned(),
                        'ύ' | 'Ύ' => return "Υ".to_owned(),
                        'ώ' | 'Ώ' => return "Ω".to_owned(),
                        _ => {}
                    }
                }
                let up: String = ch.to_uppercase().collect();
                if up.encode_utf16().count() == ch.len_utf16() {
                    up
                } else {
                    ch.to_string()
                }
            })
            .collect(),
        "lowercase" => text
            .chars()
            .map(|ch| {
                if is_turkic {
                    match ch {
                        'I' => return "ı".to_owned(),
                        'İ' => return "i".to_owned(),
                        _ => {}
                    }
                }
                let lo: String = ch.to_lowercase().collect();
                if lo.encode_utf16().count() == ch.len_utf16() {
                    lo
                } else {
                    ch.to_string()
                }
            })
            .collect(),
        _ => text.to_owned(),
    }
}
fn parts(field: &TextFieldState) -> Result<Vec<Part>, TextError> {
    let mut run_boundaries = BTreeSet::new();
    for r in &field.format_runs {
        run_boundaries.insert(r.start);
        run_boundaries.insert(r.end);
    }
    let split_tabs_for_wrap = field.multiline
        && field.word_wrap
        && field.box_size.is_some()
        && (!field.format.tab_stops.is_empty()
            || field
                .format_runs
                .iter()
                .any(|r| !r.format.tab_stops.is_empty()));
    let break_all = matches!(field.line_break.as_str(), "all" | "any")
        || matches!(field.orientation.as_str(), "vertical_ltr" | "vertical_rtl");
    let split_digits = !is_default_str(&field.digit_width) || !is_default_str(&field.digit_case);
    let enable_kinsoku = field.line_break == "auto"
        && (is_east_asian_locale(&field.locale) || field.text.chars().any(is_cjk_or_fullwidth));
    let mut result = Vec::new();
    let mut text = String::new();
    let mut start = 0;
    let mut offset = 0;
    let mut space = false;
    let mut prev_ch: Option<char> = None;
    let displayed;
    let display_source = if field.display_as_password {
        displayed = field
            .text
            .chars()
            .map(|ch| {
                if ch == '\n' {
                    "\n".into()
                } else {
                    "*".repeat(ch.len_utf16())
                }
            })
            .collect::<String>();
        displayed.as_str()
    } else if !is_default_str(&field.typographic_case) {
        displayed =
            apply_typographic_case_for_locale(&field.text, &field.typographic_case, &field.locale);
        displayed.as_str()
    } else {
        field.text.as_str()
    };
    let grapheme_bounds: BTreeSet<u32> = grapheme_utf16_boundaries(display_source)
        .into_iter()
        .collect();
    for ch in display_source.chars() {
        let newline = ch == '\n';
        let is_tab = split_tabs_for_wrap && ch == '\t';
        let nextspace = ch.is_whitespace();
        let at_grapheme = grapheme_bounds.contains(&offset);
        let hit_boundary = !run_boundaries.is_empty() && run_boundaries.contains(&offset);
        let digit_boundary = at_grapheme
            && split_digits
            && (ch.is_ascii_digit() || prev_ch.is_some_and(|p| p.is_ascii_digit()));
        let kinsoku_break = at_grapheme
            && enable_kinsoku
            && prev_ch.is_some_and(|p| {
                !p.is_whitespace()
                    && !nextspace
                    && (is_cjk_or_fullwidth(p) || is_cjk_or_fullwidth(ch))
                    && !is_kinsoku_trailing_prohibited(p)
                    && !is_kinsoku_leading_prohibited(ch)
            });
        if !text.is_empty()
            && (newline
                || is_tab
                || hit_boundary
                || (at_grapheme && space != nextspace)
                || (at_grapheme && break_all)
                || digit_boundary
                || kinsoku_break)
        {
            result.push(Part {
                text: std::mem::take(&mut text),
                start,
                end: offset,
                newline: false,
            });
            start = offset;
        }
        if newline {
            result.push(Part {
                text: if field.multiline {
                    String::new()
                } else {
                    " ".into()
                },
                start: offset,
                end: offset + 1,
                newline: field.multiline,
            });
            start = offset + 1;
            prev_ch = None;
        } else if is_tab {
            result.push(Part {
                text: "\t".into(),
                start: offset,
                end: offset + 1,
                newline: false,
            });
            start = offset + 1;
            prev_ch = Some('\t');
        } else {
            text.push(ch);
            space = nextspace;
            prev_ch = Some(ch);
        }
        offset += ch.len_utf16() as u32;
        if result.len() > MAX_PARTS {
            return Err(error("layout exceeds 16384 word/space/line segments"));
        }
    }
    if !text.is_empty() {
        result.push(Part {
            text,
            start,
            end: offset,
            newline: false,
        })
    }
    if result.len() > MAX_PARTS {
        return Err(error("layout exceeds 16384 word/space/line segments"));
    }
    Ok(result)
}
pub(super) fn measured<M>(
    strings: &[String],
    format: &TextFormat,
    measure: &mut M,
) -> Result<Vec<f64>, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    let mut result = measure(strings, format)?;
    if result.len() != strings.len() || result.iter().any(|w| !w.is_finite() || *w < 0. || *w > 1e9)
    {
        return Err(error(
            "font measurement must return one finite width in 0..=1e9 per string",
        ));
    }
    for (width, text) in result.iter_mut().zip(strings) {
        *width = (*width + text.chars().count().saturating_sub(1) as f64 * format.letter_spacing)
            .max(0.);
    }
    Ok(result)
}

fn next_tab_stop(x: f64, format: &TextFormat) -> f64 {
    format
        .tab_stops
        .iter()
        .copied()
        .find(|stop| *stop > x)
        .unwrap_or_else(|| {
            let origin = format.tab_stops.last().copied().unwrap_or(0.);
            let step = format.size * 4.;
            origin + ((x - origin) / step).floor().max(0.) * step + step
        })
}

fn align_offset(align_mode: &str, avail: f64, width: f64) -> f64 {
    match align_mode {
        "center" => (avail - width) / 2.,
        "right" | "end" => avail - width,
        _ => 0.,
    }
}

fn line_advance_for(max_size: f64, leading: f64, leading_mode: &str) -> f64 {
    if leading_mode == "percent" {
        let pct = if leading <= 0.0 { 120.0 } else { leading };
        (max_size * (pct / 100.0)).max(1.0)
    } else {
        (max_size * 1.2 + leading).max(1.0)
    }
}

fn effective_run_format(base: &TextFormat, field: &TextFieldState) -> TextFormat {
    let mut fmt = base.clone();
    if field.superscript || field.subscript {
        fmt.size = (fmt.size * 0.65).max(1.0);
    } else if matches!(
        field.typographic_case.as_str(),
        "caps_to_small_caps" | "lowercase_to_small_caps"
    ) {
        fmt.size = (fmt.size * 0.8).max(1.0);
    }
    fmt
}

impl TextFieldState {
    pub fn has_extended_layout_options(&self) -> bool {
        self.column_count > 1
            || !is_default_pad(&self.padding_left)
            || !is_default_pad(&self.padding_right)
            || !is_default_pad(&self.padding_top)
            || !is_default_pad(&self.padding_bottom)
            || !is_one_f64(&self.border_width)
            || !is_top_str(&self.vertical_align)
            || !is_auto_str(&self.first_line_offset_mode)
            || !is_horiz_str(&self.orientation)
            || !is_ltr_str(&self.direction)
            || !is_pt_str(&self.leading_mode)
            || self.highlight
            || self.strikethrough
            || self.superscript
            || self.subscript
            || !is_default_str(&self.typographic_case)
            || !is_default_str(&self.digit_width)
            || !is_default_str(&self.digit_case)
            || !is_common_str(&self.ligatures)
            || !is_auto_str(&self.glyph_rotation)
            || !is_default_locale_str(&self.locale)
            || !is_auto_str(&self.line_break)
            || !is_zero_f64(&self.baseline_shift)
            || !is_zero_f64(&self.space_before)
            || !is_zero_f64(&self.space_after)
            || !is_word_spacing_str(&self.text_justify)
            || self.format.effective_align_mode() == "justify_all"
    }
}

fn adjust_digit_and_ligature_width(
    text: &str,
    base_width: f64,
    eff_fmt: &TextFormat,
    field: &TextFieldState,
) -> f64 {
    if text.len() == 1
        && let Some(ch) = text.chars().next()
        && ch.is_ascii_digit()
    {
        match field.digit_width.as_str() {
            "tabular" => return (eff_fmt.size * 0.60).max(0.0),
            "proportional" => {
                let factor = match ch {
                    '1' => 0.38,
                    '4' | '7' => 0.52,
                    _ => 0.58,
                };
                return (eff_fmt.size * factor).max(0.0);
            }
            _ => {}
        }
    }
    if !is_common_str(&field.ligatures) && field.ligatures != "none" {
        let shaped = apply_ligatures(text, &field.ligatures);
        let orig_chars = text.chars().count();
        let shaped_chars = shaped.chars().count();
        if shaped_chars < orig_chars && orig_chars > 0 {
            let saved = (orig_chars - shaped_chars) as f64;
            let avg = base_width / orig_chars as f64;
            return (base_width - saved * avg * 0.35).max(0.0);
        }
    }
    base_width
}

fn old_style_digit_metrics(ch: char, base_size: f64) -> (f64, f64) {
    match ch {
        '0' | '1' | '2' => ((base_size * 0.78).max(1.0), base_size * 0.22),
        '3' | '4' | '5' | '7' | '9' => ((base_size * 0.82).max(1.0), base_size * 0.32),
        '6' | '8' => ((base_size * 0.92).max(1.0), base_size * 0.08),
        _ => (base_size, 0.0),
    }
}

fn layout_multirun_text<M>(field: &TextFieldState, mut measure: M) -> Result<TextLayout, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    let total_units = field.text.encode_utf16().count() as u32;
    let parts = parts(field)?;
    let mat_runs = field.materialized_runs(total_units);
    let run_index_at = |pos: u32| -> usize {
        mat_runs
            .iter()
            .position(|r| r.start <= pos && pos < r.end)
            .unwrap_or_else(|| mat_runs.len().saturating_sub(1))
    };
    let fmt_at = |pos: u32| -> &TextFormat {
        if mat_runs.is_empty() {
            &field.format
        } else {
            &mat_runs[run_index_at(pos)].format
        }
    };

    // Measure parts grouped by materialized run format to minimize host calls.
    let mut widths = vec![0.0_f64; parts.len()];
    if mat_runs.is_empty() {
        let strings: Vec<_> = parts.iter().map(|p| p.text.clone()).collect();
        let eff_fmt = effective_run_format(&field.format, field);
        widths = measured(&strings, &eff_fmt, &mut measure)?;
        for (part, w) in parts.iter().zip(&mut widths) {
            *w = adjust_digit_and_ligature_width(&part.text, *w, &eff_fmt, field);
        }
    } else {
        let mut grouped_indices: Vec<Vec<usize>> = vec![Vec::new(); mat_runs.len()];
        for (idx, part) in parts.iter().enumerate() {
            if !part.newline {
                grouped_indices[run_index_at(part.start)].push(idx);
            }
        }
        for (r_idx, indices) in grouped_indices.into_iter().enumerate() {
            if indices.is_empty() {
                continue;
            }
            let strings: Vec<String> = indices.iter().map(|&i| parts[i].text.clone()).collect();
            let eff_fmt = effective_run_format(&mat_runs[r_idx].format, field);
            let w = measured(&strings, &eff_fmt, &mut measure)?;
            for (i, width) in indices.into_iter().zip(w) {
                widths[i] = adjust_digit_and_ligature_width(&parts[i].text, width, &eff_fmt, field);
            }
        }
    }

    let (pad_l, pad_r, pad_t, pad_b) = if field.box_size.is_some() {
        (
            field.padding_left.max(0.0),
            field.padding_right.max(0.0),
            field.padding_top.max(0.0),
            field.padding_bottom.max(0.0),
        )
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    let cols = if field.box_size.is_some() {
        field.column_count.clamp(1, 50)
    } else {
        1
    };
    let gutter = if cols > 1 {
        field.column_gutter.max(0.0)
    } else {
        0.0
    };
    let is_vertical = matches!(field.orientation.as_str(), "vertical_ltr" | "vertical_rtl");
    let col_width_opt = field.box_size.map(|b| {
        if is_vertical {
            (b.height - pad_t - pad_b).max(0.0)
        } else {
            let inner_w = (b.width - pad_l - pad_r).max(0.0);
            let total_gutter = gutter * f64::from(cols - 1);
            ((inner_w - total_gutter) / f64::from(cols)).max(0.0)
        }
    });

    let mut paragraph_starts = BTreeSet::from([0u32]);
    let mut offset = 0u32;
    for ch in field.text.chars() {
        offset += ch.len_utf16() as u32;
        if ch == '\n' {
            paragraph_starts.insert(offset);
        }
    }
    let line_metrics_at = |start: u32| -> (f64, Option<f64>, &TextFormat) {
        let pfmt = fmt_at(start);
        let b_indent = if pfmt.bullet { pfmt.size } else { 0. };
        let rel_orig_x = pfmt.left_margin + pfmt.block_indent + b_indent;
        let p_indent = if paragraph_starts.contains(&start) {
            pfmt.indent
        } else {
            0.
        };
        let avail = col_width_opt.map(|cw| {
            (cw - pfmt.left_margin - pfmt.right_margin - pfmt.block_indent - b_indent - p_indent)
                .max(0.)
        });
        (rel_orig_x + p_indent, avail, pfmt)
    };
    let wrap = field.multiline
        && field.word_wrap
        && field.box_size.is_some()
        && field.line_break != "no_break";

    #[derive(Clone)]
    struct Seg {
        text: String,
        fmt: TextFormat,
        rel_x: f64,
        width: f64,
    }
    struct BuiltLine {
        line: TextLine,
        segs: Vec<Seg>,
        max_size: f64,
        leading: f64,
    }
    let mut built_lines: Vec<BuiltLine> = Vec::new();
    let mut cur_line = TextLine {
        text: String::new(),
        start: 0,
        end: 0,
        x: 0.,
        y: 0.,
        width: 0.,
        height: fmt_at(0).size * 1.2,
    };
    let mut cur_segs: Vec<Seg> = Vec::new();
    let mut cur_max_size = fmt_at(0).size;
    let mut cur_leading = fmt_at(0).leading;

    let flush = |built: &mut Vec<BuiltLine>,
                 cur_l: &mut TextLine,
                 cur_s: &mut Vec<Seg>,
                 max_sz: &mut f64,
                 lead: &mut f64|
     -> Result<(), TextError> {
        if built.len() >= MAX_PARTS {
            return Err(error("layout exceeds 16384 lines"));
        }
        cur_l.height = *max_sz * 1.2;
        let next_fmt = fmt_at(cur_l.end);
        let next_line = TextLine {
            text: String::new(),
            start: cur_l.end,
            end: cur_l.end,
            x: 0.,
            y: 0.,
            width: 0.,
            height: next_fmt.size * 1.2,
        };
        let done_line = std::mem::replace(cur_l, next_line);
        let done_segs = std::mem::take(cur_s);
        let done_max = *max_sz;
        let done_lead = *lead;
        *max_sz = next_fmt.size;
        *lead = next_fmt.leading;
        built.push(BuiltLine {
            line: done_line,
            segs: done_segs,
            max_size: done_max,
            leading: done_lead,
        });
        Ok(())
    };

    for (part, &part_w) in parts.iter().zip(&widths) {
        if part.newline {
            cur_line.end = part.end;
            flush(
                &mut built_lines,
                &mut cur_line,
                &mut cur_segs,
                &mut cur_max_size,
                &mut cur_leading,
            )?;
            continue;
        }
        let base_pfmt = fmt_at(part.start);
        let pfmt = effective_run_format(base_pfmt, field);
        if part.text == "\t" && !pfmt.tab_stops.is_empty() {
            let next_x = next_tab_stop(cur_line.width, &pfmt);
            cur_line.text.push('\t');
            cur_line.width = next_x;
            cur_line.end = part.end;
            continue;
        }
        let (_, avail_opt, _) = line_metrics_at(cur_line.start);
        let avail = avail_opt.unwrap_or(f64::MAX);
        if wrap && !cur_line.text.is_empty() && cur_line.width + part_w > avail {
            flush(
                &mut built_lines,
                &mut cur_line,
                &mut cur_segs,
                &mut cur_max_size,
                &mut cur_leading,
            )?;
        }
        if wrap && part_w > line_metrics_at(cur_line.start).1.unwrap_or(f64::MAX) {
            let chars: Vec<String> = part.text.chars().map(|c| c.to_string()).collect();
            let cw = measured(&chars, &pfmt, &mut measure)?;
            let mut char_off = part.start;
            let mut chunk = String::new();
            let mut chunk_x = cur_line.width;
            let mut chunk_w = 0.0;
            for (ch, w) in part.text.chars().zip(cw) {
                let cur_avail = line_metrics_at(cur_line.start).1.unwrap_or(f64::MAX);
                if !cur_line.text.is_empty() && cur_line.width + w > cur_avail {
                    if !chunk.is_empty() {
                        cur_segs.push(Seg {
                            text: std::mem::take(&mut chunk),
                            fmt: pfmt.clone(),
                            rel_x: chunk_x,
                            width: chunk_w,
                        });
                    }
                    flush(
                        &mut built_lines,
                        &mut cur_line,
                        &mut cur_segs,
                        &mut cur_max_size,
                        &mut cur_leading,
                    )?;
                    chunk_x = 0.0;
                    chunk_w = 0.0;
                }
                let gap = if cur_line.text.is_empty() {
                    0.0
                } else {
                    pfmt.letter_spacing
                };
                if chunk.is_empty() {
                    chunk_x = cur_line.width + gap;
                } else {
                    chunk_w += pfmt.letter_spacing;
                }
                cur_line.width += gap + w;
                cur_line.text.push(ch);
                chunk.push(ch);
                chunk_w += w;
                char_off += ch.len_utf16() as u32;
                cur_line.end = char_off;
                cur_max_size = cur_max_size.max(base_pfmt.size);
                cur_leading = cur_leading.max(base_pfmt.leading);
            }
            if !chunk.is_empty() {
                cur_segs.push(Seg {
                    text: chunk,
                    fmt: pfmt,
                    rel_x: chunk_x,
                    width: chunk_w,
                });
            }
        } else {
            let gap = if cur_line.text.is_empty() {
                0.0
            } else {
                pfmt.letter_spacing
            };
            let rel_x = cur_line.width + gap;
            cur_line.width = rel_x + part_w;
            cur_line.text.push_str(&part.text);
            cur_line.end = part.end;
            cur_max_size = cur_max_size.max(base_pfmt.size);
            cur_leading = cur_leading.max(base_pfmt.leading);
            cur_segs.push(Seg {
                text: part.text.clone(),
                fmt: pfmt,
                rel_x,
                width: part_w,
            });
        }
    }
    flush(
        &mut built_lines,
        &mut cur_line,
        &mut cur_segs,
        &mut cur_max_size,
        &mut cur_leading,
    )?;

    let first_line_top_extra = match field.first_line_offset_mode.as_str() {
        "line_height" => built_lines
            .first()
            .map_or(0.0, |b| (b.max_size * 0.2).max(0.0)),
        "pt" => field.first_line_offset.max(0.0),
        _ => 0.0,
    };
    let col_top_y = pad_t + first_line_top_extra;
    let col_avail_h_opt = field
        .box_size
        .map(|b| (b.height - pad_t - pad_b - first_line_top_extra).max(0.0));

    let line_count_usize = built_lines.len();
    let mut next_starts: Vec<Option<u32>> = built_lines
        .iter()
        .skip(1)
        .map(|b| Some(b.line.start))
        .collect();
    next_starts.push(None);

    // Compute natural height so multi-column containers either overflow at col_avail_h
    // or balance lines across columns when total content height fits inside one column.
    let natural_total_h: f64 = built_lines
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let adv = if i + 1 < line_count_usize {
                line_advance_for(b.max_size, b.leading, &field.leading_mode)
            } else {
                b.line.height
            };
            let para_gap = if i > 0 && paragraph_starts.contains(&b.line.start) {
                field.space_before.max(field.space_after).max(0.0)
            } else {
                0.0
            };
            adv + para_gap
        })
        .sum();
    let balanced_lines_per_col = if cols > 1
        && line_count_usize > 1
        && col_avail_h_opt.is_some_and(|ch| natural_total_h <= ch)
    {
        Some(line_count_usize.div_ceil(cols as usize).max(1))
    } else {
        None
    };

    let mut line_col_indices = vec![0u32; line_count_usize];
    let mut col_ranges: Vec<(usize, usize)> = Vec::new();
    let mut cur_col = 0u32;
    let mut cur_col_start_idx = 0usize;
    let mut y_in_col = 0.0_f64;
    let mut lines_in_col = 0usize;
    let mut text_width = 0.0_f64;
    let mut max_col_bottom = col_top_y;

    for (idx, bl) in built_lines.iter_mut().enumerate() {
        let (rel_base_x, avail_opt, pfmt) = line_metrics_at(bl.line.start);
        let raw_align = pfmt.effective_align_mode();
        let align_mode = if field.direction == "rtl" && pfmt.align_mode() == Some("start") {
            "right"
        } else if field.direction == "rtl" && pfmt.align_mode() == Some("end") {
            "left"
        } else {
            raw_align
        };
        let soft_wrapped = idx + 1 < line_count_usize
            && next_starts[idx].is_some_and(|ns| !paragraph_starts.contains(&ns));
        let should_justify =
            (align_mode == "justify" && soft_wrapped) || align_mode == "justify_all";
        if should_justify && let Some(avail) = avail_opt {
            let last_non_ws = bl.segs.iter().rposition(|s| !s.text.trim().is_empty());
            if let Some(last_idx) = last_non_ws {
                let content_w = bl.segs[last_idx].rel_x + bl.segs[last_idx].width;
                let slack = (avail - content_w).max(0.0);
                if slack > 0.0 {
                    let ws_indices: Vec<usize> = (0..last_idx)
                        .filter(|&i| bl.segs[i].text.chars().all(char::is_whitespace))
                        .collect();
                    if field.text_justify != "letter_spacing" && !ws_indices.is_empty() {
                        let extra_per_gap = slack / ws_indices.len() as f64;
                        let mut shift = 0.0;
                        for (s_i, seg) in bl.segs.iter_mut().enumerate() {
                            seg.rel_x += shift;
                            if s_i < last_idx && seg.text.chars().all(char::is_whitespace) {
                                seg.width += extra_per_gap;
                                shift += extra_per_gap;
                            }
                        }
                        bl.line.width = avail;
                    } else if last_idx > 0 {
                        let extra_per_seg = slack / last_idx as f64;
                        for (s_i, seg) in bl.segs.iter_mut().enumerate().take(last_idx + 1) {
                            seg.rel_x += s_i as f64 * extra_per_seg;
                        }
                        bl.line.width = avail;
                    }
                }
            }
        }

        let is_para_start = paragraph_starts.contains(&bl.line.start);
        let para_gap = if lines_in_col > 0 && is_para_start {
            field.space_before.max(field.space_after).max(0.0)
        } else {
            0.0
        };
        let need_next_col = if cur_col + 1 < cols && lines_in_col > 0 && !is_vertical {
            if let Some(target_lines) = balanced_lines_per_col {
                lines_in_col >= target_lines
            } else if let Some(col_h) = col_avail_h_opt {
                y_in_col + para_gap + bl.line.height > col_h
            } else {
                false
            }
        } else {
            false
        };
        if need_next_col {
            col_ranges.push((cur_col_start_idx, idx));
            cur_col += 1;
            cur_col_start_idx = idx;
            y_in_col = 0.0;
            lines_in_col = 0;
        } else {
            y_in_col += para_gap;
        }

        line_col_indices[idx] = cur_col;
        let col_w = col_width_opt.unwrap_or(0.0);
        let col_x_origin = if cols <= 1 {
            pad_l
        } else if field.direction == "rtl" {
            pad_l + f64::from(cols - 1 - cur_col) * (col_w + gutter)
        } else {
            pad_l + f64::from(cur_col) * (col_w + gutter)
        };
        let align_shift = avail_opt.map_or(0., |avail| {
            let mode = if align_mode == "justify_all" {
                "left"
            } else {
                align_mode
            };
            align_offset(mode, avail, bl.line.width)
        });
        bl.line.x = col_x_origin + rel_base_x + align_shift;
        bl.line.y = col_top_y + y_in_col;
        let advance = line_advance_for(bl.max_size, bl.leading, &field.leading_mode);
        y_in_col += advance;
        lines_in_col += 1;
        max_col_bottom = max_col_bottom.max(bl.line.y + bl.line.height);
        text_width = text_width.max(bl.line.width);
    }
    if cur_col_start_idx < line_count_usize {
        col_ranges.push((cur_col_start_idx, line_count_usize));
    }

    // Apply per-column vertical alignment (Top / Center / Bottom / Justify) when box_size is set.
    if let Some(col_avail_h) = col_avail_h_opt
        && !is_top_str(&field.vertical_align)
        && !is_vertical
    {
        for (start_idx, end_idx) in &col_ranges {
            if start_idx >= end_idx {
                continue;
            }
            let last_line = &built_lines[end_idx - 1].line;
            let content_h = (last_line.y - col_top_y) + last_line.height;
            let slack_v = (col_avail_h - content_h).max(0.0);
            if slack_v <= 0.0 {
                continue;
            }
            match field.vertical_align.as_str() {
                "center" => {
                    let shift = slack_v * 0.5;
                    for bl in &mut built_lines[*start_idx..*end_idx] {
                        bl.line.y += shift;
                    }
                }
                "bottom" => {
                    for bl in &mut built_lines[*start_idx..*end_idx] {
                        bl.line.y += slack_v;
                    }
                }
                "justify" if end_idx - start_idx > 1 => {
                    let count_in_col = end_idx - start_idx;
                    let step = slack_v / (count_in_col - 1) as f64;
                    for (local_i, bl) in built_lines[*start_idx..*end_idx].iter_mut().enumerate() {
                        bl.line.y += local_i as f64 * step;
                    }
                }
                _ => {}
            }
            let new_bottom = built_lines[end_idx - 1].line.y + built_lines[end_idx - 1].line.height;
            max_col_bottom = max_col_bottom.max(new_bottom);
        }
    }

    let base_advance =
        line_advance_for(field.format.size, field.format.leading, &field.leading_mode);
    let text_height = if built_lines.is_empty() {
        field.format.size * 1.2
    } else {
        (max_col_bottom - pad_t).max(field.format.size * 1.2)
    };
    let visible_height = field
        .box_size
        .map_or(text_height, |size| (size.height - pad_t - pad_b).max(0.));
    let visible_lines = (visible_height / base_advance.max(1.)).floor().max(1.) as u32;
    let line_count = built_lines.len() as u32;
    let maxscroll = if field.box_size.is_some() {
        line_count
            .saturating_sub(visible_lines)
            .saturating_add(1)
            .max(1)
    } else {
        1
    };
    let (_, first_avail, _) = line_metrics_at(0);
    let maxhscroll = if field.box_size.is_some() && !field.word_wrap {
        (text_width - first_avail.unwrap_or(text_width))
            .max(0.)
            .ceil()
            .min(1_000_000.) as u32
    } else {
        0
    };
    let scroll = field.as2_viewport.scroll.min(maxscroll);
    let hscroll = field.as2_viewport.hscroll.min(maxhscroll);
    let bottom_scroll = (scroll + (visible_height / base_advance.max(1.)).ceil().max(1.) as u32
        - 1)
    .min(line_count);
    let scroll_y = if scroll <= 1 {
        0.0
    } else {
        built_lines
            .get((scroll - 1) as usize)
            .map_or(f64::from(scroll - 1) * base_advance, |b| b.line.y - pad_t)
    };

    let mut runs = Vec::new();
    let mut decorations = Vec::new();
    let mut highlights = Vec::new();
    let mut bullets = Vec::new();
    let mut lines = Vec::with_capacity(built_lines.len());
    let sub_sup_shift = |max_size: f64| -> f64 {
        if field.superscript {
            -max_size * 0.35
        } else if field.subscript {
            max_size * 0.20
        } else {
            0.0
        }
    };
    let baseline_shift = -field.baseline_shift;

    if is_vertical {
        let box_w = field
            .box_size
            .map_or(text_width.max(field.format.size * 2.0), |b| b.width);
        let mut x_cursor = if field.orientation == "vertical_rtl" {
            (box_w - pad_r - field.format.size * 1.2).max(pad_l)
        } else {
            pad_l
        };
        for mut bl in built_lines {
            let col_step = bl.max_size * 1.2 + bl.leading.max(0.0);
            bl.line.x = x_cursor;
            bl.line.y = col_top_y;
            let mut char_y = col_top_y;
            for seg in bl.segs {
                let shaped_seg = if !is_common_str(&field.ligatures) {
                    apply_ligatures(&seg.text, &field.ligatures)
                } else {
                    seg.text.clone()
                };
                for ch in shaped_seg.chars() {
                    let s = ch.to_string();
                    let mut char_fmt = seg.fmt.clone();
                    let mut digit_y_shift = 0.0;
                    if field.digit_case == "old_style" && ch.is_ascii_digit() {
                        let (scaled, shift) = old_style_digit_metrics(ch, char_fmt.size);
                        char_fmt.size = scaled;
                        digit_y_shift = shift;
                    }
                    let rot = glyph_rotation_degrees(ch, &field.glyph_rotation, true);
                    if field.highlight && !ch.is_whitespace() {
                        highlights.push(TextDecoration {
                            x: x_cursor,
                            y: char_y,
                            width: char_fmt.size,
                            height: char_fmt.size * 1.1,
                            color: Some(field.highlight_color),
                        });
                    }
                    runs.push(TextRun {
                        text: s,
                        x: x_cursor,
                        y: char_y + baseline_shift + digit_y_shift,
                        format: Some(char_fmt),
                        rotation: rot,
                    });
                    char_y += (seg.fmt.size * 1.05 + seg.fmt.letter_spacing).max(2.0);
                }
            }
            bl.line.width = bl.max_size;
            bl.line.height = (char_y - col_top_y).max(bl.max_size * 1.2);
            lines.push(bl.line);
            if field.orientation == "vertical_rtl" {
                x_cursor -= col_step;
            } else {
                x_cursor += col_step;
            }
        }
    } else {
        for mut bl in built_lines {
            bl.line.x -= f64::from(hscroll);
            bl.line.y -= scroll_y;
            let pfmt = fmt_at(bl.line.start);
            if pfmt.bullet && paragraph_starts.contains(&bl.line.start) {
                bullets.push([
                    bl.line.x - pfmt.size * 0.6,
                    bl.line.y + pfmt.size * 0.6,
                    (pfmt.size * 0.1).max(1.),
                ]);
            }
            let v_shift = sub_sup_shift(bl.max_size) + baseline_shift;
            for seg in bl.segs {
                let sx = bl.line.x + seg.rel_x;
                let mut run_fmt = seg.fmt;
                let mut digit_y_shift = 0.0;
                if field.digit_case == "old_style"
                    && seg.text.len() == 1
                    && let Some(ch) = seg.text.chars().next()
                    && ch.is_ascii_digit()
                {
                    let (scaled, shift) = old_style_digit_metrics(ch, run_fmt.size);
                    run_fmt.size = scaled;
                    digit_y_shift = shift;
                }
                let sy =
                    bl.line.y + (bl.max_size - run_fmt.size).max(0.0) + v_shift + digit_y_shift;
                if field.highlight && !seg.text.is_empty() {
                    highlights.push(TextDecoration {
                        x: sx,
                        y: bl.line.y,
                        width: seg.width,
                        height: bl.line.height,
                        color: Some(field.highlight_color),
                    });
                }
                if run_fmt.underline && !seg.text.is_empty() {
                    decorations.push(TextDecoration {
                        x: sx,
                        y: sy + run_fmt.size * 1.05,
                        width: seg.width,
                        height: (run_fmt.size / 16.).max(1.),
                        color: Some(run_fmt.color),
                    });
                }
                if field.strikethrough && !seg.text.is_empty() {
                    decorations.push(TextDecoration {
                        x: sx,
                        y: sy + run_fmt.size * 0.55,
                        width: seg.width,
                        height: (run_fmt.size / 16.).max(1.),
                        color: Some(run_fmt.color),
                    });
                }
                let shaped_text = if !is_common_str(&field.ligatures) {
                    apply_ligatures(&seg.text, &field.ligatures)
                } else {
                    seg.text
                };
                let first_ch = shaped_text.chars().next().unwrap_or(' ');
                let rot = glyph_rotation_degrees(first_ch, &field.glyph_rotation, false);
                runs.push(TextRun {
                    text: shaped_text,
                    x: sx,
                    y: sy,
                    format: Some(run_fmt),
                    rotation: rot,
                });
            }
            lines.push(bl.line);
        }
    }

    let viewport = TextViewportMetrics {
        offset_x: f64::from(hscroll),
        offset_y: scroll_y,
        scroll,
        hscroll,
        maxscroll,
        maxhscroll,
        bottom_scroll,
    };
    let surface = TextSurface {
        width: field.box_size.map_or(text_width, |size| size.width),
        height: field.box_size.map_or(text_height, |size| size.height),
        background: field
            .as2_viewport
            .background
            .then_some(field.as2_viewport.background_color),
        border: field
            .as2_viewport
            .border
            .then_some(field.as2_viewport.border_color),
        border_width: field.border_width,
        column_count: cols,
        column_gutter: field.column_gutter,
        ligatures: field.ligatures.clone(),
        digit_width: field.digit_width.clone(),
        digit_case: field.digit_case.clone(),
        locale: field.locale.clone(),
        glyph_rotation: field.glyph_rotation.clone(),
    };
    Ok(TextLayout {
        lines,
        text_width,
        text_height,
        line_height: base_advance,
        decorations,
        highlights,
        runs,
        bullets,
        viewport,
        surface,
    })
}

/// At most three host calls for uniform text; summed segment widths choose wraps,
/// then complete lines are measured once for coherent alignment/metrics. Multi-run
/// text delegates to `layout_multirun_text` so every run renders with its own style.
pub fn layout_text<M>(field: &TextFieldState, mut measure: M) -> Result<TextLayout, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    field.validate()?;
    if field.html
        && field.format_runs.is_empty()
        && field.text.contains('<')
        && let Ok((parsed_text, parsed_format, parsed_runs)) = crate::as2_text::parse_authored_html(
            &field.text,
            field.format.clone(),
            field.condense_white,
        )
    {
        let mut effective = field.clone();
        effective.html = false;
        effective.text = parsed_text;
        effective.format = parsed_format;
        effective.format_runs = parsed_runs;
        let max_len = effective.text.encode_utf16().count() as u32;
        effective.selection_begin = effective.selection_begin.min(max_len);
        effective.selection_end = effective.selection_end.min(max_len);
        effective.caret = effective.caret.min(max_len);
        return layout_text(&effective, measure);
    }
    if !field.format_runs.is_empty() || field.has_extended_layout_options() {
        return layout_multirun_text(field, measure);
    }
    let parts = parts(field)?;
    let strings: Vec<_> = parts.iter().map(|p| p.text.clone()).collect();
    let widths = measured(&strings, &field.format, &mut measure)?;
    let padding = if field.box_size.is_some() { 2. } else { 0. };
    let bullet_indent = if field.format.bullet {
        field.format.size
    } else {
        0.
    };
    let origin_x = padding + field.format.left_margin + field.format.block_indent + bullet_indent;
    let available = field.box_size.map(|b| {
        (b.width
            - 2. * padding
            - field.format.left_margin
            - field.format.right_margin
            - field.format.block_indent
            - bullet_indent)
            .max(0.)
    });
    let mut paragraph_starts = BTreeSet::from([0u32]);
    let mut offset = 0;
    for ch in field.text.chars() {
        offset += ch.len_utf16() as u32;
        if ch == '\n' {
            paragraph_starts.insert(offset);
        }
    }
    let line_indent = |start: u32| {
        if paragraph_starts.contains(&start) {
            field.format.indent
        } else {
            0.
        }
    };
    let available_at = |start: u32| (available.unwrap_or(f64::MAX) - line_indent(start)).max(0.);
    let wrap = field.multiline && field.word_wrap && available.is_some();
    let mut glyphs = BTreeSet::new();
    if wrap {
        for (part, width) in parts.iter().zip(&widths) {
            if *width > (available.unwrap() - field.format.indent.max(0.)).max(0.) {
                glyphs.extend(part.text.chars());
                if glyphs.len() > MAX_GLYPHS {
                    return Err(error("layout exceeds 65536 fallback glyphs"));
                }
            }
        }
    }
    let glyph_strings: Vec<_> = glyphs.iter().map(char::to_string).collect();
    let glyph_widths = if glyph_strings.is_empty() {
        vec![]
    } else {
        measured(&glyph_strings, &field.format, &mut measure)?
    };
    let glyph_widths: HashMap<_, _> = glyphs.into_iter().zip(glyph_widths).collect();
    let mut lines = Vec::new();
    let mut current = TextLine {
        text: String::new(),
        start: 0,
        end: 0,
        x: 0.,
        y: 0.,
        width: 0.,
        height: field.format.size * 1.2,
    };
    let flush = |lines: &mut Vec<TextLine>, current: &mut TextLine| -> Result<(), TextError> {
        if lines.len() >= MAX_PARTS {
            return Err(error("layout exceeds 16384 lines"));
        }
        let next = TextLine {
            text: String::new(),
            start: current.end,
            end: current.end,
            x: 0.,
            y: 0.,
            width: 0.,
            height: current.height,
        };
        lines.push(std::mem::replace(current, next));
        Ok(())
    };
    for (part, width) in parts.iter().zip(&widths) {
        if part.newline {
            current.end = part.end;
            flush(&mut lines, &mut current)?;
            continue;
        }
        if wrap && part.text == "\t" && !field.format.tab_stops.is_empty() {
            let next_x = next_tab_stop(current.width, &field.format);
            if !current.text.is_empty() && next_x > available_at(current.start) {
                flush(&mut lines, &mut current)?;
                current.text.push('\t');
                current.width = next_tab_stop(0.0, &field.format);
            } else {
                current.text.push('\t');
                current.width = next_x;
            }
            current.end = part.end;
            continue;
        }
        if wrap && *width > available_at(current.start) {
            let mut offset = part.start;
            for ch in part.text.chars() {
                let width = glyph_widths[&ch];
                if !current.text.is_empty() && current.width + width > available_at(current.start) {
                    flush(&mut lines, &mut current)?
                }
                if !current.text.is_empty() {
                    current.width += field.format.letter_spacing;
                }
                current.text.push(ch);
                current.width += width;
                offset += ch.len_utf16() as u32;
                current.end = offset;
            }
        } else {
            if wrap
                && !current.text.is_empty()
                && current.width + width > available_at(current.start)
            {
                flush(&mut lines, &mut current)?
            }
            if !current.text.is_empty() {
                current.width += field.format.letter_spacing;
            }
            current.text.push_str(&part.text);
            current.width += width;
            current.end = part.end;
        }
    }
    flush(&mut lines, &mut current)?;
    let strings: Vec<_> = lines.iter().map(|line| line.text.clone()).collect();
    let mut widths = measured(&strings, &field.format, &mut measure)?;
    let mut tab_runs: Vec<Vec<(String, f64)>> = vec![];
    if !field.format.tab_stops.is_empty() {
        let segments: Vec<String> = lines
            .iter()
            .flat_map(|line| line.text.split('\t').map(str::to_owned))
            .collect();
        if segments.len() > MAX_PARTS {
            return Err(error("custom tab layout exceeds16384 segments"));
        }
        let sizes = measured(&segments, &field.format, &mut measure)?;
        let mut index = 0;
        for (line, width) in lines.iter().zip(&mut widths) {
            let mut x = 0.;
            let mut runs = vec![];
            let mut parts = line.text.split('\t').peekable();
            while let Some(part) = parts.next() {
                runs.push((part.into(), x));
                x += sizes[index];
                index += 1;
                if parts.peek().is_some() {
                    x = next_tab_stop(x, &field.format);
                }
            }
            *width = x;
            tab_runs.push(runs);
        }
    }
    let align_mode = field.format.effective_align_mode();
    if align_mode == "justify" && available.is_some() && tab_runs.is_empty() && lines.len() > 1 {
        let line_total = lines.len();
        let next_line_starts: Vec<Option<u32>> = lines
            .iter()
            .skip(1)
            .map(|l| Some(l.start))
            .chain(std::iter::once(None))
            .collect();
        let mut all_words: Vec<String> = Vec::new();
        let mut line_word_counts: Vec<usize> = Vec::with_capacity(line_total);
        for (idx, line) in lines.iter().enumerate() {
            let soft_wrapped = idx + 1 < line_total
                && next_line_starts[idx].is_some_and(|ns| !paragraph_starts.contains(&ns));
            if soft_wrapped {
                let words: Vec<String> = line.text.split_whitespace().map(str::to_owned).collect();
                line_word_counts.push(words.len());
                all_words.extend(words);
            } else {
                line_word_counts.push(0);
            }
        }
        if !all_words.is_empty() && all_words.len() <= MAX_PARTS {
            let word_widths = measured(&all_words, &field.format, &mut measure)?;
            let mut cursor = 0usize;
            for (idx, (line, width)) in lines.iter().zip(&mut widths).enumerate() {
                let count = line_word_counts[idx];
                if count >= 2 {
                    let words = &all_words[cursor..cursor + count];
                    let ww = &word_widths[cursor..cursor + count];
                    let words_sum: f64 = ww.iter().sum();
                    let target_w = available_at(line.start);
                    if target_w > words_sum {
                        let gap = (target_w - words_sum) / (count - 1) as f64;
                        let mut x = 0.0;
                        let mut runs = Vec::with_capacity(count);
                        for (w_str, &w_len) in words.iter().zip(ww) {
                            runs.push((w_str.clone(), x));
                            x += w_len + gap;
                        }
                        *width = target_w;
                        tab_runs.push(runs);
                        cursor += count;
                        continue;
                    }
                }
                cursor += count;
                tab_runs.push(vec![(line.text.clone(), 0.0)]);
            }
        }
    }
    let advance = field.format.size * 1.2 + field.format.leading;
    let mut text_width = 0_f64;
    for (index, (line, width)) in lines.iter_mut().zip(widths).enumerate() {
        line.width = width;
        line.x = origin_x
            + line_indent(line.start)
            + available.map_or(0., |_| {
                align_offset(align_mode, available_at(line.start), width)
            });
        line.y = padding + index as f64 * advance;
        text_width = text_width.max(width);
    }
    let text_height = field.format.size * 1.2 + (lines.len().saturating_sub(1)) as f64 * advance;
    let visible_height = field
        .box_size
        .map_or(text_height, |size| (size.height - 2. * padding).max(0.));
    let visible_lines = (visible_height / advance).floor().max(1.) as u32;
    let line_count = lines.len() as u32;
    let maxscroll = if field.box_size.is_some() {
        line_count
            .saturating_sub(visible_lines)
            .saturating_add(1)
            .max(1)
    } else {
        1
    };
    let maxhscroll = if field.box_size.is_some() && !field.word_wrap {
        (text_width - available.unwrap_or(text_width))
            .max(0.)
            .ceil()
            .min(1_000_000.) as u32
    } else {
        0
    };
    let scroll = field.as2_viewport.scroll.min(maxscroll);
    let hscroll = field.as2_viewport.hscroll.min(maxhscroll);
    let bottom_scroll =
        (scroll + (visible_height / advance).ceil().max(1.) as u32 - 1).min(line_count);
    for line in &mut lines {
        line.x -= f64::from(hscroll);
        line.y -= f64::from(scroll - 1) * advance;
    }
    let viewport = TextViewportMetrics {
        offset_x: f64::from(hscroll),
        offset_y: f64::from(scroll - 1) * advance,
        scroll,
        hscroll,
        maxscroll,
        maxhscroll,
        bottom_scroll,
    };
    let surface = TextSurface {
        width: field.box_size.map_or(text_width, |size| size.width),
        height: field.box_size.map_or(text_height, |size| size.height),
        background: field
            .as2_viewport
            .background
            .then_some(field.as2_viewport.background_color),
        border: field
            .as2_viewport
            .border
            .then_some(field.as2_viewport.border_color),
        border_width: field.border_width,
        column_count: 1,
        column_gutter: field.column_gutter,
        ligatures: field.ligatures.clone(),
        digit_width: field.digit_width.clone(),
        digit_case: field.digit_case.clone(),
        locale: field.locale.clone(),
        glyph_rotation: field.glyph_rotation.clone(),
    };
    let runs = tab_runs
        .into_iter()
        .zip(&lines)
        .flat_map(|(runs, line)| {
            runs.into_iter().map(move |(text, x)| TextRun {
                text,
                x: line.x + x,
                y: line.y,
                format: None,
                rotation: 0.0,
            })
        })
        .collect();
    let bullets = if field.format.bullet {
        lines
            .iter()
            .filter(|line| paragraph_starts.contains(&line.start))
            .map(|line| {
                [
                    line.x - field.format.size * 0.6,
                    line.y + field.format.size * 0.6,
                    (field.format.size * 0.1).max(1.),
                ]
            })
            .collect()
    } else {
        vec![]
    };
    let decorations = if field.format.underline {
        lines
            .iter()
            .filter(|line| !line.text.is_empty())
            .map(|line| TextDecoration {
                x: line.x,
                y: line.y + field.format.size * 1.05,
                width: line.width,
                height: (field.format.size / 16.).max(1.),
                color: None,
            })
            .collect()
    } else {
        vec![]
    };
    Ok(TextLayout {
        lines,
        text_width,
        text_height,
        line_height: advance,
        decorations,
        highlights: vec![],
        runs,
        bullets,
        viewport,
        surface,
    })
}
