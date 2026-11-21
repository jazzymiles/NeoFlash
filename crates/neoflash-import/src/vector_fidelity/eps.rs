//! Bounded pure-Rust PostScript Level 3 / Adobe Illustrator AICB EPS vector parser
//! with fallback to embedded binary EPS TIFF previews ONLY when zero vector paths are decoded.

use super::pdf_color_fn::cmyk_to_rgba;
use super::*;
use image::{ImageDecoder, ImageEncoder, RgbaImage};
use neoflash_core::{AuthoredTextFieldOptions, TextFieldState, TextFormat, normalize_text};
use std::io::Cursor;
use tiff::{decoder::Decoder, tags::Tag};

pub(super) const MAGIC: &[u8] = &[0xc5, 0xd0, 0xd3, 0xc6];
pub(super) const MAX_EPS_GSAVE_DEPTH: usize = 64;
const MAX_EPS_TOKENS: usize = 1_000_000;

pub(super) struct EpsParsedArtwork {
    pub buckets: Vec<VectorGroupBucket>,
    pub bounds: [f64; 4],
    pub stage_bounds: [f64; 4],
    pub used_tiff_fallback: bool,
}

fn section(bytes: &[u8], at: usize) -> Option<&[u8]> {
    let word = |i| Some(u32::from_le_bytes(bytes.get(i..i + 4)?.try_into().ok()?) as usize);
    let start = word(at)?;
    let len = word(at + 4)?;
    (start >= 30 && len != 0).then_some(())?;
    bytes.get(start..start.checked_add(len)?)
}

fn indexed_preview(
    bytes: &[u8],
    decoder: &mut Decoder<Cursor<&[u8]>>,
    w: u32,
    h: u32,
) -> Option<RgbaImage> {
    if decoder.get_tag_u32(Tag::PhotometricInterpretation).ok()? != 3
        || decoder.get_tag_u32(Tag::Compression).unwrap_or(1) != 1
        || decoder
            .get_tag_u16_vec(Tag::BitsPerSample)
            .ok()?
            .iter()
            .any(|b| *b != 8)
        || decoder.get_tag_u32(Tag::PlanarConfiguration).unwrap_or(1) != 1
        || decoder.get_tag_u32(Tag::Orientation).unwrap_or(1) != 1
    {
        return None;
    }
    let samples = decoder.get_tag_u32(Tag::SamplesPerPixel).unwrap_or(1) as usize;
    let alpha_type = if samples == 2 {
        decoder.get_tag_u32(Tag::ExtraSamples).ok()?
    } else {
        0
    };
    if !(1..=2).contains(&samples) || alpha_type > 2 {
        return None;
    }
    let palette = decoder.get_tag_u16_vec(Tag::ColorMap).ok()?;
    let offsets = decoder.get_tag_u32_vec(Tag::StripOffsets).ok()?;
    let lengths = decoder.get_tag_u32_vec(Tag::StripByteCounts).ok()?;
    if palette.len() != 768 || offsets.len() != lengths.len() {
        return None;
    }
    let pixels = w as usize * h as usize;
    let mut rgba = Vec::with_capacity(pixels * 4);
    for (start, len) in offsets.into_iter().zip(lengths) {
        let start = start as usize;
        let strip = bytes.get(start..start.checked_add(len as usize)?)?;
        if !strip.len().is_multiple_of(samples) || strip.len() / samples > pixels - rgba.len() / 4 {
            return None;
        }
        for sample in strip.chunks_exact(samples) {
            let index = usize::from(sample[0]);
            let alpha = if alpha_type == 0 { 255 } else { sample[1] };
            for channel in 0..3 {
                let c = (u32::from(palette[channel * 256 + index]) + 128) / 257;
                rgba.push(if alpha_type == 1 && alpha != 0 {
                    ((c * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8
                } else {
                    c as u8
                });
            }
            rgba.push(alpha);
        }
    }
    RgbaImage::from_raw(w, h, rgba)
}

fn parse_dsc_box_line(line: &str, prefix: &str) -> Option<[f64; 4]> {
    let rest = line.strip_prefix(prefix)?.trim();
    if rest.starts_with("(atend)") {
        return None;
    }
    let nums: Vec<f64> = rest
        .split_whitespace()
        .filter_map(|v| v.parse::<f64>().ok())
        .collect();
    let [mut x0, mut y0, mut x1, mut y1] = <[f64; 4]>::try_from(nums).ok()?;
    if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
        return None;
    }
    if x1 < x0 {
        std::mem::swap(&mut x0, &mut x1);
    }
    if y1 < y0 {
        std::mem::swap(&mut y0, &mut y1);
    }
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

fn parse_eps_bounds_and_art_size(ps_text: &str) -> (Option<[f64; 4]>, Option<(f64, f64)>) {
    let mut hires_box = None;
    let mut int_box = None;
    let mut art_size = None;

    // Scan header + trailer comments
    let head_len = ps_text.len().min(32_768);
    let tail_start = ps_text.len().saturating_sub(16_384);
    let regions = if tail_start > head_len {
        vec![&ps_text[..head_len], &ps_text[tail_start..]]
    } else {
        vec![ps_text]
    };

    for region in regions {
        for raw_line in region.lines() {
            let line = raw_line.trim();
            if hires_box.is_none()
                && let Some(b) = parse_dsc_box_line(line, "%%HiResBoundingBox:")
            {
                hires_box = Some(b);
            }
            if int_box.is_none()
                && let Some(b) = parse_dsc_box_line(line, "%%BoundingBox:")
            {
                int_box = Some(b);
            }
            if art_size.is_none()
                && let Some(rest) = line.strip_prefix("%AI5_ArtSize:")
            {
                let nums: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|v| v.parse::<f64>().ok())
                    .collect();
                if nums.len() >= 2 && nums[0] > 0.0 && nums[1] > 0.0 {
                    art_size = Some((nums[0], nums[1]));
                }
            }
        }
    }

    (hires_box.or(int_box), art_size)
}

#[derive(Clone, Debug)]
enum PsToken {
    Num(f64),
    Name(String),
    Str(String),
    ArrOpen,
    ArrClose,
    DictOpen,
    DictClose,
    ProcSkip,
    LayerBegin,
    LayerName(String),
    LayerEnd,
    Op(String),
}

fn unescape_ps_string(raw_inner: &str) -> String {
    let mut bytes = Vec::with_capacity(raw_inner.len());
    let mut chars = raw_inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let Some(esc) = chars.next() else {
                break;
            };
            match esc {
                'n' => bytes.push(b'\n'),
                'r' => bytes.push(b'\r'),
                't' => bytes.push(b'\t'),
                'b' => bytes.push(0x08),
                'f' => bytes.push(0x0C),
                '(' => bytes.push(b'('),
                ')' => bytes.push(b')'),
                '\\' => bytes.push(b'\\'),
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                '\n' => {}
                '0'..='7' => {
                    let mut val = (esc as u32) - ('0' as u32);
                    for _ in 0..2 {
                        if let Some(&d @ '0'..='7') = chars.peek() {
                            chars.next();
                            val = val * 8 + ((d as u32) - ('0' as u32));
                        } else {
                            break;
                        }
                    }
                    bytes.push((val & 0xFF) as u8);
                }
                other => {
                    let mut buf = [0u8; 4];
                    bytes.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                }
            }
        } else {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn tokenize_ps_body(ps_text: &str) -> Result<Vec<PsToken>, ImportError> {
    let mut tokens = Vec::new();
    let mut in_prolog_or_data = false;

    for raw_line in ps_text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.starts_with("%%BeginProlog")
            || trimmed.starts_with("%%BeginResource")
            || trimmed.starts_with("%%BeginData")
            || trimmed.starts_with("%%BeginBinary")
            || trimmed.starts_with("%%BeginPreview")
        {
            in_prolog_or_data = true;
            continue;
        }
        if trimmed.starts_with("%%EndProlog")
            || trimmed.starts_with("%%EndResource")
            || trimmed.starts_with("%%EndData")
            || trimmed.starts_with("%%EndBinary")
            || trimmed.starts_with("%%EndPreview")
        {
            in_prolog_or_data = false;
            continue;
        }
        if in_prolog_or_data {
            continue;
        }
        if trimmed == "%AI5_BeginLayer" {
            tokens.push(PsToken::LayerBegin);
            continue;
        }
        if trimmed.starts_with("%AI5_EndLayer") {
            tokens.push(PsToken::LayerEnd);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("%AI5_Name:") {
            let r = rest.trim();
            if let Some(inner) = r.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
                tokens.push(PsToken::LayerName(unescape_ps_string(inner)));
            } else if !r.is_empty() {
                tokens.push(PsToken::LayerName(r.to_owned()));
            }
            continue;
        }
        if trimmed.starts_with('%') {
            continue;
        }

        let mut chars = raw_line.chars().peekable();
        while let Some(&ch) = chars.peek() {
            if tokens.len() >= MAX_EPS_TOKENS {
                return Err(ImportError::Container(
                    "EPS stream exceeds one million PostScript tokens.".into(),
                ));
            }
            if ch.is_ascii_whitespace() {
                chars.next();
                continue;
            }
            if ch == '%' {
                break;
            }
            if ch == '{' {
                // Skip PostScript procedure literal `{ ... }` without executing arbitrary code!
                chars.next();
                let mut depth = 1usize;
                for c in chars.by_ref() {
                    if c == '{' {
                        depth += 1;
                    } else if c == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                tokens.push(PsToken::ProcSkip);
                continue;
            }
            if ch == '}' {
                chars.next();
                continue;
            }
            if ch == '(' {
                chars.next();
                let mut inner = String::new();
                let mut depth = 1usize;
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        inner.push('\\');
                        if let Some(esc) = chars.next() {
                            inner.push(esc);
                        }
                        continue;
                    }
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    inner.push(c);
                }
                tokens.push(PsToken::Str(unescape_ps_string(&inner)));
                continue;
            }
            if ch == '<' {
                chars.next();
                if chars.peek() == Some(&'<') {
                    chars.next();
                    tokens.push(PsToken::DictOpen);
                    continue;
                }
                let mut hex_nibbles = Vec::new();
                for c in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                    if let Some(d) = c.to_digit(16) {
                        hex_nibbles.push(d as u8);
                    }
                }
                if !hex_nibbles.len().is_multiple_of(2) {
                    hex_nibbles.push(0);
                }
                let bytes: Vec<u8> = hex_nibbles
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (p[0] << 4) | p[1])
                    .collect();
                tokens.push(PsToken::Str(String::from_utf8_lossy(&bytes).into_owned()));
                continue;
            }
            if ch == '>' {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(PsToken::DictClose);
                }
                continue;
            }
            if ch == '[' {
                chars.next();
                tokens.push(PsToken::ArrOpen);
                continue;
            }
            if ch == ']' {
                chars.next();
                tokens.push(PsToken::ArrClose);
                continue;
            }
            if ch == '/' {
                chars.next();
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_whitespace()
                        || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '{' | '}' | '%' | '/')
                    {
                        break;
                    }
                    name.push(chars.next().unwrap());
                }
                tokens.push(PsToken::Name(name));
                continue;
            }
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_whitespace()
                    || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '{' | '}' | '%' | '/')
                {
                    break;
                }
                word.push(chars.next().unwrap());
            }
            if let Ok(n) = word.parse::<f64>()
                && n.is_finite()
            {
                tokens.push(PsToken::Num(n));
            } else if !word.is_empty() {
                tokens.push(PsToken::Op(word));
            }
        }
    }

    Ok(tokens)
}

#[derive(Clone, Debug)]
enum PsVal {
    Num(f64),
    Name(String),
    Str(String),
    Arr(Vec<PsVal>),
    Dict(BTreeMap<String, PsVal>),
    Proc,
}

impl PsVal {
    fn as_num(&self) -> Option<f64> {
        match self {
            Self::Num(n) if n.is_finite() => Some(*n),
            _ => None,
        }
    }
    fn as_nums(&self) -> Option<Vec<f64>> {
        match self {
            Self::Arr(items) => items.iter().map(Self::as_num).collect(),
            _ => None,
        }
    }
    fn as_name(&self) -> Option<&str> {
        match self {
            Self::Name(s) => Some(s.as_str()),
            _ => None,
        }
    }
    fn as_dict(&self) -> Option<&BTreeMap<String, PsVal>> {
        match self {
            Self::Dict(d) => Some(d),
            _ => None,
        }
    }
}

fn parse_ps_shading_dict(dict: &BTreeMap<String, PsVal>) -> Option<GradientPaint> {
    let sh_type = dict.get("ShadingType")?.as_num()?.round() as i64;
    let cs_name = dict
        .get("ColorSpace")
        .and_then(PsVal::as_name)
        .unwrap_or("DeviceRGB");
    let fn_dict = dict.get("Function")?.as_dict()?;
    let stops = parse_ps_function_stops(fn_dict, cs_name)?;

    let paint = match sh_type {
        2 => {
            let coords = dict.get("Coords")?.as_nums()?;
            let [x1, y1, x2, y2] = <[f64; 4]>::try_from(coords).ok()?;
            if (x2 - x1).hypot(y2 - y1) <= 1e-9 {
                return None;
            }
            GradientPaint {
                kind: GradientKind::Linear,
                matrix: axial_gradient_matrix(x1, y1, x2, y2),
                stops,
                ..Default::default()
            }
        }
        3 => {
            let coords = dict.get("Coords")?.as_nums()?;
            let [x0, y0, r0, x1, y1, r1] = <[f64; 6]>::try_from(coords).ok()?;
            if r1 <= 1e-9 {
                return None;
            }
            let dx = x0 - x1;
            let dy = y0 - y1;
            let dist = dx.hypot(dy);
            let dr = (r1 - r0).max(1e-6);
            let s = r1 / 819.2;
            let (matrix, focal_point_ratio) = if dist > 1e-9 {
                let cos_t = dx / dist;
                let sin_t = dy / dist;
                (
                    [s * cos_t, s * sin_t, -s * sin_t, s * cos_t, x1, y1],
                    (dist / dr).clamp(-0.999, 0.999),
                )
            } else {
                ([s, 0.0, 0.0, s, x1, y1], 0.0)
            };
            GradientPaint {
                kind: GradientKind::Radial,
                matrix,
                stops,
                focal_point_ratio,
                ..Default::default()
            }
        }
        _ => return None,
    };
    paint.validate().ok()?;
    Some(paint)
}

fn ps_comps_to_rgba(cs_name: &str, comps: &[f64]) -> Option<Color> {
    match cs_name {
        "DeviceGray" => {
            let v = (comps.first()?.clamp(0.0, 1.0) * 255.0).round() as u8;
            Some([v, v, v, 255])
        }
        "DeviceCMYK" if comps.len() >= 4 => {
            Some(cmyk_to_rgba(comps[0], comps[1], comps[2], comps[3]))
        }
        _ if comps.len() >= 3 => Some([
            (comps[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (comps[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (comps[2].clamp(0.0, 1.0) * 255.0).round() as u8,
            255,
        ]),
        _ => None,
    }
}

fn parse_ps_function_stops(
    fn_dict: &BTreeMap<String, PsVal>,
    cs_name: &str,
) -> Option<Vec<GradientStop>> {
    let fn_type = fn_dict.get("FunctionType")?.as_num()?.round() as i64;
    match fn_type {
        2 => {
            let c0 = fn_dict
                .get("C0")
                .and_then(PsVal::as_nums)
                .unwrap_or_else(|| vec![0.0, 0.0, 0.0]);
            let c1 = fn_dict
                .get("C1")
                .and_then(PsVal::as_nums)
                .unwrap_or_else(|| vec![1.0, 1.0, 1.0]);
            Some(vec![
                GradientStop {
                    ratio: 0,
                    color: ps_comps_to_rgba(cs_name, &c0)?,
                },
                GradientStop {
                    ratio: 255,
                    color: ps_comps_to_rgba(cs_name, &c1)?,
                },
            ])
        }
        3 => {
            let PsVal::Arr(funcs) = fn_dict.get("Functions")? else {
                return None;
            };
            let bounds = fn_dict
                .get("Bounds")
                .and_then(PsVal::as_nums)
                .unwrap_or_default();
            if funcs.is_empty() || funcs.len() > 14 || bounds.len() + 1 != funcs.len() {
                return None;
            }
            let mut stops = Vec::with_capacity(funcs.len() + 1);
            for (idx, sub_val) in funcs.iter().enumerate() {
                let sub = sub_val.as_dict()?;
                let c0 = sub.get("C0").and_then(PsVal::as_nums)?;
                let c1 = sub.get("C1").and_then(PsVal::as_nums)?;
                if idx == 0 {
                    stops.push(GradientStop {
                        ratio: 0,
                        color: ps_comps_to_rgba(cs_name, &c0)?,
                    });
                }
                let raw_end = if idx + 1 == funcs.len() {
                    1.0
                } else {
                    bounds[idx].clamp(0.0, 1.0)
                };
                let ratio = (raw_end * 255.0).round() as u8;
                stops.push(GradientStop {
                    ratio,
                    color: ps_comps_to_rgba(cs_name, &c1)?,
                });
            }
            stops.dedup_by_key(|s| s.ratio);
            (stops.len() >= 2).then_some(stops)
        }
        _ => None,
    }
}

#[derive(Clone)]
struct PsGState {
    ctm: [f64; 6],
    fill_color: Color,
    stroke_color: Color,
    line_width: f64,
    line_cap: i64,
    line_join: i64,
    miter_limit: f64,
    dash_pattern: Vec<f64>,
    dash_phase: f64,
    ai_fill_rule: FillRule,
    font_family: String,
    font_matrix: [f64; 6],
    current_path: Vec<PathCommand>,
    cur_pt: (f64, f64),
    subpath_start: (f64, f64),
    clip: Option<std::sync::Arc<Element>>,
    clip_geom: Option<(Vec<PathCommand>, FillRule)>,
}

impl Default for PsGState {
    fn default() -> Self {
        Self {
            ctm: identity_matrix(),
            fill_color: [0, 0, 0, 255],
            stroke_color: [0, 0, 0, 255],
            line_width: 1.0,
            line_cap: 0,
            line_join: 0,
            miter_limit: 10.0,
            dash_pattern: Vec::new(),
            dash_phase: 0.0,
            ai_fill_rule: FillRule::NonZero,
            font_family: "sans-serif".to_owned(),
            font_matrix: [16.0, 0.0, 0.0, 16.0, 0.0, 0.0],
            current_path: Vec::new(),
            cur_pt: (0.0, 0.0),
            subpath_start: (0.0, 0.0),
            clip: None,
            clip_geom: None,
        }
    }
}

pub(super) fn parse_eps_artwork(
    bytes: &[u8],
    source: &str,
    options: &VectorImportOptions,
    default_size: (u32, u32),
    report: &mut ImportReport,
) -> Result<EpsParsedArtwork, ImportError> {
    let error = |message: &str| ImportError::Container(format!("{source}: {message}"));
    let (ps_bytes, binary_preview) = if bytes.starts_with(MAGIC) {
        let ps = section(bytes, 4)
            .filter(|b| b.starts_with(b"%!PS-Adobe"))
            .ok_or_else(|| error("Invalid binary EPS PostScript section."))?;
        (ps, section(bytes, 20))
    } else {
        (bytes, None)
    };

    let ps_text = String::from_utf8_lossy(ps_bytes);
    let (parsed_bbox, art_size) = parse_eps_bounds_and_art_size(&ps_text);
    let bounds = parsed_bbox.unwrap_or([
        0.0,
        0.0,
        f64::from(default_size.0.max(1)),
        f64::from(default_size.1.max(1)),
    ]);
    let [bx0, by0, bx1, by1] = bounds;
    let bw = (bx1 - bx0).max(1.0);
    let bh = (by1 - by0).max(1.0);

    let stage_bounds = if let Some((_art_w, art_h)) = art_size {
        let sy0 = art_h - by1;
        [bx0, sy0, bx0 + bw, sy0 + bh]
    } else if by1 <= 0.0 {
        // Illustrator negative-Y artboard origin (`(0, 0)` = top-left of artboard)
        [bx0, -by1, bx1, -by0]
    } else {
        [bx0, by0, bx1, by1]
    };

    let page_matrix = [1.0, 0.0, 0.0, -1.0, -bx0, by1];

    let tokens = tokenize_ps_body(&ps_text)?;
    let mut stack: Vec<PsVal> = Vec::new();
    let mut open_containers: Vec<(bool, Vec<PsVal>)> = Vec::new(); // (is_dict, items)

    let mut gstate = PsGState {
        ctm: page_matrix,
        ..PsGState::default()
    };
    let mut gstate_stack: Vec<PsGState> = Vec::new();
    let mut pending_clip: Option<FillRule> = None;
    let mut in_compound = false;
    let mut compound_cmds: Vec<PathCommand> = Vec::new();
    let mut compound_paint: Option<(bool, bool, FillRule)> = None;
    let mut saw_xa_lower = false;

    let mut buckets: Vec<VectorGroupBucket> = Vec::new();
    let mut current_bucket = VectorGroupBucket {
        name: format!("{source} Layer 1"),
        elements: Vec::new(),
        mask_elements: Vec::new(),
        mask_mode: MaskMode::Geometry,
    };
    let mut total_ops = 0usize;
    let mut vector_elements = 0usize;

    let pop_num = |st: &mut Vec<PsVal>| -> Option<f64> { st.pop()?.as_num() };

    let sync_bucket_clip = |current_bucket: &mut VectorGroupBucket,
                            buckets: &mut Vec<VectorGroupBucket>,
                            effective_clip: Option<&Element>| {
        if current_bucket.mask_elements.first().map(|e| &e.id) != effective_clip.map(|e| &e.id) {
            if !current_bucket.elements.is_empty() {
                let next = VectorGroupBucket {
                    name: current_bucket.name.clone(),
                    elements: Vec::new(),
                    mask_elements: Vec::new(),
                    mask_mode: MaskMode::Geometry,
                };
                buckets.push(std::mem::replace(current_bucket, next));
            }
            current_bucket.mask_elements = effective_clip.cloned().into_iter().collect();
        }
    };

    let emit_painted_path = |raw_cmds: &[PathCommand],
                             do_fill: bool,
                             do_stroke: bool,
                             fill_rule: FillRule,
                             gs: &PsGState,
                             options: &VectorImportOptions,
                             bucket: &mut VectorGroupBucket,
                             report: &mut ImportReport,
                             vector_elements: &mut usize|
     -> Result<(), ImportError> {
        if raw_cmds.is_empty() {
            return Ok(());
        }
        if raw_cmds.len() > MAX_SVG_PATH_COMMANDS {
            return Err(ImportError::Container(format!(
                "{source}: EPS path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
            )));
        }
        let ctm_scale = super::ctm_stroke_scale(gs.ctm);
        let dashed_local = if do_stroke && !gs.dash_pattern.is_empty() {
            Some(super::dash_subdivide_path(
                raw_cmds,
                &gs.dash_pattern,
                gs.dash_phase,
            ))
        } else {
            None
        };

        // If dashed stroke AND filled, emit fill path first, then open dashed stroke segments
        if do_fill && dashed_local.is_some() {
            let (fill_cmds, elem_tf) = match options.transform_handling {
                VectorTransformHandling::BakeTransformsIntoPaths => (
                    bake_matrix_into_commands(raw_cmds, gs.ctm),
                    Transform::default(),
                ),
                VectorTransformHandling::KeepElementTransforms => {
                    if let Some(tf) = Transform::from_matrix(gs.ctm) {
                        (raw_cmds.to_vec(), tf)
                    } else {
                        (
                            bake_matrix_into_commands(raw_cmds, gs.ctm),
                            Transform::default(),
                        )
                    }
                }
            };
            if validate_path_commands(&fill_cmds).is_ok() {
                bucket.elements.push(Element {
                    id: new_id("element"),
                    name: format!("EPS Path {}", bucket.elements.len() + 1),
                    transform: elem_tf,
                    kind: ElementKind::Path {
                        commands: fill_cmds,
                        fill_rule,
                        fill: gs.fill_color,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                });
                report.statistics.elements += 1;
                *vector_elements += 1;
            }
        }

        let source_cmds = if let Some(ref d) = dashed_local {
            d.as_slice()
        } else {
            raw_cmds
        };
        if source_cmds.len() > MAX_SVG_PATH_COMMANDS {
            return Err(ImportError::Container(format!(
                "{source}: Dashed EPS path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
            )));
        }

        let (final_cmds, elem_tf, used_elem_tf) = match options.transform_handling {
            VectorTransformHandling::BakeTransformsIntoPaths => (
                bake_matrix_into_commands(source_cmds, gs.ctm),
                Transform::default(),
                false,
            ),
            VectorTransformHandling::KeepElementTransforms => {
                if let Some(tf) = Transform::from_matrix(gs.ctm) {
                    (source_cmds.to_vec(), tf, true)
                } else {
                    (
                        bake_matrix_into_commands(source_cmds, gs.ctm),
                        Transform::default(),
                        false,
                    )
                }
            }
        };

        if validate_path_commands(&final_cmds).is_ok() {
            let fill_col = if do_fill && dashed_local.is_none() {
                gs.fill_color
            } else {
                [0, 0, 0, 0]
            };
            let stroke_col = if do_stroke {
                gs.stroke_color
            } else {
                [0, 0, 0, 0]
            };
            let sw = if do_stroke {
                if used_elem_tf {
                    gs.line_width
                } else {
                    gs.line_width * ctm_scale
                }
            } else {
                0.0
            };
            if fill_col[3] > 0 || (sw > 0.0 && stroke_col[3] > 0) {
                bucket.elements.push(Element {
                    id: new_id("element"),
                    name: format!("EPS Path {}", bucket.elements.len() + 1),
                    transform: elem_tf,
                    kind: ElementKind::Path {
                        commands: final_cmds,
                        fill_rule,
                        fill: fill_col,
                        stroke: stroke_col,
                        stroke_width: sw,
                    },
                    ..Element::default()
                });
                report.statistics.elements += 1;
                *vector_elements += 1;
            }
        }
        Ok(())
    };

    let apply_clip_commands = |cmds: &[PathCommand],
                               rule: FillRule,
                               gs: &mut PsGState,
                               options: &VectorImportOptions,
                               loc: &str,
                               report: &mut ImportReport| {
        if cmds.is_empty() {
            return;
        }
        let baked = bake_matrix_into_commands(cmds, gs.ctm);
        if validate_path_commands(&baked).is_err() {
            return;
        }
        gs.clip_geom = Some((baked.clone(), rule));
        match options.mask_handling {
            VectorMaskHandling::MaskLayers => {
                gs.clip = Some(std::sync::Arc::new(Element {
                    id: new_id("element"),
                    name: "EPS Clip Path".to_owned(),
                    transform: Transform::default(),
                    kind: ElementKind::Path {
                        commands: baked,
                        fill_rule: rule,
                        fill: [255, 255, 255, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }));
                report.diagnostics.push(Diagnostic {
                    severity: Severity::Info,
                    code: "eps-clipping-path-mapped".to_owned(),
                    location: loc.to_owned(),
                    message: "Mapped EPS clipping path to a Timeline Mask layer.".to_owned(),
                });
            }
            VectorMaskHandling::FlattenClip => {
                report.warning(
                    "eps-clipping-path-flattened",
                    loc.to_owned(),
                    "EPS clipping path was flattened according to VectorMaskHandling::FlattenClip.",
                );
            }
        }
    };

    for tok in tokens {
        match tok {
            PsToken::LayerBegin => {
                if !current_bucket.elements.is_empty() || !current_bucket.mask_elements.is_empty() {
                    buckets.push(current_bucket);
                }
                current_bucket = VectorGroupBucket {
                    name: format!("{source} Layer {}", buckets.len() + 1),
                    elements: Vec::new(),
                    mask_elements: Vec::new(),
                    mask_mode: MaskMode::Geometry,
                };
                continue;
            }
            PsToken::LayerName(name) => {
                if current_bucket.elements.is_empty() && !name.trim().is_empty() {
                    current_bucket.name = name;
                }
                continue;
            }
            PsToken::LayerEnd => {
                if !current_bucket.elements.is_empty() || !current_bucket.mask_elements.is_empty() {
                    buckets.push(current_bucket);
                    current_bucket = VectorGroupBucket {
                        name: format!("{source} Layer {}", buckets.len() + 1),
                        elements: Vec::new(),
                        mask_elements: Vec::new(),
                        mask_mode: MaskMode::Geometry,
                    };
                }
                continue;
            }
            PsToken::ArrOpen => {
                open_containers.push((false, Vec::new()));
                continue;
            }
            PsToken::DictOpen => {
                open_containers.push((true, Vec::new()));
                continue;
            }
            PsToken::ArrClose => {
                if let Some((false, items)) = open_containers.pop() {
                    let val = PsVal::Arr(items);
                    if let Some((_, parent)) = open_containers.last_mut() {
                        parent.push(val);
                    } else {
                        stack.push(val);
                    }
                }
                continue;
            }
            PsToken::DictClose => {
                if let Some((true, items)) = open_containers.pop() {
                    let mut map = BTreeMap::new();
                    let mut iter = items.into_iter();
                    while let Some(k) = iter.next() {
                        if let (PsVal::Name(key), Some(v)) = (k, iter.next()) {
                            map.insert(key, v);
                        }
                    }
                    let val = PsVal::Dict(map);
                    if let Some((_, parent)) = open_containers.last_mut() {
                        parent.push(val);
                    } else {
                        stack.push(val);
                    }
                }
                continue;
            }
            PsToken::Num(n) => {
                let val = PsVal::Num(n);
                if let Some((_, parent)) = open_containers.last_mut() {
                    parent.push(val);
                } else {
                    stack.push(val);
                }
                continue;
            }
            PsToken::Name(s) => {
                let val = PsVal::Name(s);
                if let Some((_, parent)) = open_containers.last_mut() {
                    parent.push(val);
                } else {
                    stack.push(val);
                }
                continue;
            }
            PsToken::Str(s) => {
                let val = PsVal::Str(s);
                if let Some((_, parent)) = open_containers.last_mut() {
                    parent.push(val);
                } else {
                    stack.push(val);
                }
                continue;
            }
            PsToken::ProcSkip => {
                let val = PsVal::Proc;
                if let Some((_, parent)) = open_containers.last_mut() {
                    parent.push(val);
                } else {
                    stack.push(val);
                }
                continue;
            }
            PsToken::Op(op) => {
                if !open_containers.is_empty() {
                    // Operators inside `[ ... ]` or `<< ... >>` (rare, e.g. `[ 1 2 add ]`)
                    continue;
                }
                total_ops += 1;
                let loc = format!("{source}::ps::{op}");
                match op.as_str() {
                    "def" | "bind" | "bdef" | "xdef" | "ldef" | "dict" | "begin" | "end"
                    | "userdict" | "systemdict" | "globaldict" | "currentdict" | "where"
                    | "known" | "put" | "get" | "aload" | "astore" | "pop" | "exch" | "dup"
                    | "save" | "restore" | "showpage" | "setflat" | "Ai" | "u" | "U" => {
                        stack.clear();
                    }
                    "q" | "gsave" => {
                        if gstate_stack.len() >= MAX_EPS_GSAVE_DEPTH {
                            return Err(error(&format!(
                                "EPS gsave stack depth exceeds maximum {MAX_EPS_GSAVE_DEPTH}."
                            )));
                        }
                        gstate_stack.push(gstate.clone());
                        stack.clear();
                    }
                    "Q" | "grestore" => {
                        if let Some(prev) = gstate_stack.pop() {
                            gstate = prev;
                        }
                        pending_clip = None;
                        stack.clear();
                    }
                    "cm" | "concat" => {
                        let mat = if let Some(PsVal::Arr(arr)) = stack.last() {
                            let nums: Option<Vec<f64>> = arr.iter().map(PsVal::as_num).collect();
                            nums.and_then(|v| <[f64; 6]>::try_from(v).ok())
                        } else if stack.len() >= 6 {
                            let f = pop_num(&mut stack).unwrap_or(0.0);
                            let e = pop_num(&mut stack).unwrap_or(0.0);
                            let d = pop_num(&mut stack).unwrap_or(1.0);
                            let c = pop_num(&mut stack).unwrap_or(0.0);
                            let b = pop_num(&mut stack).unwrap_or(0.0);
                            let a = pop_num(&mut stack).unwrap_or(1.0);
                            Some([a, b, c, d, e, f])
                        } else {
                            None
                        };
                        if let Some(m) = mat {
                            // Transform any active un-painted path from old CTM into new CTM's local coordinates if needed;
                            // usually concat occurs before newpath.
                            gstate.ctm = affine_multiply(gstate.ctm, m);
                        }
                        stack.clear();
                    }
                    "translate" => {
                        if stack.len() >= 2 {
                            let ty = pop_num(&mut stack).unwrap_or(0.0);
                            let tx = pop_num(&mut stack).unwrap_or(0.0);
                            gstate.ctm = affine_multiply(gstate.ctm, [1.0, 0.0, 0.0, 1.0, tx, ty]);
                        }
                        stack.clear();
                    }
                    "scale" => {
                        if stack.len() >= 2 {
                            let sy = pop_num(&mut stack).unwrap_or(1.0);
                            let sx = pop_num(&mut stack).unwrap_or(1.0);
                            gstate.ctm = affine_multiply(gstate.ctm, [sx, 0.0, 0.0, sy, 0.0, 0.0]);
                        }
                        stack.clear();
                    }
                    "rotate" => {
                        if let Some(deg) = pop_num(&mut stack) {
                            let (s, c) = deg.to_radians().sin_cos();
                            gstate.ctm = affine_multiply(gstate.ctm, [c, s, -s, c, 0.0, 0.0]);
                        }
                        stack.clear();
                    }
                    "w" | "setlinewidth" => {
                        if let Some(lw) = pop_num(&mut stack) {
                            gstate.line_width = lw.max(0.0);
                        }
                        stack.clear();
                    }
                    "J" | "setlinecap" => {
                        if let Some(cap) = pop_num(&mut stack) {
                            gstate.line_cap = (cap.round() as i64).clamp(0, 2);
                        }
                        stack.clear();
                    }
                    "j" | "setlinejoin" => {
                        if let Some(join) = pop_num(&mut stack) {
                            gstate.line_join = (join.round() as i64).clamp(0, 2);
                        }
                        stack.clear();
                    }
                    "M" | "setmiterlimit" => {
                        if let Some(ml) = pop_num(&mut stack) {
                            gstate.miter_limit = ml.max(1.0);
                        }
                        stack.clear();
                    }
                    "d" | "setdash" => {
                        if stack.len() >= 2
                            && let Some(phase) = pop_num(&mut stack)
                            && let Some(pattern) = stack.pop().and_then(|v| v.as_nums())
                        {
                            gstate.dash_pattern =
                                pattern.into_iter().filter(|&x| x >= 0.0).collect();
                            gstate.dash_phase = phase;
                        }
                        stack.clear();
                    }
                    "g" => {
                        if let Some(gray) = pop_num(&mut stack) {
                            let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
                            gstate.fill_color = [v, v, v, 255];
                        }
                        stack.clear();
                    }
                    "G" => {
                        if let Some(gray) = pop_num(&mut stack) {
                            let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
                            gstate.stroke_color = [v, v, v, 255];
                        }
                        stack.clear();
                    }
                    "setgray" => {
                        if let Some(gray) = pop_num(&mut stack) {
                            let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
                            gstate.fill_color = [v, v, v, 255];
                            gstate.stroke_color = [v, v, v, 255];
                        }
                        stack.clear();
                    }
                    "rg" => {
                        if stack.len() >= 3 {
                            let b = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            let g = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            let r = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            gstate.fill_color = [r, g, b, 255];
                        }
                        stack.clear();
                    }
                    "RG" => {
                        if stack.len() >= 3 {
                            let b = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            let g = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            let r = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            gstate.stroke_color = [r, g, b, 255];
                        }
                        stack.clear();
                    }
                    "setrgbcolor" => {
                        if stack.len() >= 3 {
                            let b = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            let g = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            let r = (pop_num(&mut stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                                .round() as u8;
                            gstate.fill_color = [r, g, b, 255];
                            gstate.stroke_color = [r, g, b, 255];
                        }
                        stack.clear();
                    }
                    "k" => {
                        if stack.len() >= 4 {
                            let k = pop_num(&mut stack).unwrap_or(0.0);
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let m = pop_num(&mut stack).unwrap_or(0.0);
                            let c = pop_num(&mut stack).unwrap_or(0.0);
                            gstate.fill_color = cmyk_to_rgba(c, m, y, k);
                        }
                        stack.clear();
                    }
                    "K" => {
                        if stack.len() >= 4 {
                            let k = pop_num(&mut stack).unwrap_or(0.0);
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let m = pop_num(&mut stack).unwrap_or(0.0);
                            let c = pop_num(&mut stack).unwrap_or(0.0);
                            gstate.stroke_color = cmyk_to_rgba(c, m, y, k);
                        }
                        stack.clear();
                    }
                    "setcmykcolor" => {
                        if stack.len() >= 4 {
                            let k = pop_num(&mut stack).unwrap_or(0.0);
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let m = pop_num(&mut stack).unwrap_or(0.0);
                            let c = pop_num(&mut stack).unwrap_or(0.0);
                            let col = cmyk_to_rgba(c, m, y, k);
                            gstate.fill_color = col;
                            gstate.stroke_color = col;
                        }
                        stack.clear();
                    }
                    "Xa" => {
                        saw_xa_lower = true;
                        let nums: Vec<f64> = stack.iter().filter_map(PsVal::as_num).collect();
                        if nums.len() >= 3 {
                            let n = nums.len();
                            let col = [
                                (nums[n - 3].clamp(0.0, 1.0) * 255.0).round() as u8,
                                (nums[n - 2].clamp(0.0, 1.0) * 255.0).round() as u8,
                                (nums[n - 1].clamp(0.0, 1.0) * 255.0).round() as u8,
                                255,
                            ];
                            gstate.fill_color = col;
                        }
                        stack.clear();
                    }
                    "XA" => {
                        let nums: Vec<f64> = stack.iter().filter_map(PsVal::as_num).collect();
                        if nums.len() >= 3 {
                            let n = nums.len();
                            let col = [
                                (nums[n - 3].clamp(0.0, 1.0) * 255.0).round() as u8,
                                (nums[n - 2].clamp(0.0, 1.0) * 255.0).round() as u8,
                                (nums[n - 1].clamp(0.0, 1.0) * 255.0).round() as u8,
                                255,
                            ];
                            if nums.len() >= 7 || saw_xa_lower {
                                gstate.stroke_color = col;
                            } else {
                                gstate.fill_color = col;
                            }
                        }
                        stack.clear();
                    }
                    "XR" => {
                        let nums: Vec<f64> = stack.iter().filter_map(PsVal::as_num).collect();
                        if nums.len() == 1 {
                            gstate.ai_fill_rule = if nums[0].round() as i64 != 0 {
                                FillRule::EvenOdd
                            } else {
                                FillRule::NonZero
                            };
                        } else if nums.len() >= 3 {
                            let n = nums.len();
                            gstate.stroke_color = [
                                (nums[n - 3].clamp(0.0, 1.0) * 255.0).round() as u8,
                                (nums[n - 2].clamp(0.0, 1.0) * 255.0).round() as u8,
                                (nums[n - 1].clamp(0.0, 1.0) * 255.0).round() as u8,
                                255,
                            ];
                        }
                        stack.clear();
                    }
                    "newpath" => {
                        gstate.current_path.clear();
                        stack.clear();
                    }
                    "m" | "moveto" => {
                        if stack.len() >= 2 {
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let x = pop_num(&mut stack).unwrap_or(0.0);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            gstate.current_path.push(PathCommand::MoveTo { x, y });
                            gstate.cur_pt = (x, y);
                            gstate.subpath_start = (x, y);
                        }
                        stack.clear();
                    }
                    "rmoveto" => {
                        if stack.len() >= 2 {
                            let dy = pop_num(&mut stack).unwrap_or(0.0);
                            let dx = pop_num(&mut stack).unwrap_or(0.0);
                            let (x, y) = (gstate.cur_pt.0 + dx, gstate.cur_pt.1 + dy);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            gstate.current_path.push(PathCommand::MoveTo { x, y });
                            gstate.cur_pt = (x, y);
                            gstate.subpath_start = (x, y);
                        }
                        stack.clear();
                    }
                    "l" | "L" | "lineto" => {
                        if stack.len() >= 2 {
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let x = pop_num(&mut stack).unwrap_or(0.0);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            if gstate.current_path.is_empty() {
                                gstate.current_path.push(PathCommand::MoveTo {
                                    x: gstate.cur_pt.0,
                                    y: gstate.cur_pt.1,
                                });
                            }
                            gstate.current_path.push(PathCommand::LineTo { x, y });
                            gstate.cur_pt = (x, y);
                        }
                        stack.clear();
                    }
                    "rlineto" => {
                        if stack.len() >= 2 {
                            let dy = pop_num(&mut stack).unwrap_or(0.0);
                            let dx = pop_num(&mut stack).unwrap_or(0.0);
                            let (x, y) = (gstate.cur_pt.0 + dx, gstate.cur_pt.1 + dy);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            if gstate.current_path.is_empty() {
                                gstate.current_path.push(PathCommand::MoveTo {
                                    x: gstate.cur_pt.0,
                                    y: gstate.cur_pt.1,
                                });
                            }
                            gstate.current_path.push(PathCommand::LineTo { x, y });
                            gstate.cur_pt = (x, y);
                        }
                        stack.clear();
                    }
                    "c" | "C" | "curveto" => {
                        if stack.len() >= 6 {
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let x = pop_num(&mut stack).unwrap_or(0.0);
                            let cy2 = pop_num(&mut stack).unwrap_or(0.0);
                            let cx2 = pop_num(&mut stack).unwrap_or(0.0);
                            let cy1 = pop_num(&mut stack).unwrap_or(0.0);
                            let cx1 = pop_num(&mut stack).unwrap_or(0.0);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            if gstate.current_path.is_empty() {
                                gstate.current_path.push(PathCommand::MoveTo {
                                    x: gstate.cur_pt.0,
                                    y: gstate.cur_pt.1,
                                });
                            }
                            gstate.current_path.push(PathCommand::CubicTo {
                                cx1,
                                cy1,
                                cx2,
                                cy2,
                                x,
                                y,
                            });
                            gstate.cur_pt = (x, y);
                        }
                        stack.clear();
                    }
                    "rcurveto" => {
                        if stack.len() >= 6 {
                            let dy = pop_num(&mut stack).unwrap_or(0.0);
                            let dx = pop_num(&mut stack).unwrap_or(0.0);
                            let dy2 = pop_num(&mut stack).unwrap_or(0.0);
                            let dx2 = pop_num(&mut stack).unwrap_or(0.0);
                            let dy1 = pop_num(&mut stack).unwrap_or(0.0);
                            let dx1 = pop_num(&mut stack).unwrap_or(0.0);
                            let (x0, y0) = gstate.cur_pt;
                            let (x, y) = (x0 + dx, y0 + dy);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            if gstate.current_path.is_empty() {
                                gstate
                                    .current_path
                                    .push(PathCommand::MoveTo { x: x0, y: y0 });
                            }
                            gstate.current_path.push(PathCommand::CubicTo {
                                cx1: x0 + dx1,
                                cy1: y0 + dy1,
                                cx2: x0 + dx2,
                                cy2: y0 + dy2,
                                x,
                                y,
                            });
                            gstate.cur_pt = (x, y);
                        }
                        stack.clear();
                    }
                    "v" | "V" => {
                        if stack.len() >= 4 {
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let x = pop_num(&mut stack).unwrap_or(0.0);
                            let cy2 = pop_num(&mut stack).unwrap_or(0.0);
                            let cx2 = pop_num(&mut stack).unwrap_or(0.0);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            if gstate.current_path.is_empty() {
                                gstate.current_path.push(PathCommand::MoveTo {
                                    x: gstate.cur_pt.0,
                                    y: gstate.cur_pt.1,
                                });
                            }
                            gstate.current_path.push(PathCommand::CubicTo {
                                cx1: gstate.cur_pt.0,
                                cy1: gstate.cur_pt.1,
                                cx2,
                                cy2,
                                x,
                                y,
                            });
                            gstate.cur_pt = (x, y);
                        }
                        stack.clear();
                    }
                    "y" | "Y" => {
                        if stack.len() >= 4 {
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let x = pop_num(&mut stack).unwrap_or(0.0);
                            let cy1 = pop_num(&mut stack).unwrap_or(0.0);
                            let cx1 = pop_num(&mut stack).unwrap_or(0.0);
                            if gstate.current_path.len() >= MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            if gstate.current_path.is_empty() {
                                gstate.current_path.push(PathCommand::MoveTo {
                                    x: gstate.cur_pt.0,
                                    y: gstate.cur_pt.1,
                                });
                            }
                            gstate.current_path.push(PathCommand::CubicTo {
                                cx1,
                                cy1,
                                cx2: x,
                                cy2: y,
                                x,
                                y,
                            });
                            gstate.cur_pt = (x, y);
                        }
                        stack.clear();
                    }
                    "h" | "H" | "closepath" => {
                        if !gstate.current_path.is_empty() {
                            gstate.current_path.push(PathCommand::Close);
                            gstate.cur_pt = gstate.subpath_start;
                        }
                        stack.clear();
                    }
                    "re" => {
                        if stack.len() >= 4 {
                            let h = pop_num(&mut stack).unwrap_or(0.0);
                            let w = pop_num(&mut stack).unwrap_or(0.0);
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let x = pop_num(&mut stack).unwrap_or(0.0);
                            if gstate.current_path.len() + 5 > MAX_SVG_PATH_COMMANDS {
                                return Err(error("EPS path exceeds 32,768 commands."));
                            }
                            gstate.current_path.push(PathCommand::MoveTo { x, y });
                            gstate
                                .current_path
                                .push(PathCommand::LineTo { x: x + w, y });
                            gstate
                                .current_path
                                .push(PathCommand::LineTo { x: x + w, y: y + h });
                            gstate
                                .current_path
                                .push(PathCommand::LineTo { x, y: y + h });
                            gstate.current_path.push(PathCommand::Close);
                            gstate.cur_pt = (x, y);
                            gstate.subpath_start = (x, y);
                        }
                        stack.clear();
                    }
                    "rectfill" | "rectstroke" | "rectclip" => {
                        if stack.len() >= 4 {
                            let h = pop_num(&mut stack).unwrap_or(0.0);
                            let w = pop_num(&mut stack).unwrap_or(0.0);
                            let y = pop_num(&mut stack).unwrap_or(0.0);
                            let x = pop_num(&mut stack).unwrap_or(0.0);
                            let rect_cmds = vec![
                                PathCommand::MoveTo { x, y },
                                PathCommand::LineTo { x: x + w, y },
                                PathCommand::LineTo { x: x + w, y: y + h },
                                PathCommand::LineTo { x, y: y + h },
                                PathCommand::Close,
                            ];
                            if op == "rectclip" {
                                apply_clip_commands(
                                    &rect_cmds,
                                    FillRule::NonZero,
                                    &mut gstate,
                                    options,
                                    &loc,
                                    report,
                                );
                            } else {
                                sync_bucket_clip(
                                    &mut current_bucket,
                                    &mut buckets,
                                    gstate.clip.as_deref(),
                                );
                                emit_painted_path(
                                    &rect_cmds,
                                    op == "rectfill",
                                    op == "rectstroke",
                                    FillRule::NonZero,
                                    &gstate,
                                    options,
                                    &mut current_bucket,
                                    report,
                                    &mut vector_elements,
                                )?;
                            }
                        }
                        stack.clear();
                    }
                    "*u" => {
                        in_compound = true;
                        compound_cmds.clear();
                        compound_paint = None;
                        stack.clear();
                    }
                    "*U" => {
                        in_compound = false;
                        if !compound_cmds.is_empty()
                            && let Some((do_fill, do_stroke, rule)) = compound_paint.take()
                        {
                            sync_bucket_clip(
                                &mut current_bucket,
                                &mut buckets,
                                gstate.clip.as_deref(),
                            );
                            emit_painted_path(
                                &compound_cmds,
                                do_fill,
                                do_stroke,
                                rule,
                                &gstate,
                                options,
                                &mut current_bucket,
                                report,
                                &mut vector_elements,
                            )?;
                        }
                        compound_cmds.clear();
                        stack.clear();
                    }
                    "clip" | "eoclip" => {
                        let rule = if op == "eoclip" {
                            FillRule::EvenOdd
                        } else {
                            FillRule::NonZero
                        };
                        apply_clip_commands(
                            &gstate.current_path.clone(),
                            rule,
                            &mut gstate,
                            options,
                            &loc,
                            report,
                        );
                        pending_clip = None;
                        stack.clear();
                    }
                    "W" | "W*" => {
                        let rule = if op == "W*" {
                            FillRule::EvenOdd
                        } else {
                            FillRule::NonZero
                        };
                        pending_clip = Some(rule);
                        stack.clear();
                    }
                    "n" | "N" => {
                        if let Some(rule) = pending_clip.take() {
                            apply_clip_commands(
                                &gstate.current_path.clone(),
                                rule,
                                &mut gstate,
                                options,
                                &loc,
                                report,
                            );
                        } else if in_compound && !gstate.current_path.is_empty() {
                            if op == "n"
                                && !matches!(gstate.current_path.last(), Some(PathCommand::Close))
                            {
                                gstate.current_path.push(PathCommand::Close);
                            }
                            if compound_cmds.len() + gstate.current_path.len()
                                > MAX_SVG_PATH_COMMANDS
                            {
                                return Err(error("EPS compound path exceeds 32,768 commands."));
                            }
                            compound_cmds.extend_from_slice(&gstate.current_path);
                        }
                        gstate.current_path.clear();
                        stack.clear();
                    }
                    "f" | "F" | "f*" | "fill" | "eofill" | "s" | "S" | "stroke" | "B" | "B*"
                    | "b" | "b*" => {
                        if matches!(op.as_str(), "s" | "b" | "b*" | "f")
                            && !gstate.current_path.is_empty()
                            && !matches!(gstate.current_path.last(), Some(PathCommand::Close))
                        {
                            gstate.current_path.push(PathCommand::Close);
                        }
                        if let Some(clip_rule) = pending_clip.take() {
                            apply_clip_commands(
                                &gstate.current_path.clone(),
                                clip_rule,
                                &mut gstate,
                                options,
                                &loc,
                                report,
                            );
                        }
                        let do_fill = matches!(
                            op.as_str(),
                            "f" | "F" | "f*" | "fill" | "eofill" | "B" | "B*" | "b" | "b*"
                        );
                        let do_stroke =
                            matches!(op.as_str(), "s" | "S" | "stroke" | "B" | "B*" | "b" | "b*");
                        let fill_rule = if matches!(op.as_str(), "f*" | "eofill" | "B*" | "b*") {
                            FillRule::EvenOdd
                        } else {
                            gstate.ai_fill_rule
                        };

                        if in_compound {
                            if compound_cmds.len() + gstate.current_path.len()
                                > MAX_SVG_PATH_COMMANDS
                            {
                                return Err(error("EPS compound path exceeds 32,768 commands."));
                            }
                            compound_cmds.extend_from_slice(&gstate.current_path);
                            compound_paint = Some((do_fill, do_stroke, fill_rule));
                        } else {
                            sync_bucket_clip(
                                &mut current_bucket,
                                &mut buckets,
                                gstate.clip.as_deref(),
                            );
                            emit_painted_path(
                                &gstate.current_path,
                                do_fill,
                                do_stroke,
                                fill_rule,
                                &gstate,
                                options,
                                &mut current_bucket,
                                report,
                                &mut vector_elements,
                            )?;
                        }
                        gstate.current_path.clear();
                        stack.clear();
                    }
                    "shfill" => {
                        if let Some(PsVal::Dict(sh_dict)) = stack.last()
                            && let Some(mut paint) = parse_ps_shading_dict(sh_dict)
                        {
                            paint.matrix = affine_multiply(gstate.ctm, paint.matrix);
                            if paint.validate().is_ok() {
                                let (commands, rule) = if let Some((cmds, r)) = &gstate.clip_geom {
                                    (cmds.clone(), *r)
                                } else {
                                    (
                                        vec![
                                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                            PathCommand::LineTo { x: bw, y: 0.0 },
                                            PathCommand::LineTo { x: bw, y: bh },
                                            PathCommand::LineTo { x: 0.0, y: bh },
                                            PathCommand::Close,
                                        ],
                                        FillRule::NonZero,
                                    )
                                };
                                // Shading already embeds gstate.clip_geom as its PaintedPath geometry!
                                sync_bucket_clip(&mut current_bucket, &mut buckets, None);
                                current_bucket.elements.push(Element {
                                    id: new_id("element"),
                                    name: "EPS Shading".into(),
                                    transform: Transform::default(),
                                    kind: ElementKind::PaintedPath {
                                        commands,
                                        fill_rule: rule,
                                        fill: [0; 4],
                                        stroke: [0; 4],
                                        fill_gradient: Some(paint),
                                        stroke_gradient: None,
                                        stroke_width: 0.0,
                                    },
                                    ..Element::default()
                                });
                                report.statistics.elements += 1;
                                vector_elements += 1;
                            }
                        }
                        stack.clear();
                    }
                    "findfont" => {
                        if let Some(PsVal::Name(fname)) = stack.pop() {
                            gstate.font_family = fname;
                        }
                        stack.clear();
                    }
                    "scalefont" => {
                        if let Some(sz) = pop_num(&mut stack) {
                            let s = sz.abs().max(1.0);
                            gstate.font_matrix = [s, 0.0, 0.0, s, 0.0, 0.0];
                        }
                        stack.clear();
                    }
                    "makefont" => {
                        if let Some(nums) = stack.pop().and_then(|v| v.as_nums())
                            && let Ok(m) = <[f64; 6]>::try_from(nums)
                        {
                            gstate.font_matrix = m;
                        }
                        stack.clear();
                    }
                    "setfont" => {
                        stack.clear();
                    }
                    "show" => {
                        if let Some(PsVal::Str(raw_str)) = stack.pop() {
                            let norm = normalize_text(&raw_str);
                            if !norm.trim().is_empty() {
                                sync_bucket_clip(
                                    &mut current_bucket,
                                    &mut buckets,
                                    gstate.clip.as_deref(),
                                );
                                let pos_m = [
                                    gstate.font_matrix[0],
                                    gstate.font_matrix[1],
                                    gstate.font_matrix[2],
                                    gstate.font_matrix[3],
                                    gstate.cur_pt.0 + gstate.font_matrix[4],
                                    gstate.cur_pt.1 + gstate.font_matrix[5],
                                ];
                                let stage_m = affine_multiply(gstate.ctm, pos_m);
                                let fs = stage_m[2]
                                    .hypot(stage_m[3])
                                    .max(stage_m[0].hypot(stage_m[1]))
                                    .clamp(1.0, 1024.0);
                                let unit_m = [
                                    stage_m[0] / fs,
                                    stage_m[1] / fs,
                                    stage_m[2] / fs,
                                    stage_m[3] / fs,
                                    stage_m[4],
                                    stage_m[5],
                                ];
                                let box_m =
                                    affine_multiply(unit_m, [1.0, 0.0, 0.0, -1.0, 0.0, fs * 0.8]);
                                let tf = Transform::from_matrix(box_m).unwrap_or(Transform {
                                    x: box_m[4],
                                    y: box_m[5],
                                    ..Transform::default()
                                });
                                let rgb = (u32::from(gstate.fill_color[0]) << 16)
                                    | (u32::from(gstate.fill_color[1]) << 8)
                                    | u32::from(gstate.fill_color[2]);
                                let mut fmt = TextFormat {
                                    font: gstate.font_family.clone(),
                                    size: fs,
                                    color: rgb,
                                    ..TextFormat::default()
                                };
                                fmt.set_field_options(Some(AuthoredTextFieldOptions {
                                    padding_left: 0.0,
                                    padding_right: 0.0,
                                    padding_top: 0.0,
                                    padding_bottom: 0.0,
                                    auto_size: "left".into(),
                                    ..AuthoredTextFieldOptions::default()
                                }));
                                let w = ((norm.chars().count() as f64) * fs * 0.62 + fs * 0.5)
                                    .clamp(16.0, 16_384.0);
                                let h = (fs * 1.25 + 4.0).clamp(16.0, 16_384.0);
                                let kind = ElementKind::TextBox {
                                    text: norm.clone(),
                                    width: w,
                                    height: h,
                                    format: fmt,
                                    word_wrap: false,
                                    multiline: norm.contains('\n'),
                                };
                                let final_kind = if TextFieldState::from_kind(&kind)
                                    .and_then(|f| f.validate().ok())
                                    .is_some()
                                {
                                    kind
                                } else {
                                    ElementKind::Text {
                                        text: norm.clone(),
                                        font_family: gstate.font_family.clone(),
                                        font_size: fs,
                                        color: gstate.fill_color,
                                    }
                                };
                                current_bucket.elements.push(Element {
                                    id: new_id("element"),
                                    name: format!("EPS Text ({norm})"),
                                    transform: tf,
                                    kind: final_kind,
                                    ..Element::default()
                                });
                                report.statistics.elements += 1;
                                vector_elements += 1;
                                gstate.current_path.clear();
                            }
                        }
                        stack.clear();
                    }
                    _ => {
                        stack.clear();
                    }
                }
            }
        }
    }

    if !current_bucket.elements.is_empty() || !current_bucket.mask_elements.is_empty() {
        buckets.push(current_bucket);
    }

    if vector_elements > 0 {
        report.diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: "eps-content-stream-parsed".to_owned(),
            location: source.to_owned(),
            message: format!(
                "Parsed {vector_elements} vector element(s) ({total_ops} ops) from PostScript/AICB EPS."
            ),
        });
        return Ok(EpsParsedArtwork {
            buckets,
            bounds,
            stage_bounds,
            used_tiff_fallback: false,
        });
    }

    // Zero vector paths decoded -> fall back to embedded binary EPS TIFF preview if present!
    let Some(preview) = binary_preview else {
        return Err(error(
            "EPS has no binary TIFF preview and no decodable vector paths. Export the artwork as PDF or SVG.",
        ));
    };

    let mut decoder = Decoder::new(Cursor::new(preview))
        .map_err(|e| error(&format!("Invalid EPS preview: {e}")))?;
    let (width, height) = decoder.dimensions().map_err(|e| error(&e.to_string()))?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(error(
            "EPS preview dimensions exceed the 4096-pixel portable bitmap limit. Export as PDF or SVG.",
        ));
    }
    let image = if let Some(indexed) = indexed_preview(preview, &mut decoder, width, height) {
        indexed
    } else {
        let mut ordinary =
            image::codecs::tiff::TiffDecoder::new(Cursor::new(preview)).map_err(|e| {
                error(&format!(
                    "Unsupported TIFF preview ({e}). Save the artwork as PDF or SVG."
                ))
            })?;
        ordinary
            .set_limits(image::Limits::default())
            .map_err(|e| error(&e.to_string()))?;
        image::DynamicImage::from_decoder(ordinary)
            .map_err(|e| error(&e.to_string()))?
            .to_rgba8()
    };

    let fallback_bounds = parsed_bbox.unwrap_or([0.0, 0.0, f64::from(width), f64::from(height)]);
    let bw_u32 = (fallback_bounds[2] - fallback_bounds[0])
        .round()
        .clamp(1.0, 65535.0) as u32;
    let bh_u32 = (fallback_bounds[3] - fallback_bounds[1])
        .round()
        .clamp(1.0, 65535.0) as u32;

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| error(&e.to_string()))?;
    let id = new_id("resource");
    let resource = neoflash_core::ResourceUnit::from_bytes(
        id.clone(),
        format!("eps/{id}.png"),
        "image/png",
        &png,
    )
    .map_err(|e| error(&e.to_string()))?;

    report.warning(
        "eps-tiff-preview",
        source,
        format!(
            "Imported the embedded {width}×{height} TIFF preview as a bitmap. Export as PDF or SVG for editable vector artwork."
        ),
    );
    report.statistics.elements += 1;

    let element = Element {
        id: new_id("element"),
        name: source.into(),
        transform: Transform {
            scale_x: f64::from(bw_u32) / f64::from(width),
            scale_y: f64::from(bh_u32) / f64::from(height),
            ..Default::default()
        },
        kind: ElementKind::Bitmap {
            resource_id: id,
            width,
            height,
            smoothing: true,
        },
        ..Default::default()
    };

    let _ = resource;
    Ok(EpsParsedArtwork {
        buckets: vec![VectorGroupBucket {
            name: source.into(),
            elements: vec![element],
            mask_elements: vec![],
            mask_mode: MaskMode::Geometry,
        }],
        bounds: [
            fallback_bounds[0],
            fallback_bounds[1],
            fallback_bounds[0] + f64::from(bw_u32),
            fallback_bounds[1] + f64::from(bh_u32),
        ],
        stage_bounds: [0.0, 0.0, f64::from(bw_u32), f64::from(bh_u32)],
        used_tiff_fallback: true,
    })
}

pub(super) fn import_with_bounds(
    project: &mut Project,
    bytes: &[u8],
    source: &str,
    options: &VectorImportOptions,
) -> Result<(ImportReport, [f64; 4]), ImportError> {
    let mut report = ImportReport {
        source: source.into(),
        format: ImportFormat::Unknown,
        version: Some("EPS".into()),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: vec![],
        binary_fla: None,
        swf: None,
        swc: None,
    };

    let mut art = parse_eps_artwork(
        bytes,
        source,
        options,
        (project.width, project.height),
        &mut report,
    )?;

    if art.used_tiff_fallback
        && let Some(preview) = section(bytes, 20)
    {
        // Re-materialize the PNG resource into `project.resources` and link element's resource_id
        let error = |message: &str| ImportError::Container(format!("{source}: {message}"));
        let mut decoder = Decoder::new(Cursor::new(preview))
            .map_err(|e| error(&format!("Invalid EPS preview: {e}")))?;
        let (width, height) = decoder.dimensions().map_err(|e| error(&e.to_string()))?;
        let image = if let Some(indexed) = indexed_preview(preview, &mut decoder, width, height) {
            indexed
        } else {
            let mut ordinary = image::codecs::tiff::TiffDecoder::new(Cursor::new(preview))
                .map_err(|e| error(&e.to_string()))?;
            ordinary
                .set_limits(image::Limits::default())
                .map_err(|e| error(&e.to_string()))?;
            image::DynamicImage::from_decoder(ordinary)
                .map_err(|e| error(&e.to_string()))?
                .to_rgba8()
        };
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| error(&e.to_string()))?;
        let id = new_id("resource");
        let resource = neoflash_core::ResourceUnit::from_bytes(
            id.clone(),
            format!("eps/{id}.png"),
            "image/png",
            &png,
        )
        .map_err(|e| error(&e.to_string()))?;
        project.resources.push(resource);
        if let Some(el) = art.buckets.first_mut().and_then(|b| b.elements.first_mut())
            && let ElementKind::Bitmap { resource_id, .. } = &mut el.kind
        {
            *resource_id = id;
        }
    }

    if options.resize_stage_to_viewbox {
        let w = (art.bounds[2] - art.bounds[0]).round().clamp(1.0, 65535.0) as u32;
        let h = (art.bounds[3] - art.bounds[1]).round().clamp(1.0, 65535.0) as u32;
        project.width = w;
        project.height = h;
    }

    assemble_vector_buckets(project, art.buckets, source, options, &mut report);
    report.can_import = true;
    Ok((report, art.stage_bounds))
}
