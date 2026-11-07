//! `NF2 Icon/12/*`, `NF2 Icon/16/*` and `NF2 Icon/20/*`, generated from
//! `interface/design-system/exports/icons`.
//!
//! Every grid has its own optical master (T-101 / R-056): 1 pt strokes on half pixels and fill edges
//! on whole pixels at 12 and 16 px, 1.5 pt strokes on .25 / .75 at 20 px. An icon asked for at
//! another grid size is drawn from the master of that size when one exists, never scaled.
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

/// One icon: grid size (12, 16 or 20), Figma name and SVG source.
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
    /// Find an icon by `"12/eye"` / `"16/eye"` / `"20/tool-pen"`.
    pub fn by_path(path: &str) -> Option<Self> {
        let (size, name) = path.split_once('/')?;
        let list: &[Icon] = match size {
            "12" => &i12::ALL,
            "16" => &i16::ALL,
            "20" => &i20::ALL,
            _ => return None,
        };
        list.iter().copied().find(|i| i.name == name)
    }
    /// The optical master of this icon drawn for a `side` point grid: `i16::EYE.at(12.0)` is
    /// `i12::EYE`. Falls back to `self` when no master of that size exists (it is then scaled).
    pub fn at(self, side: f32) -> Self {
        let size = side.round();
        if (side - size).abs() > 0.01 || size as u8 == self.size {
            return self;
        }
        Self::by_path(&format!("{}/{}", size as u8, self.name)).unwrap_or(self)
    }
    pub fn side(self) -> f32 {
        self.size as f32
    }
}

/// The exported SVGs use `#9AA3B6` (text/secondary). Render white so the tint is exact.
const EXPORT_COLOR: &str = "#9AA3B6";

/// (icon or cursor name, master grid size, drawn side, pixels-per-point bits).
type TextureKey = (&'static str, u8, u8, u32);

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
    key: (&'static str, u8),
    svg: &'static str,
    side: f32,
    recolor: bool,
) -> Option<TextureHandle> {
    let scale = ctx.pixels_per_point();
    let id = (key.0, key.1, side as u8, scale.to_bits());
    let cache = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(egui::Id::new("nf2-svg-cache"))
            .clone()
    });
    let mut map = cache.0.lock().ok()?;
    if let Some(t) = map.get(&id) {
        return Some(t.clone());
    }
    let image = rasterize(svg, side, scale, recolor)?;
    let tex = ctx.load_texture(
        format!("nf2/{}/{}@{scale}", key.1, key.0),
        image,
        TextureOptions::LINEAR,
    );
    map.insert(id, tex.clone());
    Some(tex)
}

fn key(icon: Icon) -> (&'static str, u8) {
    // Leak-free static key: the same name exists on several grids, so the master size is part of it.
    (icon.name, icon.size)
}

/// Paint `icon` centred in `rect` at its native grid size, tinted with `color`.
pub fn paint(painter: &egui::Painter, ctx: &egui::Context, rect: Rect, icon: Icon, color: Color32) {
    paint_sized(painter, ctx, rect, icon, icon.side(), color);
}

/// Paint `icon` at `side` points (compact rows and fields, Classic Flash tools). The master of
/// that grid size is used when it exists (`Icon::at`); only other sizes are scaled.
pub fn paint_sized(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    icon: Icon,
    side: f32,
    color: Color32,
) {
    let icon = icon.at(side);
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
