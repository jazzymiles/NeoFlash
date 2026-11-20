use super::*;

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
    display_text: String,
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
                display_text: FTE_GRAPHIC_CHAR.to_string(),
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

        for (c_start, c_end, cluster) in crate::grapheme_clusters_utf16(&run.text) {
            let u16_len = (c_end - c_start) as usize;
            // Combine \r\n into one mandatory-break atom spanning 2 UTF-16 code units.
            if cluster == "\r\n" {
                atoms.push(RawAtom {
                    ch: '\n',
                    display_text: "\n".to_owned(),
                    small_cap: false,
                    char_begin: utf16_pos,
                    char_end: utf16_pos + u16_len,
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
                utf16_pos += u16_len;
                prev_ch = Some('\n');
                continue;
            }

            let ch = cluster.chars().next().unwrap_or(' ');
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

            let (display_text, display_ch, small_cap) = match run.typographic_case.as_str() {
                "uppercase" | "caps" => (
                    cluster.chars().map(|c| c.to_ascii_uppercase()).collect(),
                    ch.to_ascii_uppercase(),
                    false,
                ),
                "lowercase" => (
                    cluster.chars().map(|c| c.to_ascii_lowercase()).collect(),
                    ch.to_ascii_lowercase(),
                    false,
                ),
                "smallCaps" | "capsAndSmallCaps" => (
                    cluster.chars().map(|c| c.to_ascii_uppercase()).collect(),
                    ch.to_ascii_uppercase(),
                    ch.is_lowercase(),
                ),
                "title" => {
                    if word_start {
                        let mut chars = cluster.chars();
                        let first = chars.next().map(|c| c.to_ascii_uppercase());
                        let mut s = String::with_capacity(cluster.len());
                        if let Some(f) = first {
                            s.push(f);
                        }
                        s.extend(chars);
                        (s, ch.to_ascii_uppercase(), false)
                    } else {
                        (cluster.to_owned(), ch, false)
                    }
                }
                _ => (cluster.to_owned(), ch, false),
            };

            let is_nl = matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}');
            let is_tab = ch == '\t';
            let is_sp = ch == ' ' || ch == '\u{00A0}' || ch == '\u{3000}';
            let is_emoji_cluster = cluster.chars().any(|c| (c as u32) >= 0x1F000);
            let mut adv = if is_emoji_cluster {
                if small_cap { size * 0.78 } else { size }
            } else {
                glyph_advance(display_ch, size, bold, tabular, small_cap, non_linear)
            };
            if !is_nl && !is_tab {
                adv = (adv + run.tracking_left + run.tracking_right).max(0.0);
            }
            let bidi = if cluster.chars().any(is_rtl_char) {
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
                display_text,
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
            let ch_str = atom.display_text.clone();
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
                prev_draw.text.push_str(&atom.display_text);
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
    let bounds = crate::grapheme_utf16_boundaries(raw_text);
    let u16_total = *bounds.last().unwrap_or(&0) as i32;
    if after_char_index < 0 || after_char_index >= u16_total {
        return Err(Fte3dError::Range(format!(
            "Char index {after_char_index} is out of bounds for TextBlock length {u16_total}."
        )));
    }
    for b in bounds {
        let bi = b as i32;
        if bi > after_char_index {
            return Ok(bi);
        }
    }
    Ok(u16_total)
}

pub fn find_previous_atom_boundary(
    raw_text: &str,
    before_char_index: i32,
) -> Result<i32, Fte3dError> {
    let bounds = crate::grapheme_utf16_boundaries(raw_text);
    let u16_total = *bounds.last().unwrap_or(&0) as i32;
    if before_char_index <= 0 || before_char_index > u16_total {
        return Err(Fte3dError::Range(format!(
            "Char index {before_char_index} is out of bounds for TextBlock length {u16_total}."
        )));
    }
    for &b in bounds.iter().rev() {
        let bi = b as i32;
        if bi < before_char_index {
            return Ok(bi);
        }
    }
    Ok(0)
}

fn word_boundaries_utf16(raw_text: &str) -> Vec<i32> {
    let clusters = crate::grapheme_clusters_utf16(raw_text);
    if clusters.is_empty() {
        return vec![0];
    }
    let mut bounds = vec![0i32];
    for i in 0..clusters.len() {
        let end_pos = clusters[i].1 as i32;
        if i + 1 < clusters.len() {
            let a_str = clusters[i].2;
            let b_str = clusters[i + 1].2;
            let a = a_str.chars().next().unwrap_or(' ');
            let b = b_str.chars().next().unwrap_or(' ');
            let a_ws = a_str.chars().all(char::is_whitespace);
            let b_ws = b_str.chars().all(char::is_whitespace);
            let boundary = if a_ws != b_ws || is_cjk(a) || is_cjk(b) {
                true
            } else if !a_ws && !b_ws {
                is_word_char(a) != is_word_char(b)
            } else {
                false
            };
            if boundary {
                bounds.push(end_pos);
            }
        } else if bounds.last().copied() != Some(end_pos) {
            bounds.push(end_pos);
        }
    }
    bounds
}

pub fn font_has_glyphs(font_bytes: &[u8], text: &str) -> Result<bool, Fte3dError> {
    crate::sfnt_has_glyphs(font_bytes, text)
        .ok_or_else(|| Fte3dError::Argument("Invalid SFNT/TTC/WOFF font binary.".to_owned()))
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
