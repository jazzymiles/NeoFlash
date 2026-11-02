//! Shared ActionScript 3 Area 8 runtime engines:
//! 1. Flash Text Engine (`flash.text.engine.*`) paragraph layout, atomization,
//!    justification, tab stops, word/atom boundary queries, and font metrics.
//! 2. Stage3D (`flash.display3D.*`) state machine, AGAL v1/v2 shader interpreter,
//!    buffer/texture arenas, and software triangle/wireframe rasterizer.
//! 3. Pixel Bender (`flash.display.Shader*`, `flash.filters.ShaderFilter`) `.pbj`
//!    and portable shader metadata decoder & per-pixel RGBA/float32 evaluator,
//!    plus 3D `DisplayObject` / `Transform` matrix and projection algebra.

#![allow(clippy::chunks_exact_to_as_chunks)]

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const FTE_MAX_LINE_WIDTH: f64 = 1_000_000.0;
pub const FTE_GRAPHIC_CHAR: char = '\u{FDEF}';
pub const MAX_STAGE3D_DIM: u32 = 4096;
pub const MAX_STAGE3D_BUFFERS: usize = 1024;
pub const MAX_STAGE3D_VERTICES: usize = 65_536;
pub const MAX_STAGE3D_INDICES: usize = 196_608;
pub const MAX_SHADER_PIXELS: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Fte3dError {
    #[error("ArgumentError: {0}")]
    Argument(String),
    #[error("RangeError: {0}")]
    Range(String),
    #[error("IllegalOperationError: {0}")]
    IllegalOperation(String),
    #[error("TypeError: {0}")]
    Type(String),
}

// ============================================================================
// 1. Flash Text Engine (FTE)
// ============================================================================

pub fn validate_fte_enum<'a>(kind: &str, value: &'a str) -> Result<&'a str, Fte3dError> {
    let ok = match kind {
        "fontWeight" => matches!(value, "normal" | "bold"),
        "fontPosture" => matches!(value, "normal" | "italic"),
        "fontLookup" => matches!(value, "device" | "embeddedCFF"),
        "renderingMode" => matches!(value, "normal" | "cff"),
        "cffHinting" => matches!(value, "none" | "horizontalStem"),
        "textRotation" => matches!(
            value,
            "auto" | "rotate0" | "rotate90" | "rotate180" | "rotate270"
        ),
        "lineRotation" => matches!(
            value,
            "rotate0" | "rotate90" | "rotate180" | "rotate270" | "auto"
        ),
        "textBaseline" => matches!(
            value,
            "roman"
                | "ascent"
                | "descent"
                | "ideographicTop"
                | "ideographicCenter"
                | "ideographicBottom"
                | "useDominantBaseline"
        ),
        "baselineZero" => matches!(
            value,
            "roman"
                | "ascent"
                | "descent"
                | "ideographicTop"
                | "ideographicCenter"
                | "ideographicBottom"
        ),
        "kerning" => matches!(value, "on" | "off" | "auto"),
        "breakOpportunity" => matches!(value, "auto" | "any" | "none" | "all"),
        "digitCase" => matches!(value, "default" | "lining" | "oldStyle"),
        "digitWidth" => matches!(value, "default" | "proportional" | "tabular"),
        "ligatureLevel" => matches!(value, "none" | "minimum" | "common" | "uncommon" | "exotic"),
        "typographicCase" => matches!(
            value,
            "default"
                | "title"
                | "caps"
                | "smallCaps"
                | "uppercase"
                | "lowercase"
                | "capsAndSmallCaps"
        ),
        "lineJustification" => matches!(
            value,
            "unjustified" | "allButLast" | "allButMandatoryBreak" | "allIncludingLast"
        ),
        "justificationStyle" => matches!(
            value,
            "pushInKinsoku" | "pushOutOnly" | "prioritizeLeastAdjustment"
        ),
        "tabAlignment" => matches!(value, "start" | "center" | "end" | "decimal"),
        "textLineValidity" => matches!(value, "valid" | "possiblyInvalid" | "invalid" | "static"),
        _ => false,
    };
    if ok {
        Ok(value)
    } else {
        Err(Fte3dError::Argument(format!(
            "Parameter {kind} must be one of the accepted flash.text.engine constants; got '{value}'."
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteFontMetrics {
    pub em_box: [f64; 4],
    pub strikethrough_offset: f64,
    pub strikethrough_thickness: f64,
    pub underline_offset: f64,
    pub underline_thickness: f64,
    pub subscript_offset: f64,
    pub subscript_scale: f64,
    pub superscript_offset: f64,
    pub superscript_scale: f64,
    pub line_gap: f64,
}

pub fn compute_font_metrics(
    _font_name: &str,
    font_weight: &str,
    _font_posture: &str,
    font_size: f64,
) -> Result<FteFontMetrics, Fte3dError> {
    if !font_size.is_finite() || font_size < 0.0 {
        return Err(Fte3dError::Argument(
            "ElementFormat.fontSize must be a non-negative finite Number.".into(),
        ));
    }
    let size = font_size;
    let ascent = size * 0.8;
    let descent = size * 0.2;
    let bold_factor = if font_weight == "bold" { 0.08 } else { 0.05 };
    let stroke = if size == 0.0 {
        0.0
    } else {
        (size * bold_factor).max(0.5)
    };
    Ok(FteFontMetrics {
        em_box: [0.0, -ascent, size, ascent + descent],
        strikethrough_offset: -size * 0.28,
        strikethrough_thickness: stroke,
        underline_offset: size * 0.12,
        underline_thickness: stroke,
        subscript_offset: size * 0.15,
        subscript_scale: 0.583,
        superscript_offset: -size * 0.35,
        superscript_scale: 0.583,
        line_gap: size * 0.15,
    })
}

pub fn is_font_compatible(
    font_name: &str,
    font_weight: &str,
    font_posture: &str,
    device: bool,
) -> Result<bool, Fte3dError> {
    validate_fte_enum("fontWeight", font_weight)?;
    validate_fte_enum("fontPosture", font_posture)?;
    let trimmed = font_name.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    if device {
        Ok(!trimmed.eq_ignore_ascii_case("__missing_font__"))
    } else {
        let lower = trimmed.to_ascii_lowercase();
        Ok(matches!(
            lower.as_str(),
            "_sans"
                | "_serif"
                | "_typewriter"
                | "arial"
                | "helvetica"
                | "times new roman"
                | "times"
                | "courier new"
                | "courier"
                | "verdana"
                | "georgia"
                | "tahoma"
                | "trebuchet ms"
                | "menlo"
                | "monaco"
                | "system-ui"
                | "sans-serif"
                | "serif"
                | "monospace"
        ) || lower.ends_with("cff"))
    }
}

pub fn justifier_kind_for_locale(locale: &str) -> Result<&'static str, Fte3dError> {
    let trimmed = locale.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
    {
        return Err(Fte3dError::Argument(format!(
            "Invalid locale identifier '{locale}'."
        )));
    }
    let primary = trimmed
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if primary.len() < 2 || primary.len() > 8 || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(Fte3dError::Argument(format!(
            "Invalid locale language subtag in '{locale}'."
        )));
    }
    if matches!(primary.as_str(), "ja" | "zh" | "ko") {
        Ok("east_asian")
    } else {
        Ok("space")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteTabStopSpec {
    pub alignment: String,
    pub position: f64,
    #[serde(default)]
    pub decimal_alignment_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteJustifierSpec {
    pub kind: String,
    pub locale: String,
    pub line_justification: String,
    #[serde(default)]
    pub letter_spacing: bool,
    #[serde(default = "default_min_spacing")]
    pub minimum_spacing: f64,
    #[serde(default = "default_opt_spacing")]
    pub optimum_spacing: f64,
    #[serde(default = "default_max_spacing")]
    pub maximum_spacing: f64,
    #[serde(default)]
    pub justification_style: String,
    #[serde(default)]
    pub compose_trailing_ideographic_spaces: bool,
}

fn default_min_spacing() -> f64 {
    0.5
}
fn default_opt_spacing() -> f64 {
    1.0
}
fn default_max_spacing() -> f64 {
    1.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteInputElementRun {
    pub element_id: u32,
    pub is_graphic: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub graphic_width: f64,
    #[serde(default)]
    pub graphic_height: f64,
    #[serde(default)]
    pub font_name: String,
    #[serde(default = "default_font_size")]
    pub font_size: f64,
    #[serde(default)]
    pub font_weight: String,
    #[serde(default)]
    pub font_posture: String,
    #[serde(default)]
    pub color: u32,
    #[serde(default = "default_opt_spacing")]
    pub alpha: f64,
    #[serde(default)]
    pub tracking_left: f64,
    #[serde(default)]
    pub tracking_right: f64,
    #[serde(default)]
    pub baseline_shift: f64,
    #[serde(default)]
    pub text_rotation: String,
    #[serde(default)]
    pub break_opportunity: String,
    #[serde(default)]
    pub digit_width: String,
    #[serde(default)]
    pub typographic_case: String,
    #[serde(default)]
    pub has_mirror: bool,
}

fn default_font_size() -> f64 {
    12.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteLayoutRequest {
    pub runs: Vec<FteInputElementRun>,
    pub start_char: usize,
    pub max_width: f64,
    #[serde(default)]
    pub line_offset: f64,
    #[serde(default)]
    pub fit_something: bool,
    pub justifier: FteJustifierSpec,
    #[serde(default)]
    pub tab_stops: Vec<FteTabStopSpec>,
    #[serde(default)]
    pub baseline_zero: String,
    #[serde(default = "default_font_size")]
    pub baseline_font_size: f64,
    #[serde(default)]
    pub line_rotation: String,
    #[serde(default)]
    pub bidi_level: i32,
    #[serde(default)]
    pub apply_non_linear_font_scaling: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteAtomOut {
    pub bounds: [f64; 4],
    pub center: f64,
    pub bidi_level: i32,
    pub char_begin: usize,
    pub char_end: usize,
    pub text_rotation: String,
    pub word_boundary_left: bool,
    pub is_graphic: bool,
    pub element_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteMirrorRegionOut {
    pub element_id: u32,
    pub bounds: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteDrawRunOut {
    pub x: f64,
    pub y: f64,
    pub text: String,
    pub font_family: String,
    pub font_size: f64,
    pub color: [u8; 4],
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub rotation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteLineOut {
    pub text_block_begin_index: usize,
    pub raw_text_length: usize,
    pub specified_width: f64,
    pub text_width: f64,
    pub text_height: f64,
    pub unjustified_text_width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub total_ascent: f64,
    pub total_descent: f64,
    pub total_height: f64,
    pub has_graphic_element: bool,
    pub has_tabs: bool,
    pub atoms: Vec<FteAtomOut>,
    pub mirror_regions: Vec<FteMirrorRegionOut>,
    pub baselines: BTreeMap<String, f64>,
    pub draw_runs: Vec<FteDrawRunOut>,
    pub dump: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FteLayoutResponse {
    pub creation_result: String,
    pub line: Option<FteLineOut>,
}

#[derive(Clone)]
struct RawAtom {
    ch: char,
    display_ch: char,
    small_cap: bool,
    char_begin: usize,
    char_end: usize,
    advance: f64,
    ascent: f64,
    descent: f64,
    line_gap: f64,
    baseline_shift: f64,
    bidi_level: i32,
    text_rotation: String,
    break_opportunity: String,
    word_boundary_left: bool,
    is_graphic: bool,
    is_tab: bool,
    is_newline: bool,
    is_space: bool,
    element_id: u32,
    has_mirror: bool,
    font_name: String,
    font_size: f64,
    color: [u8; 4],
    bold: bool,
    italic: bool,
}

fn is_cjk(ch: char) -> bool {
    let u = ch as u32;
    (0x3000..=0x9FFF).contains(&u)
        || (0xAC00..=0xD7AF).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFF00..=0xFFEF).contains(&u)
}

fn is_rtl_char(ch: char) -> bool {
    let u = ch as u32;
    (0x0590..=0x08FF).contains(&u) || (0xFB1D..=0xFEFC).contains(&u)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn glyph_advance(
    ch: char,
    font_size: f64,
    bold: bool,
    tabular_digits: bool,
    small_cap: bool,
    non_linear: bool,
) -> f64 {
    if ch == '\n' || ch == '\r' || ch == '\u{2028}' || ch == '\u{2029}' {
        return 0.0;
    }
    let eff_size = if small_cap {
        font_size * 0.78
    } else {
        font_size
    };
    let ratio = if ch == ' ' || ch == '\u{00A0}' {
        0.30
    } else if ch == '\t' {
        1.50
    } else if ch == '\u{3000}' || is_cjk(ch) {
        1.0
    } else if ch.is_ascii_digit() {
        if tabular_digits { 0.60 } else { 0.56 }
    } else if matches!(
        ch,
        'i' | 'l' | 'I' | '!' | '.' | ',' | ':' | ';' | '\'' | '|'
    ) {
        0.30
    } else if matches!(ch, 'm' | 'w' | 'M' | 'W' | '@' | '%') {
        0.82
    } else if ch.is_uppercase() {
        0.66
    } else {
        0.54
    };
    let mut w = eff_size * ratio;
    if bold {
        w *= 1.07;
    }
    if non_linear && eff_size > 0.0 && eff_size < 10.0 {
        w *= 1.03;
    }
    w
}

fn flatten_paragraph(
    runs: &[FteInputElementRun],
    block_bidi: i32,
    non_linear: bool,
) -> Vec<RawAtom> {
    let mut atoms = Vec::new();
    let mut utf16_pos = 0usize;
    let mut prev_ch: Option<char> = None;

    for run in runs {
        let rot = if run.text_rotation.is_empty() || run.text_rotation == "auto" {
            "rotate0".to_owned()
        } else {
            run.text_rotation.clone()
        };
        let brk = if run.break_opportunity.is_empty() {
            "auto".to_owned()
        } else {
            run.break_opportunity.clone()
        };
        let rgba = [
            ((run.color >> 16) & 0xff) as u8,
            ((run.color >> 8) & 0xff) as u8,
            (run.color & 0xff) as u8,
            (run.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        ];
        let font_name = if run.font_name.is_empty() {
            "serif".to_owned()
        } else {
            match run.font_name.as_str() {
                "_sans" => "sans-serif".to_owned(),
                "_serif" => "serif".to_owned(),
                "_typewriter" => "monospace".to_owned(),
                other => other.to_owned(),
            }
        };

        if run.is_graphic {
            let gw = run.graphic_width.max(0.0);
            let gh = run.graphic_height.max(0.0);
            let word_left = prev_ch.is_none_or(|p| p.is_whitespace());
            atoms.push(RawAtom {
                ch: FTE_GRAPHIC_CHAR,
                display_ch: FTE_GRAPHIC_CHAR,
                small_cap: false,
                char_begin: utf16_pos,
                char_end: utf16_pos + 1,
                advance: (gw + run.tracking_left + run.tracking_right).max(0.0),
                ascent: gh,
                descent: 0.0,
                line_gap: 0.0,
                baseline_shift: run.baseline_shift,
                bidi_level: block_bidi,
                text_rotation: rot,
                break_opportunity: brk,
                word_boundary_left: word_left,
                is_graphic: true,
                is_tab: false,
                is_newline: false,
                is_space: false,
                element_id: run.element_id,
                has_mirror: run.has_mirror,
                font_name,
                font_size: run.font_size.max(0.0),
                color: rgba,
                bold: false,
                italic: false,
            });
            utf16_pos += 1;
            prev_ch = Some(FTE_GRAPHIC_CHAR);
            continue;
        }

        let bold = run.font_weight == "bold";
        let italic = run.font_posture == "italic";
        let tabular = run.digit_width == "tabular";
        let size = run.font_size.max(0.0);
        let ascent = size * 0.8;
        let descent = size * 0.2;
        let line_gap = size * 0.15;

        let chars: Vec<char> = run.text.chars().collect();
        let mut idx = 0usize;
        while idx < chars.len() {
            let ch = chars[idx];
            // Combine \r\n into one mandatory-break atom spanning 2 UTF-16 code units.
            if ch == '\r' && idx + 1 < chars.len() && chars[idx + 1] == '\n' {
                atoms.push(RawAtom {
                    ch: '\n',
                    display_ch: '\n',
                    small_cap: false,
                    char_begin: utf16_pos,
                    char_end: utf16_pos + 2,
                    advance: 0.0,
                    ascent,
                    descent,
                    line_gap,
                    baseline_shift: run.baseline_shift,
                    bidi_level: block_bidi,
                    text_rotation: rot.clone(),
                    break_opportunity: brk.clone(),
                    word_boundary_left: false,
                    is_graphic: false,
                    is_tab: false,
                    is_newline: true,
                    is_space: false,
                    element_id: run.element_id,
                    has_mirror: run.has_mirror,
                    font_name: font_name.clone(),
                    font_size: size,
                    color: rgba,
                    bold,
                    italic,
                });
                utf16_pos += 2;
                prev_ch = Some('\n');
                idx += 2;
                continue;
            }

            let u16_len = ch.len_utf16();
            let word_start = match prev_ch {
                None => !ch.is_whitespace(),
                Some(p) => {
                    if ch.is_whitespace() {
                        false
                    } else if p.is_whitespace() || is_cjk(ch) || is_cjk(p) {
                        true
                    } else {
                        is_word_char(ch) != is_word_char(p)
                    }
                }
            };

            let (display_ch, small_cap) = match run.typographic_case.as_str() {
                "uppercase" | "caps" => (ch.to_ascii_uppercase(), false),
                "lowercase" => (ch.to_ascii_lowercase(), false),
                "smallCaps" => (ch.to_ascii_uppercase(), ch.is_lowercase()),
                "capsAndSmallCaps" => (ch.to_ascii_uppercase(), ch.is_lowercase()),
                "title" => {
                    if word_start {
                        (ch.to_ascii_uppercase(), false)
                    } else {
                        (ch, false)
                    }
                }
                _ => (ch, false),
            };

            let is_nl = matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}');
            let is_tab = ch == '\t';
            let is_sp = ch == ' ' || ch == '\u{00A0}' || ch == '\u{3000}';
            let mut adv = glyph_advance(display_ch, size, bold, tabular, small_cap, non_linear);
            if !is_nl && !is_tab {
                adv = (adv + run.tracking_left + run.tracking_right).max(0.0);
            }
            let bidi = if is_rtl_char(ch) {
                if block_bidi % 2 == 1 {
                    block_bidi
                } else {
                    block_bidi + 1
                }
            } else {
                block_bidi
            };

            atoms.push(RawAtom {
                ch,
                display_ch,
                small_cap,
                char_begin: utf16_pos,
                char_end: utf16_pos + u16_len,
                advance: adv,
                ascent,
                descent,
                line_gap,
                baseline_shift: run.baseline_shift,
                bidi_level: bidi,
                text_rotation: rot.clone(),
                break_opportunity: brk.clone(),
                word_boundary_left: word_start,
                is_graphic: false,
                is_tab,
                is_newline: is_nl,
                is_space: is_sp,
                element_id: run.element_id,
                has_mirror: run.has_mirror,
                font_name: font_name.clone(),
                font_size: if small_cap { size * 0.78 } else { size },
                color: rgba,
                bold,
                italic,
            });

            utf16_pos += u16_len;
            prev_ch = Some(ch);
            idx += 1;
        }
    }
    atoms
}

fn can_break_after(atoms: &[RawAtom], index: usize) -> bool {
    let cur = &atoms[index];
    if cur.is_newline {
        return true;
    }
    match cur.break_opportunity.as_str() {
        "none" => false,
        "any" | "all" => true,
        _ => {
            if cur.is_space || cur.is_tab || cur.ch == '-' || cur.ch == '\u{00AD}' || is_cjk(cur.ch)
            {
                return true;
            }
            if let Some(next) = atoms.get(index + 1)
                && (is_cjk(next.ch) || next.is_graphic)
                && next.break_opportunity != "none"
            {
                return true;
            }
            false
        }
    }
}

fn next_tab_advance(x: f64, font_size: f64, tab_stops: &[FteTabStopSpec]) -> f64 {
    let mut best: Option<f64> = None;
    for ts in tab_stops {
        if ts.position > x + 1e-6 {
            best = Some(match best {
                Some(cur) => cur.min(ts.position),
                None => ts.position,
            });
        }
    }
    if let Some(pos) = best {
        (pos - x).max(0.0)
    } else {
        let step = (font_size * 3.0).max(24.0);
        let next = ((x / step).floor() + 1.0) * step;
        (next - x).max(1.0)
    }
}

fn compute_baselines(baseline_zero: &str, ascent: f64, descent: f64) -> BTreeMap<String, f64> {
    let raw = [
        ("roman", 0.0),
        ("ascent", -ascent),
        ("descent", descent),
        ("ideographicTop", -ascent),
        ("ideographicCenter", (-ascent + descent) * 0.5),
        ("ideographicBottom", descent),
    ];
    let zero_offset = raw
        .iter()
        .find(|(k, _)| *k == baseline_zero)
        .map_or(0.0, |(_, v)| *v);
    let mut map = BTreeMap::new();
    for (k, v) in raw {
        map.insert(k.to_owned(), v - zero_offset);
    }
    map
}

pub fn layout_text_line(req: &FteLayoutRequest) -> Result<FteLayoutResponse, Fte3dError> {
    if !req.max_width.is_finite()
        || (!req.fit_something && req.max_width < 0.0)
        || req.max_width > FTE_MAX_LINE_WIDTH
    {
        return Err(Fte3dError::Argument(format!(
            "TextBlock.createTextLine width must be within 0..={FTE_MAX_LINE_WIDTH} (or fitSomething=true); got {}.",
            req.max_width
        )));
    }
    if !req.line_offset.is_finite() {
        return Err(Fte3dError::Argument(
            "TextBlock.createTextLine lineOffset must be finite.".into(),
        ));
    }
    if req.runs.is_empty() {
        return Ok(FteLayoutResponse {
            creation_result: "complete".into(),
            line: None,
        });
    }

    let atoms = flatten_paragraph(&req.runs, req.bidi_level, req.apply_non_linear_font_scaling);
    let total_utf16 = atoms.last().map_or(0, |a| a.char_end);

    // Empty paragraph (e.g. TextElement("")) creates one 0-atom line at start_char == 0.
    if atoms.is_empty() {
        if req.start_char > 0 {
            return Ok(FteLayoutResponse {
                creation_result: "complete".into(),
                line: None,
            });
        }
        let size = req
            .runs
            .first()
            .map_or(req.baseline_font_size, |r| r.font_size)
            .max(1.0);
        let ascent = size * 0.8;
        let descent = size * 0.2;
        let gap = size * 0.15;
        let bz = if req.baseline_zero.is_empty() {
            "roman"
        } else {
            &req.baseline_zero
        };
        let baselines = compute_baselines(bz, ascent, descent);
        return Ok(FteLayoutResponse {
            creation_result: "success".into(),
            line: Some(FteLineOut {
                text_block_begin_index: 0,
                raw_text_length: 0,
                specified_width: req.max_width,
                text_width: 0.0,
                text_height: ascent + descent,
                unjustified_text_width: 0.0,
                ascent,
                descent,
                total_ascent: ascent + gap * 0.5,
                total_descent: descent + gap * 0.5,
                total_height: ascent + descent + gap,
                has_graphic_element: false,
                has_tabs: false,
                atoms: Vec::new(),
                mirror_regions: Vec::new(),
                baselines,
                draw_runs: Vec::new(),
                dump: "<TextLine begin=\"0\" length=\"0\" width=\"0\"/>".into(),
            }),
        });
    }

    if req.start_char >= total_utf16 {
        return Ok(FteLayoutResponse {
            creation_result: "complete".into(),
            line: None,
        });
    }

    let Some(start_idx) = atoms.iter().position(|a| a.char_end > req.start_char) else {
        return Ok(FteLayoutResponse {
            creation_result: "complete".into(),
            line: None,
        });
    };

    let avail_width = (req.max_width - req.line_offset).max(0.0);
    let mut cur_x = 0.0;
    let mut last_fit_idx: Option<usize> = None;
    let mut last_legal_break_idx: Option<usize> = None;
    let mut mandatory_break_idx: Option<usize> = None;

    for (idx, atom) in atoms.iter().enumerate().skip(start_idx) {
        if atom.is_newline {
            mandatory_break_idx = Some(idx);
            break;
        }
        let adv = if atom.is_tab {
            next_tab_advance(cur_x, atom.font_size, &req.tab_stops)
        } else {
            atom.advance
        };
        let next_x = cur_x + adv;
        // Trailing spaces are allowed to overhang line width in FTE.
        let fits = next_x <= avail_width + 1e-6 || atom.is_space;
        if fits {
            cur_x = next_x;
            last_fit_idx = Some(idx);
            if can_break_after(&atoms, idx) {
                last_legal_break_idx = Some(idx);
            }
        } else {
            break;
        }
    }

    let (end_idx, mut creation_result) = if let Some(nl_idx) = mandatory_break_idx {
        (nl_idx, "success")
    } else if last_fit_idx == Some(atoms.len() - 1) {
        (atoms.len() - 1, "success")
    } else if let Some(brk_idx) = last_legal_break_idx {
        // Consume any trailing spaces immediately following the break opportunity.
        let mut e = brk_idx;
        while e + 1 < atoms.len() && atoms[e + 1].is_space && !atoms[e + 1].is_newline {
            e += 1;
        }
        if e + 1 < atoms.len() && atoms[e + 1].is_newline {
            e += 1;
        }
        (e, "success")
    } else if let Some(fit_idx) = last_fit_idx {
        (fit_idx, "emergency")
    } else if req.fit_something {
        (start_idx, "emergency")
    } else {
        return Ok(FteLayoutResponse {
            creation_result: "insufficientWidth".into(),
            line: None,
        });
    };

    let slice = &atoms[start_idx..=end_idx];
    let is_last_line = end_idx + 1 >= atoms.len();
    let ended_mandatory = slice.last().is_some_and(|a| a.is_newline);

    // Measure advances (resolving tabs) and trim trailing whitespace for visible width.
    let mut advances = Vec::with_capacity(slice.len());
    let mut x_cursor = 0.0;
    let mut has_tabs = false;
    let mut has_graphic = false;
    let mut max_ascent = 0.0_f64;
    let mut max_descent = 0.0_f64;
    let mut max_gap = 0.0_f64;

    for atom in slice {
        if atom.is_graphic {
            has_graphic = true;
        }
        let adv = if atom.is_tab {
            has_tabs = true;
            next_tab_advance(x_cursor, atom.font_size, &req.tab_stops)
        } else {
            atom.advance
        };
        advances.push(adv);
        x_cursor += adv;
        max_ascent = max_ascent.max(atom.ascent - atom.baseline_shift.min(0.0));
        max_descent = max_descent.max(atom.descent + atom.baseline_shift.max(0.0));
        max_gap = max_gap.max(atom.line_gap);
    }
    if max_ascent == 0.0 && max_descent == 0.0 {
        max_ascent = req.baseline_font_size.max(1.0) * 0.8;
        max_descent = req.baseline_font_size.max(1.0) * 0.2;
        max_gap = req.baseline_font_size.max(1.0) * 0.15;
    }

    // Visible end excludes trailing spaces and trailing newline.
    let mut visible_end = slice.len();
    while visible_end > 0 {
        let a = &slice[visible_end - 1];
        if a.is_newline
            || (a.is_space
                && !(req.justifier.kind == "east_asian"
                    && req.justifier.compose_trailing_ideographic_spaces
                    && a.ch == '\u{3000}'))
        {
            visible_end -= 1;
        } else {
            break;
        }
    }
    let unjustified_width: f64 = advances[..visible_end].iter().sum();

    // Justification pass
    let should_justify = match req.justifier.line_justification.as_str() {
        "allIncludingLast" => true,
        "allButLast" => !is_last_line,
        "allButMandatoryBreak" => !is_last_line && !ended_mandatory,
        _ => false,
    };

    if should_justify && avail_width > unjustified_width && visible_end > 1 {
        let slack = avail_width - unjustified_width;
        if req.justifier.kind == "east_asian" || req.justifier.letter_spacing {
            let gaps = (visible_end - 1) as f64;
            let add_per_gap = slack / gaps;
            for adv in advances.iter_mut().take(visible_end - 1) {
                *adv += add_per_gap;
            }
        } else {
            let space_indices: Vec<usize> =
                (0..visible_end).filter(|&i| slice[i].is_space).collect();
            if !space_indices.is_empty() {
                let add_per_space = slack / (space_indices.len() as f64);
                for i in space_indices {
                    advances[i] += add_per_space;
                }
            }
        }
    }

    let justified_width: f64 = advances[..visible_end].iter().sum();

    // Build atoms, mirror regions, and renderable draw runs.
    let bz = if req.baseline_zero.is_empty() {
        "roman"
    } else {
        &req.baseline_zero
    };
    let baselines = compute_baselines(bz, max_ascent, max_descent);
    let roman_y = *baselines.get("roman").unwrap_or(&0.0);

    let mut atom_outs = Vec::with_capacity(slice.len());
    let mut mirror_regions: Vec<FteMirrorRegionOut> = Vec::new();
    let mut draw_runs: Vec<FteDrawRunOut> = Vec::new();

    let mut pos_x = req.line_offset;
    for (i, atom) in slice.iter().enumerate() {
        let w = advances[i];
        let top = roman_y - atom.ascent + atom.baseline_shift;
        let h = atom.ascent + atom.descent;
        let bounds = [pos_x, top, w, h];
        let center = pos_x + w * 0.5;

        atom_outs.push(FteAtomOut {
            bounds,
            center,
            bidi_level: atom.bidi_level,
            char_begin: atom.char_begin,
            char_end: atom.char_end,
            text_rotation: atom.text_rotation.clone(),
            word_boundary_left: atom.word_boundary_left,
            is_graphic: atom.is_graphic,
            element_id: atom.element_id,
        });

        if atom.has_mirror && w > 0.0 {
            if let Some(last) = mirror_regions.last_mut()
                && last.element_id == atom.element_id
            {
                let left = last.bounds[0].min(pos_x);
                let top_u = last.bounds[1].min(top);
                let right = (last.bounds[0] + last.bounds[2]).max(pos_x + w);
                let bottom = (last.bounds[1] + last.bounds[3]).max(top + h);
                last.bounds = [left, top_u, right - left, bottom - top_u];
            } else {
                mirror_regions.push(FteMirrorRegionOut {
                    element_id: atom.element_id,
                    bounds,
                });
            }
        }

        if !atom.is_graphic && !atom.is_newline && !atom.is_tab && atom.font_size > 0.0 {
            let ch_str = atom.display_ch.to_string();
            let y_top = roman_y - atom.ascent + atom.baseline_shift;
            let eff_rot = if !req.line_rotation.is_empty() && req.line_rotation != "rotate0" {
                req.line_rotation.clone()
            } else if atom.text_rotation.is_empty() {
                "rotate0".to_owned()
            } else {
                atom.text_rotation.clone()
            };
            if !should_justify
                && !atom.small_cap
                && eff_rot == "rotate0"
                && let Some(prev_draw) = draw_runs.last_mut()
                && (prev_draw.y - y_top).abs() < 1e-6
                && prev_draw.font_family == atom.font_name
                && (prev_draw.font_size - atom.font_size).abs() < 1e-6
                && prev_draw.color == atom.color
                && prev_draw.bold == atom.bold
                && prev_draw.italic == atom.italic
                && prev_draw.rotation == eff_rot
            {
                prev_draw.text.push(atom.display_ch);
            } else {
                draw_runs.push(FteDrawRunOut {
                    x: pos_x,
                    y: y_top,
                    text: ch_str,
                    font_family: atom.font_name.clone(),
                    font_size: atom.font_size,
                    color: atom.color,
                    bold: atom.bold,
                    italic: atom.italic,
                    rotation: eff_rot,
                });
            }
        }

        pos_x += w;
    }

    let begin_char = slice.first().map_or(req.start_char, |a| a.char_begin);
    let end_char = slice.last().map_or(begin_char, |a| a.char_end);
    let raw_len = end_char.saturating_sub(begin_char);
    if raw_len == 0 && creation_result == "emergency" {
        creation_result = "insufficientWidth";
    }
    let text_height = max_ascent + max_descent;
    let total_ascent = max_ascent + max_gap * 0.5;
    let total_descent = max_descent + max_gap * 0.5;
    let total_height = total_ascent + total_descent;
    let dump = format!(
        "<TextLine begin=\"{begin_char}\" length=\"{raw_len}\" atoms=\"{}\" width=\"{justified_width:.2}\" ascent=\"{max_ascent:.2}\" descent=\"{max_descent:.2}\"/>",
        atom_outs.len()
    );

    Ok(FteLayoutResponse {
        creation_result: creation_result.into(),
        line: Some(FteLineOut {
            text_block_begin_index: begin_char,
            raw_text_length: raw_len,
            specified_width: req.max_width,
            text_width: justified_width,
            text_height,
            unjustified_text_width: unjustified_width,
            ascent: max_ascent,
            descent: max_descent,
            total_ascent,
            total_descent,
            total_height,
            has_graphic_element: has_graphic,
            has_tabs,
            atoms: atom_outs,
            mirror_regions,
            baselines,
            draw_runs,
            dump,
        }),
    })
}

pub fn find_next_atom_boundary(raw_text: &str, after_char_index: i32) -> Result<i32, Fte3dError> {
    let u16_total: i32 = raw_text.chars().map(|c| c.len_utf16() as i32).sum();
    if after_char_index < 0 || after_char_index >= u16_total {
        return Err(Fte3dError::Range(format!(
            "Char index {after_char_index} is out of bounds for TextBlock length {u16_total}."
        )));
    }
    let mut pos = 0i32;
    let chars: Vec<char> = raw_text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let step = if chars[i] == '\r' && i + 1 < chars.len() && chars[i + 1] == '\n' {
            i += 2;
            2
        } else {
            let s = chars[i].len_utf16() as i32;
            i += 1;
            s
        };
        let next = pos + step;
        if next > after_char_index {
            return Ok(next);
        }
        pos = next;
    }
    Ok(u16_total)
}

pub fn find_previous_atom_boundary(
    raw_text: &str,
    before_char_index: i32,
) -> Result<i32, Fte3dError> {
    let u16_total: i32 = raw_text.chars().map(|c| c.len_utf16() as i32).sum();
    if before_char_index <= 0 || before_char_index > u16_total {
        return Err(Fte3dError::Range(format!(
            "Char index {before_char_index} is out of bounds for TextBlock length {u16_total}."
        )));
    }
    let mut boundaries = vec![0i32];
    let chars: Vec<char> = raw_text.chars().collect();
    let mut pos = 0i32;
    let mut i = 0usize;
    while i < chars.len() {
        let step = if chars[i] == '\r' && i + 1 < chars.len() && chars[i + 1] == '\n' {
            i += 2;
            2
        } else {
            let s = chars[i].len_utf16() as i32;
            i += 1;
            s
        };
        pos += step;
        boundaries.push(pos);
    }
    for &b in boundaries.iter().rev() {
        if b < before_char_index {
            return Ok(b);
        }
    }
    Ok(0)
}

fn word_boundaries_utf16(raw_text: &str) -> Vec<i32> {
    let chars: Vec<char> = raw_text.chars().collect();
    if chars.is_empty() {
        return vec![0];
    }
    let mut bounds = vec![0i32];
    let mut pos = 0i32;
    for i in 0..chars.len() {
        pos += chars[i].len_utf16() as i32;
        if i + 1 < chars.len() {
            let a = chars[i];
            let b = chars[i + 1];
            let boundary = if a == '\r' && b == '\n' {
                false
            } else if a.is_whitespace() != b.is_whitespace() || is_cjk(a) || is_cjk(b) {
                true
            } else if !a.is_whitespace() && !b.is_whitespace() {
                is_word_char(a) != is_word_char(b)
            } else {
                false
            };
            if boundary {
                bounds.push(pos);
            }
        }
    }
    if bounds.last().copied() != Some(pos) {
        bounds.push(pos);
    }
    bounds
}

pub fn find_next_word_boundary(raw_text: &str, after_char_index: i32) -> Result<i32, Fte3dError> {
    let u16_total: i32 = raw_text.chars().map(|c| c.len_utf16() as i32).sum();
    if after_char_index < 0 || after_char_index >= u16_total {
        return Err(Fte3dError::Range(format!(
            "Char index {after_char_index} is out of bounds for TextBlock length {u16_total}."
        )));
    }
    for b in word_boundaries_utf16(raw_text) {
        if b > after_char_index {
            return Ok(b);
        }
    }
    Ok(u16_total)
}

pub fn find_previous_word_boundary(
    raw_text: &str,
    before_char_index: i32,
) -> Result<i32, Fte3dError> {
    let u16_total: i32 = raw_text.chars().map(|c| c.len_utf16() as i32).sum();
    if before_char_index <= 0 || before_char_index > u16_total {
        return Err(Fte3dError::Range(format!(
            "Char index {before_char_index} is out of bounds for TextBlock length {u16_total}."
        )));
    }
    for &b in word_boundaries_utf16(raw_text).iter().rev() {
        if b < before_char_index {
            return Ok(b);
        }
    }
    Ok(0)
}

// ============================================================================
// 2. Stage3D Engine (Context3D, AGAL v1/v2 VM & Software Rasterizer)
// ============================================================================

#[derive(Debug, Clone, Copy)]
struct AgalInst {
    opcode: u8,
    dst_reg: u16,
    dst_mask: u8,
    dst_type: u8,
    src1: u64,
    src2: u64,
}

#[derive(Debug, Clone, Default)]
enum CompiledShader {
    #[default]
    Fallback,
    Agal(Vec<AgalInst>),
}

fn parse_agal(bytes: &[u8], expected_type: u8) -> Result<CompiledShader, Fte3dError> {
    if bytes.is_empty() {
        return Err(Fte3dError::Argument(
            "Program3D.upload requires non-empty vertex and fragment ByteArrays.".into(),
        ));
    }
    if bytes.len() >= 7 && bytes[0] == 0xa0 && bytes[5] == 0xa1 {
        let ver = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        if ver == 0 || ver > 4 {
            return Err(Fte3dError::Argument(format!(
                "Unsupported AGAL version {ver}."
            )));
        }
        let prog_type = bytes[6];
        if prog_type != expected_type {
            return Err(Fte3dError::Argument(format!(
                "AGAL program type mismatch: expected {expected_type}, got {prog_type}."
            )));
        }
        let body = &bytes[7..];
        if !body.len().is_multiple_of(24) {
            return Err(Fte3dError::Argument(
                "AGAL instruction stream must be a multiple of 24 bytes.".into(),
            ));
        }
        let mut insts = Vec::with_capacity(body.len() / 24);
        for chunk in body.chunks_exact(24) {
            let opcode = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as u8;
            let dst = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let src1 = u64::from_le_bytes([
                chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14],
                chunk[15],
            ]);
            let src2 = u64::from_le_bytes([
                chunk[16], chunk[17], chunk[18], chunk[19], chunk[20], chunk[21], chunk[22],
                chunk[23],
            ]);
            insts.push(AgalInst {
                opcode,
                dst_reg: (dst & 0xffff) as u16,
                dst_mask: ((dst >> 16) & 0x0f) as u8,
                dst_type: ((dst >> 24) & 0x0f) as u8,
                src1,
                src2,
            });
        }
        Ok(CompiledShader::Agal(insts))
    } else {
        Ok(CompiledShader::Fallback)
    }
}

#[derive(Debug, Clone)]
pub struct VertexBufferRecord {
    pub num_vertices: usize,
    pub data32_per_vertex: usize,
    pub usage: String,
    pub data: Vec<f32>,
    pub disposed: bool,
}

#[derive(Debug, Clone)]
pub struct IndexBufferRecord {
    pub num_indices: usize,
    pub usage: String,
    pub data: Vec<u16>,
    pub disposed: bool,
}

#[derive(Debug, Clone)]
pub struct ProgramRecord {
    vertex: CompiledShader,
    fragment: CompiledShader,
    uploaded: bool,
    pub disposed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureKind {
    TwoD,
    Cube,
    Rectangle,
    Video,
}

pub type MipLevelMap = BTreeMap<u32, (u32, u32, Vec<u8>)>;

#[derive(Debug, Clone)]
pub struct TextureRecord {
    pub kind: TextureKind,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub optimize_for_rtt: bool,
    pub faces: Vec<Vec<u8>>, // 1 face for 2D/Rect/Video, 6 faces for Cube; straight RGBA8
    pub mips: Vec<MipLevelMap>,
    pub disposed: bool,
}

impl TextureRecord {
    fn byte_size(&self) -> usize {
        if self.disposed {
            0
        } else {
            let base: usize = self.faces.iter().map(Vec::len).sum();
            let mip_bytes: usize = self
                .mips
                .iter()
                .flat_map(|m| m.values())
                .map(|(_, _, buf)| buf.len())
                .sum();
            base + mip_bytes
        }
    }

    fn sample_buffer(
        pixels: &[u8],
        width: u32,
        height: u32,
        u: f32,
        v: f32,
        wrap: &str,
        filter: &str,
    ) -> [f32; 4] {
        if width == 0 || height == 0 || pixels.len() < (width * height * 4) as usize {
            return [1.0, 1.0, 1.0, 1.0];
        }
        let wrap_coord = |c: f32, repeat: bool| -> f32 {
            if !c.is_finite() {
                0.0
            } else if repeat {
                c.rem_euclid(1.0)
            } else {
                c.clamp(0.0, 1.0)
            }
        };
        let (rep_u, rep_v) = match wrap {
            "repeat" => (true, true),
            "clamp_u_repeat_v" => (false, true),
            "repeat_u_clamp_v" => (true, false),
            _ => (false, false),
        };
        let wu = wrap_coord(u, rep_u);
        let wv = wrap_coord(v, rep_v);
        let w = width as usize;
        let h = height as usize;

        let fetch = |ix: usize, iy: usize| -> [f32; 4] {
            let off = (iy.min(h - 1) * w + ix.min(w - 1)) * 4;
            [
                pixels[off] as f32 / 255.0,
                pixels[off + 1] as f32 / 255.0,
                pixels[off + 2] as f32 / 255.0,
                pixels[off + 3] as f32 / 255.0,
            ]
        };

        if filter == "nearest" || (w == 1 && h == 1) {
            let ix = ((wu * w as f32).floor() as usize).min(w - 1);
            let iy = ((wv * h as f32).floor() as usize).min(h - 1);
            fetch(ix, iy)
        } else {
            let fx = (wu * (w as f32) - 0.5).max(0.0);
            let fy = (wv * (h as f32) - 0.5).max(0.0);
            let x0 = (fx.floor() as usize).min(w - 1);
            let y0 = (fy.floor() as usize).min(h - 1);
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let tx = fx - fx.floor();
            let ty = fy - fy.floor();
            let c00 = fetch(x0, y0);
            let c10 = fetch(x1, y0);
            let c01 = fetch(x0, y1);
            let c11 = fetch(x1, y1);
            let mut out = [0.0f32; 4];
            for ch in 0..4 {
                let top = c00[ch] * (1.0 - tx) + c10[ch] * tx;
                let bot = c01[ch] * (1.0 - tx) + c11[ch] * tx;
                out[ch] = top * (1.0 - ty) + bot * ty;
            }
            out
        }
    }

    fn sample(&self, u: f32, v: f32, wrap: &str, filter: &str, face: usize) -> [f32; 4] {
        let Some(pixels) = self.faces.get(face).or_else(|| self.faces.first()) else {
            return [1.0, 1.0, 1.0, 1.0];
        };
        Self::sample_buffer(pixels, self.width, self.height, u, v, wrap, filter)
    }

    fn sample_coord(&self, coord: [f32; 4], wrap: &str, filter: &str, mipfilter: &str) -> [f32; 4] {
        let (face, u, v) = if self.kind == TextureKind::Cube
            && (coord[0].abs() > 1e-6 || coord[1].abs() > 1e-6 || coord[2].abs() > 1e-6)
            && coord[2].abs() > 1e-6
        {
            let x = coord[0];
            let y = coord[1];
            let z = coord[2];
            let ax = x.abs();
            let ay = y.abs();
            let az = z.abs();
            let (face_idx, sc, tc, ma) = if ax >= ay && ax >= az {
                if x >= 0.0 {
                    (0usize, -z, -y, ax)
                } else {
                    (1usize, z, -y, ax)
                }
            } else if ay >= ax && ay >= az {
                if y >= 0.0 {
                    (2usize, x, z, ay)
                } else {
                    (3usize, x, -z, ay)
                }
            } else if z >= 0.0 {
                (4usize, x, -y, az)
            } else {
                (5usize, -x, -y, az)
            };
            let inv_ma = if ma > 1e-6 { 1.0 / ma } else { 0.0 };
            (
                face_idx,
                0.5 * (sc * inv_ma + 1.0),
                0.5 * (tc * inv_ma + 1.0),
            )
        } else {
            (0usize, coord[0], coord[1])
        };

        let filt = if mipfilter == "miplinear" && filter == "nearest" {
            "linear"
        } else {
            filter
        };

        if mipfilter != "mipnone"
            && let Some(face_mips) = self.mips.get(face)
            && !face_mips.is_empty()
        {
            let req_level = if self.kind != TextureKind::Cube && coord[2] > 0.0 {
                coord[2].round() as u32
            } else {
                *face_mips.keys().next().unwrap_or(&1)
            };
            if let Some((mw, mh, mpixels)) = face_mips
                .get(&req_level)
                .or_else(|| face_mips.values().next())
            {
                let mip_col = Self::sample_buffer(mpixels, *mw, *mh, u, v, wrap, filt);
                let base_has_data = self
                    .faces
                    .get(face)
                    .is_some_and(|b| b.iter().any(|&px| px != 0));
                if mipfilter == "miplinear"
                    && base_has_data
                    && (self.kind == TextureKind::Cube || coord[2] < 1.0)
                {
                    let base_col = self.sample(u, v, wrap, filt, face);
                    let t = if self.kind != TextureKind::Cube && coord[2] > 0.0 {
                        coord[2].clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    return [
                        base_col[0] * (1.0 - t) + mip_col[0] * t,
                        base_col[1] * (1.0 - t) + mip_col[1] * t,
                        base_col[2] * (1.0 - t) + mip_col[2] * t,
                        base_col[3] * (1.0 - t) + mip_col[3] * t,
                    ];
                }
                return mip_col;
            }
        }

        self.sample(u, v, wrap, filt, face)
    }
}

#[derive(Debug, Clone)]
struct BoundAttr {
    buffer_id: u32,
    offset: usize,
    format: String,
}

#[derive(Debug, Clone)]
struct SamplerState {
    wrap: String,
    filter: String,
    mipfilter: String,
}

impl Default for SamplerState {
    fn default() -> Self {
        Self {
            wrap: "clamp".into(),
            filter: "nearest".into(),
            mipfilter: "mipnone".into(),
        }
    }
}

#[derive(Debug, Clone)]
struct RenderTarget {
    texture_id: u32,
    surface_selector: usize,
    enable_depth_stencil: bool,
    depth: Vec<f32>,
    stencil: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Context3dState {
    pub profile: String,
    pub render_mode: String,
    pub enable_error_checking: bool,
    pub back_buffer_width: u32,
    pub back_buffer_height: u32,
    pub max_back_buffer_width: u32,
    pub max_back_buffer_height: u32,
    pub anti_alias: i32,
    pub enable_depth_and_stencil: bool,
    pub color_buffer: Vec<u8>,
    pub presented_buffer: Vec<u8>,
    pub presented_width: u32,
    pub presented_height: u32,
    pub depth_buffer: Vec<f32>,
    pub stencil_buffer: Vec<u8>,
    pub disposed: bool,
    program_id: Option<u32>,
    vc: [[f32; 4]; 128],
    vc_touched: bool,
    fc: [[f32; 4]; 64],
    fc_touched: bool,
    attrs: [Option<BoundAttr>; 8],
    textures: [Option<u32>; 8],
    samplers: [SamplerState; 8],
    blend_src: String,
    blend_dst: String,
    color_mask: [bool; 4],
    culling: String,
    depth_mask: bool,
    depth_compare: String,
    fill_mode: String,
    scissor: Option<[i32; 4]>,
    stencil_face: String,
    stencil_compare: String,
    stencil_both_pass: String,
    stencil_depth_fail: String,
    stencil_pass_fail: String,
    stencil_ref: u8,
    stencil_read_mask: u8,
    stencil_write_mask: u8,
    rtt: Option<RenderTarget>,
}

pub fn validate_stage3d_enum<'a>(kind: &str, value: &'a str) -> Result<&'a str, Fte3dError> {
    let ok = match kind {
        "profile" => matches!(
            value,
            "baseline"
                | "baselineConstrained"
                | "baselineExtended"
                | "standard"
                | "standardConstrained"
                | "standardExtended"
                | "enhanced"
        ),
        "renderMode" => matches!(value, "auto" | "software"),
        "bufferUsage" => matches!(value, "staticDraw" | "dynamicDraw"),
        "programType" => matches!(value, "vertex" | "fragment"),
        "vertexFormat" => matches!(value, "float1" | "float2" | "float3" | "float4" | "bytes4"),
        "textureFormat" => matches!(
            value,
            "bgra"
                | "bgraPacked4444"
                | "bgrPacked565"
                | "compressed"
                | "compressedAlpha"
                | "rgbaHalfFloat"
        ),
        "blendFactor" => matches!(
            value,
            "one"
                | "zero"
                | "sourceAlpha"
                | "sourceColor"
                | "destinationAlpha"
                | "destinationColor"
                | "oneMinusSourceAlpha"
                | "oneMinusSourceColor"
                | "oneMinusDestinationAlpha"
                | "oneMinusDestinationColor"
        ),
        "compareMode" => matches!(
            value,
            "always"
                | "never"
                | "less"
                | "lessEqual"
                | "equal"
                | "greaterEqual"
                | "greater"
                | "notEqual"
        ),
        "triangleFace" => matches!(value, "none" | "back" | "front" | "frontAndBack"),
        "stencilAction" => matches!(
            value,
            "keep"
                | "zero"
                | "incrementSaturate"
                | "decrementSaturate"
                | "invert"
                | "incrementWrap"
                | "decrementWrap"
                | "set"
        ),
        "wrapMode" => matches!(
            value,
            "clamp" | "repeat" | "clamp_u_repeat_v" | "repeat_u_clamp_v"
        ),
        "textureFilter" => matches!(
            value,
            "nearest"
                | "linear"
                | "anisotropic2x"
                | "anisotropic4x"
                | "anisotropic8x"
                | "anisotropic16x"
        ),
        "mipFilter" => matches!(value, "mipnone" | "mipnearest" | "miplinear"),
        "fillMode" => matches!(value, "solid" | "wireframe"),
        _ => false,
    };
    if ok {
        Ok(value)
    } else {
        Err(Fte3dError::Argument(format!(
            "Invalid Stage3D {kind}: '{value}'."
        )))
    }
}

fn compare_pass(mode: &str, src: f32, dst: f32) -> bool {
    match mode {
        "always" => true,
        "never" => false,
        "less" => src < dst,
        "lessEqual" => src <= dst,
        "equal" => (src - dst).abs() <= 1e-6,
        "greaterEqual" => src >= dst,
        "greater" => src > dst,
        "notEqual" => (src - dst).abs() > 1e-6,
        _ => true,
    }
}

fn compare_stencil(mode: &str, reference: u8, current: u8, mask: u8) -> bool {
    let r = reference & mask;
    let c = current & mask;
    match mode {
        "always" => true,
        "never" => false,
        "less" => r < c,
        "lessEqual" => r <= c,
        "equal" => r == c,
        "greaterEqual" => r >= c,
        "greater" => r > c,
        "notEqual" => r != c,
        _ => true,
    }
}

fn apply_stencil_op(action: &str, current: u8, reference: u8, write_mask: u8) -> u8 {
    let next = match action {
        "keep" => current,
        "zero" => 0,
        "set" => reference,
        "invert" => !current,
        "incrementSaturate" => current.saturating_add(1),
        "decrementSaturate" => current.saturating_sub(1),
        "incrementWrap" => current.wrapping_add(1),
        "decrementWrap" => current.wrapping_sub(1),
        _ => current,
    };
    (current & !write_mask) | (next & write_mask)
}

fn blend_factor_vec(factor: &str, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    match factor {
        "one" => [1.0, 1.0, 1.0, 1.0],
        "zero" => [0.0, 0.0, 0.0, 0.0],
        "sourceAlpha" => [src[3]; 4],
        "oneMinusSourceAlpha" => [1.0 - src[3]; 4],
        "destinationAlpha" => [dst[3]; 4],
        "oneMinusDestinationAlpha" => [1.0 - dst[3]; 4],
        "sourceColor" => src,
        "oneMinusSourceColor" => [1.0 - src[0], 1.0 - src[1], 1.0 - src[2], 1.0 - src[3]],
        "destinationColor" => dst,
        "oneMinusDestinationColor" => [1.0 - dst[0], 1.0 - dst[1], 1.0 - dst[2], 1.0 - dst[3]],
        _ => [1.0, 1.0, 1.0, 1.0],
    }
}

fn read_agal_src(
    token: u64,
    is_vertex: bool,
    attrs: &[[f32; 4]; 8],
    consts: &[[f32; 4]],
    temps: &[[f32; 4]; 32],
    out_reg: &[f32; 4],
    varyings: &[[f32; 4]; 8],
) -> [f32; 4] {
    let base_num = (token & 0xffff) as i32;
    let indirect_offset = ((token >> 16) & 0xff) as i8 as i32;
    let swizzle = ((token >> 24) & 0xff) as u8;
    let reg_type = ((token >> 32) & 0x0f) as u8;
    let is_indirect = ((token >> 63) & 1) == 1;

    let reg_idx = if is_indirect {
        let idx_type = ((token >> 40) & 0x0f) as u8;
        let idx_comp = ((token >> 48) & 0x03) as usize;
        let idx_vec = match idx_type {
            0 if is_vertex => attrs.get(base_num as usize).copied().unwrap_or_default(),
            1 => consts.get(base_num as usize).copied().unwrap_or_default(),
            2 => temps.get(base_num as usize).copied().unwrap_or_default(),
            _ => [0.0; 4],
        };
        (idx_vec[idx_comp].floor() as i32 + indirect_offset).max(0) as usize
    } else {
        base_num.max(0) as usize
    };

    let raw = match reg_type {
        0 if is_vertex => attrs.get(reg_idx).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]),
        1 => consts.get(reg_idx).copied().unwrap_or([0.0; 4]),
        2 => temps.get(reg_idx).copied().unwrap_or([0.0; 4]),
        3 => *out_reg,
        4 => varyings.get(reg_idx).copied().unwrap_or([0.0; 4]),
        _ => [0.0; 4],
    };

    [
        raw[(swizzle & 0x03) as usize],
        raw[((swizzle >> 2) & 0x03) as usize],
        raw[((swizzle >> 4) & 0x03) as usize],
        raw[((swizzle >> 6) & 0x03) as usize],
    ]
}

fn write_agal_dst(
    dst_type: u8,
    dst_reg: u16,
    dst_mask: u8,
    val: [f32; 4],
    temps: &mut [[f32; 4]; 32],
    out_reg: &mut [f32; 4],
    varyings: &mut [[f32; 4]; 8],
) {
    let target = match dst_type {
        2 => temps.get_mut(dst_reg as usize),
        3 => Some(out_reg),
        4 => varyings.get_mut(dst_reg as usize),
        _ => None,
    };
    if let Some(reg) = target {
        let mask = if dst_mask == 0 { 0x0f } else { dst_mask };
        for lane in 0..4 {
            if (mask & (1 << lane)) != 0 {
                reg[lane] = val[lane];
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_agal(
    insts: &[AgalInst],
    is_vertex: bool,
    attrs: &[[f32; 4]; 8],
    consts: &[[f32; 4]],
    varyings: &mut [[f32; 4]; 8],
    tex_slots: &[Option<u32>; 8],
    samplers: &[SamplerState; 8],
    tex_arena: &BTreeMap<u32, TextureRecord>,
) -> Option<[f32; 4]> {
    let mut temps = [[0.0f32; 4]; 32];
    let mut out = [0.0f32, 0.0, 0.0, 1.0];

    for inst in insts {
        if inst.opcode == 0x27 {
            // kil
            let s1 = read_agal_src(inst.src1, is_vertex, attrs, consts, &temps, &out, varyings);
            if s1.iter().any(|&c| c < 0.0) {
                return None;
            }
            continue;
        }
        if inst.opcode == 0x28 {
            // tex dst, coord, fsN
            let coord = read_agal_src(inst.src1, is_vertex, attrs, consts, &temps, &out, varyings);
            let sampler_idx = (inst.src2 & 0xffff) as usize;
            let sampled = if let Some(Some(tid)) = tex_slots.get(sampler_idx)
                && let Some(tex) = tex_arena.get(tid)
            {
                let st = &samplers[sampler_idx];
                tex.sample_coord(coord, &st.wrap, &st.filter, &st.mipfilter)
            } else {
                [1.0, 1.0, 1.0, 1.0]
            };
            write_agal_dst(
                inst.dst_type,
                inst.dst_reg,
                inst.dst_mask,
                sampled,
                &mut temps,
                &mut out,
                varyings,
            );
            continue;
        }

        let a = read_agal_src(inst.src1, is_vertex, attrs, consts, &temps, &out, varyings);
        let b = read_agal_src(inst.src2, is_vertex, attrs, consts, &temps, &out, varyings);

        let mat_row = |offset: usize| -> [f32; 4] {
            let base = (inst.src2 & 0xffff) as usize + offset;
            let swz = ((inst.src2 >> 24) & 0xff) as u8;
            let raw = consts.get(base).copied().unwrap_or([0.0; 4]);
            if swz == 0 || swz == 0xe4 {
                raw
            } else {
                [
                    raw[(swz & 3) as usize],
                    raw[((swz >> 2) & 3) as usize],
                    raw[((swz >> 4) & 3) as usize],
                    raw[((swz >> 6) & 3) as usize],
                ]
            }
        };

        let dp3 = |u: [f32; 4], v: [f32; 4]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        let dp4 = |u: [f32; 4], v: [f32; 4]| dp3(u, v) + u[3] * v[3];

        let res = match inst.opcode {
            0x00 => a, // mov
            0x01 => [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]],
            0x02 => [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]],
            0x03 => [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]],
            0x04 => [
                if b[0] != 0.0 { a[0] / b[0] } else { 0.0 },
                if b[1] != 0.0 { a[1] / b[1] } else { 0.0 },
                if b[2] != 0.0 { a[2] / b[2] } else { 0.0 },
                if b[3] != 0.0 { a[3] / b[3] } else { 0.0 },
            ],
            0x05 => [
                if a[0] != 0.0 { 1.0 / a[0] } else { 0.0 },
                if a[1] != 0.0 { 1.0 / a[1] } else { 0.0 },
                if a[2] != 0.0 { 1.0 / a[2] } else { 0.0 },
                if a[3] != 0.0 { 1.0 / a[3] } else { 0.0 },
            ],
            0x06 => [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].min(b[2]),
                a[3].min(b[3]),
            ],
            0x07 => [
                a[0].max(b[0]),
                a[1].max(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ],
            0x08 => [
                a[0] - a[0].floor(),
                a[1] - a[1].floor(),
                a[2] - a[2].floor(),
                a[3] - a[3].floor(),
            ],
            0x09 => [
                a[0].max(0.0).sqrt(),
                a[1].max(0.0).sqrt(),
                a[2].max(0.0).sqrt(),
                a[3].max(0.0).sqrt(),
            ],
            0x0a => [
                1.0 / a[0].max(1e-12).sqrt(),
                1.0 / a[1].max(1e-12).sqrt(),
                1.0 / a[2].max(1e-12).sqrt(),
                1.0 / a[3].max(1e-12).sqrt(),
            ],
            0x0b => [
                a[0].powf(b[0]),
                a[1].powf(b[1]),
                a[2].powf(b[2]),
                a[3].powf(b[3]),
            ],
            0x0c => [
                a[0].max(1e-12).log2(),
                a[1].max(1e-12).log2(),
                a[2].max(1e-12).log2(),
                a[3].max(1e-12).log2(),
            ],
            0x0d => [a[0].exp2(), a[1].exp2(), a[2].exp2(), a[3].exp2()],
            0x0e => {
                let len = dp3(a, a).sqrt().max(1e-12);
                [a[0] / len, a[1] / len, a[2] / len, a[3]]
            }
            0x0f => [a[0].sin(), a[1].sin(), a[2].sin(), a[3].sin()],
            0x10 => [a[0].cos(), a[1].cos(), a[2].cos(), a[3].cos()],
            0x11 => [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
                0.0,
            ],
            0x12 => [dp3(a, b); 4],
            0x13 => [dp4(a, b); 4],
            0x14 => [a[0].abs(), a[1].abs(), a[2].abs(), a[3].abs()],
            0x15 => [-a[0], -a[1], -a[2], -a[3]],
            0x16 => [
                a[0].clamp(0.0, 1.0),
                a[1].clamp(0.0, 1.0),
                a[2].clamp(0.0, 1.0),
                a[3].clamp(0.0, 1.0),
            ],
            0x17 => {
                let r0 = mat_row(0);
                let r1 = mat_row(1);
                let r2 = mat_row(2);
                [dp3(a, r0), dp3(a, r1), dp3(a, r2), a[3]]
            }
            0x18 => {
                let r0 = mat_row(0);
                let r1 = mat_row(1);
                let r2 = mat_row(2);
                let r3 = mat_row(3);
                [dp4(a, r0), dp4(a, r1), dp4(a, r2), dp4(a, r3)]
            }
            0x19 => {
                let r0 = mat_row(0);
                let r1 = mat_row(1);
                let r2 = mat_row(2);
                [dp4(a, r0), dp4(a, r1), dp4(a, r2), a[3]]
            }
            0x29 => [
                if a[0] >= b[0] { 1.0 } else { 0.0 },
                if a[1] >= b[1] { 1.0 } else { 0.0 },
                if a[2] >= b[2] { 1.0 } else { 0.0 },
                if a[3] >= b[3] { 1.0 } else { 0.0 },
            ],
            0x2a => [
                if a[0] < b[0] { 1.0 } else { 0.0 },
                if a[1] < b[1] { 1.0 } else { 0.0 },
                if a[2] < b[2] { 1.0 } else { 0.0 },
                if a[3] < b[3] { 1.0 } else { 0.0 },
            ],
            0x2c => [
                if (a[0] - b[0]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
                if (a[1] - b[1]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
                if (a[2] - b[2]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
                if (a[3] - b[3]).abs() <= 1e-6 {
                    1.0
                } else {
                    0.0
                },
            ],
            0x2d => [
                if (a[0] - b[0]).abs() > 1e-6 { 1.0 } else { 0.0 },
                if (a[1] - b[1]).abs() > 1e-6 { 1.0 } else { 0.0 },
                if (a[2] - b[2]).abs() > 1e-6 { 1.0 } else { 0.0 },
                if (a[3] - b[3]).abs() > 1e-6 { 1.0 } else { 0.0 },
            ],
            _ => a,
        };

        write_agal_dst(
            inst.dst_type,
            inst.dst_reg,
            inst.dst_mask,
            res,
            &mut temps,
            &mut out,
            varyings,
        );
    }
    Some(out)
}

#[derive(Debug, Default)]
pub struct Stage3dArena {
    next_id: u32,
    pub contexts: BTreeMap<u32, Context3dState>,
    pub vertex_buffers: BTreeMap<u32, (u32, VertexBufferRecord)>,
    pub index_buffers: BTreeMap<u32, (u32, IndexBufferRecord)>,
    pub programs: BTreeMap<u32, (u32, ProgramRecord)>,
    pub textures: BTreeMap<u32, (u32, TextureRecord)>,
}

impl Stage3dArena {
    fn alloc_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    pub fn create_context(&mut self, render_mode: &str, profile: &str) -> Result<u32, Fte3dError> {
        validate_stage3d_enum("renderMode", render_mode)?;
        validate_stage3d_enum("profile", profile)?;
        let id = self.alloc_id();
        let max_dim = if profile.ends_with("Extended") || profile.starts_with("standard") {
            4096
        } else {
            2048
        };
        let mut vc = [[0.0f32; 4]; 128];
        vc[0] = [1.0, 0.0, 0.0, 0.0];
        vc[1] = [0.0, 1.0, 0.0, 0.0];
        vc[2] = [0.0, 0.0, 1.0, 0.0];
        vc[3] = [0.0, 0.0, 0.0, 1.0];
        let mut fc = [[0.0f32; 4]; 64];
        fc[0] = [1.0, 1.0, 1.0, 1.0];

        self.contexts.insert(
            id,
            Context3dState {
                profile: profile.to_owned(),
                render_mode: render_mode.to_owned(),
                enable_error_checking: false,
                back_buffer_width: 0,
                back_buffer_height: 0,
                max_back_buffer_width: max_dim,
                max_back_buffer_height: max_dim,
                anti_alias: 0,
                enable_depth_and_stencil: true,
                color_buffer: Vec::new(),
                presented_buffer: Vec::new(),
                presented_width: 0,
                presented_height: 0,
                depth_buffer: Vec::new(),
                stencil_buffer: Vec::new(),
                disposed: false,
                program_id: None,
                vc,
                vc_touched: false,
                fc,
                fc_touched: false,
                attrs: std::array::from_fn(|_| None),
                textures: [None; 8],
                samplers: std::array::from_fn(|_| SamplerState::default()),
                blend_src: "one".into(),
                blend_dst: "zero".into(),
                color_mask: [true, true, true, true],
                culling: "none".into(),
                depth_mask: true,
                depth_compare: "less".into(),
                fill_mode: "solid".into(),
                scissor: None,
                stencil_face: "frontAndBack".into(),
                stencil_compare: "always".into(),
                stencil_both_pass: "keep".into(),
                stencil_depth_fail: "keep".into(),
                stencil_pass_fail: "keep".into(),
                stencil_ref: 0,
                stencil_read_mask: 0xff,
                stencil_write_mask: 0xff,
                rtt: None,
            },
        );
        Ok(id)
    }

    fn ctx(&self, ctx_id: u32) -> Result<&Context3dState, Fte3dError> {
        let c = self
            .contexts
            .get(&ctx_id)
            .ok_or_else(|| Fte3dError::IllegalOperation("Unknown Context3D.".into()))?;
        if c.disposed {
            return Err(Fte3dError::IllegalOperation(
                "Error #3694: The Context3D object was disposed by an earlier call.".into(),
            ));
        }
        Ok(c)
    }

    fn ctx_mut(&mut self, ctx_id: u32) -> Result<&mut Context3dState, Fte3dError> {
        let c = self
            .contexts
            .get_mut(&ctx_id)
            .ok_or_else(|| Fte3dError::IllegalOperation("Unknown Context3D.".into()))?;
        if c.disposed {
            return Err(Fte3dError::IllegalOperation(
                "Error #3694: The Context3D object was disposed by an earlier call.".into(),
            ));
        }
        Ok(c)
    }

    pub fn dispose_context(&mut self, ctx_id: u32) {
        if let Some(c) = self.contexts.get_mut(&ctx_id) {
            c.disposed = true;
            c.color_buffer.clear();
            c.presented_buffer.clear();
            c.presented_width = 0;
            c.presented_height = 0;
            c.depth_buffer.clear();
            c.stencil_buffer.clear();
            c.rtt = None;
        }
        for (owner, vb) in self.vertex_buffers.values_mut() {
            if *owner == ctx_id {
                vb.disposed = true;
                vb.data.clear();
            }
        }
        for (owner, ib) in self.index_buffers.values_mut() {
            if *owner == ctx_id {
                ib.disposed = true;
                ib.data.clear();
            }
        }
        for (owner, prg) in self.programs.values_mut() {
            if *owner == ctx_id {
                prg.disposed = true;
            }
        }
        for (owner, tex) in self.textures.values_mut() {
            if *owner == ctx_id {
                tex.disposed = true;
                tex.faces.clear();
                tex.mips.clear();
            }
        }
    }

    pub fn total_gpu_memory(&self, ctx_id: u32) -> f64 {
        let Some(c) = self.contexts.get(&ctx_id) else {
            return 0.0;
        };
        if c.disposed {
            return 0.0;
        }
        let mut total = c.color_buffer.len() + c.depth_buffer.len() * 4 + c.stencil_buffer.len();
        for (owner, vb) in self.vertex_buffers.values() {
            if *owner == ctx_id && !vb.disposed {
                total += vb.data.len() * 4;
            }
        }
        for (owner, ib) in self.index_buffers.values() {
            if *owner == ctx_id && !ib.disposed {
                total += ib.data.len() * 2;
            }
        }
        for (owner, tex) in self.textures.values() {
            if *owner == ctx_id && !tex.disposed {
                total += tex.byte_size();
            }
        }
        total as f64
    }

    pub fn configure_back_buffer(
        &mut self,
        ctx_id: u32,
        width: i32,
        height: i32,
        anti_alias: i32,
        enable_depth_and_stencil: bool,
    ) -> Result<(), Fte3dError> {
        let c = self.ctx_mut(ctx_id)?;
        let max_w = c.max_back_buffer_width.min(MAX_STAGE3D_DIM) as i32;
        let max_h = c.max_back_buffer_height.min(MAX_STAGE3D_DIM) as i32;
        if width < 32 || height < 32 || width > max_w || height > max_h {
            return Err(Fte3dError::Argument(format!(
                "Context3D.configureBackBuffer dimensions must be 32..={max_w}x{max_h}; got {width}x{height}."
            )));
        }
        let w = width as u32;
        let h = height as u32;
        let pixels = (w as usize) * (h as usize);
        c.back_buffer_width = w;
        c.back_buffer_height = h;
        c.anti_alias = anti_alias;
        c.enable_depth_and_stencil = enable_depth_and_stencil;
        c.color_buffer = vec![0u8; pixels * 4];
        if enable_depth_and_stencil {
            c.depth_buffer = vec![1.0f32; pixels];
            c.stencil_buffer = vec![0u8; pixels];
        } else {
            c.depth_buffer.clear();
            c.stencil_buffer.clear();
        }
        c.rtt = None;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn clear(
        &mut self,
        ctx_id: u32,
        red: f64,
        green: f64,
        blue: f64,
        alpha: f64,
        depth: f64,
        stencil: u32,
        mask: u32,
    ) -> Result<(), Fte3dError> {
        let r_u8 = (red.clamp(0.0, 1.0) * 255.0).round() as u8;
        let g_u8 = (green.clamp(0.0, 1.0) * 255.0).round() as u8;
        let b_u8 = (blue.clamp(0.0, 1.0) * 255.0).round() as u8;
        let a_u8 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        let d_f32 = depth.clamp(0.0, 1.0) as f32;
        let s_u8 = (stencil & 0xff) as u8;

        let rtt_tex = self
            .ctx(ctx_id)?
            .rtt
            .as_ref()
            .map(|r| (r.texture_id, r.surface_selector));
        if let Some((tid, face)) = rtt_tex {
            if (mask & 1) != 0
                && let Some((_, tex)) = self.textures.get_mut(&tid)
                && let Some(buf) = tex.faces.get_mut(face)
            {
                for px in buf.chunks_exact_mut(4) {
                    px.copy_from_slice(&[r_u8, g_u8, b_u8, a_u8]);
                }
            }
            let c = self.ctx_mut(ctx_id)?;
            if let Some(rtt) = &mut c.rtt
                && rtt.enable_depth_stencil
            {
                if (mask & 2) != 0 {
                    rtt.depth.fill(d_f32);
                }
                if (mask & 4) != 0 {
                    rtt.stencil.fill(s_u8);
                }
            }
            return Ok(());
        }

        let c = self.ctx_mut(ctx_id)?;
        if c.back_buffer_width == 0 || c.back_buffer_height == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.clear requires configureBackBuffer or setRenderToTexture first.".into(),
            ));
        }
        if (mask & 1) != 0 {
            for px in c.color_buffer.chunks_exact_mut(4) {
                px.copy_from_slice(&[r_u8, g_u8, b_u8, a_u8]);
            }
        }
        if c.enable_depth_and_stencil {
            if (mask & 2) != 0 {
                c.depth_buffer.fill(d_f32);
            }
            if (mask & 4) != 0 {
                c.stencil_buffer.fill(s_u8);
            }
        }
        Ok(())
    }

    pub fn create_vertex_buffer(
        &mut self,
        ctx_id: u32,
        num_vertices: i32,
        data32_per_vertex: i32,
        usage: &str,
    ) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        validate_stage3d_enum("bufferUsage", usage)?;
        if num_vertices <= 0
            || (num_vertices as usize) > MAX_STAGE3D_VERTICES
            || !(1..=64).contains(&data32_per_vertex)
        {
            return Err(Fte3dError::Argument(format!(
                "Invalid VertexBuffer3D size ({num_vertices} vertices, {data32_per_vertex} floats/vertex)."
            )));
        }
        if self.vertex_buffers.len() >= MAX_STAGE3D_BUFFERS {
            return Err(Fte3dError::Range(
                "Stage3D VertexBuffer3D limit reached.".into(),
            ));
        }
        let id = self.alloc_id();
        let nv = num_vertices as usize;
        let stride = data32_per_vertex as usize;
        self.vertex_buffers.insert(
            id,
            (
                ctx_id,
                VertexBufferRecord {
                    num_vertices: nv,
                    data32_per_vertex: stride,
                    usage: usage.to_owned(),
                    data: vec![0.0f32; nv * stride],
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    pub fn upload_vertex_buffer(
        &mut self,
        vb_id: u32,
        floats: &[f32],
        start_vertex: i32,
        num_vertices: i32,
    ) -> Result<(), Fte3dError> {
        let (_, vb) = self
            .vertex_buffers
            .get_mut(&vb_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown VertexBuffer3D.".into()))?;
        if vb.disposed {
            return Err(Fte3dError::IllegalOperation(
                "VertexBuffer3D is disposed.".into(),
            ));
        }
        if start_vertex < 0
            || num_vertices < 0
            || (start_vertex as usize) + (num_vertices as usize) > vb.num_vertices
        {
            return Err(Fte3dError::Range(
                "VertexBuffer3D upload range exceeds buffer capacity.".into(),
            ));
        }
        let need = (num_vertices as usize) * vb.data32_per_vertex;
        if floats.len() < need {
            return Err(Fte3dError::Range(
                "VertexBuffer3D upload source has fewer elements than required.".into(),
            ));
        }
        let dst_start = (start_vertex as usize) * vb.data32_per_vertex;
        vb.data[dst_start..dst_start + need].copy_from_slice(&floats[..need]);
        Ok(())
    }

    pub fn dispose_vertex_buffer(&mut self, vb_id: u32) {
        if let Some((_, vb)) = self.vertex_buffers.get_mut(&vb_id) {
            vb.disposed = true;
            vb.data.clear();
        }
    }

    pub fn create_index_buffer(
        &mut self,
        ctx_id: u32,
        num_indices: i32,
        usage: &str,
    ) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        validate_stage3d_enum("bufferUsage", usage)?;
        if num_indices <= 0 || (num_indices as usize) > MAX_STAGE3D_INDICES {
            return Err(Fte3dError::Argument(format!(
                "Invalid IndexBuffer3D count: {num_indices}."
            )));
        }
        if self.index_buffers.len() >= MAX_STAGE3D_BUFFERS {
            return Err(Fte3dError::Range(
                "Stage3D IndexBuffer3D limit reached.".into(),
            ));
        }
        let id = self.alloc_id();
        let ni = num_indices as usize;
        self.index_buffers.insert(
            id,
            (
                ctx_id,
                IndexBufferRecord {
                    num_indices: ni,
                    usage: usage.to_owned(),
                    data: vec![0u16; ni],
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    pub fn upload_index_buffer(
        &mut self,
        ib_id: u32,
        indices: &[u16],
        start_offset: i32,
        count: i32,
    ) -> Result<(), Fte3dError> {
        let (_, ib) = self
            .index_buffers
            .get_mut(&ib_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown IndexBuffer3D.".into()))?;
        if ib.disposed {
            return Err(Fte3dError::IllegalOperation(
                "IndexBuffer3D is disposed.".into(),
            ));
        }
        if start_offset < 0
            || count < 0
            || (start_offset as usize) + (count as usize) > ib.num_indices
        {
            return Err(Fte3dError::Range(
                "IndexBuffer3D upload range exceeds buffer capacity.".into(),
            ));
        }
        let need = count as usize;
        if indices.len() < need {
            return Err(Fte3dError::Range(
                "IndexBuffer3D upload source has fewer indices than count.".into(),
            ));
        }
        let dst = start_offset as usize;
        ib.data[dst..dst + need].copy_from_slice(&indices[..need]);
        Ok(())
    }

    pub fn dispose_index_buffer(&mut self, ib_id: u32) {
        if let Some((_, ib)) = self.index_buffers.get_mut(&ib_id) {
            ib.disposed = true;
            ib.data.clear();
        }
    }

    pub fn create_program(&mut self, ctx_id: u32) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        let id = self.alloc_id();
        self.programs.insert(
            id,
            (
                ctx_id,
                ProgramRecord {
                    vertex: CompiledShader::Fallback,
                    fragment: CompiledShader::Fallback,
                    uploaded: false,
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    pub fn upload_program(
        &mut self,
        prog_id: u32,
        vertex_bytes: &[u8],
        fragment_bytes: &[u8],
    ) -> Result<(), Fte3dError> {
        let (_, prg) = self
            .programs
            .get_mut(&prog_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown Program3D.".into()))?;
        if prg.disposed {
            return Err(Fte3dError::IllegalOperation(
                "Program3D is disposed.".into(),
            ));
        }
        let vs = parse_agal(vertex_bytes, 0)?;
        let fs = parse_agal(fragment_bytes, 1)?;
        prg.vertex = vs;
        prg.fragment = fs;
        prg.uploaded = true;
        Ok(())
    }

    pub fn dispose_program(&mut self, prog_id: u32) {
        if let Some((_, prg)) = self.programs.get_mut(&prog_id) {
            prg.disposed = true;
        }
    }

    pub fn create_texture(
        &mut self,
        ctx_id: u32,
        kind: TextureKind,
        width: i32,
        height: i32,
        format: &str,
        optimize_for_rtt: bool,
    ) -> Result<u32, Fte3dError> {
        self.ctx(ctx_id)?;
        validate_stage3d_enum("textureFormat", format)?;
        if kind != TextureKind::Video {
            if width <= 0
                || height <= 0
                || width > MAX_STAGE3D_DIM as i32
                || height > MAX_STAGE3D_DIM as i32
            {
                return Err(Fte3dError::Argument(format!(
                    "Invalid Stage3D texture dimensions {width}x{height}."
                )));
            }
            let w = width as u32;
            let h = height as u32;
            if matches!(kind, TextureKind::TwoD | TextureKind::Cube)
                && (!w.is_power_of_two() || !h.is_power_of_two())
            {
                return Err(Fte3dError::Argument(format!(
                    "Texture/CubeTexture dimensions must be powers of two; got {w}x{h}."
                )));
            }
        }
        let w = width.max(1) as u32;
        let h = height.max(1) as u32;
        let face_count = if kind == TextureKind::Cube { 6 } else { 1 };
        let face_bytes = (w as usize) * (h as usize) * 4;
        let id = self.alloc_id();
        self.textures.insert(
            id,
            (
                ctx_id,
                TextureRecord {
                    kind,
                    width: w,
                    height: h,
                    format: format.to_owned(),
                    optimize_for_rtt,
                    faces: vec![vec![0u8; face_bytes]; face_count],
                    mips: vec![BTreeMap::new(); face_count],
                    disposed: false,
                },
            ),
        );
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upload_texture_rgba(
        &mut self,
        tex_id: u32,
        side: usize,
        miplevel: u32,
        src_w: u32,
        src_h: u32,
        rgba: &[u8],
        bgra_input: bool,
    ) -> Result<(), Fte3dError> {
        let (_, tex) = self
            .textures
            .get_mut(&tex_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown TextureBase.".into()))?;
        if tex.disposed {
            return Err(Fte3dError::IllegalOperation("Texture is disposed.".into()));
        }
        if side >= tex.faces.len() {
            return Err(Fte3dError::Argument(format!(
                "Invalid CubeTexture side {side}."
            )));
        }
        if miplevel > 0 {
            let expected_w = (tex.width >> miplevel).max(1);
            let expected_h = (tex.height >> miplevel).max(1);
            if src_w != expected_w || src_h != expected_h {
                return Err(Fte3dError::Argument(format!(
                    "Mip level {miplevel} expects {expected_w}x{expected_h}, got {src_w}x{src_h}."
                )));
            }
            let need = (expected_w as usize) * (expected_h as usize) * 4;
            if rgba.len() < need {
                return Err(Fte3dError::Range(
                    "Texture mip upload byte buffer is smaller than width*height*4.".into(),
                ));
            }
            let mut mip_buf = vec![0u8; need];
            if bgra_input {
                for (d, s) in mip_buf
                    .chunks_exact_mut(4)
                    .zip(rgba[..need].chunks_exact(4))
                {
                    d[0] = s[2];
                    d[1] = s[1];
                    d[2] = s[0];
                    d[3] = s[3];
                }
            } else {
                mip_buf.copy_from_slice(&rgba[..need]);
            }
            if let Some(face_mips) = tex.mips.get_mut(side) {
                face_mips.insert(miplevel, (expected_w, expected_h, mip_buf));
            }
            return Ok(());
        }
        if src_w != tex.width || src_h != tex.height {
            return Err(Fte3dError::Argument(format!(
                "Texture upload expects {}x{}, got {src_w}x{src_h}.",
                tex.width, tex.height
            )));
        }
        let need = (tex.width as usize) * (tex.height as usize) * 4;
        if rgba.len() < need {
            return Err(Fte3dError::Range(
                "Texture upload byte buffer is smaller than width*height*4.".into(),
            ));
        }
        let dst = &mut tex.faces[side];
        if bgra_input {
            for (d, s) in dst.chunks_exact_mut(4).zip(rgba[..need].chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        } else {
            dst.copy_from_slice(&rgba[..need]);
        }
        Ok(())
    }

    pub fn upload_texture_mip_rgba(
        &mut self,
        tex_id: u32,
        side: usize,
        miplevel: u32,
        rgba: &[u8],
    ) -> Result<(), Fte3dError> {
        let (w, h) = self
            .textures
            .get(&tex_id)
            .map(|(_, tex)| {
                (
                    (tex.width >> miplevel).max(1),
                    (tex.height >> miplevel).max(1),
                )
            })
            .ok_or_else(|| Fte3dError::Argument("Unknown TextureBase.".into()))?;
        self.upload_texture_rgba(tex_id, side, miplevel, w, h, rgba, false)
    }

    pub fn upload_compressed_texture(
        &mut self,
        tex_id: u32,
        bytes: &[u8],
    ) -> Result<(), Fte3dError> {
        let (_, tex) = self
            .textures
            .get_mut(&tex_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown Texture.".into()))?;
        if tex.disposed {
            return Err(Fte3dError::IllegalOperation("Texture is disposed.".into()));
        }
        if bytes.len() < 4 {
            return Err(Fte3dError::Argument(
                "Compressed texture ByteArray is empty or truncated.".into(),
            ));
        }
        // Support ATF header ("ATF") or raw packed payload; fill base mip deterministically.
        let fill = if bytes.starts_with(b"ATF") && bytes.len() >= 10 {
            [bytes[6], bytes[7], bytes[8], 255]
        } else {
            [bytes[0], bytes[1], bytes[2], bytes[3]]
        };
        for face in &mut tex.faces {
            for px in face.chunks_exact_mut(4) {
                px.copy_from_slice(&fill);
            }
        }
        Ok(())
    }

    pub fn dispose_texture(&mut self, tex_id: u32) {
        if let Some((_, tex)) = self.textures.get_mut(&tex_id) {
            tex.disposed = true;
            tex.faces.clear();
        }
    }

    pub fn set_state_cmd(
        &mut self,
        ctx_id: u32,
        op: &str,
        args: &serde_json::Value,
    ) -> Result<(), Fte3dError> {
        match op {
            "setProgram" => {
                let pid = args["programId"].as_u64().map(|v| v as u32);
                if let Some(id) = pid {
                    let (owner, prg) = self
                        .programs
                        .get(&id)
                        .ok_or_else(|| Fte3dError::Argument("Unknown Program3D.".into()))?;
                    if *owner != ctx_id || prg.disposed {
                        return Err(Fte3dError::IllegalOperation(
                            "Program3D is disposed or belongs to another Context3D.".into(),
                        ));
                    }
                }
                self.ctx_mut(ctx_id)?.program_id = pid;
            }
            "setConstants" => {
                let ptype = args["programType"].as_str().unwrap_or("");
                validate_stage3d_enum("programType", ptype)?;
                let first = args["firstRegister"].as_i64().unwrap_or(-1);
                let values: Vec<f32> = args["values"]
                    .as_array()
                    .ok_or_else(|| Fte3dError::Argument("Constants require float array.".into()))?
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect();
                let num_regs = args["numRegisters"]
                    .as_i64()
                    .unwrap_or((values.len() / 4) as i64);
                let count = if num_regs < 0 {
                    values.len() / 4
                } else {
                    num_regs as usize
                };
                if values.len() < count * 4 || first < 0 {
                    return Err(Fte3dError::Range(
                        "Insufficient float components for setProgramConstants.".into(),
                    ));
                }
                let c = self.ctx_mut(ctx_id)?;
                let first_u = first as usize;
                if ptype == "vertex" {
                    if first_u + count > c.vc.len() {
                        return Err(Fte3dError::Range(
                            "Vertex constant register index out of bounds.".into(),
                        ));
                    }
                    for r in 0..count {
                        let o = r * 4;
                        c.vc[first_u + r] =
                            [values[o], values[o + 1], values[o + 2], values[o + 3]];
                    }
                    c.vc_touched = true;
                } else {
                    if first_u + count > c.fc.len() {
                        return Err(Fte3dError::Range(
                            "Fragment constant register index out of bounds.".into(),
                        ));
                    }
                    for r in 0..count {
                        let o = r * 4;
                        c.fc[first_u + r] =
                            [values[o], values[o + 1], values[o + 2], values[o + 3]];
                    }
                    c.fc_touched = true;
                }
            }
            "setVertexBufferAt" => {
                let idx = args["index"].as_i64().unwrap_or(-1);
                if !(0..8).contains(&idx) {
                    return Err(Fte3dError::Range(
                        "Vertex attribute stream index must be 0..7.".into(),
                    ));
                }
                if args["bufferId"].is_null() {
                    self.ctx_mut(ctx_id)?.attrs[idx as usize] = None;
                } else {
                    let bid = args["bufferId"].as_u64().unwrap_or(0) as u32;
                    let offset = args["bufferOffset"].as_i64().unwrap_or(0);
                    let fmt = args["format"].as_str().unwrap_or("float4");
                    validate_stage3d_enum("vertexFormat", fmt)?;
                    let (owner, vb) = self
                        .vertex_buffers
                        .get(&bid)
                        .ok_or_else(|| Fte3dError::Argument("Unknown VertexBuffer3D.".into()))?;
                    if *owner != ctx_id || vb.disposed {
                        return Err(Fte3dError::IllegalOperation(
                            "VertexBuffer3D is disposed or foreign.".into(),
                        ));
                    }
                    let comps = match fmt {
                        "float1" | "bytes4" => 1,
                        "float2" => 2,
                        "float3" => 3,
                        _ => 4,
                    };
                    if offset < 0 || (offset as usize) + comps > vb.data32_per_vertex {
                        return Err(Fte3dError::Range(
                            "Vertex attribute offset + format exceeds buffer stride.".into(),
                        ));
                    }
                    self.ctx_mut(ctx_id)?.attrs[idx as usize] = Some(BoundAttr {
                        buffer_id: bid,
                        offset: offset as usize,
                        format: fmt.to_owned(),
                    });
                }
            }
            "setTextureAt" => {
                let sampler = args["sampler"].as_i64().unwrap_or(-1);
                if !(0..8).contains(&sampler) {
                    return Err(Fte3dError::Range("Sampler index must be 0..7.".into()));
                }
                if args["textureId"].is_null() {
                    self.ctx_mut(ctx_id)?.textures[sampler as usize] = None;
                } else {
                    let tid = args["textureId"].as_u64().unwrap_or(0) as u32;
                    let (owner, tex) = self
                        .textures
                        .get(&tid)
                        .ok_or_else(|| Fte3dError::Argument("Unknown TextureBase.".into()))?;
                    if *owner != ctx_id || tex.disposed {
                        return Err(Fte3dError::IllegalOperation(
                            "Texture is disposed or belongs to another Context3D.".into(),
                        ));
                    }
                    self.ctx_mut(ctx_id)?.textures[sampler as usize] = Some(tid);
                }
            }
            "setSamplerStateAt" => {
                let sampler = args["sampler"].as_i64().unwrap_or(-1);
                if !(0..8).contains(&sampler) {
                    return Err(Fte3dError::Range("Sampler index must be 0..7.".into()));
                }
                let wrap = validate_stage3d_enum("wrapMode", args["wrap"].as_str().unwrap_or(""))?;
                let filter =
                    validate_stage3d_enum("textureFilter", args["filter"].as_str().unwrap_or(""))?;
                let mip =
                    validate_stage3d_enum("mipFilter", args["mipfilter"].as_str().unwrap_or(""))?;
                self.ctx_mut(ctx_id)?.samplers[sampler as usize] = SamplerState {
                    wrap: wrap.to_owned(),
                    filter: filter.to_owned(),
                    mipfilter: mip.to_owned(),
                };
            }
            "setBlendFactors" => {
                let src =
                    validate_stage3d_enum("blendFactor", args["source"].as_str().unwrap_or(""))?;
                let dst = validate_stage3d_enum(
                    "blendFactor",
                    args["destination"].as_str().unwrap_or(""),
                )?;
                let c = self.ctx_mut(ctx_id)?;
                c.blend_src = src.to_owned();
                c.blend_dst = dst.to_owned();
            }
            "setColorMask" => {
                let c = self.ctx_mut(ctx_id)?;
                c.color_mask = [
                    args["r"].as_bool().unwrap_or(true),
                    args["g"].as_bool().unwrap_or(true),
                    args["b"].as_bool().unwrap_or(true),
                    args["a"].as_bool().unwrap_or(true),
                ];
            }
            "setCulling" => {
                let face =
                    validate_stage3d_enum("triangleFace", args["face"].as_str().unwrap_or(""))?;
                self.ctx_mut(ctx_id)?.culling = face.to_owned();
            }
            "setDepthTest" => {
                let mask = args["depthMask"].as_bool().unwrap_or(true);
                let cmp = validate_stage3d_enum(
                    "compareMode",
                    args["compareMode"].as_str().unwrap_or(""),
                )?;
                let c = self.ctx_mut(ctx_id)?;
                c.depth_mask = mask;
                c.depth_compare = cmp.to_owned();
            }
            "setFillMode" => {
                let fm =
                    validate_stage3d_enum("fillMode", args["fillMode"].as_str().unwrap_or(""))?;
                self.ctx_mut(ctx_id)?.fill_mode = fm.to_owned();
            }
            "setScissorRectangle" => {
                let c = self.ctx_mut(ctx_id)?;
                if args["rect"].is_null() {
                    c.scissor = None;
                } else if let Some(arr) = args["rect"].as_array()
                    && arr.len() == 4
                {
                    c.scissor = Some([
                        arr[0].as_f64().unwrap_or(0.0).round() as i32,
                        arr[1].as_f64().unwrap_or(0.0).round() as i32,
                        arr[2].as_f64().unwrap_or(0.0).round().max(0.0) as i32,
                        arr[3].as_f64().unwrap_or(0.0).round().max(0.0) as i32,
                    ]);
                }
            }
            "setStencilActions" => {
                let face = validate_stage3d_enum(
                    "triangleFace",
                    args["triangleFace"].as_str().unwrap_or("frontAndBack"),
                )?;
                let cmp = validate_stage3d_enum(
                    "compareMode",
                    args["compareMode"].as_str().unwrap_or("always"),
                )?;
                let both = validate_stage3d_enum(
                    "stencilAction",
                    args["actionOnBothPass"].as_str().unwrap_or("keep"),
                )?;
                let dfail = validate_stage3d_enum(
                    "stencilAction",
                    args["actionOnDepthFail"].as_str().unwrap_or("keep"),
                )?;
                let sfail = validate_stage3d_enum(
                    "stencilAction",
                    args["actionOnDepthPassStencilFail"]
                        .as_str()
                        .unwrap_or("keep"),
                )?;
                let c = self.ctx_mut(ctx_id)?;
                c.stencil_face = face.to_owned();
                c.stencil_compare = cmp.to_owned();
                c.stencil_both_pass = both.to_owned();
                c.stencil_depth_fail = dfail.to_owned();
                c.stencil_pass_fail = sfail.to_owned();
            }
            "setStencilReferenceValue" => {
                let c = self.ctx_mut(ctx_id)?;
                c.stencil_ref = (args["referenceValue"].as_u64().unwrap_or(0) & 0xff) as u8;
                c.stencil_read_mask = (args["readMask"].as_u64().unwrap_or(0xff) & 0xff) as u8;
                c.stencil_write_mask = (args["writeMask"].as_u64().unwrap_or(0xff) & 0xff) as u8;
            }
            "setRenderToTexture" => {
                let tid = args["textureId"].as_u64().unwrap_or(0) as u32;
                let enable_ds = args["enableDepthAndStencil"].as_bool().unwrap_or(false);
                let selector = args["surfaceSelector"].as_i64().unwrap_or(0).max(0) as usize;
                let (owner, tex) = self
                    .textures
                    .get(&tid)
                    .ok_or_else(|| Fte3dError::Argument("Unknown render target texture.".into()))?;
                if *owner != ctx_id || tex.disposed {
                    return Err(Fte3dError::IllegalOperation(
                        "Render target texture is disposed or foreign.".into(),
                    ));
                }
                if selector >= tex.faces.len() {
                    return Err(Fte3dError::Argument(
                        "Render target surfaceSelector out of range.".into(),
                    ));
                }
                let pixels = (tex.width as usize) * (tex.height as usize);
                let c = self.ctx_mut(ctx_id)?;
                c.rtt = Some(RenderTarget {
                    texture_id: tid,
                    surface_selector: selector,
                    enable_depth_stencil: enable_ds,
                    depth: if enable_ds {
                        vec![1.0f32; pixels]
                    } else {
                        Vec::new()
                    },
                    stencil: if enable_ds {
                        vec![0u8; pixels]
                    } else {
                        Vec::new()
                    },
                });
            }
            "setRenderToBackBuffer" => {
                self.ctx_mut(ctx_id)?.rtt = None;
            }
            _ => {
                return Err(Fte3dError::Argument(format!(
                    "Unknown Stage3D state command '{op}'."
                )));
            }
        }
        Ok(())
    }

    pub fn draw_triangles(
        &mut self,
        ctx_id: u32,
        ib_id: u32,
        first_index: i32,
        num_triangles: i32,
    ) -> Result<(), Fte3dError> {
        let c_snap = self.ctx(ctx_id)?.clone();
        let (ib_owner, ib) = self
            .index_buffers
            .get(&ib_id)
            .ok_or_else(|| Fte3dError::Argument("Unknown IndexBuffer3D.".into()))?;
        if *ib_owner != ctx_id || ib.disposed {
            return Err(Fte3dError::IllegalOperation(
                "IndexBuffer3D is disposed or foreign.".into(),
            ));
        }
        let first = first_index.max(0) as usize;
        if first > ib.data.len() {
            return Err(Fte3dError::Range(
                "drawTriangles firstIndex exceeds IndexBuffer3D length.".into(),
            ));
        }
        let tri_count = if num_triangles < 0 {
            (ib.data.len() - first) / 3
        } else {
            num_triangles as usize
        };
        if first + tri_count * 3 > ib.data.len() {
            return Err(Fte3dError::Range(
                "drawTriangles index range exceeds IndexBuffer3D length.".into(),
            ));
        }
        let indices: Vec<u16> = ib.data[first..first + tri_count * 3].to_vec();

        let (vs_prog, fs_prog) = if let Some(pid) = c_snap.program_id
            && let Some((_, prg)) = self.programs.get(&pid)
            && prg.uploaded
            && !prg.disposed
        {
            (prg.vertex.clone(), prg.fragment.clone())
        } else {
            (CompiledShader::Fallback, CompiledShader::Fallback)
        };

        let tex_map: BTreeMap<u32, TextureRecord> = self
            .textures
            .iter()
            .filter(|(_, (owner, t))| *owner == ctx_id && !t.disposed)
            .map(|(k, (_, t))| (*k, t.clone()))
            .collect();

        let (target_w, target_h) = if let Some(rtt) = &c_snap.rtt
            && let Some(tex) = tex_map.get(&rtt.texture_id)
        {
            (tex.width, tex.height)
        } else {
            (c_snap.back_buffer_width, c_snap.back_buffer_height)
        };
        if target_w == 0 || target_h == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.drawTriangles requires an active backbuffer or render texture.".into(),
            ));
        }

        let fetch_vertex_attrs = |v_idx: usize| -> [[f32; 4]; 8] {
            let mut out = [[0.0f32, 0.0, 0.0, 1.0]; 8];
            for (slot, bound) in c_snap.attrs.iter().enumerate() {
                if let Some(b) = bound
                    && let Some((_, vb)) = self.vertex_buffers.get(&b.buffer_id)
                    && v_idx < vb.num_vertices
                {
                    let base = v_idx * vb.data32_per_vertex + b.offset;
                    match b.format.as_str() {
                        "float1" => {
                            out[slot] = [vb.data[base], 0.0, 0.0, 1.0];
                        }
                        "float2" => {
                            out[slot] = [vb.data[base], vb.data[base + 1], 0.0, 1.0];
                        }
                        "float3" => {
                            out[slot] = [vb.data[base], vb.data[base + 1], vb.data[base + 2], 1.0];
                        }
                        "bytes4" => {
                            let bits = vb.data[base].to_bits();
                            out[slot] = [
                                (bits & 0xff) as f32 / 255.0,
                                ((bits >> 8) & 0xff) as f32 / 255.0,
                                ((bits >> 16) & 0xff) as f32 / 255.0,
                                ((bits >> 24) & 0xff) as f32 / 255.0,
                            ];
                        }
                        _ => {
                            out[slot] = [
                                vb.data[base],
                                vb.data[base + 1],
                                vb.data[base + 2],
                                vb.data[base + 3],
                            ];
                        }
                    }
                }
            }
            out
        };

        let run_vs = |attrs: &[[f32; 4]; 8]| -> ([f32; 4], [[f32; 4]; 8]) {
            let mut varyings = [[0.0f32; 4]; 8];
            match &vs_prog {
                CompiledShader::Agal(insts) => {
                    let pos = run_agal(
                        insts,
                        true,
                        attrs,
                        &c_snap.vc,
                        &mut varyings,
                        &c_snap.textures,
                        &c_snap.samplers,
                        &tex_map,
                    )
                    .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    (pos, varyings)
                }
                CompiledShader::Fallback => {
                    varyings[0] = if c_snap.attrs[1].is_some() {
                        attrs[1]
                    } else if c_snap.fc_touched {
                        c_snap.fc[0]
                    } else {
                        [1.0, 1.0, 1.0, 1.0]
                    };
                    let p = attrs[0];
                    let pos = if c_snap.vc_touched {
                        let dp =
                            |r: [f32; 4]| p[0] * r[0] + p[1] * r[1] + p[2] * r[2] + p[3] * r[3];
                        [
                            dp(c_snap.vc[0]),
                            dp(c_snap.vc[1]),
                            dp(c_snap.vc[2]),
                            dp(c_snap.vc[3]),
                        ]
                    } else {
                        p
                    };
                    (pos, varyings)
                }
            }
        };

        let sc_min_x = c_snap.scissor.map_or(0, |s| s[0].max(0));
        let sc_min_y = c_snap.scissor.map_or(0, |s| s[1].max(0));
        let sc_max_x = c_snap
            .scissor
            .map_or(target_w as i32, |s| (s[0] + s[2]).min(target_w as i32));
        let sc_max_y = c_snap
            .scissor
            .map_or(target_h as i32, |s| (s[1] + s[3]).min(target_h as i32));

        let mut shaded_writes: Vec<(usize, [f32; 4], f32, bool)> = Vec::new();
        let dummy_attrs = [[0.0f32; 4]; 8];

        for tri in indices.chunks_exact(3) {
            let a0 = fetch_vertex_attrs(tri[0] as usize);
            let a1 = fetch_vertex_attrs(tri[1] as usize);
            let a2 = fetch_vertex_attrs(tri[2] as usize);
            let (clip0, var0) = run_vs(&a0);
            let (clip1, var1) = run_vs(&a1);
            let (clip2, var2) = run_vs(&a2);

            let to_screen = |c: [f32; 4]| -> [f32; 4] {
                let inv_w = if c[3].abs() > 1e-7 { 1.0 / c[3] } else { 1.0 };
                let ndc_x = c[0] * inv_w;
                let ndc_y = c[1] * inv_w;
                let ndc_z = (c[2] * inv_w).clamp(0.0, 1.0);
                let sx = (ndc_x * 0.5 + 0.5) * (target_w as f32);
                let sy = (0.5 - ndc_y * 0.5) * (target_h as f32);
                [sx, sy, ndc_z, inv_w]
            };

            let s0 = to_screen(clip0);
            let s1 = to_screen(clip1);
            let s2 = to_screen(clip2);

            let area = (s1[0] - s0[0]) * (s2[1] - s0[1]) - (s1[1] - s0[1]) * (s2[0] - s0[0]);
            if c_snap.culling == "frontAndBack" || area.abs() < 1e-6 {
                continue;
            }
            let is_front = area > 0.0;
            if (c_snap.culling == "back" && !is_front) || (c_snap.culling == "front" && is_front) {
                continue;
            }

            let eval_frag = |vars: &mut [[f32; 4]; 8]| -> Option<[f32; 4]> {
                match &fs_prog {
                    CompiledShader::Agal(insts) => run_agal(
                        insts,
                        false,
                        &dummy_attrs,
                        &c_snap.fc,
                        vars,
                        &c_snap.textures,
                        &c_snap.samplers,
                        &tex_map,
                    ),
                    CompiledShader::Fallback => {
                        if let Some(tid) = c_snap.textures[0]
                            && let Some(tex) = tex_map.get(&tid)
                        {
                            let st = &c_snap.samplers[0];
                            Some(tex.sample_coord(vars[0], &st.wrap, &st.filter, &st.mipfilter))
                        } else {
                            Some(vars[0])
                        }
                    }
                }
            };

            if c_snap.fill_mode == "wireframe" {
                let edges = [
                    (s0, var0, s1, var1),
                    (s1, var1, s2, var2),
                    (s2, var2, s0, var0),
                ];
                for (p0, v0, p1, v1) in edges {
                    let steps = ((p1[0] - p0[0]).abs().max((p1[1] - p0[1]).abs()).ceil() as i32)
                        .clamp(1, 4096);
                    for step in 0..=steps {
                        let t = (step as f32) / (steps as f32);
                        let px = (p0[0] + (p1[0] - p0[0]) * t).floor() as i32;
                        let py = (p0[1] + (p1[1] - p0[1]) * t).floor() as i32;
                        if px < sc_min_x || px >= sc_max_x || py < sc_min_y || py >= sc_max_y {
                            continue;
                        }
                        let pz = p0[2] + (p1[2] - p0[2]) * t;
                        let mut vars = [[0.0f32; 4]; 8];
                        for slot in 0..8 {
                            for ch in 0..4 {
                                vars[slot][ch] = v0[slot][ch] * (1.0 - t) + v1[slot][ch] * t;
                            }
                        }
                        if let Some(col) = eval_frag(&mut vars) {
                            let idx = (py as usize) * (target_w as usize) + (px as usize);
                            shaded_writes.push((idx, col, pz, is_front));
                        }
                    }
                }
                continue;
            }

            let min_x = s0[0].min(s1[0]).min(s2[0]).floor() as i32;
            let max_x = s0[0].max(s1[0]).max(s2[0]).ceil() as i32;
            let min_y = s0[1].min(s1[1]).min(s2[1]).floor() as i32;
            let max_y = s0[1].max(s1[1]).max(s2[1]).ceil() as i32;

            let x0 = min_x.max(sc_min_x);
            let x1 = max_x.min(sc_max_x);
            let y0 = min_y.max(sc_min_y);
            let y1 = max_y.min(sc_max_y);
            let inv_area = 1.0 / area;

            for py in y0..y1 {
                let fy = py as f32 + 0.5;
                for px in x0..x1 {
                    let fx = px as f32 + 0.5;
                    let w0 = ((s1[0] - fx) * (s2[1] - fy) - (s1[1] - fy) * (s2[0] - fx)) * inv_area;
                    let w1 = ((s2[0] - fx) * (s0[1] - fy) - (s2[1] - fy) * (s0[0] - fx)) * inv_area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                        continue;
                    }
                    let pz = (w0 * s0[2] + w1 * s1[2] + w2 * s2[2]).clamp(0.0, 1.0);
                    let denom = (w0 * s0[3] + w1 * s1[3] + w2 * s2[3]).max(1e-7);
                    let pw0 = (w0 * s0[3]) / denom;
                    let pw1 = (w1 * s1[3]) / denom;
                    let pw2 = (w2 * s2[3]) / denom;

                    let mut vars = [[0.0f32; 4]; 8];
                    for slot in 0..8 {
                        for ch in 0..4 {
                            vars[slot][ch] =
                                pw0 * var0[slot][ch] + pw1 * var1[slot][ch] + pw2 * var2[slot][ch];
                        }
                    }
                    if let Some(col) = eval_frag(&mut vars) {
                        let idx = (py as usize) * (target_w as usize) + (px as usize);
                        shaded_writes.push((idx, col, pz, is_front));
                    }
                }
            }
        }

        // Commit shaded fragments to active render target (RTT or backbuffer)
        let rtt_target = self
            .ctx(ctx_id)?
            .rtt
            .as_ref()
            .map(|r| (r.texture_id, r.surface_selector, r.enable_depth_stencil));

        for (idx, src_raw, pz, _is_front) in shaded_writes {
            let src = [
                src_raw[0].clamp(0.0, 1.0),
                src_raw[1].clamp(0.0, 1.0),
                src_raw[2].clamp(0.0, 1.0),
                src_raw[3].clamp(0.0, 1.0),
            ];

            if let Some((tid, face, has_ds)) = rtt_target {
                let mut write_color = true;
                {
                    let c = self.ctx_mut(ctx_id)?;
                    if has_ds
                        && let Some(rtt) = &mut c.rtt
                        && idx < rtt.depth.len()
                    {
                        let st_cur = rtt.stencil[idx];
                        let st_pass = compare_stencil(
                            &c_snap.stencil_compare,
                            c_snap.stencil_ref,
                            st_cur,
                            c_snap.stencil_read_mask,
                        );
                        let dp_pass = compare_pass(&c_snap.depth_compare, pz, rtt.depth[idx]);
                        if !st_pass {
                            rtt.stencil[idx] = apply_stencil_op(
                                &c_snap.stencil_pass_fail,
                                st_cur,
                                c_snap.stencil_ref,
                                c_snap.stencil_write_mask,
                            );
                            write_color = false;
                        } else if !dp_pass {
                            rtt.stencil[idx] = apply_stencil_op(
                                &c_snap.stencil_depth_fail,
                                st_cur,
                                c_snap.stencil_ref,
                                c_snap.stencil_write_mask,
                            );
                            write_color = false;
                        } else {
                            rtt.stencil[idx] = apply_stencil_op(
                                &c_snap.stencil_both_pass,
                                st_cur,
                                c_snap.stencil_ref,
                                c_snap.stencil_write_mask,
                            );
                            if c_snap.depth_mask {
                                rtt.depth[idx] = pz;
                            }
                        }
                    }
                }
                if write_color
                    && let Some((_, tex)) = self.textures.get_mut(&tid)
                    && let Some(buf) = tex.faces.get_mut(face)
                {
                    blend_pixel(&mut buf[idx * 4..idx * 4 + 4], src, &c_snap);
                }
            } else {
                let c = self.ctx_mut(ctx_id)?;
                if c.enable_depth_and_stencil && idx < c.depth_buffer.len() {
                    let st_cur = c.stencil_buffer[idx];
                    let st_pass = compare_stencil(
                        &c_snap.stencil_compare,
                        c_snap.stencil_ref,
                        st_cur,
                        c_snap.stencil_read_mask,
                    );
                    let dp_pass = compare_pass(&c_snap.depth_compare, pz, c.depth_buffer[idx]);
                    if !st_pass {
                        c.stencil_buffer[idx] = apply_stencil_op(
                            &c_snap.stencil_pass_fail,
                            st_cur,
                            c_snap.stencil_ref,
                            c_snap.stencil_write_mask,
                        );
                        continue;
                    }
                    if !dp_pass {
                        c.stencil_buffer[idx] = apply_stencil_op(
                            &c_snap.stencil_depth_fail,
                            st_cur,
                            c_snap.stencil_ref,
                            c_snap.stencil_write_mask,
                        );
                        continue;
                    }
                    c.stencil_buffer[idx] = apply_stencil_op(
                        &c_snap.stencil_both_pass,
                        st_cur,
                        c_snap.stencil_ref,
                        c_snap.stencil_write_mask,
                    );
                    if c_snap.depth_mask {
                        c.depth_buffer[idx] = pz;
                    }
                }
                let off = idx * 4;
                if off + 4 <= c.color_buffer.len() {
                    blend_pixel(&mut c.color_buffer[off..off + 4], src, &c_snap);
                }
            }
        }
        Ok(())
    }

    pub fn present(&mut self, ctx_id: u32) -> Result<(), Fte3dError> {
        let c = self.ctx_mut(ctx_id)?;
        if c.rtt.is_some() {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.present cannot be called while rendering to a texture; call setRenderToBackBuffer() first.".into(),
            ));
        }
        if c.back_buffer_width == 0 || c.back_buffer_height == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.present requires configureBackBuffer first.".into(),
            ));
        }
        c.presented_width = c.back_buffer_width;
        c.presented_height = c.back_buffer_height;
        c.presented_buffer.clone_from(&c.color_buffer);
        Ok(())
    }

    pub fn presented_rgba(&self, ctx_id: u32) -> Result<(u32, u32, Vec<u8>), Fte3dError> {
        let c = self.ctx(ctx_id)?;
        if c.presented_width == 0 || c.presented_height == 0 || c.presented_buffer.is_empty() {
            return Ok((0, 0, Vec::new()));
        }
        Ok((
            c.presented_width,
            c.presented_height,
            c.presented_buffer.clone(),
        ))
    }

    pub fn backbuffer_rgba(&self, ctx_id: u32) -> Result<(u32, u32, Vec<u8>), Fte3dError> {
        let c = self.ctx(ctx_id)?;
        if c.back_buffer_width == 0 || c.back_buffer_height == 0 {
            return Err(Fte3dError::IllegalOperation(
                "Context3D.drawToBitmapData requires configureBackBuffer first.".into(),
            ));
        }
        Ok((
            c.back_buffer_width,
            c.back_buffer_height,
            c.color_buffer.clone(),
        ))
    }
}

fn blend_pixel(dst_bytes: &mut [u8], src: [f32; 4], c: &Context3dState) {
    let dst = [
        dst_bytes[0] as f32 / 255.0,
        dst_bytes[1] as f32 / 255.0,
        dst_bytes[2] as f32 / 255.0,
        dst_bytes[3] as f32 / 255.0,
    ];
    let sf = blend_factor_vec(&c.blend_src, src, dst);
    let df = blend_factor_vec(&c.blend_dst, src, dst);
    for ch in 0..4 {
        if c.color_mask[ch] {
            let v = (src[ch] * sf[ch] + dst[ch] * df[ch]).clamp(0.0, 1.0);
            dst_bytes[ch] = (v * 255.0).round() as u8;
        }
    }
}

// ============================================================================
// 3. Pixel Bender Shaders (`flash.display.Shader*`, `flash.filters.ShaderFilter`)
// ============================================================================

pub fn validate_shader_precision(value: &str) -> Result<&str, Fte3dError> {
    if matches!(value, "full" | "fast") {
        Ok(value)
    } else {
        Err(Fte3dError::Argument(format!(
            "Invalid ShaderPrecision: '{value}'."
        )))
    }
}

pub fn shader_param_component_count(param_type: &str) -> Result<usize, Fte3dError> {
    match param_type {
        "float" | "int" | "bool" => Ok(1),
        "float2" | "int2" | "bool2" => Ok(2),
        "float3" | "int3" | "bool3" => Ok(3),
        "float4" | "int4" | "bool4" | "matrix2x2" => Ok(4),
        "matrix3x3" => Ok(9),
        "matrix4x4" => Ok(16),
        _ => Err(Fte3dError::Argument(format!(
            "Invalid ShaderParameterType: '{param_type}'."
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderInputMeta {
    pub name: String,
    pub index: u32,
    pub channels: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderParamMeta {
    pub name: String,
    pub index: u32,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub default_value: Vec<f64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub reg: u16,
    #[serde(default = "default_pb_mask")]
    pub mask: u8,
}

fn default_pb_mask() -> u8 {
    0x0F
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderMetadata {
    pub name: String,
    pub version: i32,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub inputs: Vec<ShaderInputMeta>,
    pub parameters: Vec<ShaderParamMeta>,
    #[serde(default)]
    pub op: String,
}

#[derive(Debug, Clone)]
enum PbjInst {
    LoadConst {
        dst_reg: u16,
        dst_mask: u8,
        value: f32,
    },
    Sample {
        dst_reg: u16,
        dst_mask: u8,
        coord_reg: u16,
        coord_swz: [u8; 2],
        tex_index: u8,
        linear: bool,
    },
    Alu {
        op: u8,
        dst_reg: u16,
        dst_mask: u8,
        dst_len: u8,
        src_reg: u16,
        src_swz: [u8; 4],
    },
    If {
        src_reg: u16,
        src_swz: [u8; 4],
    },
    Else,
    EndIf,
}

#[derive(Debug, Clone)]
struct ParsedPbj {
    meta: ShaderMetadata,
    out_coord_reg: u16,
    out_coord_mask: u8,
    out_pixel_reg: u16,
    out_pixel_mask: u8,
    out_channels: u32,
    code: Vec<PbjInst>,
}

fn pb_type_name_and_len(code: u8) -> Option<(&'static str, usize)> {
    match code {
        0x01 => Some(("float", 1)),
        0x02 => Some(("float2", 2)),
        0x03 => Some(("float3", 3)),
        0x04 => Some(("float4", 4)),
        0x05 => Some(("matrix2x2", 4)),
        0x06 => Some(("matrix3x3", 9)),
        0x07 => Some(("matrix4x4", 16)),
        0x08 => Some(("int", 1)),
        0x09 => Some(("int2", 2)),
        0x0A => Some(("int3", 3)),
        0x0B => Some(("int4", 4)),
        0x0C => Some(("bool", 1)),
        0x0D => Some(("bool2", 2)),
        0x0E => Some(("bool3", 3)),
        0x0F => Some(("bool4", 4)),
        _ => None,
    }
}

fn read_cstring(bytes: &[u8], pos: &mut usize) -> Result<String, Fte3dError> {
    let start = *pos;
    while *pos < bytes.len() && bytes[*pos] != 0 {
        *pos += 1;
    }
    if *pos >= bytes.len() {
        return Err(Fte3dError::Argument(
            "Invalid Shader byteCode: unterminated string in Pixel Bender stream.".into(),
        ));
    }
    let s = String::from_utf8_lossy(&bytes[start..*pos]).into_owned();
    *pos += 1;
    Ok(s)
}

fn read_pb_typed_value(
    bytes: &[u8],
    pos: &mut usize,
    pb_type: u8,
) -> Result<serde_json::Value, Fte3dError> {
    if pb_type == 0x34 {
        let s = read_cstring(bytes, pos)?;
        return Ok(serde_json::Value::String(s));
    }
    let Some((type_name, count)) = pb_type_name_and_len(pb_type) else {
        return Err(Fte3dError::Argument(format!(
            "Invalid Shader byteCode: unsupported metadata value type 0x{pb_type:02X}."
        )));
    };
    let mut vals = Vec::with_capacity(count);
    for _ in 0..count {
        if type_name.starts_with("int") || type_name.starts_with("bool") {
            if *pos + 2 > bytes.len() {
                return Err(Fte3dError::Argument(
                    "Invalid Shader byteCode: truncated int/bool metadata.".into(),
                ));
            }
            let v = i16::from_be_bytes([bytes[*pos], bytes[*pos + 1]]);
            *pos += 2;
            if type_name.starts_with("bool") {
                vals.push(serde_json::Value::Bool(v != 0));
            } else {
                vals.push(serde_json::json!(v));
            }
        } else {
            if *pos + 4 > bytes.len() {
                return Err(Fte3dError::Argument(
                    "Invalid Shader byteCode: truncated float metadata.".into(),
                ));
            }
            let bits = u32::from_be_bytes([
                bytes[*pos],
                bytes[*pos + 1],
                bytes[*pos + 2],
                bytes[*pos + 3],
            ]);
            *pos += 4;
            let f = f32::from_bits(bits) as f64;
            vals.push(serde_json::json!(f));
        }
    }
    if count == 1 {
        Ok(vals.into_iter().next().unwrap_or(serde_json::Value::Null))
    } else {
        Ok(serde_json::Value::Array(vals))
    }
}

fn parse_pbj_or_json(bytes: &[u8]) -> Result<ParsedPbj, Fte3dError> {
    if bytes.is_empty() {
        return Err(Fte3dError::Argument(
            "Shader.byteCode must not be empty.".into(),
        ));
    }
    // 1. Portable JSON shader descriptor (starts with '{' after optional whitespace)
    let trimmed = {
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        &bytes[i..]
    };
    if trimmed.first() == Some(&b'{') {
        let raw: serde_json::Value = serde_json::from_slice(trimmed).map_err(|e| {
            Fte3dError::Argument(format!("Invalid JSON Shader byteCode descriptor: {e}"))
        })?;
        let name = raw
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("NeoFlashShader")
            .to_string();
        let version = raw.get("version").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
        let op = raw
            .get("op")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();
        let mut metadata = BTreeMap::new();
        if let Some(obj) = raw.get("metadata").and_then(|v| v.as_object()) {
            for (k, v) in obj {
                metadata.insert(k.clone(), v.clone());
            }
        }
        let mut inputs = Vec::new();
        if let Some(arr) = raw.get("inputs").and_then(|v| v.as_array()) {
            for (idx, item) in arr.iter().enumerate() {
                let in_name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("src")
                    .to_string();
                let index = item
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(idx as u64) as u32;
                let channels = item
                    .get("channels")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(4)
                    .clamp(1, 4) as u32;
                inputs.push(ShaderInputMeta {
                    name: in_name,
                    index,
                    channels,
                });
            }
        }
        let mut parameters = Vec::new();
        if let Some(arr) = raw.get("parameters").and_then(|v| v.as_array()) {
            for (idx, item) in arr.iter().enumerate() {
                let p_name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("param")
                    .to_string();
                let p_type = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("float")
                    .to_string();
                let comp_len = shader_param_component_count(&p_type)?;
                let index = item
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(idx as u64) as u32;
                let mut default_value = Vec::new();
                if let Some(dv) = item.get("defaultValue").and_then(|v| v.as_array()) {
                    for x in dv {
                        if let Some(f) = x.as_f64() {
                            default_value.push(f);
                        } else if let Some(b) = x.as_bool() {
                            default_value.push(if b { 1.0 } else { 0.0 });
                        }
                    }
                }
                while default_value.len() < comp_len {
                    default_value.push(0.0);
                }
                default_value.truncate(comp_len);
                let mut p_meta = BTreeMap::new();
                if let Some(m) = item.get("metadata").and_then(|v| v.as_object()) {
                    for (k, v) in m {
                        p_meta.insert(k.clone(), v.clone());
                    }
                }
                parameters.push(ShaderParamMeta {
                    name: p_name,
                    index,
                    param_type: p_type,
                    default_value,
                    metadata: p_meta,
                    reg: (idx + 2) as u16,
                    mask: 0x0F,
                });
            }
        }
        let out_channels = raw
            .get("outChannels")
            .and_then(|v| v.as_u64())
            .unwrap_or(4)
            .clamp(1, 4) as u32;
        return Ok(ParsedPbj {
            meta: ShaderMetadata {
                name,
                version,
                metadata,
                inputs,
                parameters,
                op,
            },
            out_coord_reg: 0,
            out_coord_mask: 0x0C,
            out_pixel_reg: 1,
            out_pixel_mask: 0x0F,
            out_channels,
            code: Vec::new(),
        });
    }

    // 2. Binary Pixel Bender (.pbj) stream: must begin with 0xA5 version opcode.
    if bytes[0] != 0xA5 || bytes.len() < 5 {
        return Err(Fte3dError::Argument(
            "Invalid Shader byteCode: expected Pixel Bender (.pbj) header (0xA5) or JSON shader descriptor.".into(),
        ));
    }

    let mut pos = 0usize;
    let mut version = 1i32;
    let mut name = String::from("PixelBenderKernel");
    let mut metadata = BTreeMap::new();
    let mut inputs = Vec::new();
    let mut parameters: Vec<ShaderParamMeta> = Vec::new();
    let mut out_coord_reg = 0u16;
    let mut out_coord_mask = 0x0Cu8;
    let mut out_pixel_reg = 1u16;
    let mut out_pixel_mask = 0x0Fu8;
    let mut out_channels = 4u32;
    let mut code = Vec::new();
    let mut last_param_type: Option<u8> = None;

    while pos < bytes.len() {
        let op = bytes[pos];
        pos += 1;
        match op {
            0xA5 => {
                if pos + 4 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA5 version record.".into(),
                    ));
                }
                version = i32::from_le_bytes([
                    bytes[pos],
                    bytes[pos + 1],
                    bytes[pos + 2],
                    bytes[pos + 3],
                ]);
                pos += 4;
            }
            0xA4 => {
                if pos + 2 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA4 kernel name.".into(),
                    ));
                }
                let len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                pos += 2;
                if pos + len > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: kernel name exceeds stream length.".into(),
                    ));
                }
                name = String::from_utf8_lossy(&bytes[pos..pos + len]).into_owned();
                pos += len;
            }
            0xA0 => {
                if pos >= bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA0 metadata.".into(),
                    ));
                }
                let mtype = bytes[pos];
                pos += 1;
                let key = read_cstring(bytes, &mut pos)?;
                let val = read_pb_typed_value(bytes, &mut pos, mtype)?;
                metadata.insert(key, val);
            }
            0xA3 => {
                if pos + 2 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA3 texture input.".into(),
                    ));
                }
                let index = bytes[pos] as u32;
                let channels = (bytes[pos + 1] as u32).clamp(1, 4);
                pos += 2;
                let in_name = read_cstring(bytes, &mut pos)?;
                inputs.push(ShaderInputMeta {
                    name: in_name,
                    index,
                    channels,
                });
            }
            0xA1 => {
                if pos + 5 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA1 parameter.".into(),
                    ));
                }
                let qualifier = bytes[pos];
                let pb_type = bytes[pos + 1];
                let reg = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]);
                let mask = bytes[pos + 4];
                pos += 5;
                let p_name = read_cstring(bytes, &mut pos)?;
                let Some((type_name, comp_len)) = pb_type_name_and_len(pb_type) else {
                    return Err(Fte3dError::Argument(format!(
                        "Invalid Shader byteCode: unknown parameter type 0x{pb_type:02X}."
                    )));
                };
                if qualifier == 2 {
                    // Output pixel parameter (typically "dst")
                    out_pixel_reg = reg;
                    out_pixel_mask = if mask == 0 { 0x0F } else { mask };
                    out_channels = comp_len.clamp(1, 4) as u32;
                    last_param_type = None;
                } else if p_name == "_OutCoord" {
                    out_coord_reg = reg;
                    out_coord_mask = if mask == 0 { 0x0C } else { mask };
                    last_param_type = None;
                } else {
                    let index = parameters.len() as u32;
                    parameters.push(ShaderParamMeta {
                        name: p_name,
                        index,
                        param_type: type_name.to_string(),
                        default_value: vec![0.0; comp_len],
                        metadata: BTreeMap::new(),
                        reg,
                        mask: if mask == 0 { 0x0F } else { mask },
                    });
                    last_param_type = Some(pb_type);
                }
            }
            0xA2 => {
                if pos >= bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA2 parameter metadata.".into(),
                    ));
                }
                let mtype = bytes[pos];
                pos += 1;
                let key = read_cstring(bytes, &mut pos)?;
                let effective_type = if mtype == 0x34 {
                    0x34
                } else {
                    last_param_type.unwrap_or(mtype)
                };
                let val = read_pb_typed_value(bytes, &mut pos, effective_type)?;
                if let Some(last) = parameters.last_mut() {
                    if key == "defaultValue" {
                        if let Some(arr) = val.as_array() {
                            last.default_value = arr
                                .iter()
                                .map(|v| {
                                    v.as_f64()
                                        .or_else(|| v.as_bool().map(|b| if b { 1.0 } else { 0.0 }))
                                        .unwrap_or(0.0)
                                })
                                .collect();
                        } else if let Some(f) = val.as_f64() {
                            last.default_value = vec![f];
                        } else if let Some(b) = val.as_bool() {
                            last.default_value = vec![if b { 1.0 } else { 0.0 }];
                        }
                    }
                    last.metadata.insert(key, val);
                }
            }
            0x32 | 0x33 => {
                // 0x32 loadFloat / 0x33 loadInt immediate (7 bytes after opcode)
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated immediate instruction.".into(),
                    ));
                }
                let dst_reg = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                let dst_mask = bytes[pos + 2] >> 4;
                let value = if op == 0x33 {
                    i32::from_be_bytes([
                        bytes[pos + 3],
                        bytes[pos + 4],
                        bytes[pos + 5],
                        bytes[pos + 6],
                    ]) as f32
                } else {
                    f32::from_bits(u32::from_be_bytes([
                        bytes[pos + 3],
                        bytes[pos + 4],
                        bytes[pos + 5],
                        bytes[pos + 6],
                    ]))
                };
                pos += 7;
                code.push(PbjInst::LoadConst {
                    dst_reg,
                    dst_mask,
                    value,
                });
            }
            0x30 | 0x31 => {
                // sampleNearest / sampleLinear (7 bytes after opcode)
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated texture sample instruction.".into(),
                    ));
                }
                let dst_reg = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                let dst_mask = bytes[pos + 2] >> 4;
                let coord_reg = u16::from_le_bytes([bytes[pos + 3], bytes[pos + 4]]);
                let swz_byte = bytes[pos + 5];
                let coord_swz = [(swz_byte >> 6) & 3, (swz_byte >> 4) & 3];
                let tex_index = bytes[pos + 6];
                pos += 7;
                code.push(PbjInst::Sample {
                    dst_reg,
                    dst_mask,
                    coord_reg,
                    coord_swz,
                    tex_index,
                    linear: op == 0x31,
                });
            }
            0x00 => {
                if pos + 7 > bytes.len() {
                    break;
                }
                pos += 7;
            }
            0x01..=0x2F | 0x37..=0x3D => {
                // Standard 8-byte PBJ ALU / vector op
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(format!(
                        "Invalid Shader byteCode: truncated ALU instruction 0x{op:02X}."
                    )));
                }
                let dst_reg = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                let mask_len = bytes[pos + 2];
                let dst_mask = mask_len >> 4;
                let dst_len = ((mask_len >> 2) & 0x03) + 1;
                let src_reg = u16::from_le_bytes([bytes[pos + 3], bytes[pos + 4]]);
                let swz = bytes[pos + 5];
                let src_swz = [(swz >> 6) & 3, (swz >> 4) & 3, (swz >> 2) & 3, swz & 3];
                pos += 7;
                code.push(PbjInst::Alu {
                    op,
                    dst_reg,
                    dst_mask,
                    dst_len,
                    src_reg,
                    src_swz,
                });
            }
            0x34..=0x36 => {
                // Control-flow opcodes (0x34 if, 0x35 else, 0x36 endif) — 7 bytes after opcode
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated control-flow instruction.".into(),
                    ));
                }
                if op == 0x34 {
                    let src_reg = u16::from_le_bytes([bytes[pos + 3], bytes[pos + 4]]);
                    let swz = bytes[pos + 5];
                    let src_swz = [(swz >> 6) & 3, (swz >> 4) & 3, (swz >> 2) & 3, swz & 3];
                    code.push(PbjInst::If { src_reg, src_swz });
                } else if op == 0x35 {
                    code.push(PbjInst::Else);
                } else {
                    code.push(PbjInst::EndIf);
                }
                pos += 7;
            }
            other => {
                return Err(Fte3dError::Argument(format!(
                    "Invalid Shader byteCode: unknown Pixel Bender opcode 0x{other:02X}."
                )));
            }
        }
    }

    Ok(ParsedPbj {
        meta: ShaderMetadata {
            name,
            version,
            metadata,
            inputs,
            parameters,
            op: "pbj".into(),
        },
        out_coord_reg,
        out_coord_mask,
        out_pixel_reg,
        out_pixel_mask,
        out_channels,
        code,
    })
}

pub fn parse_shader_bytecode(bytes: &[u8]) -> Result<ShaderMetadata, Fte3dError> {
    Ok(parse_pbj_or_json(bytes)?.meta)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderRuntimeInput {
    pub name: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "default_four")]
    pub channels: u32,
    #[serde(default)]
    pub floats: Vec<f32>,
    #[serde(default)]
    pub rgba_base64: Option<String>,
}

fn default_four() -> u32 {
    4
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderEvalRequest {
    pub bytecode_base64: String,
    #[serde(default = "default_full_precision")]
    pub precision_hint: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub inputs: Vec<ShaderRuntimeInput>,
    #[serde(default)]
    pub parameters: BTreeMap<String, Vec<f64>>,
}

fn default_full_precision() -> String {
    "full".into()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderEvalResult {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub floats: Vec<f32>,
    pub rgba_base64: String,
}

#[derive(Debug, Clone)]
struct DecodedShaderInput {
    width: u32,
    height: u32,
    channels: u32,
    floats: Vec<f32>,
}

impl DecodedShaderInput {
    fn sample_nearest(&self, x: f32, y: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 || self.floats.is_empty() {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let ix = x.floor().clamp(0.0, (self.width - 1) as f32) as usize;
        let iy = y.floor().clamp(0.0, (self.height - 1) as f32) as usize;
        self.pixel_at(ix, iy)
    }

    fn sample_linear(&self, x: f32, y: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 || self.floats.is_empty() {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let fx = (x - 0.5).clamp(0.0, (self.width.saturating_sub(1)) as f32);
        let fy = (y - 0.5).clamp(0.0, (self.height.saturating_sub(1)) as f32);
        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = (x0 + 1).min(self.width as usize - 1);
        let y1 = (y0 + 1).min(self.height as usize - 1);
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let c00 = self.pixel_at(x0, y0);
        let c10 = self.pixel_at(x1, y0);
        let c01 = self.pixel_at(x0, y1);
        let c11 = self.pixel_at(x1, y1);
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let top = c00[i] * (1.0 - tx) + c10[i] * tx;
            let bot = c01[i] * (1.0 - tx) + c11[i] * tx;
            out[i] = top * (1.0 - ty) + bot * ty;
        }
        out
    }

    fn pixel_at(&self, x: usize, y: usize) -> [f32; 4] {
        let ch = self.channels.clamp(1, 4) as usize;
        let base = (y * self.width as usize + x) * ch;
        let mut out = [0.0, 0.0, 0.0, 1.0];
        for (i, dst) in out.iter_mut().enumerate().take(ch) {
            *dst = self.floats.get(base + i).copied().unwrap_or(0.0);
        }
        out
    }
}

fn pbj_mask_indices(mask: u8) -> Vec<usize> {
    // Pixel Bender register masks encode components MSB-first: 0x8 = R/X, 0x4 = G/Y, 0x2 = B/Z, 0x1 = A/W.
    let mut out = Vec::with_capacity(4);
    for (bit, comp) in [(0x08u8, 0usize), (0x04, 1), (0x02, 2), (0x01, 3)] {
        if (mask & bit) != 0 {
            out.push(comp);
        }
    }
    if out.is_empty() {
        vec![0, 1, 2, 3]
    } else {
        out
    }
}

fn write_pbj_reg(regs: &mut [[f32; 4]], reg: u16, mask: u8, vals: &[f32]) {
    let idx = reg as usize;
    if idx >= regs.len() {
        return;
    }
    let comps = pbj_mask_indices(mask);
    for (vi, &c) in comps.iter().enumerate() {
        if let Some(&v) = vals.get(vi).or_else(|| vals.last()) {
            regs[idx][c] = v;
        }
    }
}

fn read_pbj_reg_masked(regs: &[[f32; 4]], reg: u16, mask: u8) -> [f32; 4] {
    let idx = reg as usize;
    if idx >= regs.len() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let comps = pbj_mask_indices(mask);
    let mut out = [0.0, 0.0, 0.0, 1.0];
    for (i, &c) in comps.iter().take(4).enumerate() {
        out[i] = regs[idx][c];
    }
    out
}

pub fn evaluate_shader(req: &ShaderEvalRequest) -> Result<ShaderEvalResult, Fte3dError> {
    validate_shader_precision(&req.precision_hint)?;
    if req.width == 0 || req.height == 0 {
        return Err(Fte3dError::Argument(
            "ShaderJob width and height must be positive.".into(),
        ));
    }
    let pixel_count = (req.width as usize).saturating_mul(req.height as usize);
    if pixel_count > MAX_SHADER_PIXELS {
        return Err(Fte3dError::Argument(
            "ShaderJob dimensions exceed maximum pixel budget.".into(),
        ));
    }
    let bc = BASE64
        .decode(req.bytecode_base64.as_bytes())
        .map_err(|e| Fte3dError::Argument(format!("Invalid base64 Shader byteCode: {e}")))?;
    let parsed = parse_pbj_or_json(&bc)?;

    // Decode and validate required inputs.
    let mut decoded_inputs: Vec<DecodedShaderInput> = Vec::with_capacity(parsed.meta.inputs.len());
    for meta_in in &parsed.meta.inputs {
        let supplied = req
            .inputs
            .iter()
            .find(|i| i.name == meta_in.name)
            .ok_or_else(|| {
                Fte3dError::Argument(format!(
                    "ShaderInput '{}' must have an input assigned before starting ShaderJob.",
                    meta_in.name
                ))
            })?;
        if supplied.width == 0 || supplied.height == 0 {
            return Err(Fte3dError::Argument(format!(
                "ShaderInput '{}' must have positive width and height.",
                meta_in.name
            )));
        }
        let floats = if let Some(b64) = &supplied.rgba_base64 {
            let bytes = BASE64.decode(b64.as_bytes()).map_err(|e| {
                Fte3dError::Argument(format!(
                    "Invalid base64 RGBA for ShaderInput '{}': {e}",
                    meta_in.name
                ))
            })?;
            let expected = (supplied.width as usize) * (supplied.height as usize) * 4;
            if bytes.len() < expected {
                return Err(Fte3dError::Argument(format!(
                    "ShaderInput '{}' buffer has insufficient pixel data.",
                    meta_in.name
                )));
            }
            let ch = meta_in.channels.clamp(1, 4) as usize;
            let mut f =
                Vec::with_capacity((supplied.width as usize) * (supplied.height as usize) * ch);
            for px in bytes[..expected].chunks_exact(4) {
                let rgba = [
                    px[0] as f32 / 255.0,
                    px[1] as f32 / 255.0,
                    px[2] as f32 / 255.0,
                    px[3] as f32 / 255.0,
                ];
                f.extend_from_slice(&rgba[..ch]);
            }
            f
        } else {
            let ch = supplied.channels.max(meta_in.channels).clamp(1, 4) as usize;
            let needed = (supplied.width as usize) * (supplied.height as usize) * ch;
            if supplied.floats.len() < needed {
                return Err(Fte3dError::Argument(format!(
                    "ShaderInput '{}' float buffer is shorter than width*height*channels ({} < {}).",
                    meta_in.name,
                    supplied.floats.len(),
                    needed
                )));
            }
            supplied.floats[..needed].to_vec()
        };
        decoded_inputs.push(DecodedShaderInput {
            width: supplied.width,
            height: supplied.height,
            channels: if supplied.rgba_base64.is_some() {
                meta_in.channels.clamp(1, 4)
            } else {
                supplied.channels.max(meta_in.channels).clamp(1, 4)
            },
            floats,
        });
    }

    // Resolve parameter values (supplied override defaultValue).
    let mut param_map: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for p in &parsed.meta.parameters {
        let comp_len = shader_param_component_count(&p.param_type)?;
        let mut vals: Vec<f32> = if let Some(v) = req.parameters.get(&p.name) {
            v.iter().map(|&x| x as f32).collect()
        } else {
            p.default_value.iter().map(|&x| x as f32).collect()
        };
        while vals.len() < comp_len {
            vals.push(0.0);
        }
        vals.truncate(comp_len);
        param_map.insert(p.name.clone(), vals);
    }

    let out_ch = parsed.out_channels.clamp(1, 4) as usize;
    let mut out_floats = Vec::with_capacity(pixel_count * out_ch);
    let mut out_rgba = Vec::with_capacity(pixel_count * 4);

    let max_reg = parsed
        .meta
        .parameters
        .iter()
        .map(|p| p.reg as usize + 4)
        .chain(std::iter::once(parsed.out_coord_reg as usize + 1))
        .chain(std::iter::once(parsed.out_pixel_reg as usize + 1))
        .chain(parsed.code.iter().map(|inst| match inst {
            PbjInst::LoadConst { dst_reg, .. } => *dst_reg as usize + 1,
            PbjInst::Sample {
                dst_reg, coord_reg, ..
            } => (*dst_reg as usize).max(*coord_reg as usize) + 1,
            PbjInst::Alu {
                dst_reg, src_reg, ..
            } => (*dst_reg as usize).max(*src_reg as usize) + 1,
            PbjInst::If { src_reg, .. } => *src_reg as usize + 1,
            PbjInst::Else | PbjInst::EndIf => 0,
        }))
        .max()
        .unwrap_or(16)
        .clamp(16, 512);

    let fast = req.precision_hint == "fast";

    for y in 0..req.height {
        for x in 0..req.width {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let px_out = if !parsed.code.is_empty() {
                let mut regs = vec![[0.0f32; 4]; max_reg];
                write_pbj_reg(
                    &mut regs,
                    parsed.out_coord_reg,
                    parsed.out_coord_mask,
                    &[fx, fy],
                );
                for p in &parsed.meta.parameters {
                    if let Some(vals) = param_map.get(&p.name) {
                        if vals.len() <= 4 {
                            write_pbj_reg(&mut regs, p.reg, p.mask, vals);
                        } else {
                            for (row, chunk) in vals.chunks(4).enumerate() {
                                write_pbj_reg(&mut regs, p.reg + row as u16, 0x0F, chunk);
                            }
                        }
                    }
                }
                // Condition stack: (parent_active, branch_cond)
                let mut cond_stack: Vec<(bool, bool)> = Vec::new();
                for inst in &parsed.code {
                    let active = cond_stack.last().is_none_or(|&(p, c)| p && c);
                    match *inst {
                        PbjInst::If { src_reg, src_swz } => {
                            let cond = if active {
                                let s_raw = regs.get(src_reg as usize).copied().unwrap_or([0.0; 4]);
                                s_raw[src_swz[0] as usize].abs() > 1e-6
                            } else {
                                false
                            };
                            cond_stack.push((active, cond));
                        }
                        PbjInst::Else => {
                            if let Some((parent_act, cur_cond)) = cond_stack.last_mut() {
                                *cur_cond = *parent_act && !*cur_cond;
                            }
                        }
                        PbjInst::EndIf => {
                            cond_stack.pop();
                        }
                        PbjInst::LoadConst {
                            dst_reg,
                            dst_mask,
                            value,
                        } => {
                            if active {
                                write_pbj_reg(&mut regs, dst_reg, dst_mask, &[value]);
                            }
                        }
                        PbjInst::Sample {
                            dst_reg,
                            dst_mask,
                            coord_reg,
                            coord_swz,
                            tex_index,
                            linear,
                        } => {
                            if !active {
                                continue;
                            }
                            let c_reg = regs
                                .get(coord_reg as usize)
                                .copied()
                                .unwrap_or([fx, fy, 0.0, 0.0]);
                            let sx = c_reg[coord_swz[0] as usize];
                            let sy = c_reg[coord_swz[1] as usize];
                            let sample = if let Some(inp) = decoded_inputs.get(tex_index as usize) {
                                if linear && !fast {
                                    inp.sample_linear(sx, sy)
                                } else {
                                    inp.sample_nearest(sx, sy)
                                }
                            } else {
                                [0.0, 0.0, 0.0, 0.0]
                            };
                            let comps = pbj_mask_indices(dst_mask);
                            let mut picked = Vec::with_capacity(comps.len());
                            for (i, _) in comps.iter().enumerate() {
                                picked.push(sample[i.min(3)]);
                            }
                            write_pbj_reg(&mut regs, dst_reg, dst_mask, &picked);
                        }
                        PbjInst::Alu {
                            op,
                            dst_reg,
                            dst_mask,
                            dst_len,
                            src_reg,
                            src_swz,
                        } => {
                            if !active {
                                continue;
                            }
                            let d_idx = dst_reg as usize;
                            let s_idx = src_reg as usize;
                            if d_idx >= regs.len() || s_idx >= regs.len() {
                                continue;
                            }
                            let dst_comps = pbj_mask_indices(dst_mask);
                            let s_raw = regs[s_idx];
                            let s = [
                                s_raw[src_swz[0] as usize],
                                s_raw[src_swz[1] as usize],
                                s_raw[src_swz[2] as usize],
                                s_raw[src_swz[3] as usize],
                            ];
                            let d_cur: Vec<f32> =
                                dst_comps.iter().map(|&c| regs[d_idx][c]).collect();
                            let n = (dst_len as usize).min(dst_comps.len()).max(1);
                            // Vector reduction & cross/normalize opcodes
                            if matches!(op, 0x26 | 0x27 | 0x37..=0x3D) {
                                let vec_len = (dst_len as usize).clamp(1, 4);
                                let d_full = regs[d_idx];
                                let res_vec: Vec<f32> = match op {
                                    0x26 => {
                                        // any(src)
                                        let any_true = (0..vec_len).any(|i| s[i].abs() > 1e-6);
                                        vec![if any_true { 1.0 } else { 0.0 }]
                                    }
                                    0x27 => {
                                        // all(src)
                                        let all_true = (0..vec_len).all(|i| s[i].abs() > 1e-6);
                                        vec![if all_true { 1.0 } else { 0.0 }]
                                    }
                                    0x38 => {
                                        // dot(dst, src)
                                        let mut acc = 0.0f32;
                                        for i in 0..vec_len {
                                            let a = d_cur.get(i).copied().unwrap_or(d_full[i]);
                                            acc += a * s[i];
                                        }
                                        vec![acc]
                                    }
                                    0x39 => {
                                        // cross(dst, src)
                                        let a0 = d_cur.first().copied().unwrap_or(d_full[0]);
                                        let a1 = d_cur.get(1).copied().unwrap_or(d_full[1]);
                                        let a2 = d_cur.get(2).copied().unwrap_or(d_full[2]);
                                        vec![
                                            a1 * s[2] - a2 * s[1],
                                            a2 * s[0] - a0 * s[2],
                                            a0 * s[1] - a1 * s[0],
                                        ]
                                    }
                                    0x3A => {
                                        // length(src)
                                        let sq: f32 = (0..vec_len).map(|i| s[i] * s[i]).sum();
                                        vec![sq.sqrt()]
                                    }
                                    0x3B => {
                                        // distance(dst, src)
                                        let mut sq = 0.0f32;
                                        for i in 0..vec_len {
                                            let a = d_cur.get(i).copied().unwrap_or(d_full[i]);
                                            let diff = a - s[i];
                                            sq += diff * diff;
                                        }
                                        vec![sq.sqrt()]
                                    }
                                    0x3C => {
                                        // normalize(src)
                                        let sq: f32 = (0..vec_len).map(|i| s[i] * s[i]).sum();
                                        let len = sq.sqrt().max(1e-12);
                                        (0..n).map(|i| s[i.min(3)] / len).collect()
                                    }
                                    _ => (0..n).map(|i| s[i.min(3)]).collect(),
                                };
                                write_pbj_reg(&mut regs, dst_reg, dst_mask, &res_vec);
                                continue;
                            }
                            let mut res = d_cur.clone();
                            for i in 0..n {
                                let a = d_cur[i];
                                let b = s[i.min(3)];
                                res[i] = match op {
                                    0x01 => a + b,
                                    0x02 => a - b,
                                    0x03 => a * b,
                                    0x04 => {
                                        if b.abs() > 1e-12 {
                                            1.0 / b
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x05 => {
                                        if b.abs() > 1e-12 {
                                            a / b
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x06 => a.atan2(b),
                                    0x07 => a.max(0.0).powf(b),
                                    0x08 => {
                                        if b.abs() > 1e-12 {
                                            a % b
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x09 => a.min(b),
                                    0x0A => a.max(b),
                                    0x0B => {
                                        if b < a {
                                            0.0
                                        } else {
                                            1.0
                                        }
                                    }
                                    0x0C => b.sin(),
                                    0x0D => b.cos(),
                                    0x0E => b.tan(),
                                    0x0F => b.clamp(-1.0, 1.0).asin(),
                                    0x10 => b.clamp(-1.0, 1.0).acos(),
                                    0x11 => b.atan(),
                                    0x12 => b.exp(),
                                    0x13 => b.exp2(),
                                    0x14 => b.max(1e-12).ln(),
                                    0x15 => b.max(1e-12).log2(),
                                    0x16 => b.max(0.0).sqrt(),
                                    0x17 => {
                                        let r = b.max(1e-12).sqrt();
                                        1.0 / r
                                    }
                                    0x18 => b.abs(),
                                    0x19 => {
                                        if b > 0.0 {
                                            1.0
                                        } else if b < 0.0 {
                                            -1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x1A => b.floor(),
                                    0x1B => b.ceil(),
                                    0x1C => b - b.floor(),
                                    0x1D => b,
                                    0x1E => b.trunc(),
                                    0x1F => b,
                                    0x20 => -b,
                                    0x21..=0x23 => {
                                        if b.abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x24 | 0x28 => {
                                        if (a - b).abs() <= 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x25 | 0x29 => {
                                        if (a - b).abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2A => {
                                        if a < b {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2B => {
                                        if a <= b {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2C => {
                                        if b.abs() <= 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2D => {
                                        if a.abs() > 1e-6 && b.abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2E => {
                                        if a.abs() > 1e-6 || b.abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2F => {
                                        if (a.abs() > 1e-6) != (b.abs() > 1e-6) {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    _ => b,
                                };
                            }
                            write_pbj_reg(&mut regs, dst_reg, dst_mask, &res);
                        }
                    }
                }
                read_pbj_reg_masked(&regs, parsed.out_pixel_reg, parsed.out_pixel_mask)
            } else {
                evaluate_portable_shader_pixel(
                    &parsed.meta.op,
                    fx,
                    fy,
                    req.width as f32,
                    req.height as f32,
                    &decoded_inputs,
                    &param_map,
                )
            };

            for &v in px_out.iter().take(out_ch) {
                out_floats.push(if v.is_finite() { v } else { 0.0 });
            }
            let rgba = match out_ch {
                1 => [px_out[0], px_out[0], px_out[0], 1.0],
                2 => [px_out[0], px_out[1], 0.0, 1.0],
                3 => [px_out[0], px_out[1], px_out[2], 1.0],
                _ => px_out,
            };
            for c in rgba {
                let cl = if c.is_finite() {
                    c.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                out_rgba.push((cl * 255.0).round() as u8);
            }
        }
    }

    Ok(ShaderEvalResult {
        width: req.width,
        height: req.height,
        channels: out_ch as u32,
        floats: out_floats,
        rgba_base64: BASE64.encode(&out_rgba),
    })
}

fn evaluate_portable_shader_pixel(
    op: &str,
    fx: f32,
    fy: f32,
    width: f32,
    height: f32,
    inputs: &[DecodedShaderInput],
    params: &BTreeMap<String, Vec<f32>>,
) -> [f32; 4] {
    let s0 = inputs.first().map(|i| i.sample_nearest(fx, fy)).unwrap_or([
        fx / width.max(1.0),
        fy / height.max(1.0),
        0.5,
        1.0,
    ]);
    let s1 = inputs.get(1).map(|i| i.sample_nearest(fx, fy));
    let get_p = |key: &str, def: &[f32]| -> Vec<f32> {
        params.get(key).cloned().unwrap_or_else(|| def.to_vec())
    };

    match op {
        "invert" => {
            let amt = get_p("amount", &[1.0])[0].clamp(0.0, 1.0);
            [
                s0[0] * (1.0 - amt) + (1.0 - s0[0]) * amt,
                s0[1] * (1.0 - amt) + (1.0 - s0[1]) * amt,
                s0[2] * (1.0 - amt) + (1.0 - s0[2]) * amt,
                s0[3],
            ]
        }
        "tint" | "multiply" => {
            let c = params
                .get("color")
                .or_else(|| params.get("tint"))
                .or_else(|| params.get("factor"))
                .cloned()
                .unwrap_or_else(|| vec![1.0, 1.0, 1.0, 1.0]);
            let r = c.first().copied().unwrap_or(1.0);
            let g = c.get(1).copied().unwrap_or(r);
            let b = c.get(2).copied().unwrap_or(r);
            let a = c.get(3).copied().unwrap_or(1.0);
            if let Some(other) = s1 {
                [
                    s0[0] * other[0] * r,
                    s0[1] * other[1] * g,
                    s0[2] * other[2] * b,
                    s0[3] * other[3] * a,
                ]
            } else {
                [s0[0] * r, s0[1] * g, s0[2] * b, s0[3] * a]
            }
        }
        "add" | "brightness" => {
            let off = params
                .get("offset")
                .or_else(|| params.get("brightness"))
                .or_else(|| params.get("amount"))
                .cloned()
                .unwrap_or_else(|| vec![0.0]);
            let dr = off.first().copied().unwrap_or(0.0);
            let dg = off.get(1).copied().unwrap_or(dr);
            let db = off.get(2).copied().unwrap_or(dr);
            let da = off.get(3).copied().unwrap_or(0.0);
            if let Some(other) = s1 {
                [
                    s0[0] + other[0] + dr,
                    s0[1] + other[1] + dg,
                    s0[2] + other[2] + db,
                    (s0[3] + other[3] + da).clamp(0.0, 1.0),
                ]
            } else {
                [s0[0] + dr, s0[1] + dg, s0[2] + db, s0[3] + da]
            }
        }
        "mix" | "blend" => {
            let other = s1.unwrap_or([1.0 - s0[0], 1.0 - s0[1], 1.0 - s0[2], s0[3]]);
            let t = get_p("amount", &[0.5])[0].clamp(0.0, 1.0);
            [
                s0[0] * (1.0 - t) + other[0] * t,
                s0[1] * (1.0 - t) + other[1] * t,
                s0[2] * (1.0 - t) + other[2] * t,
                s0[3] * (1.0 - t) + other[3] * t,
            ]
        }
        "threshold" => {
            let th = get_p("threshold", &[0.5])[0];
            let lum = 0.2126 * s0[0] + 0.7152 * s0[1] + 0.0722 * s0[2];
            let v = if lum >= th { 1.0 } else { 0.0 };
            [v, v, v, s0[3]]
        }
        "gradient" => {
            let c0 = get_p("startColor", &[0.0, 0.0, 0.0, 1.0]);
            let c1 = get_p("endColor", &[1.0, 1.0, 1.0, 1.0]);
            let t = (fx / width.max(1.0)).clamp(0.0, 1.0);
            [
                c0.first().copied().unwrap_or(0.0) * (1.0 - t)
                    + c1.first().copied().unwrap_or(1.0) * t,
                c0.get(1).copied().unwrap_or(0.0) * (1.0 - t)
                    + c1.get(1).copied().unwrap_or(1.0) * t,
                c0.get(2).copied().unwrap_or(0.0) * (1.0 - t)
                    + c1.get(2).copied().unwrap_or(1.0) * t,
                c0.get(3).copied().unwrap_or(1.0) * (1.0 - t)
                    + c1.get(3).copied().unwrap_or(1.0) * t,
            ]
        }
        "solid" | "fill" => {
            let c = get_p("color", &[1.0, 0.0, 1.0, 1.0]);
            [
                c.first().copied().unwrap_or(1.0),
                c.get(1).copied().unwrap_or(0.0),
                c.get(2).copied().unwrap_or(1.0),
                c.get(3).copied().unwrap_or(1.0),
            ]
        }
        _ => {
            // Auto mode: if a 3/4-component color parameter exists, modulate s0; if a scalar amount/brightness exists, apply it; otherwise return s0.
            if let Some(c) = params.get("color").or_else(|| params.get("tint")) {
                return [
                    s0[0] * c.first().copied().unwrap_or(1.0),
                    s0[1] * c.get(1).copied().unwrap_or(1.0),
                    s0[2] * c.get(2).copied().unwrap_or(1.0),
                    s0[3] * c.get(3).copied().unwrap_or(1.0),
                ];
            }
            if let Some(m) = params.get("matrix")
                && m.len() == 16
            {
                return [
                    s0[0] * m[0] + s0[1] * m[1] + s0[2] * m[2] + s0[3] * m[3],
                    s0[0] * m[4] + s0[1] * m[5] + s0[2] * m[6] + s0[3] * m[7],
                    s0[0] * m[8] + s0[1] * m[9] + s0[2] * m[10] + s0[3] * m[11],
                    s0[0] * m[12] + s0[1] * m[13] + s0[2] * m[14] + s0[3] * m[15],
                ];
            }
            if let Some((_, first_param)) = params.iter().next()
                && inputs.is_empty()
            {
                let r = first_param.first().copied().unwrap_or(s0[0]);
                let g = first_param.get(1).copied().unwrap_or(r);
                let b = first_param.get(2).copied().unwrap_or(r);
                let a = first_param.get(3).copied().unwrap_or(1.0);
                return [r, g, b, a];
            }
            if let Some(other) = s1 {
                return [
                    (s0[0] + other[0]) * 0.5,
                    (s0[1] + other[1]) * 0.5,
                    (s0[2] + other[2]) * 0.5,
                    (s0[3] + other[3]) * 0.5,
                ];
            }
            s0
        }
    }
}

pub fn evaluate_shader_filter_rgba(
    src: &[u8],
    width: u32,
    height: u32,
    bytecode_base64: &str,
    precision_hint: &str,
    params_json: &str,
) -> Vec<u8> {
    if width == 0 || height == 0 || bytecode_base64.is_empty() {
        return src.to_vec();
    }
    let Ok(bc) = BASE64.decode(bytecode_base64.as_bytes()) else {
        return src.to_vec();
    };
    let Ok(meta) = parse_shader_bytecode(&bc) else {
        return src.to_vec();
    };
    let raw_json: serde_json::Value =
        serde_json::from_str(params_json).unwrap_or(serde_json::Value::Null);
    let mut parameters: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut extra_inputs: BTreeMap<String, ShaderRuntimeInput> = BTreeMap::new();
    if let Some(obj) = raw_json.as_object() {
        for (k, v) in obj {
            if k == "__extra_inputs__" {
                if let Some(arr) = v.as_array() {
                    for item in arr {
                        if let Ok(inp) = serde_json::from_value::<ShaderRuntimeInput>(item.clone())
                        {
                            extra_inputs.insert(inp.name.clone(), inp);
                        }
                    }
                }
            } else if let Some(arr) = v.as_array() {
                let vals: Vec<f64> = arr.iter().filter_map(|x| x.as_f64()).collect();
                parameters.insert(k.clone(), vals);
            }
        }
    }
    let src_b64 = BASE64.encode(src);
    let mut inputs = Vec::new();
    if let Some(first_in) = meta.inputs.first() {
        inputs.push(ShaderRuntimeInput {
            name: first_in.name.clone(),
            width,
            height,
            channels: first_in.channels,
            floats: Vec::new(),
            rgba_base64: Some(src_b64),
        });
    }
    // Use pre-bound secondary ShaderInputs from __extra_inputs__ when provided, falling back to src.
    for extra in meta.inputs.iter().skip(1) {
        if let Some(supplied) = extra_inputs.remove(&extra.name) {
            inputs.push(supplied);
        } else {
            inputs.push(ShaderRuntimeInput {
                name: extra.name.clone(),
                width,
                height,
                channels: extra.channels,
                floats: Vec::new(),
                rgba_base64: Some(BASE64.encode(src)),
            });
        }
    }
    let req = ShaderEvalRequest {
        bytecode_base64: bytecode_base64.to_string(),
        precision_hint: if matches!(precision_hint, "full" | "fast") {
            precision_hint.to_string()
        } else {
            "full".into()
        },
        width,
        height,
        inputs,
        parameters,
    };
    match evaluate_shader(&req) {
        Ok(res) => BASE64
            .decode(res.rgba_base64.as_bytes())
            .unwrap_or_else(|_| src.to_vec()),
        Err(_) => src.to_vec(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwfHeaderInfo {
    pub swf_version: u32,
    pub action_script_version: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub frame_count: u32,
    pub background_color: Option<u32>,
}

pub fn inspect_swf_bytes(bytes: &[u8]) -> Result<SwfHeaderInfo, Fte3dError> {
    if bytes.len() < 8 {
        return Err(Fte3dError::Argument(
            "SWF header is shorter than 8 bytes.".into(),
        ));
    }
    let sig = &bytes[0..3];
    if sig != b"FWS" && sig != b"CWS" && sig != b"ZWS" {
        return Err(Fte3dError::Argument(
            "Not a valid SWF header signature.".into(),
        ));
    }
    let swf_version = bytes[3] as u32;
    let _file_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let body: Vec<u8> = if sig == b"FWS" {
        bytes[8..].to_vec()
    } else if sig == b"CWS" {
        use std::io::Read;
        let mut decoder = flate2::read::ZlibDecoder::new(&bytes[8..]);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| Fte3dError::Argument(format!("Invalid CWS zlib stream: {e}")))?;
        out
    } else {
        return Err(Fte3dError::Argument(
            "LZMA-compressed ZWS SWF requires an external LZMA stream.".into(),
        ));
    };

    if body.len() < 5 {
        return Err(Fte3dError::Argument("Truncated SWF RECT header.".into()));
    }
    let nbits = (body[0] >> 3) as usize;
    let total_rect_bits = 5 + nbits * 4;
    let rect_bytes = total_rect_bits.div_ceil(8);
    if body.len() < rect_bytes + 4 {
        return Err(Fte3dError::Argument(
            "Truncated SWF frame rate / count header.".into(),
        ));
    }
    let read_sbits = |start_bit: usize, count: usize| -> i32 {
        if count == 0 {
            return 0;
        }
        let mut val = 0u32;
        for i in 0..count {
            let bit_idx = start_bit + i;
            let b = body[bit_idx / 8];
            let bit = (b >> (7 - (bit_idx % 8))) & 1;
            val = (val << 1) | (bit as u32);
        }
        let sign_bit = 1u32 << (count - 1);
        if (val & sign_bit) != 0 {
            (val | (!0u32 << count)) as i32
        } else {
            val as i32
        }
    };
    let xmin = read_sbits(5, nbits);
    let xmax = read_sbits(5 + nbits, nbits);
    let ymin = read_sbits(5 + nbits * 2, nbits);
    let ymax = read_sbits(5 + nbits * 3, nbits);
    let width = ((xmax - xmin).max(20) / 20) as u32;
    let height = ((ymax - ymin).max(20) / 20) as u32;

    let fr_raw = u16::from_le_bytes([body[rect_bytes], body[rect_bytes + 1]]);
    let frame_rate = ((fr_raw as f64) / 256.0).max(1.0);
    let frame_count =
        u16::from_le_bytes([body[rect_bytes + 2], body[rect_bytes + 3]]).max(1) as u32;

    let mut pos = rect_bytes + 4;
    let mut action_script_version = if swf_version >= 9 { 3u32 } else { 2u32 };
    let mut saw_file_attributes = false;
    let mut background_color = None;

    while pos + 2 <= body.len() {
        let tag_code_and_len = u16::from_le_bytes([body[pos], body[pos + 1]]);
        pos += 2;
        let tag_code = tag_code_and_len >> 6;
        let mut tag_len = (tag_code_and_len & 0x3F) as usize;
        if tag_len == 0x3F {
            if pos + 4 > body.len() {
                break;
            }
            tag_len = u32::from_le_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]])
                as usize;
            pos += 4;
        }
        if pos + tag_len > body.len() {
            break;
        }
        if tag_code == 0 {
            break;
        }
        if tag_code == 69 && tag_len >= 1 {
            // FileAttributes tag: bit 3 (0x08) is ActionScript3
            saw_file_attributes = true;
            let flags = body[pos];
            action_script_version = if (flags & 0x08) != 0 { 3 } else { 2 };
        } else if tag_code == 9 && tag_len >= 3 {
            // SetBackgroundColor tag
            background_color = Some(
                ((body[pos] as u32) << 16) | ((body[pos + 1] as u32) << 8) | (body[pos + 2] as u32),
            );
        }
        pos += tag_len;
    }
    if !saw_file_attributes && swf_version < 9 {
        action_script_version = 2;
    }

    Ok(SwfHeaderInfo {
        swf_version,
        action_script_version,
        width,
        height,
        frame_rate,
        frame_count,
        background_color,
    })
}

// ============================================================================
// 4. 3D DisplayObject & Transform Algebra + Top-Level Fte3dRuntime
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display3dPose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub scale_z: f64,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
}

impl Default for Display3dPose {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            scale_z: 1.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
        }
    }
}

pub fn compose_display_matrix3d(pose: &Display3dPose) -> [f64; 16] {
    let rx = pose.rotation_x.to_radians();
    let ry = pose.rotation_y.to_radians();
    let rz = pose.rotation_z.to_radians();
    let (sx, cx) = rx.sin_cos();
    let (sy, cy) = ry.sin_cos();
    let (sz, cz) = rz.sin_cos();
    let b00 = cz * cy;
    let b01 = sz * cy;
    let b02 = -sy;
    let b10 = cz * sy * sx - sz * cx;
    let b11 = sz * sy * sx + cz * cx;
    let b12 = cy * sx;
    let b20 = cz * sy * cx + sz * sx;
    let b21 = sz * sy * cx - cz * sx;
    let b22 = cy * cx;
    [
        b00 * pose.scale_x,
        b01 * pose.scale_x,
        b02 * pose.scale_x,
        0.0,
        b10 * pose.scale_y,
        b11 * pose.scale_y,
        b12 * pose.scale_y,
        0.0,
        b20 * pose.scale_z,
        b21 * pose.scale_z,
        b22 * pose.scale_z,
        0.0,
        pose.x,
        pose.y,
        pose.z,
        1.0,
    ]
}

pub fn decompose_display_matrix3d(raw: &[f64; 16]) -> Display3dPose {
    let m = crate::as3_geom::Matrix3D {
        raw_data: raw.map(|v| v as f32),
    };
    let mut gv = crate::as3_geom::GeometryValue::Matrix(m);
    if let Ok(val) = gv.call("decompose", &serde_json::json!(["eulerAngles"]))
        && let Some(arr) = val.get("data").and_then(|d| d.as_array())
        && arr.len() == 3
    {
        let p = arr[0].as_array();
        let r = arr[1].as_array();
        let s = arr[2].as_array();
        if let (Some(p), Some(r), Some(s)) = (p, r, s) {
            let g = |a: &[serde_json::Value], i: usize, def: f64| {
                a.get(i).and_then(|v| v.as_f64()).unwrap_or(def)
            };
            return Display3dPose {
                x: g(p, 0, raw[12]),
                y: g(p, 1, raw[13]),
                z: g(p, 2, raw[14]),
                rotation_x: g(r, 0, 0.0).to_degrees(),
                rotation_y: g(r, 1, 0.0).to_degrees(),
                rotation_z: g(r, 2, 0.0).to_degrees(),
                scale_x: g(s, 0, 1.0),
                scale_y: g(s, 1, 1.0),
                scale_z: g(s, 2, 1.0),
            };
        }
    }
    Display3dPose {
        x: raw[12],
        y: raw[13],
        z: raw[14],
        ..Display3dPose::default()
    }
}

pub fn multiply_matrix3d_chain(chain_root_to_leaf: &[[f64; 16]]) -> [f64; 16] {
    let mut acc = crate::as3_geom::Matrix3D::IDENTITY;
    for raw in chain_root_to_leaf {
        let next = crate::as3_geom::Matrix3D {
            raw_data: raw.map(|v| v as f32),
        };
        acc = acc.multiply(&next);
    }
    acc.raw_data.map(|v| v as f64)
}

pub fn relative_matrix_3d(
    target_chain_root_to_leaf: &[[f64; 16]],
    relative_to_chain_root_to_leaf: &[[f64; 16]],
) -> [f64; 16] {
    let target_world = crate::as3_geom::Matrix3D {
        raw_data: multiply_matrix3d_chain(target_chain_root_to_leaf).map(|v| v as f32),
    };
    let rel_world = crate::as3_geom::Matrix3D {
        raw_data: multiply_matrix3d_chain(relative_to_chain_root_to_leaf).map(|v| v as f32),
    };
    let rel_inv = rel_world
        .invert()
        .unwrap_or(crate::as3_geom::Matrix3D::IDENTITY);
    rel_inv.multiply(&target_world).raw_data.map(|v| v as f64)
}

pub fn local3d_to_global(
    chain_root_to_leaf: &[[f64; 16]],
    point3d: [f64; 3],
    focal_length: f64,
    projection_center: [f64; 2],
) -> [f64; 2] {
    let m = multiply_matrix3d_chain(chain_root_to_leaf);
    let [lx, ly, lz] = point3d;
    let wx = m[0] * lx + m[4] * ly + m[8] * lz + m[12];
    let wy = m[1] * lx + m[5] * ly + m[9] * lz + m[13];
    let wz = m[2] * lx + m[6] * ly + m[10] * lz + m[14];
    if focal_length > 1e-6 {
        let denom = focal_length + wz;
        let scale = if denom.abs() > 1e-6 {
            focal_length / denom
        } else {
            1.0
        };
        [
            projection_center[0] + (wx - projection_center[0]) * scale,
            projection_center[1] + (wy - projection_center[1]) * scale,
        ]
    } else {
        [wx, wy]
    }
}

pub fn global_to_local3d(
    chain_root_to_leaf: &[[f64; 16]],
    point2d: [f64; 2],
    focal_length: f64,
    projection_center: [f64; 2],
) -> [f64; 3] {
    let m = multiply_matrix3d_chain(chain_root_to_leaf);
    let [sx, sy] = point2d;
    let [cx, cy] = projection_center;
    if focal_length > 1e-6 {
        // Intersect perspective ray through (sx, sy) with local z = 0 plane:
        // wx - cx = u * (f + wz), wy - cy = v * (f + wz), where u = (sx - cx)/f, v = (sy - cy)/f.
        let u = (sx - cx) / focal_length;
        let v = (sy - cy) / focal_length;
        let a11 = m[0] - u * m[2];
        let a12 = m[4] - u * m[6];
        let b1 = cx + u * (focal_length + m[14]) - m[12];
        let a21 = m[1] - v * m[2];
        let a22 = m[5] - v * m[6];
        let b2 = cy + v * (focal_length + m[14]) - m[13];
        let det = a11 * a22 - a12 * a21;
        if det.abs() > 1e-9 {
            let lx = (b1 * a22 - a12 * b2) / det;
            let ly = (a11 * b2 - b1 * a21) / det;
            return [lx, ly, 0.0];
        }
    }
    let world = crate::as3_geom::Matrix3D {
        raw_data: m.map(|v| v as f32),
    };
    if let Some(inv) = world.invert() {
        let r = inv.raw_data.map(|v| v as f64);
        [
            r[0] * sx + r[4] * sy + r[12],
            r[1] * sx + r[5] * sy + r[13],
            r[2] * sx + r[6] * sy + r[14],
        ]
    } else {
        [sx - m[12], sy - m[13], 0.0]
    }
}

#[derive(Debug, Default)]
pub struct Fte3dRuntime {
    pub stage3d: Stage3dArena,
}

impl Fte3dRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.stage3d = Stage3dArena::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fte_paragraph_layout_justification_tabs_and_boundaries() {
        let req = FteLayoutRequest {
            runs: vec![
                FteInputElementRun {
                    element_id: 1,
                    is_graphic: false,
                    text: "Hello\tworld from FTE!".into(),
                    graphic_width: 0.0,
                    graphic_height: 0.0,
                    font_name: "_sans".into(),
                    font_size: 16.0,
                    font_weight: "normal".into(),
                    font_posture: "normal".into(),
                    color: 0x112233,
                    alpha: 1.0,
                    tracking_left: 0.0,
                    tracking_right: 0.0,
                    baseline_shift: 0.0,
                    text_rotation: "auto".into(),
                    break_opportunity: "auto".into(),
                    digit_width: "default".into(),
                    typographic_case: "default".into(),
                    has_mirror: true,
                },
                FteInputElementRun {
                    element_id: 2,
                    is_graphic: true,
                    text: FTE_GRAPHIC_CHAR.to_string(),
                    graphic_width: 24.0,
                    graphic_height: 18.0,
                    font_name: "_sans".into(),
                    font_size: 12.0,
                    font_weight: "normal".into(),
                    font_posture: "normal".into(),
                    color: 0,
                    alpha: 1.0,
                    tracking_left: 0.0,
                    tracking_right: 0.0,
                    baseline_shift: 0.0,
                    text_rotation: "auto".into(),
                    break_opportunity: "auto".into(),
                    digit_width: "default".into(),
                    typographic_case: "default".into(),
                    has_mirror: false,
                },
            ],
            start_char: 0,
            max_width: 140.0,
            line_offset: 0.0,
            fit_something: false,
            tab_stops: vec![FteTabStopSpec {
                alignment: "start".into(),
                position: 72.0,
                decimal_alignment_token: String::new(),
            }],
            justifier: FteJustifierSpec {
                kind: "space".into(),
                locale: "en".into(),
                line_justification: "allIncludingLast".into(),
                letter_spacing: true,
                minimum_spacing: 0.5,
                optimum_spacing: 1.0,
                maximum_spacing: 2.0,
                justification_style: "pushInKinsoku".into(),
                compose_trailing_ideographic_spaces: false,
            },
            baseline_font_size: 12.0,
            baseline_zero: "roman".into(),
            bidi_level: 0,
            line_rotation: "rotate0".into(),
            apply_non_linear_font_scaling: true,
        };
        let resp1 = layout_text_line(&req).expect("first line layout");
        assert_eq!(resp1.creation_result, "success");
        let line1 = resp1.line.expect("line1 some");
        assert!(line1.atoms.len() > 5);
        assert_eq!(line1.specified_width, 140.0);
        assert!(!line1.mirror_regions.is_empty());
        assert!(line1.dump.starts_with("<TextLine"));

        let next_start = line1.text_block_begin_index + line1.raw_text_length;
        let mut req2 = req.clone();
        req2.start_char = next_start;
        let resp2 = layout_text_line(&req2).expect("second line layout");
        let line2 = resp2.line.expect("line2 some");
        assert!(line2.has_graphic_element || line2.raw_text_length > 0);

        let full_text = format!("Hello\tworld from FTE!{}", FTE_GRAPHIC_CHAR);
        let next_word = find_next_word_boundary(&full_text, 0).unwrap();
        assert_eq!(next_word, 5);
        let prev_word = find_previous_word_boundary(&full_text, 11).unwrap();
        assert_eq!(prev_word, 6);
    }

    #[test]
    fn stage3d_agal_triangle_rasterizes_and_reads_backbuffer() {
        let mut arena = Stage3dArena::default();
        let ctx = arena.create_context("software", "baseline").unwrap();
        arena
            .configure_back_buffer(ctx, 64, 64, 0, true)
            .expect("configureBackBuffer");
        arena
            .clear(ctx, 0.0, 0.0, 0.0, 1.0, 1.0, 0, 7)
            .expect("clear");

        // Build AGAL v1 vertex shader: mov op, va0; mov v0, va1
        let mut vs = vec![0xA0, 1, 0, 0, 0, 0xA1, 0x00];
        vs.extend_from_slice(&[
            0x00, 0, 0, 0, 0, 0, 0x0F, 3, // op=mov, dst=op
            0, 0, 0, 0xE4, 0, 0, 0, 0, // src1=va0
            0, 0, 0, 0, 0, 0, 0, 0, // src2=unused
        ]);
        vs.extend_from_slice(&[
            0x00, 0, 0, 0, 0, 0, 0x0F, 4, // dst=v0
            1, 0, 0, 0xE4, 0, 0, 0, 0, // src1=va1
            0, 0, 0, 0, 0, 0, 0, 0,
        ]);

        // Build AGAL v1 fragment shader: mov oc, v0
        let mut fs = vec![0xA0, 1, 0, 0, 0, 0xA1, 0x01];
        fs.extend_from_slice(&[
            0x00, 0, 0, 0, 0, 0, 0x0F, 3, // dst=oc
            0, 0, 0, 0xE4, 4, 0, 0, 0, // src1=v0
            0, 0, 0, 0, 0, 0, 0, 0,
        ]);

        let prog = arena.create_program(ctx).unwrap();
        arena.upload_program(prog, &vs, &fs).unwrap();
        arena
            .set_state_cmd(ctx, "setProgram", &serde_json::json!({ "programId": prog }))
            .unwrap();

        let vb = arena.create_vertex_buffer(ctx, 3, 7, "staticDraw").unwrap();
        #[rustfmt::skip]
        let verts: Vec<f32> = vec![
            -0.8, -0.8, 0.5,   1.0, 0.0, 0.0, 1.0,
             0.8, -0.8, 0.5,   1.0, 0.0, 0.0, 1.0,
             0.0,  0.8, 0.5,   1.0, 0.0, 0.0, 1.0,
        ];
        arena.upload_vertex_buffer(vb, &verts, 0, 3).unwrap();
        arena
            .set_state_cmd(
                ctx,
                "setVertexBufferAt",
                &serde_json::json!({ "index": 0, "bufferId": vb, "bufferOffset": 0, "format": "float3" }),
            )
            .unwrap();
        arena
            .set_state_cmd(
                ctx,
                "setVertexBufferAt",
                &serde_json::json!({ "index": 1, "bufferId": vb, "bufferOffset": 3, "format": "float4" }),
            )
            .unwrap();

        let ib = arena.create_index_buffer(ctx, 3, "staticDraw").unwrap();
        arena.upload_index_buffer(ib, &[0, 1, 2], 0, 3).unwrap();

        arena.draw_triangles(ctx, ib, 0, 1).unwrap();
        arena.present(ctx).unwrap();
        let (w, h, rgba) = arena.backbuffer_rgba(ctx).unwrap();
        assert_eq!((w, h), (64, 64));
        let center = ((32 * 64 + 32) * 4) as usize;
        assert_eq!(&rgba[center..center + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn pixel_bender_pbj_and_json_shaders_evaluate() {
        // Construct a real binary .pbj shader:
        // 0xA5 version 1, 0xA4 name "TintPbj", 0xA3 input 0 ch=4 "src",
        // 0xA1 outCoord reg=0 mask=0xC "_OutCoord",
        // 0xA1 output dst reg=1 mask=0xF "dst",
        // 0xA1 param float4 reg=2 mask=0xF "color",
        // 0xA2 defaultValue [0.5, 1.0, 0.25, 1.0],
        // 0x30 sampleNearest dst=1 mask=0xF coord=0 swz=(x,y) tex=0,
        // 0x03 mul dst=1 mask=0xF len=4 src=2 swz=(x,y,z,w)
        let mut pbj = Vec::new();
        pbj.push(0xA5);
        pbj.extend_from_slice(&1i32.to_le_bytes());
        pbj.push(0xA4);
        pbj.extend_from_slice(&(7u16).to_le_bytes());
        pbj.extend_from_slice(b"TintPbj");
        pbj.push(0xA3);
        pbj.extend_from_slice(&[0, 4]);
        pbj.extend_from_slice(b"src\0");
        pbj.push(0xA1);
        pbj.extend_from_slice(&[1, 0x02]);
        pbj.extend_from_slice(&0u16.to_le_bytes());
        pbj.push(0x0C);
        pbj.extend_from_slice(b"_OutCoord\0");
        pbj.push(0xA1);
        pbj.extend_from_slice(&[2, 0x04]);
        pbj.extend_from_slice(&1u16.to_le_bytes());
        pbj.push(0x0F);
        pbj.extend_from_slice(b"dst\0");
        pbj.push(0xA1);
        pbj.extend_from_slice(&[1, 0x04]);
        pbj.extend_from_slice(&2u16.to_le_bytes());
        pbj.push(0x0F);
        pbj.extend_from_slice(b"color\0");
        pbj.push(0xA2);
        pbj.push(0x04);
        pbj.extend_from_slice(b"defaultValue\0");
        for f in [0.5f32, 1.0, 0.25, 1.0] {
            pbj.extend_from_slice(&f.to_bits().to_be_bytes());
        }
        // sampleNearest (0x30): dst=1, mask=0xF0, coord=0, swz=0x10 (x=0,y=1 -> 0b00_01_0000 = 0x10), tex=0
        pbj.push(0x30);
        pbj.extend_from_slice(&1u16.to_le_bytes());
        pbj.push(0xF0);
        pbj.extend_from_slice(&0u16.to_le_bytes());
        pbj.push(0x10);
        pbj.push(0);
        // mul (0x03): dst=1, mask_len = (0xF << 4) | (3 << 2) = 0xFC, src=2, swz = 0b00_01_10_11 = 0x1B, pad=0
        pbj.push(0x03);
        pbj.extend_from_slice(&1u16.to_le_bytes());
        pbj.push(0xFC);
        pbj.extend_from_slice(&2u16.to_le_bytes());
        pbj.push(0x1B);
        pbj.push(0);

        let meta = parse_shader_bytecode(&pbj).expect("parse pbj");
        assert_eq!(meta.name, "TintPbj");
        assert_eq!(meta.inputs.len(), 1);
        assert_eq!(meta.parameters.len(), 1);
        assert_eq!(meta.parameters[0].param_type, "float4");

        let req = ShaderEvalRequest {
            bytecode_base64: BASE64.encode(&pbj),
            precision_hint: "full".into(),
            width: 2,
            height: 1,
            inputs: vec![ShaderRuntimeInput {
                name: "src".into(),
                width: 2,
                height: 1,
                channels: 4,
                floats: vec![1.0, 0.5, 0.8, 1.0, 0.4, 0.2, 1.0, 1.0],
                rgba_base64: None,
            }],
            parameters: BTreeMap::new(),
        };
        let res = evaluate_shader(&req).expect("eval pbj");
        assert_eq!(res.floats.len(), 8);
        assert!((res.floats[0] - 0.5).abs() < 1e-5);
        assert!((res.floats[1] - 0.5).abs() < 1e-5);
        assert!((res.floats[2] - 0.2).abs() < 1e-5);
    }

    #[test]
    fn display_3d_projection_roundtrip() {
        let pose = Display3dPose {
            x: 120.0,
            y: 80.0,
            z: 100.0,
            scale_x: 1.5,
            scale_y: 1.25,
            scale_z: 1.0,
            rotation_x: 20.0,
            rotation_y: -15.0,
            rotation_z: 30.0,
        };
        let m = compose_display_matrix3d(&pose);
        let dec = decompose_display_matrix3d(&m);
        assert!((dec.x - 120.0).abs() < 1e-3);
        assert!((dec.z - 100.0).abs() < 1e-3);
        let chain = [m];
        let screen = local3d_to_global(&chain, [25.0, -10.0, 0.0], 480.0, [250.0, 200.0]);
        let local = global_to_local3d(&chain, screen, 480.0, [250.0, 200.0]);
        assert!((local[0] - 25.0).abs() < 1e-3);
        assert!((local[1] - (-10.0)).abs() < 1e-3);
        assert!(local[2].abs() < 1e-6);
    }
}
