//! Image, font, and precomposition asset staging and raster transcoding for Lottie export.

use super::{LottieBuilder, LottieImageFormat, LottieNoteKind, round4, sanitize_id};
use crate::ExportError;
use base64::Engine as _;
use image::{ExtendedColorType, ImageEncoder as _};
use neoflash_core::{
    BitmapCompression, Color, Keyframe, Layer, LayerRole, SymbolKind, Timeline,
    sample_layer_in_at_fps,
};
use serde_json::json;

#[derive(Clone, Debug)]
pub(super) struct ExternalImageFile {
    pub(super) relative_path: String,
    pub(super) bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetectedRaster {
    Png,
    Jpeg,
    Webp,
    Gif,
    Other,
}

pub(super) fn detect_raster_magic(bytes: &[u8]) -> DetectedRaster {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        DetectedRaster::Png
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        DetectedRaster::Jpeg
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        DetectedRaster::Webp
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        DetectedRaster::Gif
    } else {
        DetectedRaster::Other
    }
}

fn encode_png_rgba(img: &neoflash_core::DecodedImage) -> Result<Vec<u8>, ExportError> {
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(&img.rgba, img.width, img.height, ExtendedColorType::Rgba8)
        .map_err(|e| ExportError::Options(format!("Failed to encode PNG bitmap: {e}")))?;
    Ok(buf)
}

fn encode_webp_rgba(img: &neoflash_core::DecodedImage) -> Result<Vec<u8>, ExportError> {
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
        .encode(&img.rgba, img.width, img.height, ExtendedColorType::Rgba8)
        .map_err(|e| ExportError::Options(format!("Failed to encode WebP bitmap: {e}")))?;
    Ok(buf)
}

fn encode_jpeg_composited(
    img: &neoflash_core::DecodedImage,
    bg: Color,
    quality: u8,
) -> Result<(Vec<u8>, bool), ExportError> {
    let has_alpha = img.rgba.as_chunks::<4>().0.iter().any(|px| px[3] < 255);
    let mut rgb = Vec::with_capacity((img.width as usize) * (img.height as usize) * 3);
    for px in img.rgba.as_chunks::<4>().0 {
        let a = px[3] as u16;
        let inv = 255 - a;
        rgb.push(((px[0] as u16 * a + bg[0] as u16 * inv + 127) / 255) as u8);
        rgb.push(((px[1] as u16 * a + bg[1] as u16 * inv + 127) / 255) as u8);
        rgb.push(((px[2] as u16 * a + bg[2] as u16 * inv + 127) / 255) as u8);
    }
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100))
        .write_image(&rgb, img.width, img.height, ExtendedColorType::Rgb8)
        .map_err(|e| ExportError::Options(format!("Failed to encode JPEG bitmap: {e}")))?;
    Ok((buf, has_alpha))
}

impl<'a> LottieBuilder<'a> {
    pub(super) fn alloc_unique_asset_id(&mut self, prefix: &str, raw: &str) -> String {
        let slug = sanitize_id(raw);
        let base = if slug.is_empty() {
            format!("{prefix}asset")
        } else {
            format!("{prefix}{slug}")
        };
        if self.used_asset_ids.insert(base.clone()) {
            return base;
        }
        let mut suffix = 2u32;
        loop {
            let candidate = format!("{base}_{suffix}");
            if self.used_asset_ids.insert(candidate.clone()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    pub(super) fn register_font(&mut self, family: &str, bold: bool, italic: bool) -> String {
        let clean_family = if family.trim().is_empty() {
            "sans-serif"
        } else {
            family.trim()
        };
        let style = match (bold, italic) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            (false, false) => "Regular",
        };
        let f_name = if bold || italic {
            format!("{clean_family}-{}", style.replace(' ', ""))
        } else {
            clean_family.to_owned()
        };
        self.fonts.entry(f_name.clone()).or_insert_with(|| {
            json!({
                "fName": f_name,
                "fFamily": clean_family,
                "fStyle": style,
                "ascent": 75.0
            })
        });
        f_name
    }

    pub(super) fn ensure_image_asset(
        &mut self,
        resource_id: &str,
        fallback_w: u32,
        fallback_h: u32,
        symbol_id: Option<&str>,
        layer_id: Option<&str>,
        frame: Option<u32>,
    ) -> Result<Option<String>, ExportError> {
        self.check_cancelled()?;
        if let Some(cached) = self.image_id_by_resource.get(resource_id) {
            return Ok(cached.clone());
        }

        let Some(unit) = self.project.resources.iter().find(|r| r.id == resource_id) else {
            self.add_note_coded(
                LottieNoteKind::Missing,
                Some("LOTTIE_MISSING_BITMAP_RESOURCE"),
                format!("Resource '{resource_id}'"),
                format!("Bitmap resource '{resource_id}' is missing from project resources."),
                symbol_id.map(str::to_owned),
                layer_id.map(str::to_owned),
                frame,
            );
            self.image_id_by_resource
                .insert(resource_id.to_owned(), None);
            return Ok(None);
        };

        let raw_bytes = unit.decoded_bytes().ok().filter(|b| !b.is_empty());
        let decoded = unit.decode_image().ok();
        self.check_cancelled()?;

        let Some(img) = decoded else {
            self.add_note_coded(
                LottieNoteKind::Missing,
                Some("LOTTIE_INVALID_BITMAP_RESOURCE"),
                format!("Resource '{}'", unit.id),
                format!(
                    "Bitmap resource '{}' ('{}') could not be decoded as a valid raster image.",
                    unit.id, unit.path
                ),
                symbol_id.map(str::to_owned),
                layer_id.map(str::to_owned),
                frame,
            );
            self.image_id_by_resource
                .insert(resource_id.to_owned(), None);
            return Ok(None);
        };

        let w = if img.width > 0 {
            img.width
        } else {
            fallback_w.max(1)
        };
        let h = if img.height > 0 {
            img.height
        } else {
            fallback_h.max(1)
        };
        let detected = raw_bytes
            .as_deref()
            .map_or(DetectedRaster::Other, detect_raster_magic);
        let default_jpeg_quality = unit
            .bitmap_export
            .as_ref()
            .map(|s| s.jpeg_quality.clamp(1, 100))
            .unwrap_or(92);

        let (ext, mime_type, encoded_bytes) = match self.options.image_format {
            LottieImageFormat::Original => {
                if let Some(settings) = unit.bitmap_export.as_ref() {
                    match settings.compression {
                        BitmapCompression::PhotoJpeg => {
                            let (bytes, has_alpha) = encode_jpeg_composited(
                                &img,
                                self.project.background,
                                settings.jpeg_quality,
                            )?;
                            if has_alpha {
                                self.add_note_coded(
                                    LottieNoteKind::Approximated,
                                    Some("LOTTIE_JPEG_ALPHA_COMPOSITED"),
                                    format!("Resource '{}'", unit.id),
                                    format!(
                                        "Bitmap '{}' has transparency, which JPEG does not support; composited over stage background.",
                                        unit.path
                                    ),
                                    symbol_id.map(str::to_owned),
                                    layer_id.map(str::to_owned),
                                    frame,
                                );
                            }
                            ("jpg", "image/jpeg", bytes)
                        }
                        BitmapCompression::LosslessPng => {
                            let bytes = if detected == DetectedRaster::Png {
                                raw_bytes.unwrap()
                            } else {
                                encode_png_rgba(&img)?
                            };
                            ("png", "image/png", bytes)
                        }
                        BitmapCompression::Webp => {
                            let bytes = if detected == DetectedRaster::Webp {
                                raw_bytes.unwrap()
                            } else {
                                encode_webp_rgba(&img)?
                            };
                            ("webp", "image/webp", bytes)
                        }
                    }
                } else {
                    match detected {
                        DetectedRaster::Png => ("png", "image/png", raw_bytes.unwrap()),
                        DetectedRaster::Jpeg => ("jpg", "image/jpeg", raw_bytes.unwrap()),
                        DetectedRaster::Webp => ("webp", "image/webp", raw_bytes.unwrap()),
                        DetectedRaster::Gif => ("gif", "image/gif", raw_bytes.unwrap()),
                        DetectedRaster::Other => {
                            let png = encode_png_rgba(&img)?;
                            self.add_note_coded(
                                LottieNoteKind::Approximated,
                                Some("LOTTIE_BITMAP_TRANSCODED"),
                                format!("Resource '{}'", unit.id),
                                format!(
                                    "Bitmap '{}' ('{}') was transcoded to lossless PNG for Lottie compatibility.",
                                    unit.path, unit.media_type
                                ),
                                symbol_id.map(str::to_owned),
                                layer_id.map(str::to_owned),
                                frame,
                            );
                            ("png", "image/png", png)
                        }
                    }
                }
            }
            LottieImageFormat::Png => {
                let bytes = if detected == DetectedRaster::Png {
                    raw_bytes.unwrap()
                } else {
                    encode_png_rgba(&img)?
                };
                ("png", "image/png", bytes)
            }
            LottieImageFormat::Jpeg => {
                let bytes = if detected == DetectedRaster::Jpeg {
                    raw_bytes.unwrap()
                } else {
                    let (jpg, has_alpha) = encode_jpeg_composited(
                        &img,
                        self.project.background,
                        default_jpeg_quality,
                    )?;
                    if has_alpha {
                        self.add_note_coded(
                            LottieNoteKind::Approximated,
                            Some("LOTTIE_JPEG_ALPHA_COMPOSITED"),
                            format!("Resource '{}'", unit.id),
                            format!(
                                "Bitmap '{}' has transparency, which JPEG does not support; composited over stage background.",
                                unit.path
                            ),
                            symbol_id.map(str::to_owned),
                            layer_id.map(str::to_owned),
                            frame,
                        );
                    }
                    jpg
                };
                ("jpg", "image/jpeg", bytes)
            }
            LottieImageFormat::Webp => {
                let bytes = if detected == DetectedRaster::Webp {
                    raw_bytes.unwrap()
                } else {
                    encode_webp_rgba(&img)?
                };
                ("webp", "image/webp", bytes)
            }
        };

        self.check_cancelled()?;

        let asset_id = self.alloc_unique_asset_id("img_", resource_id);
        if self.package_external_images {
            let filename = format!("{asset_id}.{ext}");
            self.external_images.push(ExternalImageFile {
                relative_path: format!("images/{filename}"),
                bytes: encoded_bytes,
            });
            self.assets.push(json!({
                "id": asset_id,
                "w": w,
                "h": h,
                "u": "images/",
                "p": filename,
                "e": 0
            }));
        } else {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&encoded_bytes);
            let data_uri = format!("data:{mime_type};base64,{b64}");
            self.assets.push(json!({
                "id": asset_id,
                "w": w,
                "h": h,
                "u": "",
                "p": data_uri,
                "e": 1
            }));
        }
        self.exported_count += 1;
        self.image_id_by_resource
            .insert(resource_id.to_owned(), Some(asset_id.clone()));
        Ok(Some(asset_id))
    }

    pub(super) fn ensure_symbol_precomp(
        &mut self,
        symbol_id: &str,
        caller_symbol_id: &str,
        caller_layer_id: Option<&str>,
        caller_frame: Option<u32>,
        stack: &mut Vec<String>,
    ) -> Result<Option<String>, ExportError> {
        self.check_cancelled()?;
        if stack.iter().any(|s| s == symbol_id) {
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_SYMBOL_CYCLE"),
                format!("Symbol '{symbol_id}'"),
                format!("Recursive symbol reference to '{symbol_id}' was skipped."),
                Some(caller_symbol_id.to_owned()),
                caller_layer_id.map(str::to_owned),
                caller_frame,
            );
            return Ok(None);
        }
        if let Some(cached) = self.precomp_id_by_symbol.get(symbol_id) {
            return Ok(cached.clone());
        }
        let Some(symbol) = self.project.symbol(symbol_id) else {
            self.add_note_coded(
                LottieNoteKind::Missing,
                Some("LOTTIE_MISSING_SYMBOL"),
                format!("Symbol '{symbol_id}'"),
                format!("Referenced symbol '{symbol_id}' is missing from project library."),
                Some(caller_symbol_id.to_owned()),
                caller_layer_id.map(str::to_owned),
                caller_frame,
            );
            self.precomp_id_by_symbol.insert(symbol_id.to_owned(), None);
            return Ok(None);
        };

        let asset_id = self.alloc_unique_asset_id("comp_", symbol_id);
        self.precomp_id_by_symbol
            .insert(symbol_id.to_owned(), Some(asset_id.clone()));
        stack.push(symbol_id.to_owned());

        let precomp_w = self.comp_w.max(1) * 2;
        let precomp_h = self.comp_h.max(1) * 2;
        let origin = [self.comp_w as f64, self.comp_h as f64];

        let layers = if symbol.kind == SymbolKind::Button {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_BUTTON_INTERACTIVE"),
                format!("Button symbol '{}'", symbol.name),
                format!(
                    "Button symbol '{}' interactive Over/Down/Hit states are not supported by Lottie players; exported static Up state.",
                    symbol.name
                ),
                Some(symbol.id.clone()),
                caller_layer_id.map(str::to_owned),
                caller_frame,
            );
            let up_timeline = build_button_up_timeline(&symbol.timeline, self.source_fps);
            self.build_timeline_layers(
                &symbol.id,
                &symbol.name,
                &up_timeline,
                origin,
                false,
                stack,
            )?
        } else {
            self.build_timeline_layers(
                &symbol.id,
                &symbol.name,
                &symbol.timeline,
                origin,
                false,
                stack,
            )?
        };
        stack.pop();

        self.assets.push(json!({
            "id": asset_id,
            "nm": symbol.name,
            "fr": round4(self.fps),
            "w": precomp_w,
            "h": precomp_h,
            "layers": layers
        }));
        Ok(Some(asset_id))
    }

    pub(super) fn ensure_mask_matte_precomp(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        mask_layer: &Layer,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
    ) -> Result<Option<String>, ExportError> {
        self.check_cancelled()?;
        let cache_key = format!("{symbol_id}::{}", mask_layer.id);
        if let Some(cached) = self.matte_precomps.get(&cache_key) {
            return Ok(cached.clone());
        }

        let mut matte_layer = mask_layer.clone();
        matte_layer.properties.role = LayerRole::Normal;
        matte_layer.properties.mask = None;
        matte_layer.visible = true;
        let mut matte_layers_vec = Vec::new();
        for l in &timeline.layers {
            if l.properties.role == LayerRole::Guide {
                matte_layers_vec.push(l.clone());
            }
        }
        matte_layers_vec.push(matte_layer);
        let matte_timeline = Timeline {
            frame_count: timeline.frame_count.max(1),
            layers: matte_layers_vec,
            scripts: Vec::new(),
            guides: Vec::new(),
        };

        let asset_id =
            self.alloc_unique_asset_id("comp_matte_", &format!("{symbol_id}_{}", mask_layer.id));
        self.matte_precomps
            .insert(cache_key.clone(), Some(asset_id.clone()));

        let layers = self.build_timeline_layers(
            symbol_id,
            &format!("Matte · {}", mask_layer.name),
            &matte_timeline,
            origin_offset,
            false,
            stack,
        )?;
        if layers.is_empty() {
            self.matte_precomps.insert(cache_key, None);
            return Ok(None);
        }

        self.assets.push(json!({
            "id": asset_id,
            "nm": format!("Matte · {}", mask_layer.name),
            "fr": round4(self.fps),
            "w": self.comp_w.max(1),
            "h": self.comp_h.max(1),
            "layers": layers
        }));
        Ok(Some(asset_id))
    }
}

fn build_button_up_timeline(timeline: &Timeline, fps: f64) -> Timeline {
    let mut layers = Vec::with_capacity(timeline.layers.len());
    for layer in &timeline.layers {
        let mut up_layer = layer.clone();
        let up_elements = sample_layer_in_at_fps(timeline, layer, 0, fps);
        let any_sound = layer.keyframes.iter().find_map(|k| k.sound.clone());
        let any_video = layer.keyframes.iter().find_map(|k| k.video.clone());
        let all_filters: Vec<_> = layer
            .keyframes
            .iter()
            .flat_map(|k| k.filters.iter().cloned())
            .collect();
        if up_elements.is_empty()
            && any_sound.is_none()
            && any_video.is_none()
            && all_filters.is_empty()
        {
            up_layer.keyframes = Vec::new();
        } else {
            up_layer.keyframes = vec![Keyframe {
                frame: 0,
                elements: up_elements,
                sound: any_sound,
                video: any_video,
                filters: all_filters,
                ..Keyframe::default()
            }];
        }
        up_layer.properties.end_frame = None;
        layers.push(up_layer);
    }
    Timeline {
        frame_count: 1,
        layers,
        scripts: timeline.scripts.clone(),
        guides: Vec::new(),
    }
}
