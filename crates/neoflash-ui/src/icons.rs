//! `NF2 Icon/16/*` and `NF2 Icon/20/*`, generated from `interface/design-system/exports/icons`.
//!
//! Icons are monochrome strokes. They are rasterised once per (icon, display scale) in white and
//! tinted at paint time, so the colour always comes from a token:
//! `text_secondary` default, `text_primary` hover, `accent_default` active, `text_on_accent` on primary.
//!
//! ```ignore
//! use neoflash_ui::icons::{i16, i20};
//! neoflash_ui::icons::paint(ui.painter(), ui.ctx(), rect, i16::EYE, palette.text_secondary);
//! ```
use eframe::egui::{self, Color32, Rect, TextureHandle, TextureOptions, emath::GuiRounding as _};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One icon: grid size (16 or 20), Figma name and SVG source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    pub size: u8,
    pub name: &'static str,
    pub svg: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

impl Icon {
    /// `"16/eye"`, the name used in the reference and the outlines.
    pub fn path(self) -> String {
        format!("{}/{}", self.size, self.name)
    }
    /// Find an icon by `"16/eye"` / `"20/tool-pen"`.
    pub fn by_path(path: &str) -> Option<Self> {
        let (size, name) = path.split_once('/')?;
        let list: &[Icon] = match size {
            "16" => &i16::ALL,
            "20" => &i20::ALL,
            _ => return None,
        };
        list.iter().copied().find(|i| i.name == name)
    }
    pub fn side(self) -> f32 {
        self.size as f32
    }
}

/// The exported SVGs use `#9AA3B6` (text/secondary). Render white so the tint is exact.
const EXPORT_COLOR: &str = "#9AA3B6";

/// (icon or cursor name, grid size, pixels-per-point bits).
type TextureKey = (&'static str, u8, u32);

#[derive(Clone, Default)]
struct Cache(Arc<Mutex<HashMap<TextureKey, TextureHandle>>>);

/// Rasterise an SVG in white at `scale` pixels per point.
pub(crate) fn rasterize(
    svg: &str,
    side: f32,
    scale: f32,
    recolor: bool,
) -> Option<egui::ColorImage> {
    let source = if recolor {
        svg.replace(EXPORT_COLOR, "#FFFFFF")
    } else {
        svg.to_owned()
    };
    let tree = resvg::usvg::Tree::from_str(&source, &resvg::usvg::Options::default()).ok()?;
    let px = (side * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let k = px as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [px as usize, px as usize],
        pixmap.data(),
    ))
}

pub(crate) fn texture(
    ctx: &egui::Context,
    key: &'static str,
    svg: &'static str,
    side: f32,
    recolor: bool,
) -> Option<TextureHandle> {
    let scale = ctx.pixels_per_point();
    let id = (key, side as u8, scale.to_bits());
    let cache = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(egui::Id::new("nf2-svg-cache"))
            .clone()
    });
    let mut map = cache.0.lock().ok()?;
    if let Some(t) = map.get(&id) {
        return Some(t.clone());
    }
    let image = rasterize(svg, side, scale, recolor)?;
    let tex = ctx.load_texture(format!("nf2/{key}@{scale}"), image, TextureOptions::LINEAR);
    map.insert(id, tex.clone());
    Some(tex)
}

fn key(icon: Icon) -> &'static str {
    // Leak-free static key: name is 'static; size is part of the cache id.
    icon.name
}

/// Paint `icon` centred in `rect` at its native grid size, tinted with `color`.
pub fn paint(painter: &egui::Painter, ctx: &egui::Context, rect: Rect, icon: Icon, color: Color32) {
    paint_sized(painter, ctx, rect, icon, icon.side(), color);
}

/// Paint `icon` scaled to `side` points (compact steppers, 12 px chevrons). Prefer native sizes.
pub fn paint_sized(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    icon: Icon,
    side: f32,
    color: Color32,
) {
    let target = Rect::from_center_size(rect.center(), egui::vec2(side, side));
    // Snap to physical pixels so 1.4 px strokes stay crisp.
    let target = Rect::from_min_size(
        target.min.round_to_pixels(painter.pixels_per_point()),
        target.size(),
    );
    if let Some(tex) = texture(ctx, key(icon), icon.svg, side, true) {
        painter.image(
            tex.id(),
            target,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            color,
        );
    }
}

/// An icon as a non-interactive widget (inline in labels, rows, empty states).
pub fn show(ui: &mut egui::Ui, icon: Icon, color: Color32) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(icon.side(), icon.side()), egui::Sense::hover());
    paint(ui.painter(), ui.ctx(), rect, icon, color);
    response
}
