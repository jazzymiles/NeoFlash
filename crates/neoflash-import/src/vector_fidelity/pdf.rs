//! PDF 1.4–1.7 & Adobe Illustrator (`.ai` / `com.adobe.pdf`) page tree, box priority
//! (`/ArtBox` > `/TrimBox` > `/BleedBox` > `/CropBox` > `/MediaBox`), scoped resources,
//! `/ExtGState`, `/Shading`, `/Pattern`, `/Font`, `/Form` XObjects (`<= 32` depth),
//! and `/Image` XObjects (`DCTDecode` / `FlateDecode` + `/SMask`, `<= 4096x4096`, `<= 64 MiB`).

use super::pdf_color_fn::{
    PdfColorSpace, cmyk_to_rgba, function_dict, numbers, parse_pdf_color_space,
    parse_shading_pattern_with_cs, parse_shading_with_cs, scalar,
};
use super::pdf_text_font::{PdfFontResource, parse_pdf_font_resource};
use super::pdf_xref::{
    MAX_PDF_DECODED_STREAM_BYTES, decode_pdf_stream_bounded, decode_pdf_stream_with_doc,
    load_pdf_document_bounded,
};
use super::*;
use image::{ImageEncoder, Limits, RgbaImage};
use lopdf::{Dictionary, Document, Object, ObjectId};
use neoflash_core::BlendMode;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;

pub(super) const MAX_PDF_FORM_DEPTH: usize = 32;
pub(super) const MAX_PDF_IMAGE_DIM: u32 = 4096;
pub(super) const MAX_PDF_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) struct PdfSoftMask {
    pub mode: MaskMode,
    pub form: Box<PdfFormXObject>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PdfExtGState {
    pub fill_alpha: Option<f64>,
    pub stroke_alpha: Option<f64>,
    pub blend_mode: Option<BlendMode>,
    pub blend_approx: Option<String>,
    pub line_width: Option<f64>,
    pub line_cap: Option<i64>,
    pub line_join: Option<i64>,
    pub miter_limit: Option<f64>,
    pub dash: Option<(Vec<f64>, f64)>,
    pub clear_smask: bool,
    pub smask: Option<PdfSoftMask>,
}

#[derive(Clone, Debug)]
pub(super) struct PdfFormXObject {
    pub id: Option<ObjectId>,
    pub name: String,
    pub bbox: [f64; 4],
    pub matrix: [f64; 6],
    pub content: String,
    pub scope: Box<PdfResourceScope>,
}

#[derive(Clone, Debug)]
pub(super) struct PdfImageXObject {
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(super) struct PdfResourceScope {
    pub doc: Arc<Document>,
    pub color_spaces: BTreeMap<String, PdfColorSpace>,
    pub shadings: BTreeMap<String, GradientPaint>,
    pub patterns: BTreeMap<String, GradientPaint>,
    pub ext_gstates: BTreeMap<String, PdfExtGState>,
    pub fonts: BTreeMap<String, PdfFontResource>,
    pub forms: BTreeMap<String, PdfFormXObject>,
    pub cyclic_forms: HashSet<String>,
    pub images: BTreeMap<String, PdfImageXObject>,
    pub unsupported_xobjects: BTreeMap<String, String>,
    pub warnings: Vec<(String, String)>,
}

impl PdfResourceScope {
    pub fn empty() -> Self {
        Self {
            doc: Arc::new(Document::new()),
            color_spaces: BTreeMap::new(),
            shadings: BTreeMap::new(),
            patterns: BTreeMap::new(),
            ext_gstates: BTreeMap::new(),
            fonts: BTreeMap::new(),
            forms: BTreeMap::new(),
            cyclic_forms: HashSet::new(),
            images: BTreeMap::new(),
            unsupported_xobjects: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }
}

pub(super) struct Page {
    pub content: String,
    /// Active artwork bounds (`/ArtBox` > `/TrimBox` > `/BleedBox` > `/CropBox` > `/MediaBox`).
    pub bounds: [f64; 4],
    /// Full artboard/page `/MediaBox` when present, used to compute exact Stage coordinates for `Paste in Place`.
    pub media_box: Option<[f64; 4]>,
    pub rotation: i64,
    pub scope: PdfResourceScope,
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

    /// Stage-space `[min_x, min_y, max_x, max_y]` (Y-down relative to `/MediaBox` top-left).
    pub fn stage_bounds(&self) -> [f64; 4] {
        let [bx0, by0, bx1, by1] = self.bounds;
        let bw = (bx1 - bx0).max(0.0);
        let bh = (by1 - by0).max(0.0);
        if let Some([mx0, _my0, _mx1, my1]) = self.media_box
            && self.media_box != Some(self.bounds)
        {
            let sx0 = bx0 - mx0;
            let sy0 = my1 - by1;
            return [sx0, sy0, sx0 + bw, sy0 + bh];
        }
        [0.0, 0.0, bw, bh]
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

fn valid_box(obj: &Object) -> Option<[f64; 4]> {
    let raw = numbers(obj)?;
    let [mut x0, mut y0, mut x1, mut y1] = <[f64; 4]>::try_from(raw).ok()?;
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

fn map_pdf_blend_mode_name(name: &[u8]) -> Option<(BlendMode, Option<String>)> {
    match name {
        b"Normal" | b"Compatible" => Some((BlendMode::Normal, None)),
        b"Multiply" => Some((BlendMode::Multiply, None)),
        b"Screen" => Some((BlendMode::Screen, None)),
        b"Overlay" => Some((BlendMode::Overlay, None)),
        b"Darken" => Some((BlendMode::Darken, None)),
        b"Lighten" => Some((BlendMode::Lighten, None)),
        b"HardLight" => Some((BlendMode::Hardlight, None)),
        b"Difference" => Some((BlendMode::Difference, None)),
        b"Exclusion" => Some((
            BlendMode::Difference,
            Some("PDF /Exclusion blend mode was approximated to Difference.".into()),
        )),
        b"ColorDodge" => Some((
            BlendMode::Add,
            Some("PDF /ColorDodge blend mode was approximated to Add.".into()),
        )),
        b"ColorBurn" => Some((
            BlendMode::Multiply,
            Some("PDF /ColorBurn blend mode was approximated to Multiply.".into()),
        )),
        b"SoftLight" => Some((
            BlendMode::Overlay,
            Some("PDF /SoftLight blend mode was approximated to Overlay.".into()),
        )),
        _ => None,
    }
}

struct PdfParseCaches {
    decoded_images: HashMap<ObjectId, PdfImageXObject>,
    total_decoded_bytes: usize,
}

fn decode_image_raw_to_rgba(
    doc: &Document,
    dict: &Dictionary,
    raw: &[u8],
    width: u32,
    height: u32,
    resources_cs: Option<&Dictionary>,
) -> Option<RgbaImage> {
    let bpc = dict
        .get(b"BitsPerComponent")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(8) as u32;
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return None;
    }

    let is_image_mask = dict
        .get(b"ImageMask")
        .ok()
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    let cs = if is_image_mask {
        PdfColorSpace::Gray { gamma: 1.0 }
    } else if let Ok(cs_obj) = dict.get(b"ColorSpace") {
        parse_pdf_color_space(doc, cs_obj, resources_cs, 0)?
    } else {
        PdfColorSpace::Gray { gamma: 1.0 }
    };

    let n_ch = cs.channels().max(1);
    let decode_arr = dict.get(b"Decode").ok().and_then(numbers);

    let default_max = if matches!(cs, PdfColorSpace::Indexed { .. }) {
        ((1u32 << bpc.min(16)) - 1) as f64
    } else {
        1.0
    };

    let row_bits = (width as usize)
        .checked_mul(n_ch)?
        .checked_mul(bpc as usize)?;
    let row_bytes = row_bits.div_ceil(8);
    if raw.len() < row_bytes.checked_mul(height as usize)? {
        return None;
    }

    let max_sample = ((1u32 << bpc.min(16)) - 1).max(1) as f64;
    let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
    let mut comps = vec![0.0f64; n_ch];

    for y in 0..(height as usize) {
        let row = &raw[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..(width as usize) {
            for (c, slot) in comps.iter_mut().enumerate().take(n_ch) {
                let sample_idx = x * n_ch + c;
                let sample_val: u32 = match bpc {
                    8 => u32::from(*row.get(sample_idx)?),
                    16 => {
                        let b = sample_idx * 2;
                        u32::from(u16::from_be_bytes([*row.get(b)?, *row.get(b + 1)?]))
                    }
                    1 | 2 | 4 => {
                        let bit_offset = sample_idx * (bpc as usize);
                        let byte = *row.get(bit_offset / 8)?;
                        let shift = 8 - (bpc as usize) - (bit_offset % 8);
                        u32::from((byte >> shift) & ((1u8 << bpc) - 1))
                    }
                    _ => return None,
                };
                let frac = (sample_val as f64) / max_sample;
                let d0 = decode_arr
                    .as_ref()
                    .and_then(|d| d.get(2 * c).copied())
                    .unwrap_or(0.0);
                let d1 = decode_arr
                    .as_ref()
                    .and_then(|d| d.get(2 * c + 1).copied())
                    .unwrap_or(default_max);
                *slot = d0 + frac * (d1 - d0);
            }
            if is_image_mask {
                // Stencil mask: 0 paints, 1 leaves transparent (after /Decode)
                let paint_opacity = ((1.0 - comps[0].clamp(0.0, 1.0)) * 255.0).round() as u8;
                rgba.extend_from_slice(&[0, 0, 0, paint_opacity]);
            } else {
                let col = cs.to_rgba(doc, &comps, 0)?;
                rgba.extend_from_slice(&col);
            }
        }
    }

    RgbaImage::from_raw(width, height, rgba)
}

fn decode_pdf_image_rgba(
    doc: &Document,
    stream: &lopdf::Stream,
    resources_cs: Option<&Dictionary>,
    depth: usize,
) -> Result<Option<RgbaImage>, ImportError> {
    if depth > 4 {
        return Ok(None);
    }
    let dict = &stream.dict;
    let width = dict
        .get(b"Width")
        .or_else(|_| dict.get(b"W"))
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0);
    let height = dict
        .get(b"Height")
        .or_else(|_| dict.get(b"H"))
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0);
    if width <= 0 || height <= 0 {
        return Ok(None);
    }
    if width > i64::from(MAX_PDF_IMAGE_DIM) || height > i64::from(MAX_PDF_IMAGE_DIM) {
        return Err(ImportError::Container(format!(
            "PDF Image XObject dimensions ({width}x{height}) exceed maximum {MAX_PDF_IMAGE_DIM}x{MAX_PDF_IMAGE_DIM}."
        )));
    }
    let width = width as u32;
    let height = height as u32;
    let raw_rgba_budget = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| ImportError::Container("PDF Image XObject size overflow.".into()))?;
    if raw_rgba_budget > MAX_PDF_IMAGE_BYTES {
        return Err(ImportError::Container(
            "PDF Image XObject exceeds 64 MiB decoded pixel budget.".into(),
        ));
    }

    // Inspect filter list to detect DCTDecode (JPEG) or JPXDecode
    let filter_names: Vec<Vec<u8>> = match dict.get(b"Filter").or_else(|_| dict.get(b"F")) {
        Ok(Object::Name(n)) => vec![n.clone()],
        Ok(Object::Array(arr)) => arr
            .iter()
            .filter_map(|o| {
                doc.dereference(o)
                    .map_or(o, |(_, r)| r)
                    .as_name()
                    .ok()
                    .map(|s| s.to_vec())
            })
            .collect(),
        Ok(other) => doc
            .dereference(other)
            .ok()
            .and_then(|(_, r)| r.as_name().ok().map(|s| vec![s.to_vec()]))
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    if filter_names.iter().any(|f| {
        f.as_slice() == b"JPXDecode"
            || f.as_slice() == b"CCITTFaxDecode"
            || f.as_slice() == b"JBIG2Decode"
    }) {
        return Ok(None);
    }

    let (decoded_bytes, is_dct_jpeg) =
        decode_pdf_stream_with_doc(stream, Some(doc), MAX_PDF_IMAGE_BYTES, true)?;
    let mut base_img = if is_dct_jpeg {
        let mut reader = image::ImageReader::with_format(
            Cursor::new(decoded_bytes.as_slice()),
            image::ImageFormat::Jpeg,
        );
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_PDF_IMAGE_DIM);
        limits.max_image_height = Some(MAX_PDF_IMAGE_DIM);
        limits.max_alloc = Some(MAX_PDF_IMAGE_BYTES as u64);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|e| ImportError::Container(format!("PDF DCTDecode JPEG error: {e}")))?;
        let rgba = decoded.to_rgba8();
        if rgba.width() > MAX_PDF_IMAGE_DIM || rgba.height() > MAX_PDF_IMAGE_DIM {
            return Err(ImportError::Container(
                "PDF DCTDecode JPEG dimensions exceed 4096x4096.".into(),
            ));
        }
        rgba
    } else {
        let Some(img) =
            decode_image_raw_to_rgba(doc, dict, &decoded_bytes, width, height, resources_cs)
        else {
            return Ok(None);
        };
        img
    };

    // Apply `/SMask` soft-mask Image XObject alpha channel when present!
    if let Ok(smask_obj) = dict.get(b"SMask")
        && let Object::Stream(smask_stream) =
            doc.dereference(smask_obj).map_or(smask_obj, |(_, r)| r)
        && let Some(mask_img) = decode_pdf_image_rgba(doc, smask_stream, resources_cs, depth + 1)?
    {
        let mw = mask_img.width().max(1);
        let mh = mask_img.height().max(1);
        let bw = base_img.width();
        let bh = base_img.height();
        let mask_raw = mask_img.as_raw();
        let base_raw = base_img.as_mut();
        for y in 0..bh {
            let my = ((u64::from(y) * u64::from(mh)) / u64::from(bh)).min(u64::from(mh - 1)) as u32;
            for x in 0..bw {
                let mx =
                    ((u64::from(x) * u64::from(mw)) / u64::from(bw)).min(u64::from(mw - 1)) as u32;
                let m_idx = ((my * mw + mx) * 4) as usize;
                let b_idx = ((y * bw + x) * 4) as usize;
                let alpha = mask_raw[m_idx]; // Grayscale mask has R == G == B == luminance
                base_raw[b_idx + 3] =
                    ((u16::from(base_raw[b_idx + 3]) * u16::from(alpha) + 127) / 255) as u8;
            }
        }
    }

    Ok(Some(base_img))
}

#[allow(clippy::too_many_arguments)]
fn parse_form_xobject(
    doc: &Arc<Document>,
    obj_id: Option<ObjectId>,
    name: &str,
    stream: &lopdf::Stream,
    parent_scope: Option<&PdfResourceScope>,
    caches: &mut PdfParseCaches,
    active_forms: &mut HashSet<ObjectId>,
    form_depth: usize,
) -> Result<PdfFormXObject, ImportError> {
    if form_depth >= MAX_PDF_FORM_DEPTH {
        return Err(ImportError::Container(format!(
            "PDF Form XObject nesting exceeds maximum depth of {MAX_PDF_FORM_DEPTH}."
        )));
    }
    if let Some(id) = obj_id
        && !active_forms.insert(id)
    {
        return Err(ImportError::Container(
            "Cyclic PDF Form XObject reference detected.".into(),
        ));
    }

    let result = (|| {
        let dict = &stream.dict;
        let bbox = dict
            .get(b"BBox")
            .ok()
            .and_then(|o| doc.dereference(o).ok().map(|(_, r)| r).or(Some(o)))
            .and_then(numbers)
            .and_then(|v| <[f64; 4]>::try_from(v).ok())
            .filter(|b| b.iter().all(|v| v.is_finite()))
            .unwrap_or([0.0, 0.0, 100.0, 100.0]);
        let matrix = dict
            .get(b"Matrix")
            .ok()
            .and_then(|o| doc.dereference(o).ok().map(|(_, r)| r).or(Some(o)))
            .and_then(numbers)
            .and_then(|v| <[f64; 6]>::try_from(v).ok())
            .filter(|m| m.iter().all(|v| v.is_finite()))
            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

        let remaining = MAX_PDF_DECODED_STREAM_BYTES.saturating_sub(caches.total_decoded_bytes);
        if remaining == 0 {
            return Err(ImportError::Container(
                "PDF Form XObject streams exceed the 64 MiB decoding budget.".into(),
            ));
        }
        let raw = decode_pdf_stream_bounded(stream, remaining.min(16 * 1024 * 1024))?;
        caches.total_decoded_bytes = caches.total_decoded_bytes.saturating_add(raw.len());

        let res_dict = dict
            .get(b"Resources")
            .ok()
            .and_then(|o| function_dict(doc, o));
        let scope = parse_resource_scope(
            doc,
            res_dict,
            parent_scope,
            caches,
            active_forms,
            form_depth + 1,
        )?;

        Ok(PdfFormXObject {
            id: obj_id,
            name: name.to_owned(),
            bbox,
            matrix,
            content: String::from_utf8_lossy(&raw).into_owned(),
            scope: Box::new(scope),
        })
    })();

    if let Some(id) = obj_id {
        active_forms.remove(&id);
    }
    result
}

fn parse_ext_gstate(
    doc: &Arc<Document>,
    obj: &Object,
    parent_scope: Option<&PdfResourceScope>,
    caches: &mut PdfParseCaches,
    active_forms: &mut HashSet<ObjectId>,
    form_depth: usize,
) -> Result<Option<PdfExtGState>, ImportError> {
    let Some(dict) = function_dict(doc, obj) else {
        return Ok(None);
    };
    let mut gs = PdfExtGState::default();
    if let Some(ca) = dict.get(b"ca").ok().and_then(scalar) {
        gs.fill_alpha = Some(ca.clamp(0.0, 1.0));
    }
    if let Some(stroke_ca) = dict.get(b"CA").ok().and_then(scalar) {
        gs.stroke_alpha = Some(stroke_ca.clamp(0.0, 1.0));
    }
    if let Ok(bm_obj) = dict.get(b"BM") {
        let resolved_bm = doc.dereference(bm_obj).map_or(bm_obj, |(_, o)| o);
        if let Ok(name) = resolved_bm.as_name() {
            if let Some((bm, warn)) = map_pdf_blend_mode_name(name) {
                gs.blend_mode = Some(bm);
                gs.blend_approx = warn;
            } else {
                gs.blend_mode = Some(BlendMode::Normal);
                gs.blend_approx = Some(format!(
                    "PDF blend mode '/{}' was approximated to Normal.",
                    String::from_utf8_lossy(name)
                ));
            }
        } else if let Ok(arr) = resolved_bm.as_array() {
            for candidate in arr {
                if let Ok(name) = candidate.as_name()
                    && let Some((bm, warn)) = map_pdf_blend_mode_name(name)
                {
                    gs.blend_mode = Some(bm);
                    gs.blend_approx = warn;
                    break;
                }
            }
        }
    }
    if let Some(lw) = dict.get(b"LW").ok().and_then(scalar) {
        gs.line_width = Some(lw.max(0.0));
    }
    if let Some(lc) = dict.get(b"LC").ok().and_then(|o| o.as_i64().ok()) {
        gs.line_cap = Some(lc.clamp(0, 2));
    }
    if let Some(lj) = dict.get(b"LJ").ok().and_then(|o| o.as_i64().ok()) {
        gs.line_join = Some(lj.clamp(0, 2));
    }
    if let Some(ml) = dict.get(b"ML").ok().and_then(scalar) {
        gs.miter_limit = Some(ml.max(1.0));
    }
    if let Ok(d_obj) = dict.get(b"D")
        && let Ok(d_arr) = doc.dereference(d_obj).map_or(d_obj, |(_, o)| o).as_array()
        && d_arr.len() >= 2
        && let (Some(pattern), Some(phase)) = (numbers(&d_arr[0]), scalar(&d_arr[1]))
    {
        gs.dash = Some((pattern, phase));
    }
    if let Ok(sm_obj) = dict.get(b"SMask") {
        let resolved_sm = doc.dereference(sm_obj).map_or(sm_obj, |(_, o)| o);
        if resolved_sm.as_name().ok() == Some(b"None") {
            gs.clear_smask = true;
        } else if let Some(sm_dict) = function_dict(doc, resolved_sm)
            && let Ok(g_ref) = sm_dict.get(b"G")
        {
            let subtype = sm_dict
                .get(b"S")
                .ok()
                .and_then(|o| o.as_name().ok())
                .unwrap_or(b"Luminosity");
            let mode = if subtype == b"Alpha" {
                MaskMode::Alpha
            } else {
                MaskMode::Luminance
            };
            let (form_oid, resolved_g) = match doc.dereference(g_ref) {
                Ok((oid, o)) => (oid, o),
                Err(_) => (None, g_ref),
            };
            if let Object::Stream(g_stream) = resolved_g {
                let form = parse_form_xobject(
                    doc,
                    form_oid,
                    "SMask",
                    g_stream,
                    parent_scope,
                    caches,
                    active_forms,
                    form_depth,
                )?;
                gs.smask = Some(PdfSoftMask {
                    mode,
                    form: Box::new(form),
                });
            }
        }
    }

    Ok(Some(gs))
}

fn parse_resource_scope(
    doc: &Arc<Document>,
    resources: Option<&Dictionary>,
    parent_scope: Option<&PdfResourceScope>,
    caches: &mut PdfParseCaches,
    active_forms: &mut HashSet<ObjectId>,
    form_depth: usize,
) -> Result<PdfResourceScope, ImportError> {
    let mut scope = if let Some(parent) = parent_scope {
        let mut inherited_scope = parent.clone();
        inherited_scope.warnings.clear();
        inherited_scope
    } else {
        let mut s = PdfResourceScope::empty();
        s.doc = Arc::clone(doc);
        s
    };

    let Some(res) = resources else {
        return Ok(scope);
    };

    let cs_dict = res
        .get(b"ColorSpace")
        .ok()
        .and_then(|obj| function_dict(doc, obj));

    if let Some(dict) = cs_dict {
        for (name, value) in dict.iter() {
            if let Some(cs) = parse_pdf_color_space(doc, value, Some(dict), 0) {
                scope
                    .color_spaces
                    .insert(String::from_utf8_lossy(name).into_owned(), cs);
            }
        }
    }

    if let Some(dict) = res
        .get(b"Shading")
        .ok()
        .and_then(|obj| function_dict(doc, obj))
    {
        for (name, value) in dict.iter() {
            if let Some((paint, warn)) = parse_shading_with_cs(doc, value, cs_dict) {
                let key = String::from_utf8_lossy(name).into_owned();
                if let Some(msg) = warn {
                    scope.warnings.push((key.clone(), msg));
                }
                scope.shadings.insert(key, paint);
            }
        }
    }

    if let Some(dict) = res
        .get(b"Pattern")
        .ok()
        .and_then(|obj| function_dict(doc, obj))
    {
        for (name, value) in dict.iter() {
            if let Some((paint, warn)) = parse_shading_pattern_with_cs(doc, value, cs_dict) {
                let key = String::from_utf8_lossy(name).into_owned();
                if let Some(msg) = warn {
                    scope.warnings.push((key.clone(), msg));
                }
                scope.patterns.insert(key, paint);
            }
        }
    }

    if let Some(dict) = res
        .get(b"Font")
        .ok()
        .and_then(|obj| function_dict(doc, obj))
    {
        for (name, value) in dict.iter() {
            if let Some(font) = parse_pdf_font_resource(doc, value) {
                scope
                    .fonts
                    .insert(String::from_utf8_lossy(name).into_owned(), font);
            }
        }
    }

    if let Some(dict) = res
        .get(b"ExtGState")
        .ok()
        .and_then(|obj| function_dict(doc, obj))
    {
        for (name, value) in dict.iter() {
            if let Some(gs) =
                parse_ext_gstate(doc, value, Some(&scope), caches, active_forms, form_depth)?
            {
                scope
                    .ext_gstates
                    .insert(String::from_utf8_lossy(name).into_owned(), gs);
            }
        }
    }

    if let Some(dict) = res
        .get(b"XObject")
        .ok()
        .and_then(|obj| function_dict(doc, obj))
    {
        for (name, value) in dict.iter() {
            let key = String::from_utf8_lossy(name).into_owned();
            let (oid, resolved) = match doc.dereference(value) {
                Ok((oid, r)) => (oid, r),
                Err(_) => (None, value),
            };
            let Object::Stream(stream) = resolved else {
                continue;
            };
            let subtype = stream
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|o| o.as_name().ok())
                .unwrap_or(b"");
            match subtype {
                b"Form" => {
                    if let Some(id) = oid
                        && active_forms.contains(&id)
                    {
                        // Shared `/Resources` dictionaries often list ancestor `/Form` XObjects;
                        // record in `cyclic_forms` so only an actual recursive `Do` invocation errors!
                        scope.forms.remove(&key);
                        scope.cyclic_forms.insert(key);
                        continue;
                    }
                    let form = parse_form_xobject(
                        doc,
                        oid,
                        &key,
                        stream,
                        Some(&scope),
                        caches,
                        active_forms,
                        form_depth,
                    )?;
                    scope.forms.insert(key, form);
                }
                b"Image" => {
                    if let Some(id) = oid
                        && let Some(cached) = caches.decoded_images.get(&id)
                    {
                        scope.images.insert(key, cached.clone());
                        continue;
                    }
                    if let Some(rgba) = decode_pdf_image_rgba(doc, stream, cs_dict, 0)? {
                        let w = rgba.width();
                        let h = rgba.height();
                        let mut png = Vec::new();
                        image::codecs::png::PngEncoder::new(&mut png)
                            .write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
                            .map_err(|e| {
                                ImportError::Container(format!(
                                    "PDF Image XObject PNG encode failed: {e}"
                                ))
                            })?;
                        let img_xobj = PdfImageXObject {
                            width: w,
                            height: h,
                            png_bytes: png,
                        };
                        if let Some(id) = oid {
                            caches.decoded_images.insert(id, img_xobj.clone());
                        }
                        scope.images.insert(key, img_xobj);
                    } else {
                        scope
                            .unsupported_xobjects
                            .insert(key, "Unsupported PDF Image XObject encoding".into());
                    }
                }
                other => {
                    scope.unsupported_xobjects.insert(
                        key,
                        format!(
                            "Unsupported PDF XObject subtype '/{}'",
                            String::from_utf8_lossy(other)
                        ),
                    );
                }
            }
        }
    }

    Ok(scope)
}

pub(super) fn pages(bytes: &[u8]) -> Result<Vec<Page>, ImportError> {
    let error = |msg: &str| ImportError::Container(format!("PDF page import: {msg}"));
    let doc = Arc::new(load_pdf_document_bounded(bytes)?);
    if doc.is_encrypted() {
        return Err(error("Unlock the document and save an unencrypted PDF."));
    }
    let mut result = Vec::new();
    let mut caches = PdfParseCaches {
        decoded_images: HashMap::new(),
        total_decoded_bytes: 0,
    };

    for id in doc.page_iter() {
        if result.len() >= 256 {
            return Err(error("document exceeds 256 pages"));
        }
        let media_box = inherited(&doc, id, b"MediaBox").and_then(valid_box);
        // Illustrator selection clipboard sets `/ArtBox` to exact copied artwork bounds while
        // `/MediaBox` holds the artboard! Box priority: ArtBox > TrimBox > BleedBox > CropBox > MediaBox.
        let bounds = inherited(&doc, id, b"ArtBox")
            .and_then(valid_box)
            .or_else(|| inherited(&doc, id, b"TrimBox").and_then(valid_box))
            .or_else(|| inherited(&doc, id, b"BleedBox").and_then(valid_box))
            .or_else(|| inherited(&doc, id, b"CropBox").and_then(valid_box))
            .or(media_box)
            .ok_or_else(|| error("missing or invalid page bounds"))?;

        let rotation = inherited(&doc, id, b"Rotate")
            .and_then(|v| v.as_i64().ok())
            .unwrap_or(0);

        let mut content = Vec::new();
        let page_limit = (MAX_PDF_DECODED_STREAM_BYTES.saturating_sub(caches.total_decoded_bytes))
            .min(16 * 1024 * 1024);
        for stream_id in doc.get_page_contents(id) {
            let stream = doc
                .get_object(stream_id)
                .and_then(Object::as_stream)
                .map_err(|e| error(&e.to_string()))?;
            let remaining = page_limit.saturating_sub(content.len());
            if remaining == 0 {
                return Err(error("page instructions exceed the decoding budget"));
            }
            let decoded = decode_pdf_stream_bounded(stream, remaining)?;
            content.extend_from_slice(&decoded);
            content.push(b'\n');
            if content.len() > page_limit {
                return Err(error("page instructions exceed the decoding budget"));
            }
        }
        caches.total_decoded_bytes = caches.total_decoded_bytes.saturating_add(content.len());

        let res_dict = inherited(&doc, id, b"Resources").and_then(|v| function_dict(&doc, v));
        let mut active_forms = HashSet::new();
        let scope = parse_resource_scope(&doc, res_dict, None, &mut caches, &mut active_forms, 0)?;

        result.push(Page {
            content: String::from_utf8_lossy(&content).into_owned(),
            bounds,
            media_box,
            rotation,
            scope,
        });
    }
    if result.is_empty() {
        return Err(error("document has no readable pages"));
    }
    Ok(result)
}

pub(super) fn tokenize_pdf_content(stream: &str) -> Result<Vec<String>, ImportError> {
    let mut tokens = Vec::new();
    let mut chars = stream.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if tokens.len() >= 1_000_000 {
            return Err(ImportError::Container(
                "PDF page exceeds one million operands and operators.".into(),
            ));
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
        if ch == '(' {
            chars.next();
            let mut s = String::from("(");
            let mut depth = 1usize;
            while let Some(c) = chars.next() {
                if c == '\\' {
                    s.push('\\');
                    if let Some(esc) = chars.next() {
                        s.push(esc);
                        if esc == '\r' && chars.peek() == Some(&'\n') {
                            s.push(chars.next().unwrap());
                        }
                    }
                    continue;
                }
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        s.push(')');
                        break;
                    }
                }
                s.push(c);
            }
            if !s.ends_with(')') {
                s.push(')');
            }
            tokens.push(s);
            continue;
        }
        if ch == '<' {
            chars.next();
            if chars.peek() == Some(&'<') {
                chars.next();
                tokens.push("<<".to_owned());
                continue;
            }
            let mut hex_tok = String::from("<");
            for c in chars.by_ref() {
                if c == '>' {
                    hex_tok.push('>');
                    break;
                }
                if !c.is_ascii_whitespace() {
                    hex_tok.push(c);
                }
            }
            if !hex_tok.ends_with('>') {
                hex_tok.push('>');
            }
            tokens.push(hex_tok);
            continue;
        }
        if ch == '>' {
            chars.next();
            if chars.peek() == Some(&'>') {
                chars.next();
                tokens.push(">>".to_owned());
            } else {
                tokens.push(">".to_owned());
            }
            continue;
        }
        if matches!(ch, '[' | ']' | ')') {
            tokens.push(chars.next().unwrap().to_string());
            continue;
        }
        let mut word = String::new();
        if ch == '/' {
            word.push(chars.next().unwrap());
        }
        while let Some(&c) = chars.peek() {
            if c.is_ascii_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '%' | '/')
            {
                break;
            }
            word.push(chars.next().unwrap());
        }
        if !word.is_empty() {
            tokens.push(word);
        }
    }
    Ok(tokens)
}

#[allow(dead_code)]
pub(super) fn resolve_color_in_cs(
    scope: &PdfResourceScope,
    cs_name: Option<&str>,
    comps: &[f64],
) -> Option<Color> {
    if let Some(name) = cs_name {
        match name {
            "DeviceGray" | "G" => {
                let v = (comps.first()?.clamp(0.0, 1.0) * 255.0).round() as u8;
                return Some([v, v, v, 255]);
            }
            "DeviceRGB" | "RGB" => {
                if comps.len() >= 3 {
                    return Some([
                        (comps[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (comps[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (comps[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                        255,
                    ]);
                }
            }
            "DeviceCMYK" | "CMYK" => {
                if comps.len() >= 4 {
                    return Some(cmyk_to_rgba(comps[0], comps[1], comps[2], comps[3]));
                }
            }
            other => {
                if let Some(cs) = scope.color_spaces.get(other) {
                    return cs.to_rgba(&scope.doc, comps, 0);
                }
            }
        }
    }
    match comps {
        [gray] => {
            let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
            Some([v, v, v, 255])
        }
        [r, g, b] => Some([
            (r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (b.clamp(0.0, 1.0) * 255.0).round() as u8,
            255,
        ]),
        [c, m, y, k] => Some(cmyk_to_rgba(*c, *m, *y, *k)),
        _ => None,
    }
}
