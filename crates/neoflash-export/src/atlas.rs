//! Sprite-sheet and texture-atlas publishing (`T-110`).
//!
//! Rasterizes timeline or symbol frame sequences using the shared CPU renderer, trims transparent
//! borders (`spriteSourceSize` / `sourceSize`), deduplicates identical frames, packs sprites
//! deterministically via MaxRects Best-Short-Side-Fit (`BSSF`), and emits a PNG sheet alongside
//! TexturePacker-compatible JSON Array, JSON Hash (PixiJS / Phaser), or Starling XML metadata.

use crate::{ExportError, atomic_write, video::render_authored_scene_rgba};
use image::{ImageFormat, ImageReader, Limits, RgbaImage};
use neoflash_core::{Project, evaluate_scene, flatten_scenes_with_ranges};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Write as _,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const MAX_BITMAP_SHEET_DECODED_BYTES: usize = 128 * 1024 * 1024;
const MAX_BITMAP_SHEET_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
const MAX_BITMAP_SHEET_DIMENSION: u32 = 8192;

fn checked_rgba_bytes(width: u32, height: u32, context: &str) -> Result<usize, ExportError> {
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ExportError::Options(format!("{context} RGBA dimensions overflow.")))?;
    Ok(bytes)
}

fn reserve_bitmap_decode_bytes(total: &mut usize, bytes: usize) -> Result<(), ExportError> {
    *total = total.checked_add(bytes).ok_or_else(|| {
        ExportError::Options("Bitmap sprite-sheet decode budget overflow.".into())
    })?;
    if *total > MAX_BITMAP_SHEET_DECODED_BYTES {
        return Err(ExportError::Options(format!(
            "Bitmap resources require {} decoded RGBA bytes, exceeding the {} MiB sprite-sheet input budget.",
            *total,
            MAX_BITMAP_SHEET_DECODED_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

fn grow_bitmap_sheet_dimension(value: u32) -> Option<u32> {
    if value >= MAX_BITMAP_SHEET_DIMENSION {
        return None;
    }
    Some(
        value
            .checked_mul(2)
            .unwrap_or(MAX_BITMAP_SHEET_DIMENSION)
            .max(value.saturating_add(16))
            .min(MAX_BITMAP_SHEET_DIMENSION),
    )
}

/// Image encoding used for packed texture-atlas pages (`T-250`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtlasImageFormat {
    #[default]
    Png,
    Webp,
    Jpeg,
}

impl AtlasImageFormat {
    pub const ALL: [Self; 3] = [Self::Png, Self::Webp, Self::Jpeg];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG (RGBA)",
            Self::Webp => "WebP (Lossless RGBA)",
            Self::Jpeg => "JPEG (RGB)",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Jpeg => "jpg",
        }
    }
}

/// Metadata schema emitted alongside the packed sprite sheet (`T-110`, `T-250`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtlasFormat {
    #[default]
    JsonHash,
    JsonArray,
    StarlingXml,
    EaselJs,
    Css,
}

impl AtlasFormat {
    pub const ALL: [Self; 5] = [
        Self::JsonHash,
        Self::JsonArray,
        Self::StarlingXml,
        Self::EaselJs,
        Self::Css,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::JsonHash => "JSON Hash (PixiJS / Phaser)",
            Self::JsonArray => "JSON Array (TexturePacker)",
            Self::StarlingXml => "Starling / Sparrow XML",
            Self::EaselJs => "EaselJS / CreateJS JSON",
            Self::Css => "CSS Sprite Sheet",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::JsonHash | Self::JsonArray | Self::EaselJs => "json",
            Self::StarlingXml => "xml",
            Self::Css => "css",
        }
    }
}

const fn default_atlas_jpeg_quality() -> u8 {
    90
}

const fn default_atlas_true() -> bool {
    true
}

/// Configuration for sprite-sheet / texture-atlas generation (`T-110`, `T-250`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasOptions {
    /// Target symbol ID; `None` exports the flattened main timeline (or `project.root`).
    pub symbol_id: Option<String>,
    /// Zero-based start frame (`0` by default).
    pub start_frame: u32,
    /// Optional inclusive zero-based end frame (`None` exports the entire symbol/timeline).
    pub end_frame: Option<u32>,
    /// Frame render width (`None` uses `project.width`).
    pub frame_width: Option<u32>,
    /// Frame render height (`None` uses `project.height`).
    pub frame_height: Option<u32>,
    /// Maximum atlas texture width in pixels (default `4096`).
    pub max_width: u32,
    /// Maximum atlas texture height in pixels (default `4096`).
    pub max_height: u32,
    /// Outer margin around the packed sheet in pixels (default `2`).
    pub border_padding: u32,
    /// Spacing between adjacent sprites in pixels (default `2`).
    pub shape_padding: u32,
    /// Duplicate edge border pixels (`0..=16`) around each sprite to prevent GPU UV bleed.
    #[serde(default)]
    pub extrude: u32,
    /// Strip transparent borders and record offsets in `sprite_source_size`.
    pub trim: bool,
    /// Share atlas rectangles for bit-identical frames.
    pub deduplicate: bool,
    /// Round final sheet dimensions up to powers of two.
    pub power_of_two: bool,
    /// Render frames with transparent stage background (default `true` for sprites).
    pub transparent_background: bool,
    /// Metadata output format.
    pub format: AtlasFormat,
    /// Sheet image format (`PNG`, `WebP`, or `JPEG`).
    #[serde(default)]
    pub image_format: AtlasImageFormat,
    /// JPEG quality (`1..=100`) when `image_format == AtlasImageFormat::Jpeg`.
    #[serde(default = "default_atlas_jpeg_quality")]
    pub jpeg_quality: u8,
    /// Step live scripts and independent child MovieClips during frame capture.
    #[serde(default = "default_atlas_true")]
    pub runtime_capture: bool,
    /// Also include all `image/*` Library bitmap resources as named sprites in the atlas.
    #[serde(default)]
    pub pack_library_bitmaps: bool,
    /// Automatically spill into additional atlas pages when sprites overflow `max_width × max_height`.
    #[serde(default = "default_atlas_true")]
    pub multi_page: bool,
}

impl Default for AtlasOptions {
    fn default() -> Self {
        Self {
            symbol_id: None,
            start_frame: 0,
            end_frame: None,
            frame_width: None,
            frame_height: None,
            max_width: 4096,
            max_height: 4096,
            border_padding: 2,
            shape_padding: 2,
            extrude: 0,
            trim: true,
            deduplicate: true,
            power_of_two: false,
            transparent_background: true,
            format: AtlasFormat::JsonHash,
            image_format: AtlasImageFormat::Png,
            jpeg_quality: 90,
            runtime_capture: true,
            pack_library_bitmaps: false,
            multi_page: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasSize {
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasFrameEntry {
    pub filename: String,
    pub frame: AtlasRect,
    pub rotated: bool,
    pub trimmed: bool,
    #[serde(rename = "spriteSourceSize")]
    pub sprite_source_size: AtlasRect,
    #[serde(rename = "sourceSize")]
    pub source_size: AtlasSize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub page: usize,
}

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}

/// One packed texture page inside a multi-page or single-page atlas (`T-250`).
#[derive(Clone, Debug)]
pub struct RenderedAtlasPage {
    pub page_index: usize,
    pub image_filename: String,
    pub image_bytes: Vec<u8>,
    pub sheet_width: u32,
    pub sheet_height: u32,
    pub frames: Vec<AtlasFrameEntry>,
}

/// In-memory result of rendering and packing a sprite sheet.
#[derive(Clone, Debug)]
pub struct RenderedAtlas {
    pub png_bytes: Vec<u8>,
    pub metadata_text: String,
    pub sheet_width: u32,
    pub sheet_height: u32,
    pub frames: Vec<AtlasFrameEntry>,
    pub unique_sprites: usize,
    pub pages: Vec<RenderedAtlasPage>,
}

/// Disk export summary for `export_project_atlas`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasExportReport {
    pub image_path: PathBuf,
    pub metadata_path: PathBuf,
    pub sheet_width: u32,
    pub sheet_height: u32,
    pub frame_count: usize,
    pub unique_sprites: usize,
    #[serde(default)]
    pub page_paths: Vec<PathBuf>,
}

struct TrimmedFrame {
    name: String,
    source_w: u32,
    source_h: u32,
    trim_x: u32,
    trim_y: u32,
    trim_w: u32,
    trim_h: u32,
    trimmed: bool,
    pixels: Vec<u8>,
}

fn trim_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
    do_trim: bool,
) -> (u32, u32, u32, u32, bool, Vec<u8>) {
    if !do_trim || width == 0 || height == 0 {
        return (0, 0, width.max(1), height.max(1), false, rgba.to_vec());
    }
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any_opaque = false;

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if rgba.get(idx + 3).copied().unwrap_or(0) > 0 {
                any_opaque = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if !any_opaque {
        // Keep a 1x1 transparent pixel when the entire frame is empty.
        return (0, 0, 1, 1, true, vec![0, 0, 0, 0]);
    }

    let tw = max_x - min_x + 1;
    let th = max_y - min_y + 1;
    let trimmed = min_x > 0 || min_y > 0 || tw < width || th < height;
    if !trimmed {
        return (0, 0, width, height, false, rgba.to_vec());
    }

    let mut cropped = vec![0u8; (tw * th * 4) as usize];
    for row in 0..th {
        let src_start = (((min_y + row) * width + min_x) * 4) as usize;
        let src_end = src_start + (tw * 4) as usize;
        let dst_start = (row * tw * 4) as usize;
        let dst_end = dst_start + (tw * 4) as usize;
        cropped[dst_start..dst_end].copy_from_slice(&rgba[src_start..src_end]);
    }
    (min_x, min_y, tw, th, true, cropped)
}

/// Deterministic MaxRects Best-Short-Side-Fit (`BSSF`) bin packer.
struct MaxRectsBin {
    free_rects: Vec<AtlasRect>,
}

impl MaxRectsBin {
    fn new(width: u32, height: u32, border: u32) -> Self {
        let usable_w = width.saturating_sub(border * 2).max(1);
        let usable_h = height.saturating_sub(border * 2).max(1);
        Self {
            free_rects: vec![AtlasRect {
                x: border,
                y: border,
                w: usable_w,
                h: usable_h,
            }],
        }
    }

    fn insert(&mut self, w: u32, h: u32, pad: u32) -> Option<AtlasRect> {
        let req_w = w.saturating_add(pad);
        let req_h = h.saturating_add(pad);
        let mut best_idx = None;
        let mut best_short = u32::MAX;
        let mut best_long = u32::MAX;
        let mut best_pos = (u32::MAX, u32::MAX);

        for (i, r) in self.free_rects.iter().enumerate() {
            let allow_w = if r.w >= req_w {
                req_w
            } else if r.w >= w {
                w
            } else {
                continue;
            };
            let allow_h = if r.h >= req_h {
                req_h
            } else if r.h >= h {
                h
            } else {
                continue;
            };
            let leftover_h = r.w.saturating_sub(allow_w);
            let leftover_v = r.h.saturating_sub(allow_h);
            let short = leftover_h.min(leftover_v);
            let long = leftover_h.max(leftover_v);
            let pos = (r.y, r.x);
            if (short, long, pos) < (best_short, best_long, best_pos) {
                best_short = short;
                best_long = long;
                best_pos = pos;
                best_idx = Some(i);
            }
        }

        let idx = best_idx?;
        let chosen = self.free_rects[idx];
        let placed = AtlasRect {
            x: chosen.x,
            y: chosen.y,
            w,
            h,
        };
        let split_box = AtlasRect {
            x: chosen.x,
            y: chosen.y,
            w: req_w.min(chosen.w),
            h: req_h.min(chosen.h),
        };
        self.split_free_rects(split_box);
        self.prune_free_rects();
        Some(placed)
    }

    fn split_free_rects(&mut self, used: AtlasRect) {
        let mut next = Vec::with_capacity(self.free_rects.len() + 4);
        for r in self.free_rects.drain(..) {
            if used.x >= r.x + r.w
                || used.x + used.w <= r.x
                || used.y >= r.y + r.h
                || used.y + used.h <= r.y
            {
                next.push(r);
                continue;
            }
            if used.x > r.x {
                next.push(AtlasRect {
                    x: r.x,
                    y: r.y,
                    w: used.x - r.x,
                    h: r.h,
                });
            }
            if used.x + used.w < r.x + r.w {
                next.push(AtlasRect {
                    x: used.x + used.w,
                    y: r.y,
                    w: (r.x + r.w) - (used.x + used.w),
                    h: r.h,
                });
            }
            if used.y > r.y {
                next.push(AtlasRect {
                    x: r.x,
                    y: r.y,
                    w: r.w,
                    h: used.y - r.y,
                });
            }
            if used.y + used.h < r.y + r.h {
                next.push(AtlasRect {
                    x: r.x,
                    y: used.y + used.h,
                    w: r.w,
                    h: (r.y + r.h) - (used.y + used.h),
                });
            }
        }
        self.free_rects = next;
    }

    fn prune_free_rects(&mut self) {
        let mut keep = vec![true; self.free_rects.len()];
        for i in 0..self.free_rects.len() {
            if !keep[i] {
                continue;
            }
            for j in (i + 1)..self.free_rects.len() {
                if !keep[j] {
                    continue;
                }
                let a = self.free_rects[i];
                let b = self.free_rects[j];
                if a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h {
                    keep[i] = false;
                    break;
                }
                if b.x >= a.x && b.y >= a.y && b.x + b.w <= a.x + a.w && b.y + b.h <= a.y + a.h {
                    keep[j] = false;
                }
            }
        }
        let mut idx = 0;
        self.free_rects.retain(|_| {
            let k = keep[idx];
            idx += 1;
            k
        });
    }
}

fn encode_atlas_sheet_bytes(
    sheet: &RgbaImage,
    format: AtlasImageFormat,
    jpeg_quality: u8,
    bg: [u8; 4],
) -> Result<Vec<u8>, ExportError> {
    match format {
        AtlasImageFormat::Png => {
            crate::bitmap_encode::encode_rgba_png(sheet.as_raw(), sheet.width(), sheet.height())
        }
        AtlasImageFormat::Webp => crate::bitmap_encode::encode_rgba_webp_lossless(
            sheet.as_raw(),
            sheet.width(),
            sheet.height(),
        ),
        AtlasImageFormat::Jpeg => crate::bitmap_encode::encode_rgba_jpeg_composited(
            sheet.as_raw(),
            sheet.width(),
            sheet.height(),
            jpeg_quality,
            bg,
        ),
    }
}

fn page_image_filename(base_filename: &str, page_index: usize) -> String {
    if page_index == 0 {
        return base_filename.to_owned();
    }
    if let Some((stem, ext)) = base_filename.rsplit_once('.') {
        format!("{stem}_{}.{ext}", page_index + 1)
    } else {
        format!("{base_filename}_{}", page_index + 1)
    }
}

fn atlas_xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {}
            _ => out.push(ch),
        }
    }
    out
}

fn atlas_css_escape_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            ')' => out.push_str("\\)"),
            '\n' | '\r' | '\0' => {}
            _ => out.push(ch),
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn format_metadata(
    format: AtlasFormat,
    image_filename: &str,
    sheet_w: u32,
    sheet_h: u32,
    entries: &[AtlasFrameEntry],
    pages: &[RenderedAtlasPage],
    animation_prefix: &str,
    timeline_frame_count: usize,
) -> String {
    let textures_json = (pages.len() > 1).then(|| {
        pages
            .iter()
            .map(|p| {
                serde_json::json!({
                    "image": p.image_filename,
                    "format": "RGBA8888",
                    "size": { "w": p.sheet_width, "h": p.sheet_height },
                    "scale": "1",
                    "frames": p.frames,
                })
            })
            .collect::<Vec<_>>()
    });

    match format {
        AtlasFormat::JsonArray => {
            let mut root = serde_json::json!({
                "frames": entries,
                "meta": {
                    "app": "NeoFlash",
                    "version": "1.0",
                    "image": image_filename,
                    "format": "RGBA8888",
                    "size": { "w": sheet_w, "h": sheet_h },
                    "scale": "1"
                }
            });
            if let Some(textures) = textures_json
                && let Some(obj) = root.as_object_mut()
            {
                obj.insert("textures".into(), serde_json::json!(textures));
            }
            serde_json::to_string_pretty(&root).unwrap_or_default()
        }
        AtlasFormat::JsonHash => {
            let mut map = serde_json::Map::new();
            for entry in entries {
                let mut item = serde_json::json!({
                    "frame": entry.frame,
                    "rotated": entry.rotated,
                    "trimmed": entry.trimmed,
                    "spriteSourceSize": entry.sprite_source_size,
                    "sourceSize": entry.source_size,
                });
                if pages.len() > 1
                    && let Some(obj) = item.as_object_mut()
                {
                    obj.insert("page".into(), serde_json::json!(entry.page));
                }
                map.insert(entry.filename.clone(), item);
            }
            let mut root = serde_json::json!({
                "frames": map,
                "meta": {
                    "app": "NeoFlash",
                    "version": "1.0",
                    "image": image_filename,
                    "format": "RGBA8888",
                    "size": { "w": sheet_w, "h": sheet_h },
                    "scale": "1"
                }
            });
            if let Some(textures) = textures_json
                && let Some(obj) = root.as_object_mut()
            {
                obj.insert("textures".into(), serde_json::json!(textures));
            }
            serde_json::to_string_pretty(&root).unwrap_or_default()
        }
        AtlasFormat::StarlingXml => {
            let mut xml = String::new();
            let _ = writeln!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
            let esc_img = atlas_xml_escape(image_filename);
            let _ = writeln!(
                xml,
                "<TextureAtlas imagePath=\"{esc_img}\" width=\"{sheet_w}\" height=\"{sheet_h}\">"
            );
            for e in entries {
                let page_attr = if pages.len() > 1 {
                    format!(" page=\"{}\"", e.page)
                } else {
                    String::new()
                };
                let esc_name = atlas_xml_escape(&e.filename);
                if e.trimmed {
                    let _ = writeln!(
                        xml,
                        "  <SubTexture name=\"{esc_name}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" frameX=\"-{}\" frameY=\"-{}\" frameWidth=\"{}\" frameHeight=\"{}\"{page_attr}/>",
                        e.frame.x,
                        e.frame.y,
                        e.frame.w,
                        e.frame.h,
                        e.sprite_source_size.x,
                        e.sprite_source_size.y,
                        e.source_size.w,
                        e.source_size.h,
                    );
                } else {
                    let _ = writeln!(
                        xml,
                        "  <SubTexture name=\"{esc_name}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{page_attr}/>",
                        e.frame.x, e.frame.y, e.frame.w, e.frame.h,
                    );
                }
            }
            let _ = writeln!(xml, "</TextureAtlas>");
            xml
        }
        AtlasFormat::EaselJs => {
            let images: Vec<&str> = if pages.is_empty() {
                vec![image_filename]
            } else {
                pages.iter().map(|p| p.image_filename.as_str()).collect()
            };
            let frames: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let reg_x = -(e.sprite_source_size.x as i64);
                    let reg_y = -(e.sprite_source_size.y as i64);
                    serde_json::json!([
                        e.frame.x, e.frame.y, e.frame.w, e.frame.h, e.page, reg_x, reg_y
                    ])
                })
                .collect();
            let mut animations = serde_json::Map::new();
            let anim_len = timeline_frame_count.min(entries.len());
            if anim_len > 0 {
                animations.insert(
                    animation_prefix.to_owned(),
                    serde_json::json!([0, anim_len.saturating_sub(1)]),
                );
            }
            let root = serde_json::json!({
                "images": images,
                "frames": frames,
                "animations": animations,
                "meta": {
                    "app": "NeoFlash",
                    "version": "1.0"
                }
            });
            serde_json::to_string_pretty(&root).unwrap_or_default()
        }
        AtlasFormat::Css => {
            let mut css = String::new();
            let _ = writeln!(css, "/* Generated by NeoFlash Texture Atlas Exporter */");
            let esc_main_img = atlas_css_escape_url(image_filename);
            let _ = writeln!(
                css,
                ".nf-sprite {{ display: inline-block; background-image: url('{esc_main_img}'); background-repeat: no-repeat; }}"
            );
            let mut used_classes = std::collections::HashSet::<String>::new();
            for e in entries {
                let base_cls: String = e
                    .filename
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect();
                let stem = if base_cls.is_empty() {
                    "frame".to_owned()
                } else {
                    base_cls
                };
                let mut safe_cls = stem.clone();
                let mut seq = 2usize;
                while !used_classes.insert(safe_cls.clone()) {
                    safe_cls = format!("{stem}-{seq}");
                    seq += 1;
                }
                let page_bg = if e.page > 0
                    && let Some(p) = pages.get(e.page)
                {
                    format!(
                        " background-image: url('{}');",
                        atlas_css_escape_url(&p.image_filename)
                    )
                } else {
                    String::new()
                };
                let _ = writeln!(
                    css,
                    ".nf-sprite-{safe_cls} {{ width: {}px; height: {}px; background-position: -{}px -{}px;{page_bg} }}",
                    e.frame.w, e.frame.h, e.frame.x, e.frame.y
                );
            }
            css
        }
    }
}

/// Render and pack a sprite-sheet / texture-atlas in memory.
pub fn render_project_atlas(
    project: &Project,
    image_filename: &str,
    options: &AtlasOptions,
) -> Result<RenderedAtlas, ExportError> {
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    let eval_project = if options.symbol_id.is_none() && !project.scenes.is_empty() {
        flatten_scenes_with_ranges(project).0
    } else {
        project.clone()
    };
    let target_symbol_id = options
        .symbol_id
        .as_deref()
        .unwrap_or(eval_project.root.as_str());
    let symbol = eval_project
        .symbol(target_symbol_id)
        .ok_or_else(|| ExportError::Io(std::io::Error::other("target symbol not found")))?;
    let total_frames = symbol.timeline.frame_count.max(1);
    let start_f = options.start_frame.min(total_frames - 1);
    let end_f = options.end_frame.unwrap_or(total_frames - 1).max(start_f);
    let clamped_end_f =
        if total_frames == 1 && options.end_frame.is_some() && options.runtime_capture {
            end_f.min(9_999)
        } else {
            end_f.min(total_frames - 1).max(start_f)
        };

    let fw = options.frame_width.unwrap_or(eval_project.width).max(1);
    let fh = options.frame_height.unwrap_or(eval_project.height).max(1);
    let prefix = symbol.name.replace(' ', "_");
    let extrude = options.extrude.min(16);

    let mut bitmap_cache = HashMap::new();
    let mut trimmed_frames = Vec::with_capacity((clamped_end_f - start_f + 1) as usize);

    let mut playback = options.runtime_capture.then(|| {
        let mut p = neoflash_core::AuthoredPlayback::with_live_scripts(
            &eval_project,
            target_symbol_id,
            0,
            true,
        );
        for _ in 0..start_f {
            p.tick(&eval_project);
        }
        p
    });

    for f in start_f..=clamped_end_f {
        if f > start_f
            && let Some(p) = &mut playback
        {
            p.tick(&eval_project);
        }
        let scene = if let Some(p) = &playback {
            p.scene(&eval_project, false, true)
        } else {
            evaluate_scene(&eval_project, target_symbol_id, f, false, true)
        };
        let rgba = render_authored_scene_rgba(
            &eval_project,
            target_symbol_id,
            f,
            &scene,
            fw,
            fh,
            options.transparent_background,
            &mut bitmap_cache,
        )?;
        let (tx, ty, tw, th, trimmed, pixels) = trim_rgba(&rgba, fw, fh, options.trim);
        trimmed_frames.push(TrimmedFrame {
            name: format!("{prefix}_{:04}", f + 1),
            source_w: fw,
            source_h: fh,
            trim_x: tx,
            trim_y: ty,
            trim_w: tw,
            trim_h: th,
            trimmed,
            pixels,
        });
    }
    let timeline_frame_count = trimmed_frames.len();

    if options.pack_library_bitmaps {
        for res in &eval_project.resources {
            if !res.is_image() {
                continue;
            }
            if let Ok(decoded) = res.decode_image() {
                let (tx, ty, tw, th, trimmed, pixels) =
                    trim_rgba(&decoded.rgba, decoded.width, decoded.height, options.trim);
                trimmed_frames.push(TrimmedFrame {
                    name: res.path.clone(),
                    source_w: decoded.width,
                    source_h: decoded.height,
                    trim_x: tx,
                    trim_y: ty,
                    trim_w: tw,
                    trim_h: th,
                    trimmed,
                    pixels,
                });
            }
        }
    }

    // Deduplicate identical sprite buffers if enabled.
    let mut unique_indices: Vec<usize> = Vec::new();
    let mut frame_to_unique: Vec<usize> = Vec::with_capacity(trimmed_frames.len());
    for (idx, tf) in trimmed_frames.iter().enumerate() {
        if options.deduplicate
            && let Some(pos) = unique_indices.iter().copied().find(|&u| {
                let other = &trimmed_frames[u];
                other.trim_w == tf.trim_w && other.trim_h == tf.trim_h && other.pixels == tf.pixels
            })
        {
            frame_to_unique.push(pos);
        } else {
            unique_indices.push(idx);
            frame_to_unique.push(idx);
        }
    }

    // Sort unique sprites largest-area-first (tie-break by index) for optimal MaxRects packing.
    let mut pack_order = unique_indices.clone();
    pack_order.sort_by(|&a, &b| {
        let fa = &trimmed_frames[a];
        let fb = &trimmed_frames[b];
        let area_a = u64::from(fa.trim_w) * u64::from(fa.trim_h);
        let area_b = u64::from(fb.trim_w) * u64::from(fb.trim_h);
        area_b
            .cmp(&area_a)
            .then_with(|| fb.trim_h.max(fb.trim_w).cmp(&fa.trim_h.max(fa.trim_w)))
            .then_with(|| a.cmp(&b))
    });

    let max_sprite_w = pack_order
        .iter()
        .map(|&u| trimmed_frames[u].trim_w + extrude * 2)
        .max()
        .unwrap_or(fw);
    let max_sprite_h = pack_order
        .iter()
        .map(|&u| trimmed_frames[u].trim_h + extrude * 2)
        .max()
        .unwrap_or(fh);
    let max_w = options
        .max_width
        .max(max_sprite_w + options.border_padding * 2);
    let max_h = options
        .max_height
        .max(max_sprite_h + options.border_padding * 2);

    struct BinPage {
        bin: MaxRectsBin,
        used_w: u32,
        used_h: u32,
        sprites: Vec<(usize, AtlasRect)>,
    }

    let mut bin_pages: Vec<BinPage> = vec![BinPage {
        bin: MaxRectsBin::new(max_w, max_h, options.border_padding),
        used_w: 1,
        used_h: 1,
        sprites: Vec::new(),
    }];
    let mut placed_map: HashMap<usize, (usize, AtlasRect)> =
        HashMap::with_capacity(pack_order.len());

    for u_idx in pack_order {
        let tf = &trimmed_frames[u_idx];
        let req_w = tf.trim_w + extrude * 2;
        let req_h = tf.trim_h + extrude * 2;
        let mut placed = None;
        for (p_idx, page) in bin_pages.iter_mut().enumerate() {
            if let Some(outer_rect) = page.bin.insert(req_w, req_h, options.shape_padding) {
                page.used_w = page
                    .used_w
                    .max(outer_rect.x + outer_rect.w + options.border_padding);
                page.used_h = page
                    .used_h
                    .max(outer_rect.y + outer_rect.h + options.border_padding);
                let inner_rect = AtlasRect {
                    x: outer_rect.x + extrude,
                    y: outer_rect.y + extrude,
                    w: tf.trim_w,
                    h: tf.trim_h,
                };
                page.sprites.push((u_idx, inner_rect));
                placed = Some((p_idx, inner_rect));
                break;
            }
        }
        if placed.is_none() {
            if !options.multi_page {
                return Err(ExportError::Io(std::io::Error::other(
                    "sprites exceed maximum atlas dimensions",
                )));
            }
            let p_idx = bin_pages.len();
            let mut new_bin = MaxRectsBin::new(max_w, max_h, options.border_padding);
            let outer_rect = new_bin
                .insert(req_w, req_h, options.shape_padding)
                .ok_or_else(|| {
                    ExportError::Io(std::io::Error::other(
                        "sprites exceed maximum atlas dimensions",
                    ))
                })?;
            let inner_rect = AtlasRect {
                x: outer_rect.x + extrude,
                y: outer_rect.y + extrude,
                w: tf.trim_w,
                h: tf.trim_h,
            };
            bin_pages.push(BinPage {
                bin: new_bin,
                used_w: (outer_rect.x + outer_rect.w + options.border_padding).max(1),
                used_h: (outer_rect.y + outer_rect.h + options.border_padding).max(1),
                sprites: vec![(u_idx, inner_rect)],
            });
            placed = Some((p_idx, inner_rect));
        }
        placed_map.insert(u_idx, placed.expect("sprite placed"));
    }

    let mut entries = Vec::with_capacity(trimmed_frames.len());
    for (idx, tf) in trimmed_frames.iter().enumerate() {
        let u_idx = frame_to_unique[idx];
        let (page_idx, frame_rect) = placed_map[&u_idx];
        entries.push(AtlasFrameEntry {
            filename: tf.name.clone(),
            frame: frame_rect,
            rotated: false,
            trimmed: tf.trimmed,
            sprite_source_size: AtlasRect {
                x: tf.trim_x,
                y: tf.trim_y,
                w: tf.trim_w,
                h: tf.trim_h,
            },
            source_size: AtlasSize {
                w: tf.source_w,
                h: tf.source_h,
            },
            page: page_idx,
        });
    }

    let mut pages = Vec::with_capacity(bin_pages.len());
    for (p_idx, page) in bin_pages.iter().enumerate() {
        let sheet_w = if options.power_of_two {
            page.used_w.next_power_of_two()
        } else {
            page.used_w
        };
        let sheet_h = if options.power_of_two {
            page.used_h.next_power_of_two()
        } else {
            page.used_h
        };
        let mut sheet = RgbaImage::new(sheet_w, sheet_h);
        for &(u_idx, rect) in &page.sprites {
            let tf = &trimmed_frames[u_idx];
            let sample_px = |sx: i32, sy: i32| -> image::Rgba<u8> {
                let cx = sx.clamp(0, tf.trim_w.saturating_sub(1) as i32) as u32;
                let cy = sy.clamp(0, tf.trim_h.saturating_sub(1) as i32) as u32;
                let p_idx = ((cy * tf.trim_w + cx) * 4) as usize;
                image::Rgba([
                    tf.pixels[p_idx],
                    tf.pixels[p_idx + 1],
                    tf.pixels[p_idx + 2],
                    tf.pixels[p_idx + 3],
                ])
            };
            let ext_i = extrude as i32;
            for dy in -ext_i..(tf.trim_h as i32 + ext_i) {
                for dx in -ext_i..(tf.trim_w as i32 + ext_i) {
                    let px = (rect.x as i32 + dx) as u32;
                    let py = (rect.y as i32 + dy) as u32;
                    if px < sheet_w && py < sheet_h {
                        sheet.put_pixel(px, py, sample_px(dx, dy));
                    }
                }
            }
        }
        let image_bytes = encode_atlas_sheet_bytes(
            &sheet,
            options.image_format,
            options.jpeg_quality,
            eval_project.background,
        )?;
        let page_frames: Vec<AtlasFrameEntry> = entries
            .iter()
            .filter(|e| e.page == p_idx)
            .cloned()
            .collect();
        pages.push(RenderedAtlasPage {
            page_index: p_idx,
            image_filename: page_image_filename(image_filename, p_idx),
            image_bytes,
            sheet_width: sheet_w,
            sheet_height: sheet_h,
            frames: page_frames,
        });
    }

    let first_w = pages.first().map_or(1, |p| p.sheet_width);
    let first_h = pages.first().map_or(1, |p| p.sheet_height);
    let first_bytes = pages
        .first()
        .map(|p| p.image_bytes.clone())
        .unwrap_or_default();
    let metadata_text = format_metadata(
        options.format,
        image_filename,
        first_w,
        first_h,
        &entries,
        &pages,
        &prefix,
        timeline_frame_count,
    );

    Ok(RenderedAtlas {
        png_bytes: first_bytes,
        metadata_text,
        sheet_width: first_w,
        sheet_height: first_h,
        frames: entries,
        unique_sprites: unique_indices.len(),
        pages,
    })
}

/// Export a sprite-sheet image (`.png`/`.webp`/`.jpg`) and its metadata file (`.json`/`.xml`/`.css`) to disk.
pub fn export_project_atlas(
    project: &Project,
    metadata_output_path: impl AsRef<Path>,
    options: &AtlasOptions,
) -> Result<AtlasExportReport, ExportError> {
    let metadata_path = metadata_output_path.as_ref().to_path_buf();
    let parent = metadata_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let img_ext = options.image_format.extension();
    let image_path = metadata_path.with_extension(img_ext);
    let default_name = format!("spritesheet.{img_ext}");
    let image_filename = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&default_name);

    let rendered = render_project_atlas(project, image_filename, options)?;
    let mut page_paths = Vec::with_capacity(rendered.pages.len());
    for page in &rendered.pages {
        let p_path = if page.page_index == 0 {
            image_path.clone()
        } else {
            parent.join(&page.image_filename)
        };
        atomic_write(&p_path, &page.image_bytes)?;
        page_paths.push(p_path);
    }
    atomic_write(&metadata_path, rendered.metadata_text.as_bytes())?;

    Ok(AtlasExportReport {
        image_path,
        metadata_path,
        sheet_width: rendered.sheet_width,
        sheet_height: rendered.sheet_height,
        frame_count: rendered.frames.len(),
        unique_sprites: rendered.unique_sprites,
        page_paths,
    })
}

/// Packs all `image/*` bitmap resources in `project` into a combined `sprite_sheet.png` and
/// `sprite_sheet.json` manifest (with per-bitmap `{ id, path, media_type, frame, sourceSize }`
/// and TexturePacker-compatible `frames`). Returns `Ok(None)` when the project has no bitmap
/// resources.
pub fn pack_project_bitmap_resources_sprite_sheet(
    project: &Project,
    pretty: bool,
) -> Result<Option<(Vec<u8>, String)>, ExportError> {
    pack_project_bitmap_resources_sprite_sheet_filtered(project, None, pretty)
}

/// Packs `image/*` bitmap resources in `project` (optionally restricted to `only_ids`) into a
/// combined `sprite_sheet.png` and `sprite_sheet.json` manifest.
pub fn pack_project_bitmap_resources_sprite_sheet_filtered(
    project: &Project,
    only_ids: Option<&std::collections::BTreeSet<String>>,
    pretty: bool,
) -> Result<Option<(Vec<u8>, String)>, ExportError> {
    struct DecodedBitmap {
        id: String,
        path: String,
        media_type: String,
        rgba: RgbaImage,
    }

    let mut bitmaps = Vec::<DecodedBitmap>::new();
    let mut decoded_rgba_bytes = 0usize;
    for res in &project.resources {
        if !res.is_image() {
            continue;
        }
        if let Some(allowed) = only_ids
            && !allowed.contains(&res.id)
        {
            continue;
        }
        let bytes = res.decoded_bytes().map_err(|e| {
            ExportError::Options(format!(
                "Bitmap resource '{}' ({}) could not be decoded for the sprite sheet: {e}",
                res.id, res.path
            ))
        })?;
        let format = image::guess_format(&bytes).map_err(|e| {
            ExportError::Options(format!(
                "Bitmap resource '{}' ({}) is corrupt or unsupported for the sprite sheet: {e}",
                res.id, res.path
            ))
        })?;
        let (width, height) = ImageReader::with_format(Cursor::new(&bytes), format)
            .into_dimensions()
            .map_err(|e| {
                ExportError::Options(format!(
                    "Bitmap resource '{}' ({}) has an unreadable image header: {e}",
                    res.id, res.path
                ))
            })?;
        if width == 0 || height == 0 {
            return Err(ExportError::Options(format!(
                "Bitmap resource '{}' ({}) has zero dimensions and cannot be packed.",
                res.id, res.path
            )));
        }
        if width > neoflash_core::MAX_BITMAP_DIMENSION
            || height > neoflash_core::MAX_BITMAP_DIMENSION
        {
            return Err(ExportError::Options(format!(
                "Bitmap resource '{}' ({}) is {width}x{height}, exceeding the {}x{} image limit.",
                res.id,
                res.path,
                neoflash_core::MAX_BITMAP_DIMENSION,
                neoflash_core::MAX_BITMAP_DIMENSION
            )));
        }
        let rgba_bytes = checked_rgba_bytes(width, height, "Bitmap resource")?;
        reserve_bitmap_decode_bytes(&mut decoded_rgba_bytes, rgba_bytes)?;

        let mut limits = Limits::default();
        limits.max_image_width = Some(neoflash_core::MAX_BITMAP_DIMENSION);
        limits.max_image_height = Some(neoflash_core::MAX_BITMAP_DIMENSION);
        limits.max_alloc = Some(MAX_BITMAP_SHEET_DECODED_BYTES as u64);
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        reader.limits(limits);
        let rgba = reader
            .decode()
            .map_err(|e| {
                ExportError::Options(format!(
                    "Bitmap resource '{}' ({}) could not be decoded within sprite-sheet limits: {e}",
                    res.id, res.path
                ))
            })?
            .into_rgba8();
        bitmaps.push(DecodedBitmap {
            id: res.id.clone(),
            path: res.path.clone(),
            media_type: res.media_type.clone(),
            rgba,
        });
    }

    if bitmaps.is_empty() {
        return Ok(None);
    }

    let border = 1u32;
    let pad = 1u32;
    let max_w = bitmaps.iter().map(|b| b.rgba.width()).max().unwrap_or(1);
    let max_h = bitmaps.iter().map(|b| b.rgba.height()).max().unwrap_or(1);
    let total_area = bitmaps.iter().try_fold(0u64, |total, bitmap| {
        let padded_width = bitmap.rgba.width().checked_add(pad).ok_or_else(|| {
            ExportError::Options("Bitmap sprite-sheet padded width overflow.".into())
        })?;
        let padded_height = bitmap.rgba.height().checked_add(pad).ok_or_else(|| {
            ExportError::Options("Bitmap sprite-sheet padded height overflow.".into())
        })?;
        let area = u64::from(padded_width)
            .checked_mul(u64::from(padded_height))
            .ok_or_else(|| ExportError::Options("Bitmap sprite-sheet area overflow.".into()))?;
        total
            .checked_add(area)
            .ok_or_else(|| ExportError::Options("Bitmap sprite-sheet total area overflow.".into()))
    })?;
    let sqrt_side = (total_area as f64).sqrt().ceil() as u32;
    let minimum_width = max_w
        .checked_add(border * 2)
        .ok_or_else(|| ExportError::Options("Bitmap sprite-sheet width overflow.".into()))?;
    let minimum_height = max_h
        .checked_add(border * 2)
        .ok_or_else(|| ExportError::Options("Bitmap sprite-sheet height overflow.".into()))?;
    let mut sheet_w = minimum_width.max(sqrt_side).max(4);
    let mut sheet_h = minimum_height.max(sqrt_side).max(4);
    if sheet_w > MAX_BITMAP_SHEET_DIMENSION || sheet_h > MAX_BITMAP_SHEET_DIMENSION {
        return Err(ExportError::Options(format!(
            "Bitmap resources need an initial {sheet_w}x{sheet_h} sheet, exceeding the {MAX_BITMAP_SHEET_DIMENSION}x{MAX_BITMAP_SHEET_DIMENSION} limit."
        )));
    }

    let mut order: Vec<usize> = (0..bitmaps.len()).collect();
    order.sort_by_key(|&i| {
        let w = bitmaps[i].rgba.width();
        let h = bitmaps[i].rgba.height();
        (std::cmp::Reverse(w.max(h)), std::cmp::Reverse(w * h), i)
    });

    let placed_rects = loop {
        let mut bin = MaxRectsBin::new(sheet_w, sheet_h, border);
        let mut rects = vec![
            AtlasRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            };
            bitmaps.len()
        ];
        let mut all_fit = true;
        for &idx in &order {
            let w = bitmaps[idx].rgba.width();
            let h = bitmaps[idx].rgba.height();
            if let Some(rect) = bin.insert(w, h, pad) {
                rects[idx] = rect;
            } else {
                all_fit = false;
                break;
            }
        }
        if all_fit {
            break rects;
        }
        if sheet_w <= sheet_h
            && let Some(next) = grow_bitmap_sheet_dimension(sheet_w)
        {
            sheet_w = next;
        } else if let Some(next) = grow_bitmap_sheet_dimension(sheet_h) {
            sheet_h = next;
        } else if let Some(next) = grow_bitmap_sheet_dimension(sheet_w) {
            sheet_w = next;
        } else {
            return Err(ExportError::Options(format!(
                "Bitmap resources do not fit within the {MAX_BITMAP_SHEET_DIMENSION}x{MAX_BITMAP_SHEET_DIMENSION} sprite-sheet limit."
            )));
        }
    };

    let used_w = placed_rects.iter().try_fold(1u32, |used, rect| {
        rect.x
            .checked_add(rect.w)
            .and_then(|value| value.checked_add(border))
            .map(|value| used.max(value))
            .ok_or_else(|| ExportError::Options("Bitmap sprite-sheet width overflow.".into()))
    })?;
    let used_h = placed_rects.iter().try_fold(1u32, |used, rect| {
        rect.y
            .checked_add(rect.h)
            .and_then(|value| value.checked_add(border))
            .map(|value| used.max(value))
            .ok_or_else(|| ExportError::Options("Bitmap sprite-sheet height overflow.".into()))
    })?;
    let output_bytes = checked_rgba_bytes(used_w, used_h, "Bitmap sprite sheet")?;
    if output_bytes > MAX_BITMAP_SHEET_OUTPUT_BYTES {
        return Err(ExportError::Options(format!(
            "Packed bitmap sprite sheet would require {output_bytes} RGBA bytes, exceeding the {} MiB output budget.",
            MAX_BITMAP_SHEET_OUTPUT_BYTES / (1024 * 1024)
        )));
    }
    let mut sheet = RgbaImage::new(used_w, used_h);
    for (idx, bmp) in bitmaps.iter().enumerate() {
        let rect = placed_rects[idx];
        for y in 0..bmp.rgba.height() {
            for x in 0..bmp.rgba.width() {
                sheet.put_pixel(rect.x + x, rect.y + y, *bmp.rgba.get_pixel(x, y));
            }
        }
    }

    let mut png_bytes = Vec::new();
    sheet
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;

    let mut frames_map = serde_json::Map::new();
    let mut bitmaps_list = Vec::with_capacity(bitmaps.len());
    for (idx, bmp) in bitmaps.iter().enumerate() {
        let rect = placed_rects[idx];
        let key = if bmp.path.trim().is_empty() {
            bmp.id.clone()
        } else {
            bmp.path.clone()
        };
        frames_map.insert(
            key,
            serde_json::json!({
                "frame": rect,
                "rotated": false,
                "trimmed": false,
                "spriteSourceSize": { "x": 0, "y": 0, "w": rect.w, "h": rect.h },
                "sourceSize": { "w": rect.w, "h": rect.h },
            }),
        );
        bitmaps_list.push(serde_json::json!({
            "id": bmp.id,
            "path": bmp.path,
            "media_type": bmp.media_type,
            "frame": rect,
            "sourceSize": { "w": rect.w, "h": rect.h },
        }));
    }

    let manifest_val = serde_json::json!({
        "generator": "NeoFlash",
        "image": "sprite_sheet.png",
        "format": "RGBA8888",
        "size": { "w": used_w, "h": used_h },
        "bitmaps": bitmaps_list,
        "frames": frames_map,
    });
    let manifest_json = if pretty {
        serde_json::to_string_pretty(&manifest_val)?
    } else {
        serde_json::to_string(&manifest_val)?
    };

    Ok(Some((png_bytes, manifest_json)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Easing, Element, ElementKind, Keyframe, ResourceUnit, Transform, new_id};

    #[test]
    fn bitmap_sprite_sheet_rejects_corrupt_resources_instead_of_hiding_them() {
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("broken", "images/broken.png", "image/png", b"not-png")
                .unwrap(),
        );
        let error = pack_project_bitmap_resources_sprite_sheet(&project, false).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("broken"), "got: {message}");
        assert!(message.contains("corrupt or unsupported"), "got: {message}");
    }

    #[test]
    fn bitmap_sprite_sheet_enforces_decode_and_sheet_budgets() {
        let mut oversized_png = Vec::new();
        RgbaImage::new(neoflash_core::MAX_BITMAP_DIMENSION + 1, 1)
            .write_to(&mut Cursor::new(&mut oversized_png), ImageFormat::Png)
            .unwrap();
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("wide", "images/wide.png", "image/png", &oversized_png)
                .unwrap(),
        );
        let error = pack_project_bitmap_resources_sprite_sheet(&project, false).unwrap_err();
        assert!(error.to_string().contains("image limit"), "got: {error}");

        let mut total = MAX_BITMAP_SHEET_DECODED_BYTES - 4;
        let error = reserve_bitmap_decode_bytes(&mut total, 8).unwrap_err();
        assert!(error.to_string().contains("input budget"), "got: {error}");
        assert_eq!(
            grow_bitmap_sheet_dimension(MAX_BITMAP_SHEET_DIMENSION),
            None
        );
        let output_bytes = checked_rgba_bytes(
            MAX_BITMAP_SHEET_DIMENSION,
            MAX_BITMAP_SHEET_DIMENSION,
            "test sheet",
        )
        .unwrap();
        assert!(output_bytes > MAX_BITMAP_SHEET_OUTPUT_BYTES);
    }

    #[test]
    fn bitmap_sprite_sheet_does_not_drop_case_variant_image_media_types() {
        let mut png = Vec::new();
        RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes("upper", "images/upper.png", "IMAGE/PNG", &png).unwrap(),
        );
        let (_, manifest) = pack_project_bitmap_resources_sprite_sheet(&project, false)
            .unwrap()
            .expect("uppercase image media type is packed");
        assert!(manifest.contains("upper"));
        assert!(manifest.contains("IMAGE/PNG"));
    }

    #[test]
    fn sprite_sheet_atlas_trims_deduplicates_and_packs_deterministically() {
        let mut project = Project::blank();
        project.width = 64;
        project.height = 64;
        project.symbols[0].timeline.frame_count = 3;
        let rect_id = new_id("box");
        project.symbols[0].timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: rect_id.clone(),
                    name: "Box".into(),
                    transform: Transform {
                        x: 8.0,
                        y: 12.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 20.0,
                        height: 16.0,
                        fill: [255, 40, 80, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                tween: Some(Easing::Linear),
                ..Keyframe::default()
            },
            Keyframe {
                frame: 1,
                elements: vec![Element {
                    id: rect_id.clone(),
                    name: "Box".into(),
                    transform: Transform {
                        x: 24.0,
                        y: 12.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 20.0,
                        height: 16.0,
                        fill: [255, 40, 80, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                tween: None,
                ..Keyframe::default()
            },
        ];

        let opts = AtlasOptions {
            power_of_two: true,
            ..AtlasOptions::default()
        };
        let rendered1 = render_project_atlas(&project, "sheet.png", &opts).expect("atlas 1");
        let rendered2 = render_project_atlas(&project, "sheet.png", &opts).expect("atlas 2");
        assert_eq!(rendered1.png_bytes, rendered2.png_bytes);
        assert_eq!(rendered1.metadata_text, rendered2.metadata_text);
        assert_eq!(rendered1.frames.len(), 3);
        // Frame 1 and Frame 2 hold the same pose (or same 20x16 red box), so unique_sprites < 3!
        assert!(rendered1.unique_sprites <= 2);
        assert!(rendered1.sheet_width.is_power_of_two());
        assert!(rendered1.sheet_height.is_power_of_two());
        assert!(rendered1.frames[0].trimmed);
        assert_eq!(rendered1.frames[0].sprite_source_size.x, 8);
        assert_eq!(rendered1.frames[0].sprite_source_size.y, 12);
    }
}
