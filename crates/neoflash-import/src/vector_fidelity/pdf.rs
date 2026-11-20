//! Follow PDF page references. Illustrator also embeds previews, private editing data and ICC
//! profiles as streams; none of those streams consists of page marking operators.
use super::*;
use lopdf::{Dictionary, Document, Object, ObjectId};

pub(super) struct Page {
    pub content: String,
    pub bounds: [f64; 4],
    pub rotation: i64,
    pub shadings: BTreeMap<String, GradientPaint>,
    pub patterns: BTreeMap<String, GradientPaint>,
}

impl Page {
    pub fn size_and_transform(&self) -> ((u32, u32), [f64; 6]) {
        let [x0, y0, x1, y1] = self.bounds;
        let w = (x1 - x0).round().clamp(1., 65535.) as u32;
        let h = (y1 - y0).round().clamp(1., 65535.) as u32;
        match self.rotation.rem_euclid(360) {
            90 => ((h, w), [0., 1., 1., 0., -y0, -x0]),
            180 => ((w, h), [-1., 0., 0., 1., x1, -y0]),
            270 => ((h, w), [0., -1., -1., 0., y1, x1]),
            _ => ((w, h), [1., 0., 0., -1., -x0, y1]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShadingColorSpace {
    Gray,
    Rgb,
    Cmyk,
}

impl ShadingColorSpace {
    fn channels(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }

    fn to_rgba(self, comps: &[f64]) -> Option<Color> {
        match self {
            Self::Gray => {
                let g = (*comps.first()?).clamp(0.0, 1.0);
                let v = (g * 255.0).round() as u8;
                Some([v, v, v, 255])
            }
            Self::Rgb => {
                if comps.len() < 3 {
                    return None;
                }
                Some([
                    (comps[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (comps[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (comps[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                    255,
                ])
            }
            Self::Cmyk => {
                if comps.len() < 4 {
                    return None;
                }
                let c = comps[0].clamp(0.0, 1.0);
                let m = comps[1].clamp(0.0, 1.0);
                let y = comps[2].clamp(0.0, 1.0);
                let k = comps[3].clamp(0.0, 1.0);
                let inv_k = 1.0 - k;
                Some([
                    ((1.0 - c) * inv_k * 255.0).round() as u8,
                    ((1.0 - m) * inv_k * 255.0).round() as u8,
                    ((1.0 - y) * inv_k * 255.0).round() as u8,
                    255,
                ])
            }
        }
    }
}

fn inherited<'a>(doc: &'a Document, mut id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..64 {
        let page = doc.get_dictionary(id).ok()?;
        if let Ok(value) = page.get(key) {
            return doc.dereference(value).ok().map(|v| v.1);
        }
        id = page.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn scalar(object: &Object) -> Option<f64> {
    object
        .as_float()
        .ok()
        .map(f64::from)
        .or_else(|| object.as_i64().ok().map(|i| i as f64))
}

fn numbers(object: &Object) -> Option<Vec<f64>> {
    object.as_array().ok()?.iter().map(scalar).collect()
}

fn parse_color_space(doc: &Document, obj: &Object) -> Option<ShadingColorSpace> {
    let resolved = doc.dereference(obj).map_or(obj, |(_, o)| o);
    if let Ok(name) = resolved.as_name() {
        return match name {
            b"DeviceRGB" | b"CalRGB" => Some(ShadingColorSpace::Rgb),
            b"DeviceGray" | b"CalGray" => Some(ShadingColorSpace::Gray),
            b"DeviceCMYK" => Some(ShadingColorSpace::Cmyk),
            _ => None,
        };
    }
    let arr = resolved.as_array().ok()?;
    let kind = arr.first()?.as_name().ok()?;
    match kind {
        b"DeviceRGB" | b"CalRGB" => Some(ShadingColorSpace::Rgb),
        b"DeviceGray" | b"CalGray" => Some(ShadingColorSpace::Gray),
        b"DeviceCMYK" => Some(ShadingColorSpace::Cmyk),
        b"ICCBased" => {
            let stream_obj = doc.dereference(arr.get(1)?).ok()?.1;
            let dict = match stream_obj {
                Object::Stream(s) => &s.dict,
                Object::Dictionary(d) => d,
                _ => return None,
            };
            if let Ok(alt) = dict.get(b"Alternate")
                && let Some(cs) = parse_color_space(doc, alt)
            {
                return Some(cs);
            }
            match dict.get(b"N").ok().and_then(|n| n.as_i64().ok())? {
                1 => Some(ShadingColorSpace::Gray),
                3 => Some(ShadingColorSpace::Rgb),
                4 => Some(ShadingColorSpace::Cmyk),
                _ => None,
            }
        }
        _ => None,
    }
}

fn function_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    let resolved = doc.dereference(obj).map_or(obj, |(_, o)| o);
    match resolved {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn function_domain(dict: &Dictionary) -> [f64; 2] {
    dict.get(b"Domain")
        .ok()
        .and_then(numbers)
        .and_then(|v| <[f64; 2]>::try_from(v).ok())
        .filter(|d| d[0].is_finite() && d[1].is_finite())
        .unwrap_or([0.0, 1.0])
}

fn eval_pdf_function(
    doc: &Document,
    fn_obj: &Object,
    u: f64,
    expected_channels: usize,
    depth: usize,
) -> Option<Vec<f64>> {
    if depth > 8 || !u.is_finite() {
        return None;
    }
    let resolved = doc.dereference(fn_obj).map_or(fn_obj, |(_, o)| o);
    if let Ok(arr) = resolved.as_array() {
        if arr.len() != expected_channels {
            return None;
        }
        let mut out = Vec::with_capacity(expected_channels);
        for item in arr {
            let vals = eval_pdf_function(doc, item, u, 1, depth + 1)?;
            out.push(*vals.first()?);
        }
        return Some(out);
    }

    let dict = function_dict(doc, resolved)?;
    let fn_type = dict.get(b"FunctionType").ok()?.as_i64().ok()?;
    let [d0, d1] = function_domain(dict);
    let lo = d0.min(d1);
    let hi = d0.max(d1);
    let x = u.clamp(lo, hi);

    match fn_type {
        2 => {
            // Exponential interpolation: c(x) = C0 + t^N * (C1 - C0)
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
            if let Ok(range_obj) = dict.get(b"Range")
                && let Some(ranges) = numbers(range_obj)
            {
                for (i, pair) in ranges
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .take(expected_channels)
                    .enumerate()
                {
                    out[i] = out[i].clamp(pair[0].min(pair[1]), pair[0].max(pair[1]));
                }
            }
            Some(out)
        }
        3 => {
            // Stitching function over k sub-functions
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
            eval_pdf_function(doc, &functions[seg], mapped, expected_channels, depth + 1)
        }
        _ => None,
    }
}

fn exact_linear_ramp(
    doc: &Document,
    fn_obj: &Object,
    cs: ShadingColorSpace,
) -> Option<(Color, Color)> {
    let dict = function_dict(doc, fn_obj)?;
    if dict.get(b"FunctionType").ok()?.as_i64().ok()? != 2 {
        return None;
    }
    let n = dict.get(b"N").ok().and_then(scalar)?;
    if (n - 1.0).abs() > 1e-6 {
        return None;
    }
    let [d0, d1] = function_domain(dict);
    let a = cs.to_rgba(&eval_pdf_function(doc, fn_obj, d0, cs.channels(), 0)?)?;
    let b = cs.to_rgba(&eval_pdf_function(doc, fn_obj, d1, cs.channels(), 0)?)?;
    Some((a, b))
}

fn extract_shading_stops(
    doc: &Document,
    fn_obj: &Object,
    cs: ShadingColorSpace,
    domain_override: Option<[f64; 2]>,
) -> Option<Vec<GradientStop>> {
    let resolved = doc.dereference(fn_obj).map_or(fn_obj, |(_, o)| o);
    let [t0, t1] = domain_override
        .or_else(|| function_dict(doc, resolved).map(function_domain))
        .unwrap_or([0.0, 1.0]);

    // 1. Exact 2-stop linear FunctionType 2
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

    // 2. Exact multi-stop piecewise-linear FunctionType 3 when <= 14 continuous intervals
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
                eval_pdf_function(doc, sub_fn, e0, cs.channels(), 1),
                eval_pdf_function(doc, sub_fn, e1, cs.channels(), 1),
            ) else {
                exact_ok = false;
                break;
            };
            let (Some(start_color), Some(end_color)) =
                (cs.to_rgba(&start_vals), cs.to_rgba(&end_vals))
            else {
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

    // 3. General curve / stitched / channel-array sampling into <= 15 monotonic stops
    //    (covers exponential FunctionType 2 N != 1, arbitrary Encode/Domain FunctionType 3,
    //    discontinuous stitched transitions, and per-channel Function arrays).
    let mut sample_fracs: Vec<f64> = Vec::with_capacity(15);
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
        let comps = eval_pdf_function(doc, resolved, u, cs.channels(), 0)?;
        let color = cs.to_rgba(&comps)?;
        stops.push(GradientStop { ratio, color });
    }
    (stops.len() >= 2 && stops.len() <= 15).then_some(stops)
}

fn remap_inner_radius_stops(stops: Vec<GradientStop>, inner_ratio: f64) -> Vec<GradientStop> {
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

pub(super) fn parse_shading(doc: &Document, object: &Object) -> Option<GradientPaint> {
    let shading = function_dict(doc, object)?;
    let shading_type = shading.get(b"ShadingType").ok()?.as_i64().ok()?;
    if !matches!(shading_type, 2 | 3) {
        return None;
    }
    let cs = parse_color_space(doc, shading.get(b"ColorSpace").ok()?)?;
    let fn_obj = shading.get(b"Function").ok()?;
    let domain_override = shading
        .get(b"Domain")
        .ok()
        .and_then(numbers)
        .and_then(|v| <[f64; 2]>::try_from(v).ok())
        .filter(|d| d[0].is_finite() && d[1].is_finite());
    let mut stops = extract_shading_stops(doc, fn_obj, cs, domain_override)?;

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
            stops = remap_inner_radius_stops(stops, r0 / r1);
            let dx = x0 - x1;
            let dy = y0 - y1;
            let dist = dx.hypot(dy);
            let s = r1 / 819.2;
            let (matrix, focal_point_ratio) = if dist > 1e-9 {
                let cos_t = dx / dist;
                let sin_t = dy / dist;
                (
                    [s * cos_t, s * sin_t, -s * sin_t, s * cos_t, x1, y1],
                    (dist / r1).clamp(-0.999, 0.999),
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

fn parse_shading_pattern(doc: &Document, object: &Object) -> Option<GradientPaint> {
    let pat = function_dict(doc, object)?;
    if pat.get(b"PatternType").ok()?.as_i64().ok()? != 2 {
        return None;
    }
    let mut paint = parse_shading(doc, pat.get(b"Shading").ok()?)?;
    if let Some(mat) = pat
        .get(b"Matrix")
        .ok()
        .and_then(numbers)
        .and_then(|v| <[f64; 6]>::try_from(v).ok())
    {
        paint.matrix = affine_multiply(mat, paint.matrix);
        paint.validate().ok()?;
    }
    Some(paint)
}

pub(super) fn pages(bytes: &[u8]) -> Result<Vec<Page>, ImportError> {
    let error = |msg: &str| ImportError::Container(format!("PDF page import: {msg}"));
    let doc = Document::load_mem(bytes).map_err(|e| error(&e.to_string()))?;
    if doc.is_encrypted() {
        return Err(error("Unlock the document and save an unencrypted PDF."));
    }
    let mut result = Vec::new();
    let mut total = 0usize;
    for id in doc.page_iter() {
        if result.len() >= 256 {
            return Err(error("document exceeds 256 pages"));
        }
        let bounds = inherited(&doc, id, b"CropBox")
            .or_else(|| inherited(&doc, id, b"MediaBox"))
            .and_then(numbers)
            .and_then(|v| <[f64; 4]>::try_from(v).ok())
            .ok_or_else(|| error("missing or invalid page bounds"))?;
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            return Err(error("empty page bounds"));
        }
        let rotation = inherited(&doc, id, b"Rotate")
            .and_then(|v| v.as_i64().ok())
            .unwrap_or(0);
        let mut content = Vec::new();
        let limit = (64 * 1024 * 1024usize - total).min(16 * 1024 * 1024);
        for stream_id in doc.get_page_contents(id) {
            let stream = doc
                .get_object(stream_id)
                .and_then(Object::as_stream)
                .map_err(|e| error(&e.to_string()))?;
            let remaining = limit.saturating_sub(content.len());
            match stream.filters().unwrap_or_default().as_slice() {
                [] if stream.content.len() <= remaining => {
                    content.extend_from_slice(&stream.content)
                }
                [b"FlateDecode" | b"Fl"] => {
                    flate2::read::ZlibDecoder::new(stream.content.as_slice())
                        .take(remaining as u64 + 1)
                        .read_to_end(&mut content)
                        .map_err(|e| error(&e.to_string()))?;
                }
                [] => return Err(error("page instructions exceed the decoding budget")),
                _ => {
                    return Err(error(
                        "unsupported page compression; save the PDF using Flate compression or export to SVG.",
                    ));
                }
            }
            content.push(b'\n');
            if content.len() > limit {
                return Err(error("page instructions exceed the decoding budget"));
            }
        }
        total += content.len();
        let mut shadings = BTreeMap::new();
        let mut patterns = BTreeMap::new();
        if let Some(resources) = inherited(&doc, id, b"Resources").and_then(|v| v.as_dict().ok()) {
            if let Some(dict) = resources
                .get(b"Shading")
                .ok()
                .and_then(|obj| doc.dereference(obj).ok())
                .and_then(|v| v.1.as_dict().ok())
            {
                for (name, value) in dict.iter() {
                    if let Some(paint) = parse_shading(&doc, value) {
                        shadings.insert(String::from_utf8_lossy(name).into_owned(), paint);
                    }
                }
            }
            if let Some(dict) = resources
                .get(b"Pattern")
                .ok()
                .and_then(|obj| doc.dereference(obj).ok())
                .and_then(|v| v.1.as_dict().ok())
            {
                for (name, value) in dict.iter() {
                    if let Some(paint) = parse_shading_pattern(&doc, value) {
                        patterns.insert(String::from_utf8_lossy(name).into_owned(), paint);
                    }
                }
            }
        }
        result.push(Page {
            content: String::from_utf8_lossy(&content).into_owned(),
            bounds,
            rotation,
            shadings,
            patterns,
        });
    }
    if result.is_empty() {
        return Err(error("document has no readable pages"));
    }
    Ok(result)
}
