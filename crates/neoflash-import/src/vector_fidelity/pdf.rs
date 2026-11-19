//! Follow PDF page references. Illustrator also embeds previews, private editing data and ICC
//! profiles as streams; none of those streams consists of page marking operators.
use super::*;
use lopdf::{Dictionary, Document, Object, ObjectId};

pub(super) struct Page {
    pub content: String,
    pub bounds: [f64; 4],
    pub rotation: i64,
    pub shadings: BTreeMap<String, GradientPaint>,
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

fn numbers(object: &Object) -> Option<Vec<f64>> {
    object
        .as_array()
        .ok()?
        .iter()
        .map(|v| v.as_float().ok().map(f64::from))
        .collect()
}

fn rgb(function: &Dictionary, key: &[u8]) -> Option<Color> {
    let c = numbers(function.get(key).ok()?)?;
    let [r, g, b] = *c.as_slice() else {
        return None;
    };
    Some([
        (r.clamp(0., 1.) * 255.).round() as u8,
        (g.clamp(0., 1.) * 255.).round() as u8,
        (b.clamp(0., 1.) * 255.).round() as u8,
        255,
    ])
}

fn linear_ramp(f: &Dictionary) -> Option<(Color, Color)> {
    (f.get(b"FunctionType").ok()?.as_i64().ok()? == 2 && f.get(b"N").ok()?.as_float().ok()? == 1.)
        .then_some(())?;
    Some((rgb(f, b"C0")?, rgb(f, b"C1")?))
}

fn axial_shading(doc: &Document, object: &Object) -> Option<GradientPaint> {
    let shading = doc.dereference(object).ok()?.1.as_dict().ok()?;
    (shading.get(b"ShadingType").ok()?.as_i64().ok()? == 2
        && shading.get(b"ColorSpace").ok()?.as_name().ok()? == b"DeviceRGB")
        .then_some(())?;
    let coords = numbers(shading.get(b"Coords").ok()?)?;
    let [x1, y1, x2, y2] = *coords.as_slice() else {
        return None;
    };
    let function = doc
        .dereference(shading.get(b"Function").ok()?)
        .ok()?
        .1
        .as_dict()
        .ok()?;
    let mut stops = Vec::new();
    if let Some((a, b)) = linear_ramp(function) {
        stops.extend([
            GradientStop { ratio: 0, color: a },
            GradientStop {
                ratio: 255,
                color: b,
            },
        ]);
    } else {
        if function.get(b"FunctionType").ok()?.as_i64().ok()? != 3 {
            return None;
        }
        let ranges = numbers(function.get(b"Bounds").ok()?)?;
        let functions = function.get(b"Functions").ok()?.as_array().ok()?;
        let encode = numbers(function.get(b"Encode").ok()?)?;
        // A stitched Illustrator ramp consists of consecutive, continuous linear intervals.
        if functions.len() != ranges.len() + 1
            || functions.len() > 14
            || encode.len() != 2 * functions.len()
            || encode
                .as_chunks::<2>()
                .0
                .iter()
                .any(|pair| *pair != [0., 1.])
        {
            return None;
        }
        let ends = ranges.into_iter().chain(std::iter::once(1.));
        for (function, end) in functions.iter().zip(ends) {
            let (start_color, end_color) =
                linear_ramp(doc.dereference(function).ok()?.1.as_dict().ok()?)?;
            if stops.is_empty() {
                stops.push(GradientStop {
                    ratio: 0,
                    color: start_color,
                });
            }
            if stops.last()?.color != start_color {
                return None;
            }
            let ratio = (end.clamp(0., 1.) * 255.).round() as u8;
            if stops.last()?.ratio >= ratio {
                return None;
            }
            stops.push(GradientStop {
                ratio,
                color: end_color,
            });
        }
    }
    Some(GradientPaint {
        kind: GradientKind::Linear,
        matrix: axial_gradient_matrix(x1, y1, x2, y2),
        stops,
        ..Default::default()
    })
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
        if let Some(dict) = inherited(&doc, id, b"Resources")
            .and_then(|v| v.as_dict().ok())
            .and_then(|resources| resources.get(b"Shading").ok())
            .and_then(|obj| doc.dereference(obj).ok())
            .and_then(|v| v.1.as_dict().ok())
        {
            for (name, value) in dict.iter() {
                if let Some(paint) = axial_shading(&doc, value) {
                    shadings.insert(String::from_utf8_lossy(name).into_owned(), paint);
                }
            }
        }
        result.push(Page {
            content: String::from_utf8_lossy(&content).into_owned(),
            bounds,
            rotation,
            shadings,
        });
    }
    if result.is_empty() {
        return Err(error("document has no readable pages"));
    }
    Ok(result)
}
