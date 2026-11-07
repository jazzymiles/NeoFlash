//! Sprite-sheet and texture-atlas publishing (`T-110`).
//!
//! Rasterizes timeline or symbol frame sequences using the shared CPU renderer, trims transparent
//! borders (`spriteSourceSize` / `sourceSize`), deduplicates identical frames, packs sprites
//! deterministically via MaxRects Best-Short-Side-Fit (`BSSF`), and emits a PNG sheet alongside
//! TexturePacker-compatible JSON Array, JSON Hash (PixiJS / Phaser), or Starling XML metadata.

use crate::{ExportError, video::render_authored_scene_rgba};
use image::{ImageFormat, RgbaImage};
use neoflash_core::{Project, evaluate_scene, flatten_scenes_with_ranges};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Write as _,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

/// Metadata schema emitted alongside the packed PNG sprite sheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtlasFormat {
    #[default]
    JsonHash,
    JsonArray,
    StarlingXml,
}

impl AtlasFormat {
    pub const ALL: [Self; 3] = [Self::JsonHash, Self::JsonArray, Self::StarlingXml];

    pub fn label(self) -> &'static str {
        match self {
            Self::JsonHash => "JSON Hash (PixiJS / Phaser)",
            Self::JsonArray => "JSON Array (TexturePacker)",
            Self::StarlingXml => "Starling / Sparrow XML",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::JsonHash | Self::JsonArray => "json",
            Self::StarlingXml => "xml",
        }
    }
}

/// Configuration for sprite-sheet / texture-atlas generation (`T-110`).
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
            trim: true,
            deduplicate: true,
            power_of_two: false,
            transparent_background: true,
            format: AtlasFormat::JsonHash,
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

fn format_metadata(
    format: AtlasFormat,
    image_filename: &str,
    sheet_w: u32,
    sheet_h: u32,
    entries: &[AtlasFrameEntry],
) -> String {
    match format {
        AtlasFormat::JsonArray => {
            let root = serde_json::json!({
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
            serde_json::to_string_pretty(&root).unwrap_or_default()
        }
        AtlasFormat::JsonHash => {
            let mut map = serde_json::Map::new();
            for entry in entries {
                map.insert(
                    entry.filename.clone(),
                    serde_json::json!({
                        "frame": entry.frame,
                        "rotated": entry.rotated,
                        "trimmed": entry.trimmed,
                        "spriteSourceSize": entry.sprite_source_size,
                        "sourceSize": entry.source_size,
                    }),
                );
            }
            let root = serde_json::json!({
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
            serde_json::to_string_pretty(&root).unwrap_or_default()
        }
        AtlasFormat::StarlingXml => {
            let mut xml = String::new();
            let _ = writeln!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
            let _ = writeln!(
                xml,
                "<TextureAtlas imagePath=\"{image_filename}\" width=\"{sheet_w}\" height=\"{sheet_h}\">"
            );
            for e in entries {
                if e.trimmed {
                    let _ = writeln!(
                        xml,
                        "  <SubTexture name=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" frameX=\"-{}\" frameY=\"-{}\" frameWidth=\"{}\" frameHeight=\"{}\"/>",
                        e.filename,
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
                        "  <SubTexture name=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                        e.filename, e.frame.x, e.frame.y, e.frame.w, e.frame.h,
                    );
                }
            }
            let _ = writeln!(xml, "</TextureAtlas>");
            xml
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
    let end_f = options
        .end_frame
        .unwrap_or(total_frames - 1)
        .min(total_frames - 1)
        .max(start_f);

    let fw = options.frame_width.unwrap_or(eval_project.width).max(1);
    let fh = options.frame_height.unwrap_or(eval_project.height).max(1);
    let prefix = symbol.name.replace(' ', "_");

    let mut bitmap_cache = HashMap::new();
    let mut trimmed_frames = Vec::with_capacity((end_f - start_f + 1) as usize);

    for f in start_f..=end_f {
        let scene = evaluate_scene(&eval_project, target_symbol_id, f, false, true);
        let rgba = render_authored_scene_rgba(
            &eval_project,
            &scene,
            fw,
            fh,
            options.transparent_background,
            &mut bitmap_cache,
        );
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

    let max_w = options.max_width.max(fw + options.border_padding * 2);
    let max_h = options.max_height.max(fh + options.border_padding * 2);
    let mut bin = MaxRectsBin::new(max_w, max_h, options.border_padding);
    let mut placed_map: HashMap<usize, AtlasRect> = HashMap::with_capacity(pack_order.len());
    let mut used_w = 1u32;
    let mut used_h = 1u32;

    for u_idx in pack_order {
        let tf = &trimmed_frames[u_idx];
        let rect = bin
            .insert(tf.trim_w, tf.trim_h, options.shape_padding)
            .ok_or_else(|| {
                ExportError::Io(std::io::Error::other(
                    "sprites exceed maximum atlas dimensions",
                ))
            })?;
        used_w = used_w.max(rect.x + rect.w + options.border_padding);
        used_h = used_h.max(rect.y + rect.h + options.border_padding);
        placed_map.insert(u_idx, rect);
    }

    let sheet_w = if options.power_of_two {
        used_w.next_power_of_two()
    } else {
        used_w
    };
    let sheet_h = if options.power_of_two {
        used_h.next_power_of_two()
    } else {
        used_h
    };

    let mut sheet = RgbaImage::new(sheet_w, sheet_h);
    for (&u_idx, rect) in &placed_map {
        let tf = &trimmed_frames[u_idx];
        for row in 0..tf.trim_h {
            for col in 0..tf.trim_w {
                let p_idx = ((row * tf.trim_w + col) * 4) as usize;
                let px = [
                    tf.pixels[p_idx],
                    tf.pixels[p_idx + 1],
                    tf.pixels[p_idx + 2],
                    tf.pixels[p_idx + 3],
                ];
                sheet.put_pixel(rect.x + col, rect.y + row, image::Rgba(px));
            }
        }
    }

    let mut png_bytes = Vec::new();
    sheet
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;

    let mut entries = Vec::with_capacity(trimmed_frames.len());
    for (idx, tf) in trimmed_frames.iter().enumerate() {
        let u_idx = frame_to_unique[idx];
        let frame_rect = placed_map[&u_idx];
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
        });
    }

    let metadata_text = format_metadata(options.format, image_filename, sheet_w, sheet_h, &entries);

    Ok(RenderedAtlas {
        png_bytes,
        metadata_text,
        sheet_width: sheet_w,
        sheet_height: sheet_h,
        frames: entries,
        unique_sprites: unique_indices.len(),
    })
}

/// Export a sprite-sheet PNG and its metadata file (`.json` or `.xml`) to disk.
pub fn export_project_atlas(
    project: &Project,
    metadata_output_path: impl AsRef<Path>,
    options: &AtlasOptions,
) -> Result<AtlasExportReport, ExportError> {
    let metadata_path = metadata_output_path.as_ref().to_path_buf();
    if let Some(parent) = metadata_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let image_path = metadata_path.with_extension("png");
    let image_filename = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("spritesheet.png");

    let rendered = render_project_atlas(project, image_filename, options)?;
    fs::write(&image_path, &rendered.png_bytes)?;
    fs::write(&metadata_path, &rendered.metadata_text)?;

    Ok(AtlasExportReport {
        image_path,
        metadata_path,
        sheet_width: rendered.sheet_width,
        sheet_height: rendered.sheet_height,
        frame_count: rendered.frames.len(),
        unique_sprites: rendered.unique_sprites,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Easing, Element, ElementKind, Keyframe, Transform, new_id};

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
