//! PDF 1.4–1.7 ColorSpaces (`/DeviceRGB`, `/DeviceCMYK`, `/DeviceGray`, `/CalRGB`, `/CalGray`,
//! `/ICCBased`, `/Separation`, `/DeviceN`, `/Indexed`, `/Pattern`) and Function evaluators
//! (`/FunctionType 0` sampled, `2` exponential, `3` stitching, and `4` PostScript calculator).

use super::pdf_xref::decode_pdf_stream_bounded;
use super::*;
use lopdf::{Dictionary, Document, Object};

const MAX_FUNCTION_STREAM_BYTES: usize = 4 * 1024 * 1024;
const MAX_PS_CALC_OPS: usize = 4096;
const MAX_PS_CALC_STACK: usize = 128;

pub(super) fn scalar(object: &Object) -> Option<f64> {
    object
        .as_float()
        .ok()
        .map(f64::from)
        .or_else(|| object.as_i64().ok().map(|i| i as f64))
}

pub(super) fn numbers(object: &Object) -> Option<Vec<f64>> {
    object.as_array().ok()?.iter().map(scalar).collect()
}

pub(super) fn function_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    let resolved = doc.dereference(obj).map_or(obj, |(_, o)| o);
    match resolved {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

pub(super) fn cmyk_to_rgba(c: f64, m: f64, y: f64, k: f64) -> Color {
    let inv_k = 1.0 - k.clamp(0.0, 1.0);
    [
        ((1.0 - c.clamp(0.0, 1.0)) * inv_k * 255.0).round() as u8,
        ((1.0 - m.clamp(0.0, 1.0)) * inv_k * 255.0).round() as u8,
        ((1.0 - y.clamp(0.0, 1.0)) * inv_k * 255.0).round() as u8,
        255,
    ]
}

#[derive(Clone, Debug)]
pub(super) enum PdfColorSpace {
    Gray {
        gamma: f64,
    },
    Rgb {
        gamma: [f64; 3],
    },
    Cmyk,
    Separation {
        alternate: Box<PdfColorSpace>,
        tint_transform: Object,
    },
    DeviceN {
        num_components: usize,
        alternate: Box<PdfColorSpace>,
        tint_transform: Object,
    },
    Indexed {
        base: Box<PdfColorSpace>,
        hival: usize,
        lookup: Vec<u8>,
    },
    Pattern {
        underlying: Option<Box<PdfColorSpace>>,
    },
}

impl PdfColorSpace {
    pub fn channels(&self) -> usize {
        match self {
            Self::Gray { .. } => 1,
            Self::Rgb { .. } => 3,
            Self::Cmyk => 4,
            Self::Separation { .. } => 1,
            Self::DeviceN { num_components, .. } => *num_components,
            Self::Indexed { .. } => 1,
            Self::Pattern { underlying } => underlying.as_ref().map_or(0, |u| u.channels()),
        }
    }

    pub fn to_rgba(&self, doc: &Document, comps: &[f64], depth: usize) -> Option<Color> {
        if depth > 8 {
            return None;
        }
        match self {
            Self::Gray { gamma } => {
                let g = (*comps.first()?).clamp(0.0, 1.0);
                let corrected = if (*gamma - 1.0).abs() < 1e-6 || *gamma <= 0.0 {
                    g
                } else {
                    g.powf(gamma.clamp(0.1, 10.0)).clamp(0.0, 1.0)
                };
                let v = (corrected * 255.0).round() as u8;
                Some([v, v, v, 255])
            }
            Self::Rgb { gamma } => {
                if comps.len() < 3 {
                    return None;
                }
                let ch = |idx: usize| -> u8 {
                    let v = comps[idx].clamp(0.0, 1.0);
                    let gm = gamma[idx];
                    let c = if (gm - 1.0).abs() < 1e-6 || gm <= 0.0 {
                        v
                    } else {
                        v.powf(gm.clamp(0.1, 10.0)).clamp(0.0, 1.0)
                    };
                    (c * 255.0).round() as u8
                };
                Some([ch(0), ch(1), ch(2), 255])
            }
            Self::Cmyk => {
                if comps.len() < 4 {
                    return None;
                }
                Some(cmyk_to_rgba(comps[0], comps[1], comps[2], comps[3]))
            }
            Self::Separation {
                alternate,
                tint_transform,
            } => {
                let tint = (*comps.first()?).clamp(0.0, 1.0);
                let out_ch = alternate.channels();
                if let Some(mapped) =
                    eval_pdf_function_multi(doc, tint_transform, &[tint], out_ch, depth + 1)
                {
                    alternate.to_rgba(doc, &mapped, depth + 1)
                } else {
                    // Fallback linear tint from white to black
                    let v = ((1.0 - tint) * 255.0).round() as u8;
                    Some([v, v, v, 255])
                }
            }
            Self::DeviceN {
                num_components,
                alternate,
                tint_transform,
            } => {
                if comps.len() < *num_components {
                    return None;
                }
                let out_ch = alternate.channels();
                let mapped = eval_pdf_function_multi(
                    doc,
                    tint_transform,
                    &comps[..*num_components],
                    out_ch,
                    depth + 1,
                )?;
                alternate.to_rgba(doc, &mapped, depth + 1)
            }
            Self::Indexed {
                base,
                hival,
                lookup,
            } => {
                let idx = (*comps.first()?).round().clamp(0.0, *hival as f64) as usize;
                let bch = base.channels().max(1);
                let start = idx.checked_mul(bch)?;
                let slice = lookup.get(start..start + bch)?;
                let norm: Vec<f64> = slice.iter().map(|&b| f64::from(b) / 255.0).collect();
                base.to_rgba(doc, &norm, depth + 1)
            }
            Self::Pattern { underlying } => {
                let under = underlying.as_ref()?;
                under.to_rgba(doc, comps, depth + 1)
            }
        }
    }
}

fn extract_lookup_bytes(doc: &Document, obj: &Object) -> Option<Vec<u8>> {
    let resolved = doc.dereference(obj).map_or(obj, |(_, o)| o);
    match resolved {
        Object::String(bytes, _) => Some(bytes.clone()),
        Object::Stream(stream) => decode_pdf_stream_bounded(stream, MAX_FUNCTION_STREAM_BYTES).ok(),
        _ => None,
    }
}

pub(super) fn parse_pdf_color_space(
    doc: &Document,
    obj: &Object,
    resources_cs: Option<&Dictionary>,
    depth: usize,
) -> Option<PdfColorSpace> {
    if depth > 8 {
        return None;
    }
    let resolved = doc.dereference(obj).map_or(obj, |(_, o)| o);
    if let Ok(name) = resolved.as_name() {
        return match name {
            b"DeviceRGB" | b"RGB" => Some(PdfColorSpace::Rgb { gamma: [1.0; 3] }),
            b"DeviceGray" | b"G" => Some(PdfColorSpace::Gray { gamma: 1.0 }),
            b"DeviceCMYK" | b"CMYK" => Some(PdfColorSpace::Cmyk),
            b"Pattern" => Some(PdfColorSpace::Pattern { underlying: None }),
            other => {
                let cs_dict = resources_cs?;
                let entry = cs_dict.get(other).ok()?;
                parse_pdf_color_space(doc, entry, None, depth + 1)
            }
        };
    }

    let arr = resolved.as_array().ok()?;
    let kind = arr.first()?.as_name().ok()?;
    match kind {
        b"DeviceRGB" | b"RGB" => Some(PdfColorSpace::Rgb { gamma: [1.0; 3] }),
        b"DeviceGray" | b"G" => Some(PdfColorSpace::Gray { gamma: 1.0 }),
        b"DeviceCMYK" | b"CMYK" => Some(PdfColorSpace::Cmyk),
        b"CalGray" => {
            let gamma = arr
                .get(1)
                .and_then(|o| function_dict(doc, o))
                .and_then(|d| d.get(b"Gamma").ok())
                .and_then(scalar)
                .unwrap_or(1.0);
            Some(PdfColorSpace::Gray { gamma })
        }
        b"CalRGB" => {
            let gamma = arr
                .get(1)
                .and_then(|o| function_dict(doc, o))
                .and_then(|d| d.get(b"Gamma").ok())
                .and_then(numbers)
                .and_then(|v| <[f64; 3]>::try_from(v).ok())
                .unwrap_or([1.0, 1.0, 1.0]);
            Some(PdfColorSpace::Rgb { gamma })
        }
        b"ICCBased" => {
            let stream_obj = doc.dereference(arr.get(1)?).ok()?.1;
            let dict = match stream_obj {
                Object::Stream(s) => &s.dict,
                Object::Dictionary(d) => d,
                _ => return None,
            };
            if let Ok(alt) = dict.get(b"Alternate")
                && let Some(cs) = parse_pdf_color_space(doc, alt, resources_cs, depth + 1)
            {
                return Some(cs);
            }
            match dict.get(b"N").ok().and_then(|n| n.as_i64().ok())? {
                1 => Some(PdfColorSpace::Gray { gamma: 1.0 }),
                3 => Some(PdfColorSpace::Rgb { gamma: [1.0; 3] }),
                4 => Some(PdfColorSpace::Cmyk),
                _ => None,
            }
        }
        b"Separation" => {
            // [/Separation name alternateSpace tintTransform]
            let alt_obj = arr.get(2)?;
            let tint_obj = arr.get(3)?.clone();
            let alternate = parse_pdf_color_space(doc, alt_obj, resources_cs, depth + 1)?;
            Some(PdfColorSpace::Separation {
                alternate: Box::new(alternate),
                tint_transform: tint_obj,
            })
        }
        b"DeviceN" => {
            // [/DeviceN names alternateSpace tintTransform (attributes)]
            let names = doc
                .dereference(arr.get(1)?)
                .map_or(arr.get(1)?, |(_, o)| o)
                .as_array()
                .ok()?;
            if names.is_empty() || names.len() > 32 {
                return None;
            }
            let alt_obj = arr.get(2)?;
            let tint_obj = arr.get(3)?.clone();
            let alternate = parse_pdf_color_space(doc, alt_obj, resources_cs, depth + 1)?;
            Some(PdfColorSpace::DeviceN {
                num_components: names.len(),
                alternate: Box::new(alternate),
                tint_transform: tint_obj,
            })
        }
        b"Indexed" | b"I" => {
            // [/Indexed base hival lookup]
            let base = parse_pdf_color_space(doc, arr.get(1)?, resources_cs, depth + 1)?;
            let hival = arr.get(2)?.as_i64().ok()?.clamp(0, 255) as usize;
            let lookup = extract_lookup_bytes(doc, arr.get(3)?)?;
            Some(PdfColorSpace::Indexed {
                base: Box::new(base),
                hival,
                lookup,
            })
        }
        b"Pattern" => {
            let underlying = arr
                .get(1)
                .and_then(|u| parse_pdf_color_space(doc, u, resources_cs, depth + 1))
                .map(Box::new);
            Some(PdfColorSpace::Pattern { underlying })
        }
        _ => None,
    }
}

pub(super) fn function_domain_1d(dict: &Dictionary) -> [f64; 2] {
    dict.get(b"Domain")
        .ok()
        .and_then(numbers)
        .and_then(|v| {
            if v.len() >= 2 && v[0].is_finite() && v[1].is_finite() {
                Some([v[0], v[1]])
            } else {
                None
            }
        })
        .unwrap_or([0.0, 1.0])
}

fn clamp_to_range(out: &mut [f64], dict: &Dictionary) {
    if let Ok(range_obj) = dict.get(b"Range")
        && let Some(ranges) = numbers(range_obj)
    {
        for (i, pair) in ranges.as_chunks::<2>().0.iter().take(out.len()).enumerate() {
            out[i] = out[i].clamp(pair[0].min(pair[1]), pair[0].max(pair[1]));
        }
    }
}

fn eval_sampled_function_0(
    stream: &lopdf::Stream,
    inputs: &[f64],
    expected_channels: usize,
) -> Option<Vec<f64>> {
    let dict = &stream.dict;
    let sizes: Vec<usize> = dict
        .get(b"Size")
        .ok()?
        .as_array()
        .ok()?
        .iter()
        .map(|o| {
            o.as_i64()
                .ok()
                .filter(|&v| v > 0 && v <= 65536)
                .map(|v| v as usize)
        })
        .collect::<Option<Vec<_>>>()?;
    let m = sizes.len();
    if m == 0 || m > 8 || inputs.len() < m {
        return None;
    }
    let bps = dict.get(b"BitsPerSample").ok()?.as_i64().ok()? as u32;
    if !matches!(bps, 1 | 2 | 4 | 8 | 12 | 16 | 24 | 32) {
        return None;
    }
    let domain = dict
        .get(b"Domain")
        .ok()
        .and_then(numbers)
        .filter(|v| v.len() >= 2 * m)?;
    let range = dict
        .get(b"Range")
        .ok()
        .and_then(numbers)
        .filter(|v| v.len() >= 2 * expected_channels)?;
    let n = (range.len() / 2).max(expected_channels);

    let encode: Vec<f64> = dict
        .get(b"Encode")
        .ok()
        .and_then(numbers)
        .filter(|v| v.len() >= 2 * m)
        .unwrap_or_else(|| {
            let mut v = Vec::with_capacity(2 * m);
            for &sz in &sizes {
                v.push(0.0);
                v.push((sz.saturating_sub(1)) as f64);
            }
            v
        });
    let decode: Vec<f64> = dict
        .get(b"Decode")
        .ok()
        .and_then(numbers)
        .filter(|v| v.len() >= 2 * n)
        .unwrap_or_else(|| range.clone());

    let raw = decode_pdf_stream_bounded(stream, MAX_FUNCTION_STREAM_BYTES).ok()?;

    // Map inputs into continuous sample index coordinates e_i in [0, Size[i] - 1]
    let mut coords = Vec::with_capacity(m);
    for i in 0..m {
        let d0 = domain[2 * i];
        let d1 = domain[2 * i + 1];
        let x = inputs[i].clamp(d0.min(d1), d0.max(d1));
        let t = if (d1 - d0).abs() < 1e-12 {
            0.0
        } else {
            (x - d0) / (d1 - d0)
        };
        let e0 = encode[2 * i];
        let e1 = encode[2 * i + 1];
        let e = (e0 + t * (e1 - e0)).clamp(0.0, (sizes[i].saturating_sub(1)) as f64);
        coords.push(e);
    }

    let max_sample_val = if bps == 32 {
        f64::from(u32::MAX)
    } else {
        ((1u64 << bps) - 1) as f64
    };

    let read_sample = |flat_sample_idx: usize, comp: usize| -> Option<f64> {
        let total_idx = flat_sample_idx.checked_mul(n)?.checked_add(comp)?;
        let raw_val: u64 = match bps {
            8 => u64::from(*raw.get(total_idx)?),
            16 => {
                let b = total_idx.checked_mul(2)?;
                u64::from(u16::from_be_bytes([*raw.get(b)?, *raw.get(b + 1)?]))
            }
            24 => {
                let b = total_idx.checked_mul(3)?;
                (u64::from(*raw.get(b)?) << 16)
                    | (u64::from(*raw.get(b + 1)?) << 8)
                    | u64::from(*raw.get(b + 2)?)
            }
            32 => {
                let b = total_idx.checked_mul(4)?;
                u64::from(u32::from_be_bytes([
                    *raw.get(b)?,
                    *raw.get(b + 1)?,
                    *raw.get(b + 2)?,
                    *raw.get(b + 3)?,
                ]))
            }
            1 | 2 | 4 | 12 => {
                let bit_pos = total_idx.checked_mul(bps as usize)?;
                let mut acc = 0u64;
                for bit in 0..(bps as usize) {
                    let pos = bit_pos + bit;
                    let byte = *raw.get(pos / 8)?;
                    let shift = 7 - (pos % 8);
                    acc = (acc << 1) | u64::from((byte >> shift) & 1);
                }
                acc
            }
            _ => return None,
        };
        let frac = (raw_val as f64) / max_sample_val;
        let dec0 = decode[2 * comp];
        let dec1 = decode[2 * comp + 1];
        let r0 = range[2 * comp];
        let r1 = range[2 * comp + 1];
        Some((dec0 + frac * (dec1 - dec0)).clamp(r0.min(r1), r0.max(r1)))
    };

    // Multilinear interpolation over 2^m corners (m <= 8 -> <= 256 corners)
    let mut strides = vec![1usize; m];
    for i in 1..m {
        strides[i] = strides[i - 1].checked_mul(sizes[i - 1])?;
    }

    let mut out = vec![0.0; expected_channels];
    let corners = 1usize << m;
    for corner in 0..corners {
        let mut weight = 1.0;
        let mut flat_idx = 0usize;
        for dim in 0..m {
            let i0 = (coords[dim].floor() as usize).min(sizes[dim].saturating_sub(1));
            let i1 = (i0 + 1).min(sizes[dim].saturating_sub(1));
            let frac = coords[dim] - (i0 as f64);
            let hi = ((corner >> dim) & 1) == 1;
            let idx = if hi { i1 } else { i0 };
            let w = if i0 == i1 {
                if hi { 0.0 } else { 1.0 }
            } else if hi {
                frac
            } else {
                1.0 - frac
            };
            weight *= w;
            flat_idx = flat_idx.checked_add(idx.checked_mul(strides[dim])?)?;
        }
        if weight <= 0.0 {
            continue;
        }
        for (c, slot) in out.iter_mut().enumerate().take(expected_channels) {
            *slot += weight * read_sample(flat_idx, c)?;
        }
    }
    clamp_to_range(&mut out, dict);
    Some(out)
}

#[derive(Clone, Debug)]
enum PsCalcToken {
    Num(f64),
    Bool(bool),
    Op(String),
    Proc(Vec<PsCalcToken>),
}

fn tokenize_ps_calc(src: &str) -> Option<Vec<PsCalcToken>> {
    fn parse_seq(
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        depth: usize,
    ) -> Option<Vec<PsCalcToken>> {
        if depth > 16 {
            return None;
        }
        let mut out = Vec::new();
        while let Some(&ch) = chars.peek() {
            if out.len() > MAX_PS_CALC_OPS {
                return None;
            }
            if ch.is_ascii_whitespace() {
                chars.next();
                continue;
            }
            if ch == '%' {
                for c in chars.by_ref() {
                    if c == '\n' || c == '\r' {
                        break;
                    }
                }
                continue;
            }
            if ch == '{' {
                chars.next();
                let inner = parse_seq(chars, depth + 1)?;
                out.push(PsCalcToken::Proc(inner));
                continue;
            }
            if ch == '}' {
                chars.next();
                return Some(out);
            }
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_whitespace() || matches!(c, '{' | '}' | '%') {
                    break;
                }
                word.push(chars.next()?);
            }
            match word.as_str() {
                "true" => out.push(PsCalcToken::Bool(true)),
                "false" => out.push(PsCalcToken::Bool(false)),
                _ if let Ok(n) = word.parse::<f64>() => out.push(PsCalcToken::Num(n)),
                _ => out.push(PsCalcToken::Op(word)),
            }
        }
        Some(out)
    }

    let trimmed = src.trim();
    let body = trimmed
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(trimmed);
    let mut chars = body.chars().peekable();
    parse_seq(&mut chars, 0)
}

#[derive(Clone, Debug)]
enum PsCalcVal {
    Num(f64),
    Bool(bool),
    Proc(Vec<PsCalcToken>),
}

impl PsCalcVal {
    fn as_num(&self) -> Option<f64> {
        match self {
            Self::Num(n) if n.is_finite() => Some(*n),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

fn exec_ps_calc(
    tokens: &[PsCalcToken],
    stack: &mut Vec<PsCalcVal>,
    ops_left: &mut usize,
) -> Option<()> {
    for tok in tokens {
        if *ops_left == 0 || stack.len() > MAX_PS_CALC_STACK {
            return None;
        }
        *ops_left -= 1;
        match tok {
            PsCalcToken::Num(n) => stack.push(PsCalcVal::Num(*n)),
            PsCalcToken::Bool(b) => stack.push(PsCalcVal::Bool(*b)),
            PsCalcToken::Proc(p) => stack.push(PsCalcVal::Proc(p.clone())),
            PsCalcToken::Op(op) => match op.as_str() {
                "pop" => {
                    stack.pop()?;
                }
                "exch" => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(b);
                    stack.push(a);
                }
                "dup" => {
                    let top = stack.last()?.clone();
                    stack.push(top);
                }
                "copy" => {
                    let n = stack.pop()?.as_num()?.round() as isize;
                    if n < 0
                        || (n as usize) > stack.len()
                        || stack.len() + (n as usize) > MAX_PS_CALC_STACK
                    {
                        return None;
                    }
                    let start = stack.len() - (n as usize);
                    let tail = stack[start..].to_vec();
                    stack.extend(tail);
                }
                "index" => {
                    let idx = stack.pop()?.as_num()?.round() as isize;
                    if idx < 0 || (idx as usize) >= stack.len() {
                        return None;
                    }
                    let val = stack[stack.len() - 1 - (idx as usize)].clone();
                    stack.push(val);
                }
                "roll" => {
                    let j = stack.pop()?.as_num()?.round() as isize;
                    let n = stack.pop()?.as_num()?.round() as isize;
                    if n < 0 || (n as usize) > stack.len() {
                        return None;
                    }
                    let nu = n as usize;
                    if nu > 1 {
                        let start = stack.len() - nu;
                        let shift = j.rem_euclid(n) as usize;
                        stack[start..].rotate_right(shift);
                    }
                }
                "cvr" => {
                    let v = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Num(v));
                }
                "cvi" | "truncate" => {
                    let v = stack.pop()?.as_num()?.trunc();
                    stack.push(PsCalcVal::Num(v));
                }
                "ceiling" => {
                    let v = stack.pop()?.as_num()?.ceil();
                    stack.push(PsCalcVal::Num(v));
                }
                "floor" => {
                    let v = stack.pop()?.as_num()?.floor();
                    stack.push(PsCalcVal::Num(v));
                }
                "round" => {
                    let v = stack.pop()?.as_num()?.round();
                    stack.push(PsCalcVal::Num(v));
                }
                "neg" => {
                    let v = -stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Num(v));
                }
                "abs" => {
                    let v = stack.pop()?.as_num()?.abs();
                    stack.push(PsCalcVal::Num(v));
                }
                "sqrt" => {
                    let v = stack.pop()?.as_num()?.max(0.0).sqrt();
                    stack.push(PsCalcVal::Num(v));
                }
                "sin" => {
                    let v = stack.pop()?.as_num()?.to_radians().sin();
                    stack.push(PsCalcVal::Num(v));
                }
                "cos" => {
                    let v = stack.pop()?.as_num()?.to_radians().cos();
                    stack.push(PsCalcVal::Num(v));
                }
                "atan" => {
                    let den = stack.pop()?.as_num()?;
                    let num = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Num(
                        num.atan2(den).to_degrees().rem_euclid(360.0),
                    ));
                }
                "exp" => {
                    let exp = stack.pop()?.as_num()?;
                    let base = stack.pop()?.as_num()?;
                    let r = base.powf(exp);
                    stack.push(PsCalcVal::Num(if r.is_finite() { r } else { 0.0 }));
                }
                "ln" => {
                    let v = stack.pop()?.as_num()?.max(1e-12).ln();
                    stack.push(PsCalcVal::Num(v));
                }
                "log" => {
                    let v = stack.pop()?.as_num()?.max(1e-12).log10();
                    stack.push(PsCalcVal::Num(v));
                }
                "add" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Num(a + b));
                }
                "sub" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Num(a - b));
                }
                "mul" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Num(a * b));
                }
                "div" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Num(if b.abs() < 1e-12 { 0.0 } else { a / b }));
                }
                "idiv" => {
                    let b = stack.pop()?.as_num()?.trunc() as i64;
                    let a = stack.pop()?.as_num()?.trunc() as i64;
                    if b == 0 {
                        return None;
                    }
                    stack.push(PsCalcVal::Num((a / b) as f64));
                }
                "mod" => {
                    let b = stack.pop()?.as_num()?.trunc() as i64;
                    let a = stack.pop()?.as_num()?.trunc() as i64;
                    if b == 0 {
                        return None;
                    }
                    stack.push(PsCalcVal::Num((a % b) as f64));
                }
                "eq" => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    let res = match (a, b) {
                        (PsCalcVal::Num(x), PsCalcVal::Num(y)) => (x - y).abs() <= 1e-9,
                        (PsCalcVal::Bool(x), PsCalcVal::Bool(y)) => x == y,
                        _ => false,
                    };
                    stack.push(PsCalcVal::Bool(res));
                }
                "ne" => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    let res = match (a, b) {
                        (PsCalcVal::Num(x), PsCalcVal::Num(y)) => (x - y).abs() > 1e-9,
                        (PsCalcVal::Bool(x), PsCalcVal::Bool(y)) => x != y,
                        _ => true,
                    };
                    stack.push(PsCalcVal::Bool(res));
                }
                "gt" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Bool(a > b));
                }
                "ge" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Bool(a >= b));
                }
                "lt" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Bool(a < b));
                }
                "le" => {
                    let b = stack.pop()?.as_num()?;
                    let a = stack.pop()?.as_num()?;
                    stack.push(PsCalcVal::Bool(a <= b));
                }
                "and" => match (stack.pop()?, stack.pop()?) {
                    (PsCalcVal::Bool(b), PsCalcVal::Bool(a)) => stack.push(PsCalcVal::Bool(a && b)),
                    (PsCalcVal::Num(b), PsCalcVal::Num(a)) => {
                        stack.push(PsCalcVal::Num(((a as i64) & (b as i64)) as f64))
                    }
                    _ => return None,
                },
                "or" => match (stack.pop()?, stack.pop()?) {
                    (PsCalcVal::Bool(b), PsCalcVal::Bool(a)) => stack.push(PsCalcVal::Bool(a || b)),
                    (PsCalcVal::Num(b), PsCalcVal::Num(a)) => {
                        stack.push(PsCalcVal::Num(((a as i64) | (b as i64)) as f64))
                    }
                    _ => return None,
                },
                "xor" => match (stack.pop()?, stack.pop()?) {
                    (PsCalcVal::Bool(b), PsCalcVal::Bool(a)) => stack.push(PsCalcVal::Bool(a ^ b)),
                    (PsCalcVal::Num(b), PsCalcVal::Num(a)) => {
                        stack.push(PsCalcVal::Num(((a as i64) ^ (b as i64)) as f64))
                    }
                    _ => return None,
                },
                "not" => match stack.pop()? {
                    PsCalcVal::Bool(b) => stack.push(PsCalcVal::Bool(!b)),
                    PsCalcVal::Num(n) => stack.push(PsCalcVal::Num((!(n as i64)) as f64)),
                    _ => return None,
                },
                "bitshift" => {
                    let shift = stack.pop()?.as_num()?.round() as i32;
                    let val = stack.pop()?.as_num()?.trunc() as i64;
                    let shifted = if shift >= 0 {
                        val.checked_shl(shift.min(62) as u32).unwrap_or(0)
                    } else {
                        val.checked_shr((-shift).min(62) as u32).unwrap_or(0)
                    };
                    stack.push(PsCalcVal::Num(shifted as f64));
                }
                "if" => {
                    let proc_body = match stack.pop()? {
                        PsCalcVal::Proc(p) => p,
                        _ => return None,
                    };
                    let cond = stack.pop()?.as_bool()?;
                    if cond {
                        exec_ps_calc(&proc_body, stack, ops_left)?;
                    }
                }
                "ifelse" => {
                    let false_proc = match stack.pop()? {
                        PsCalcVal::Proc(p) => p,
                        _ => return None,
                    };
                    let true_proc = match stack.pop()? {
                        PsCalcVal::Proc(p) => p,
                        _ => return None,
                    };
                    let cond = stack.pop()?.as_bool()?;
                    if cond {
                        exec_ps_calc(&true_proc, stack, ops_left)?;
                    } else {
                        exec_ps_calc(&false_proc, stack, ops_left)?;
                    }
                }
                _ => return None,
            },
        }
    }
    Some(())
}

fn eval_ps_calculator_4(
    stream: &lopdf::Stream,
    inputs: &[f64],
    expected_channels: usize,
) -> Option<Vec<f64>> {
    let dict = &stream.dict;
    let domain = dict.get(b"Domain").ok().and_then(numbers)?;
    let m = domain.len() / 2;
    if m == 0 || inputs.len() < m {
        return None;
    }
    let raw = decode_pdf_stream_bounded(stream, MAX_FUNCTION_STREAM_BYTES).ok()?;
    let code = std::str::from_utf8(&raw).ok()?;
    let tokens = tokenize_ps_calc(code)?;
    let mut stack = Vec::with_capacity(m + expected_channels + 8);
    for i in 0..m {
        let lo = domain[2 * i].min(domain[2 * i + 1]);
        let hi = domain[2 * i].max(domain[2 * i + 1]);
        stack.push(PsCalcVal::Num(inputs[i].clamp(lo, hi)));
    }
    let mut ops_left = MAX_PS_CALC_OPS;
    exec_ps_calc(&tokens, &mut stack, &mut ops_left)?;
    if stack.len() < expected_channels {
        return None;
    }
    let start = stack.len() - expected_channels;
    let mut out = Vec::with_capacity(expected_channels);
    for val in &stack[start..] {
        out.push(val.as_num()?);
    }
    clamp_to_range(&mut out, dict);
    Some(out)
}

pub(super) fn eval_pdf_function_multi(
    doc: &Document,
    fn_obj: &Object,
    inputs: &[f64],
    expected_channels: usize,
    depth: usize,
) -> Option<Vec<f64>> {
    if depth > 8 || inputs.is_empty() || inputs.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let resolved = doc.dereference(fn_obj).map_or(fn_obj, |(_, o)| o);
    if let Ok(arr) = resolved.as_array() {
        if arr.len() != expected_channels {
            return None;
        }
        let mut out = Vec::with_capacity(expected_channels);
        for item in arr {
            let vals = eval_pdf_function_multi(doc, item, inputs, 1, depth + 1)?;
            out.push(*vals.first()?);
        }
        return Some(out);
    }

    let dict = function_dict(doc, resolved)?;
    let fn_type = dict.get(b"FunctionType").ok()?.as_i64().ok()?;

    match fn_type {
        0 => {
            let Object::Stream(stream) = resolved else {
                return None;
            };
            eval_sampled_function_0(stream, inputs, expected_channels)
        }
        2 => {
            let [d0, d1] = function_domain_1d(dict);
            let x = inputs[0].clamp(d0.min(d1), d0.max(d1));
            let n = dict.get(b"N").ok().and_then(scalar)?;
            if !n.is_finite() || n <= 0.0 {
                return None;
            }
            let c0 = dict
                .get(b"C0")
                .ok()
                .and_then(numbers)
                .unwrap_or_else(|| vec![0.0; expected_channels]);
            let c1 = dict
                .get(b"C1")
                .ok()
                .and_then(numbers)
                .unwrap_or_else(|| vec![1.0; expected_channels]);
            if c0.len() < expected_channels || c1.len() < expected_channels {
                return None;
            }
            let t = if (d1 - d0).abs() < 1e-12 {
                0.0
            } else {
                ((x - d0) / (d1 - d0)).clamp(0.0, 1.0)
            };
            let factor = if (n - 1.0).abs() < 1e-9 { t } else { t.powf(n) };
            let mut out = Vec::with_capacity(expected_channels);
            for i in 0..expected_channels {
                out.push(c0[i] + factor * (c1[i] - c0[i]));
            }
            clamp_to_range(&mut out, dict);
            Some(out)
        }
        3 => {
            let [d0, d1] = function_domain_1d(dict);
            let x = inputs[0].clamp(d0.min(d1), d0.max(d1));
            let functions = dict.get(b"Functions").ok()?.as_array().ok()?;
            let k = functions.len();
            if k == 0 || k > 64 {
                return None;
            }
            let bounds = if k == 1 {
                Vec::new()
            } else {
                numbers(dict.get(b"Bounds").ok()?)?
            };
            let encode = numbers(dict.get(b"Encode").ok()?)?;
            if bounds.len() + 1 != k || encode.len() != 2 * k {
                return None;
            }
            let ascending = d1 >= d0;
            let mut seg = k - 1;
            for (idx, &b) in bounds.iter().enumerate() {
                if (ascending && x < b) || (!ascending && x > b) {
                    seg = idx;
                    break;
                }
            }
            let seg_start = if seg == 0 { d0 } else { bounds[seg - 1] };
            let seg_end = if seg + 1 == k { d1 } else { bounds[seg] };
            let e0 = encode[2 * seg];
            let e1 = encode[2 * seg + 1];
            let mapped = if (seg_end - seg_start).abs() < 1e-12 {
                e0
            } else {
                let frac = ((x - seg_start) / (seg_end - seg_start)).clamp(0.0, 1.0);
                e0 + frac * (e1 - e0)
            };
            eval_pdf_function_multi(
                doc,
                &functions[seg],
                &[mapped],
                expected_channels,
                depth + 1,
            )
        }
        4 => {
            let Object::Stream(stream) = resolved else {
                return None;
            };
            eval_ps_calculator_4(stream, inputs, expected_channels)
        }
        _ => None,
    }
}

fn exact_linear_ramp(
    doc: &Document,
    fn_obj: &Object,
    cs: &PdfColorSpace,
) -> Option<(Color, Color)> {
    // Only exact 2-stop when ColorSpace itself is linear (Gray/Rgb/Cmyk) and FunctionType 2 N == 1
    if !matches!(
        cs,
        PdfColorSpace::Gray { .. } | PdfColorSpace::Rgb { .. } | PdfColorSpace::Cmyk
    ) {
        return None;
    }
    let dict = function_dict(doc, fn_obj)?;
    if dict.get(b"FunctionType").ok()?.as_i64().ok()? != 2 {
        return None;
    }
    let n = dict.get(b"N").ok().and_then(scalar)?;
    if (n - 1.0).abs() > 1e-6 {
        return None;
    }
    let [d0, d1] = function_domain_1d(dict);
    let a = cs.to_rgba(
        doc,
        &eval_pdf_function_multi(doc, fn_obj, &[d0], cs.channels(), 0)?,
        0,
    )?;
    let b = cs.to_rgba(
        doc,
        &eval_pdf_function_multi(doc, fn_obj, &[d1], cs.channels(), 0)?,
        0,
    )?;
    Some((a, b))
}

pub(super) fn extract_shading_stops(
    doc: &Document,
    fn_obj: &Object,
    cs: &PdfColorSpace,
    domain_override: Option<[f64; 2]>,
) -> Option<Vec<GradientStop>> {
    let resolved = doc.dereference(fn_obj).map_or(fn_obj, |(_, o)| o);
    let [t0, t1] = domain_override
        .or_else(|| function_dict(doc, resolved).map(function_domain_1d))
        .unwrap_or([0.0, 1.0]);

    if domain_override.is_none()
        && let Some((c0, c1)) = exact_linear_ramp(doc, resolved, cs)
    {
        return Some(vec![
            GradientStop {
                ratio: 0,
                color: c0,
            },
            GradientStop {
                ratio: 255,
                color: c1,
            },
        ]);
    }

    if let Some(dict) = function_dict(doc, resolved)
        && dict.get(b"FunctionType").ok().and_then(|v| v.as_i64().ok()) == Some(3)
        && let (Some(functions), Some(bounds), Some(encode)) = (
            dict.get(b"Functions").ok().and_then(|v| v.as_array().ok()),
            dict.get(b"Bounds").ok().and_then(numbers),
            dict.get(b"Encode").ok().and_then(numbers),
        )
        && !functions.is_empty()
        && functions.len() <= 14
        && bounds.len() + 1 == functions.len()
        && encode.len() == 2 * functions.len()
        && (t1 - t0).abs() > 1e-9
    {
        let mut exact_stops: Vec<GradientStop> = Vec::with_capacity(functions.len() + 1);
        let mut exact_ok = true;
        for (idx, sub_fn) in functions.iter().enumerate() {
            let Some(sub_dict) = function_dict(doc, sub_fn) else {
                exact_ok = false;
                break;
            };
            let is_linear_sub = sub_dict
                .get(b"FunctionType")
                .ok()
                .and_then(|v| v.as_i64().ok())
                == Some(2)
                && sub_dict
                    .get(b"N")
                    .ok()
                    .and_then(scalar)
                    .is_some_and(|n| (n - 1.0).abs() < 1e-6);
            if !is_linear_sub {
                exact_ok = false;
                break;
            }
            let e0 = encode[2 * idx];
            let e1 = encode[2 * idx + 1];
            let (Some(start_vals), Some(end_vals)) = (
                eval_pdf_function_multi(doc, sub_fn, &[e0], cs.channels(), 1),
                eval_pdf_function_multi(doc, sub_fn, &[e1], cs.channels(), 1),
            ) else {
                exact_ok = false;
                break;
            };
            let (Some(start_color), Some(end_color)) = (
                cs.to_rgba(doc, &start_vals, 0),
                cs.to_rgba(doc, &end_vals, 0),
            ) else {
                exact_ok = false;
                break;
            };
            if idx == 0 {
                exact_stops.push(GradientStop {
                    ratio: 0,
                    color: start_color,
                });
            } else if exact_stops
                .last()
                .is_some_and(|prev| prev.color != start_color)
            {
                exact_ok = false;
                break;
            }
            let raw_end = if idx + 1 == functions.len() {
                t1
            } else {
                bounds[idx]
            };
            let frac = ((raw_end - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let ratio = (frac * 255.0).round() as u8;
            if exact_stops.last().is_some_and(|prev| prev.ratio >= ratio) {
                exact_ok = false;
                break;
            }
            exact_stops.push(GradientStop {
                ratio,
                color: end_color,
            });
        }
        if exact_ok && exact_stops.len() >= 2 && exact_stops.len() <= 15 {
            return Some(exact_stops);
        }
    }

    let mut sample_fracs: Vec<f64> = Vec::with_capacity(24);
    sample_fracs.push(0.0);
    sample_fracs.push(1.0);
    if let Some(dict) = function_dict(doc, resolved)
        && dict.get(b"FunctionType").ok().and_then(|v| v.as_i64().ok()) == Some(3)
        && let Some(bounds) = dict.get(b"Bounds").ok().and_then(numbers)
        && (t1 - t0).abs() > 1e-9
    {
        for b in bounds {
            let f = ((b - t0) / (t1 - t0)).clamp(0.0, 1.0);
            sample_fracs.push((f - 0.004).clamp(0.0, 1.0));
            sample_fracs.push(f);
            sample_fracs.push((f + 0.004).clamp(0.0, 1.0));
        }
    }
    const UNIFORM_GRID: [f64; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
    sample_fracs.extend_from_slice(&UNIFORM_GRID);
    sample_fracs.sort_by(|a, b| a.total_cmp(b));

    let mut ratio_grid: Vec<u8> = sample_fracs
        .into_iter()
        .map(|f| (f.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    ratio_grid.dedup();
    if ratio_grid.len() > 15 {
        let total = ratio_grid.len();
        let mut thinned = Vec::with_capacity(15);
        for i in 0..15 {
            let pick = (i * (total - 1) + 7) / 14;
            thinned.push(ratio_grid[pick]);
        }
        thinned.dedup();
        ratio_grid = thinned;
    }

    let mut stops = Vec::with_capacity(ratio_grid.len());
    for ratio in ratio_grid {
        let frac = f64::from(ratio) / 255.0;
        let u = t0 + frac * (t1 - t0);
        let comps = eval_pdf_function_multi(doc, resolved, &[u], cs.channels(), 0)?;
        let color = cs.to_rgba(doc, &comps, 0)?;
        stops.push(GradientStop { ratio, color });
    }
    (stops.len() >= 2 && stops.len() <= 15).then_some(stops)
}

pub(super) fn remap_inner_radius_stops(
    stops: Vec<GradientStop>,
    inner_ratio: f64,
) -> Vec<GradientStop> {
    let r_frac = inner_ratio.clamp(0.0, 0.98);
    if r_frac <= 1e-4 || stops.is_empty() {
        return stops;
    }
    let start_ratio = (r_frac * 255.0).round().clamp(1.0, 250.0) as u8;
    let max_extra = 14usize;
    let src = if stops.len() > max_extra {
        let n = stops.len();
        (0..max_extra)
            .map(|i| stops[(i * (n - 1)) / (max_extra - 1)].clone())
            .collect::<Vec<_>>()
    } else {
        stops
    };
    let mut out = Vec::with_capacity(src.len() + 1);
    out.push(GradientStop {
        ratio: 0,
        color: src[0].color,
    });
    for st in src {
        let mapped = (f64::from(start_ratio)
            + (f64::from(st.ratio) / 255.0) * f64::from(255 - start_ratio))
        .round()
        .clamp(f64::from(start_ratio), 255.0) as u8;
        if let Some(last) = out.last_mut()
            && last.ratio == mapped
        {
            last.color = st.color;
            continue;
        }
        out.push(GradientStop {
            ratio: mapped,
            color: st.color,
        });
    }
    out
}

#[derive(Clone, Debug)]
pub(super) enum PdfShadingPaint {
    Gradient(GradientPaint),
    Mesh(neoflash_core::VectorMeshPaint),
}

struct PdfBitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> PdfBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    fn remaining_bits(&self) -> usize {
        self.data
            .len()
            .saturating_mul(8)
            .saturating_sub(self.bit_pos)
    }

    fn align_to_byte(&mut self) {
        self.bit_pos = (self.bit_pos.saturating_add(7) & !7).min(self.data.len().saturating_mul(8));
    }

    fn read_bits(&mut self, bits: u32) -> Option<u32> {
        if bits == 0 || bits > 32 || self.remaining_bits() < bits as usize {
            return None;
        }
        let mut acc = 0u64;
        for _ in 0..bits {
            let byte_idx = self.bit_pos >> 3;
            let bit_idx = 7 - (self.bit_pos & 7);
            let bit = u64::from((self.data[byte_idx] >> bit_idx) & 1);
            acc = (acc << 1) | bit;
            self.bit_pos += 1;
        }
        Some(acc as u32)
    }
}

fn eval_shading_vertex_color(
    doc: &Document,
    cs: &PdfColorSpace,
    fn_obj: Option<&Object>,
    raw_comps: &[f64],
) -> Option<Color> {
    if let Some(fobj) = fn_obj {
        let t = *raw_comps.first()?;
        let resolved_fn = doc.dereference(fobj).map_or(fobj, |(_, r)| r);
        let mapped = if let Ok(arr) = resolved_fn.as_array() {
            let mut out = Vec::with_capacity(arr.len());
            for sub in arr {
                let v = eval_pdf_function_multi(doc, sub, &[t], 1, 0)?;
                out.push(*v.first()?);
            }
            out
        } else {
            eval_pdf_function_multi(doc, resolved_fn, &[t], cs.channels(), 0)?
        };
        cs.to_rgba(doc, &mapped, 0)
    } else {
        cs.to_rgba(doc, raw_comps, 0)
    }
}

pub(super) fn decode_mesh_shading_stream(
    doc: &Document,
    shading_type: i64,
    dict: &Dictionary,
    raw_bytes: &[u8],
    cs: &PdfColorSpace,
) -> Option<neoflash_core::VectorMeshPaint> {
    use neoflash_core::{
        MAX_VECTOR_MESH_PATCHES, MAX_VECTOR_MESH_TRIANGLES, VectorMeshPaint, VectorMeshPatch,
        VectorMeshTriangle,
    };

    let bpc_coord = dict.get(b"BitsPerCoordinate").ok()?.as_i64().ok()? as u32;
    if !matches!(bpc_coord, 1 | 2 | 4 | 8 | 12 | 16 | 24 | 32) {
        return None;
    }
    let bpc_comp = dict.get(b"BitsPerComponent").ok()?.as_i64().ok()? as u32;
    if !matches!(bpc_comp, 1 | 2 | 4 | 8 | 12 | 16) {
        return None;
    }
    let bpf = if shading_type == 5 {
        0u32
    } else {
        let f = dict.get(b"BitsPerFlag").ok()?.as_i64().ok()? as u32;
        if !matches!(f, 2 | 4 | 8) {
            return None;
        }
        f
    };

    let fn_obj = dict.get(b"Function").ok();
    let n_comp = if fn_obj.is_some() { 1 } else { cs.channels() };
    if n_comp == 0 || n_comp > 32 {
        return None;
    }

    let decode_obj = dict
        .get(b"Decode")
        .ok()
        .map(|o| doc.dereference(o).map_or(o, |(_, r)| r))?;
    let decode = numbers(decode_obj)?;
    if decode.len() < 4 + 2 * n_comp || !decode.iter().all(|v| v.is_finite()) {
        return None;
    }
    let (xmin, xmax, ymin, ymax) = (decode[0], decode[1], decode[2], decode[3]);
    let coord_max = if bpc_coord == 32 {
        f64::from(u32::MAX)
    } else {
        f64::from((1u32 << bpc_coord) - 1).max(1.0)
    };
    let comp_max = f64::from((1u32 << bpc_comp) - 1).max(1.0);

    let mut reader = PdfBitReader::new(raw_bytes);
    let read_xy = |r: &mut PdfBitReader<'_>| -> Option<[f64; 2]> {
        let rx = r.read_bits(bpc_coord)?;
        let ry = r.read_bits(bpc_coord)?;
        let x = xmin + (f64::from(rx) / coord_max) * (xmax - xmin);
        let y = ymin + (f64::from(ry) / coord_max) * (ymax - ymin);
        (x.is_finite() && y.is_finite()).then_some([x, y])
    };
    let read_color = |r: &mut PdfBitReader<'_>| -> Option<Color> {
        let mut comps = [0.0f64; 32];
        for (j, slot) in comps.iter_mut().enumerate().take(n_comp) {
            let rc = r.read_bits(bpc_comp)?;
            let c0 = decode[4 + 2 * j];
            let c1 = decode[4 + 2 * j + 1];
            *slot = c0 + (f64::from(rc) / comp_max) * (c1 - c0);
        }
        eval_shading_vertex_color(doc, cs, fn_obj, &comps[..n_comp])
    };

    let mut paint = VectorMeshPaint::default();

    match shading_type {
        4 => {
            type GouraudTriState = ([f64; 2], Color, [f64; 2], Color, [f64; 2], Color);
            let vertex_bits =
                (bpf as usize) + 2 * (bpc_coord as usize) + n_comp * (bpc_comp as usize);
            let aligned_vertex_bits = (vertex_bits + 7) & !7;
            let mut prev_tri: Option<GouraudTriState> = None;
            while reader.remaining_bits() >= vertex_bits
                && paint.triangles.len() < MAX_VECTOR_MESH_TRIANGLES
            {
                let flag = reader.read_bits(bpf)? & 0b11;
                let p_a = read_xy(&mut reader)?;
                let c_a = read_color(&mut reader)?;
                reader.align_to_byte();
                match flag {
                    0 => {
                        if reader.remaining_bits() < aligned_vertex_bits + vertex_bits {
                            break;
                        }
                        let _f_b = reader.read_bits(bpf)?;
                        let p_b = read_xy(&mut reader)?;
                        let c_b = read_color(&mut reader)?;
                        reader.align_to_byte();
                        let _f_c = reader.read_bits(bpf)?;
                        let p_c = read_xy(&mut reader)?;
                        let c_c = read_color(&mut reader)?;
                        reader.align_to_byte();
                        paint.triangles.push(VectorMeshTriangle {
                            points: [p_a, p_b, p_c],
                            colors: [c_a, c_b, c_c],
                        });
                        prev_tri = Some((p_a, c_a, p_b, c_b, p_c, c_c));
                    }
                    1 => {
                        let Some((_, _, p_b, c_b, p_c, c_c)) = prev_tri else {
                            break;
                        };
                        paint.triangles.push(VectorMeshTriangle {
                            points: [p_b, p_c, p_a],
                            colors: [c_b, c_c, c_a],
                        });
                        prev_tri = Some((p_b, c_b, p_c, c_c, p_a, c_a));
                    }
                    2 => {
                        let Some((p_0, c_0, _, _, p_c, c_c)) = prev_tri else {
                            break;
                        };
                        paint.triangles.push(VectorMeshTriangle {
                            points: [p_0, p_c, p_a],
                            colors: [c_0, c_c, c_a],
                        });
                        prev_tri = Some((p_0, c_0, p_c, c_c, p_a, c_a));
                    }
                    _ => break,
                }
            }
        }
        5 => {
            let vpr = dict.get(b"VerticesPerRow").ok()?.as_i64().ok()?;
            if !(2..=4096).contains(&vpr) {
                return None;
            }
            let vpr = vpr as usize;
            let vertex_bits = 2 * (bpc_coord as usize) + n_comp * (bpc_comp as usize);
            let max_vertices = (MAX_VECTOR_MESH_TRIANGLES / 2 + vpr + 2).min(65_536);
            let mut verts: Vec<([f64; 2], Color)> = Vec::new();
            while reader.remaining_bits() >= vertex_bits && verts.len() < max_vertices {
                let pt = read_xy(&mut reader)?;
                let col = read_color(&mut reader)?;
                reader.align_to_byte();
                verts.push((pt, col));
            }
            let rows = verts.len() / vpr;
            if rows < 2 {
                return None;
            }
            'rows_loop: for r in 0..(rows - 1) {
                for c in 0..(vpr - 1) {
                    if paint.triangles.len() + 2 > MAX_VECTOR_MESH_TRIANGLES {
                        break 'rows_loop;
                    }
                    let v00 = verts[r * vpr + c];
                    let v01 = verts[r * vpr + c + 1];
                    let v10 = verts[(r + 1) * vpr + c];
                    let v11 = verts[(r + 1) * vpr + c + 1];
                    paint.triangles.push(VectorMeshTriangle {
                        points: [v00.0, v01.0, v11.0],
                        colors: [v00.1, v01.1, v11.1],
                    });
                    paint.triangles.push(VectorMeshTriangle {
                        points: [v00.0, v11.0, v10.0],
                        colors: [v00.1, v11.1, v10.1],
                    });
                }
            }
        }
        6 => {
            let mut prev: Option<([[f64; 2]; 12], [Color; 4])> = None;
            while reader.remaining_bits() >= bpf as usize
                && paint.patches.len() < MAX_VECTOR_MESH_PATCHES
            {
                let start_bits = reader.remaining_bits();
                let Some(raw_flag) = reader.read_bits(bpf) else {
                    break;
                };
                let flag = raw_flag & 0b11;
                let need_coords = if flag == 0 { 12usize } else { 8usize };
                let need_cols = if flag == 0 { 4usize } else { 2usize };
                let need_bits = need_coords * 2 * (bpc_coord as usize)
                    + need_cols * n_comp * (bpc_comp as usize);
                if reader.remaining_bits() < need_bits {
                    let _ = start_bits;
                    break;
                }
                let mut pts = [[0.0; 2]; 12];
                let mut cols = [[0u8; 4]; 4];
                if flag == 0 {
                    for slot in &mut pts {
                        *slot = read_xy(&mut reader)?;
                    }
                    for slot in &mut cols {
                        *slot = read_color(&mut reader)?;
                    }
                } else {
                    let Some((pp, pc)) = prev else {
                        break;
                    };
                    match flag {
                        1 => {
                            pts[0..4].copy_from_slice(&pp[3..7]);
                            cols[0..2].copy_from_slice(&pc[1..3]);
                        }
                        2 => {
                            pts[0..4].copy_from_slice(&pp[6..10]);
                            cols[0..2].copy_from_slice(&pc[2..4]);
                        }
                        3 => {
                            pts[0..3].copy_from_slice(&pp[9..12]);
                            pts[3] = pp[0];
                            cols[0] = pc[3];
                            cols[1] = pc[0];
                        }
                        _ => break,
                    }
                    for slot in &mut pts[4..12] {
                        *slot = read_xy(&mut reader)?;
                    }
                    for slot in &mut cols[2..4] {
                        *slot = read_color(&mut reader)?;
                    }
                }
                reader.align_to_byte();
                paint
                    .patches
                    .push(VectorMeshPatch::from_pdf_coons_12(pts, cols));
                prev = Some((pts, cols));
            }
        }
        7 => {
            let mut prev: Option<([[f64; 2]; 16], [Color; 4])> = None;
            while reader.remaining_bits() >= bpf as usize
                && paint.patches.len() < MAX_VECTOR_MESH_PATCHES
            {
                let Some(raw_flag) = reader.read_bits(bpf) else {
                    break;
                };
                let flag = raw_flag & 0b11;
                let need_coords = if flag == 0 { 16usize } else { 12usize };
                let need_cols = if flag == 0 { 4usize } else { 2usize };
                let need_bits = need_coords * 2 * (bpc_coord as usize)
                    + need_cols * n_comp * (bpc_comp as usize);
                if reader.remaining_bits() < need_bits {
                    break;
                }
                let mut pts = [[0.0; 2]; 16];
                let mut cols = [[0u8; 4]; 4];
                if flag == 0 {
                    for slot in &mut pts {
                        *slot = read_xy(&mut reader)?;
                    }
                    for slot in &mut cols {
                        *slot = read_color(&mut reader)?;
                    }
                } else {
                    let Some((pp, pc)) = prev else {
                        break;
                    };
                    match flag {
                        1 => {
                            pts[0..4].copy_from_slice(&pp[3..7]);
                            cols[0..2].copy_from_slice(&pc[1..3]);
                        }
                        2 => {
                            pts[0..4].copy_from_slice(&pp[6..10]);
                            cols[0..2].copy_from_slice(&pc[2..4]);
                        }
                        3 => {
                            pts[0..3].copy_from_slice(&pp[9..12]);
                            pts[3] = pp[0];
                            cols[0] = pc[3];
                            cols[1] = pc[0];
                        }
                        _ => break,
                    }
                    for slot in &mut pts[4..16] {
                        *slot = read_xy(&mut reader)?;
                    }
                    for slot in &mut cols[2..4] {
                        *slot = read_color(&mut reader)?;
                    }
                }
                reader.align_to_byte();
                paint
                    .patches
                    .push(VectorMeshPatch::from_pdf_tensor_16(pts, cols));
                prev = Some((pts, cols));
            }
        }
        _ => return None,
    }

    paint.validate().ok()?;
    Some(paint)
}

pub(super) fn parse_shading_with_cs(
    doc: &Document,
    object: &Object,
    resources_cs: Option<&Dictionary>,
) -> Option<(PdfShadingPaint, Option<String>)> {
    let resolved_obj = doc.dereference(object).map_or(object, |(_, r)| r);
    let shading = function_dict(doc, resolved_obj)?;
    let shading_type = shading.get(b"ShadingType").ok()?.as_i64().ok()?;
    if matches!(shading_type, 4..=7) {
        let Object::Stream(stream) = resolved_obj else {
            return None;
        };
        let cs = parse_pdf_color_space(doc, shading.get(b"ColorSpace").ok()?, resources_cs, 0)?;
        if cs.channels() == 0 {
            return None;
        }
        let (decoded_bytes, _) =
            super::pdf_xref::decode_pdf_stream_with_doc(stream, Some(doc), 16 * 1024 * 1024, false)
                .ok()?;
        let mesh = decode_mesh_shading_stream(doc, shading_type, shading, &decoded_bytes, &cs)?;
        return Some((PdfShadingPaint::Mesh(mesh), None));
    }
    if !matches!(shading_type, 2 | 3) {
        return None;
    }
    let cs = parse_pdf_color_space(doc, shading.get(b"ColorSpace").ok()?, resources_cs, 0)?;
    if cs.channels() == 0 {
        return None;
    }
    let fn_obj = shading.get(b"Function").ok()?;
    let domain_override = shading
        .get(b"Domain")
        .ok()
        .and_then(numbers)
        .and_then(|v| <[f64; 2]>::try_from(v).ok())
        .filter(|d| d[0].is_finite() && d[1].is_finite());
    let mut stops = extract_shading_stops(doc, fn_obj, &cs, domain_override)?;
    let mut approx_warning = None;

    let paint = match shading_type {
        2 => {
            let coords = numbers(shading.get(b"Coords").ok()?)?;
            let [x1, y1, x2, y2] = *coords.as_slice() else {
                return None;
            };
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
            let coords = numbers(shading.get(b"Coords").ok()?)?;
            let [mut x0, mut y0, mut r0, mut x1, mut y1, mut r1] = *coords.as_slice() else {
                return None;
            };
            if r0 < 0.0 || r1 < 0.0 || (r0 <= 1e-9 && r1 <= 1e-9) {
                return None;
            }
            if r0 > r1 {
                std::mem::swap(&mut x0, &mut x1);
                std::mem::swap(&mut y0, &mut y1);
                std::mem::swap(&mut r0, &mut r1);
                for st in &mut stops {
                    st.ratio = 255u8.saturating_sub(st.ratio);
                }
                stops.reverse();
            }
            if r1 <= 1e-9 {
                return None;
            }
            let dx = x0 - x1;
            let dy = y0 - y1;
            let dist = dx.hypot(dy);
            let dr = r1 - r0;
            if dr <= 1e-9 {
                approx_warning = Some(
                    "PDF ShadingType 3 equal-radius (r0 == r1) shading cannot be represented as a conical radial focal gradient and was approximated."
                        .to_owned(),
                );
                if dist > 1e-9 {
                    GradientPaint {
                        kind: GradientKind::Linear,
                        matrix: axial_gradient_matrix(x0, y0, x1, y1),
                        stops,
                        ..Default::default()
                    }
                } else {
                    let s = r1 / 819.2;
                    let col = stops.last().map(|st| st.color).unwrap_or([0, 0, 0, 255]);
                    GradientPaint {
                        kind: GradientKind::Radial,
                        matrix: [s, 0.0, 0.0, s, x1, y1],
                        stops: vec![
                            GradientStop {
                                ratio: 0,
                                color: col,
                            },
                            GradientStop {
                                ratio: 255,
                                color: col,
                            },
                        ],
                        focal_point_ratio: 0.0,
                        ..Default::default()
                    }
                }
            } else {
                stops = remap_inner_radius_stops(stops, r0 / r1);
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
        }
        _ => return None,
    };
    paint.validate().ok()?;
    Some((PdfShadingPaint::Gradient(paint), approx_warning))
}

pub(super) fn parse_shading_pattern_with_cs(
    doc: &Document,
    object: &Object,
    resources_cs: Option<&Dictionary>,
) -> Option<(PdfShadingPaint, Option<String>)> {
    let pat = function_dict(doc, object)?;
    if pat.get(b"PatternType").ok()?.as_i64().ok()? != 2 {
        return None;
    }
    let (mut paint, warn) = parse_shading_with_cs(doc, pat.get(b"Shading").ok()?, resources_cs)?;
    if let Some(mat) = pat
        .get(b"Matrix")
        .ok()
        .and_then(numbers)
        .and_then(|v| <[f64; 6]>::try_from(v).ok())
    {
        match &mut paint {
            PdfShadingPaint::Gradient(grad) => {
                grad.matrix = affine_multiply(mat, grad.matrix);
                grad.validate().ok()?;
            }
            PdfShadingPaint::Mesh(mesh) => {
                mesh.transform_in_place(mat);
                mesh.validate().ok()?;
            }
        }
    }
    Some((paint, warn))
}
