//! Read the portable preview of a binary EPS without executing PostScript.
use super::*;
use image::{ImageDecoder, ImageEncoder, RgbaImage};
use std::io::Cursor;
use tiff::{decoder::Decoder, tags::Tag};

pub(super) const MAGIC: &[u8] = &[0xc5, 0xd0, 0xd3, 0xc6];

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
    // Illustrator writes uncompressed indexed pixels, optionally followed by an alpha sample.
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

pub(super) fn import(
    project: &mut Project,
    bytes: &[u8],
    source: &str,
    options: &VectorImportOptions,
) -> Result<ImportReport, ImportError> {
    let error = |message: &str| ImportError::Container(format!("{source}: {message}"));
    let ps = section(bytes, 4)
        .filter(|b| b.starts_with(b"%!PS-Adobe"))
        .ok_or_else(|| error("Invalid binary EPS PostScript section."))?;
    let preview = section(bytes,20).ok_or_else(|| error("EPS has no embedded TIFF preview. Save the artwork as PDF or SVG to import editable paths."))?;
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
    let header = String::from_utf8_lossy(&ps[..ps.len().min(8192)]);
    let bounds = header
        .lines()
        .find_map(|line| {
            let numbers: Vec<f64> = line
                .strip_prefix("%%BoundingBox:")?
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            let [x0, y0, x1, y1] = numbers.as_slice() else {
                return None;
            };
            (x1 > x0 && y1 > y0).then(|| {
                (
                    (x1 - x0).round().clamp(1., 65535.) as u32,
                    (y1 - y0).round().clamp(1., 65535.) as u32,
                )
            })
        })
        .unwrap_or((width, height));
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
    if options.resize_stage_to_viewbox {
        (project.width, project.height) = bounds;
    }
    project.resources.push(resource);
    let mut report = ImportReport {
        source: source.into(),
        format: ImportFormat::Unknown,
        version: Some("EPS".into()),
        can_import: true,
        statistics: Statistics {
            elements: 1,
            ..Default::default()
        },
        diagnostics: vec![],
        binary_fla: None,
        swf: None,
        swc: None,
    };
    report.warning("eps-tiff-preview",source,format!("Imported the embedded {width}×{height} TIFF preview as a bitmap. Export as PDF or SVG for editable vector artwork."));
    let element = Element {
        id: new_id("element"),
        name: source.into(),
        transform: Transform {
            scale_x: f64::from(bounds.0) / f64::from(width),
            scale_y: f64::from(bounds.1) / f64::from(height),
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
    assemble_vector_buckets(
        project,
        vec![VectorGroupBucket {
            name: source.into(),
            elements: vec![element],
            mask_elements: vec![],
            mask_mode: MaskMode::Geometry,
        }],
        source,
        options,
        &mut report,
    );
    Ok(report)
}
