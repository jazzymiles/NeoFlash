//! Shared SVG paint servers for the vector and single-file banner exporters.
use crate::ExportError;
use neoflash_core::{
    AuthoredBitmapFill, ColorTransform, Element, ElementKind, Project, invert_affine_matrix,
    path_bounds,
};
use std::{collections::HashMap, fmt::Write};

#[derive(Default)]
pub(super) struct SvgBitmapPaints {
    images: HashMap<String, (String, u32, u32)>,
    patterns: HashMap<String, String>,
    filters: usize,
}

impl SvgBitmapPaints {
    pub(super) fn definitions(&self) -> usize {
        self.images.len() + self.patterns.len() + self.filters
    }

    pub(super) fn paint(
        &mut self,
        project: &Project,
        kind: &ElementKind,
        fill: &AuthoredBitmapFill,
        color: ColorTransform,
        defs: &mut String,
    ) -> Result<String, ExportError> {
        if !self.images.contains_key(&fill.resource_id) {
            let resource = project.resource(&fill.resource_id).ok_or_else(|| {
                ExportError::Options(format!(
                    "Missing bitmap fill resource '{}'",
                    fill.resource_id
                ))
            })?;
            let image = resource
                .decode_image()
                .map_err(|e| ExportError::Options(e.to_string()))?;
            let id = format!("nf-img-{}", self.images.len());
            let _ = write!(
                defs,
                "<image id=\"{id}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:{};base64,{}\"/>",
                image.width, image.height, resource.media_type, resource.data_base64
            );
            self.images
                .insert(fill.resource_id.clone(), (id, image.width, image.height));
        }
        let (image_id, width, height) = &self.images[&fill.resource_id];
        let w = f64::from(*width);
        let h = f64::from(*height);
        let [a, b, c, d, tx, ty] = fill.matrix;
        let Some([ia, ib, ic, id, ix, iy]) = invert_affine_matrix(fill.matrix) else {
            let image = project
                .resource(&fill.resource_id)
                .unwrap()
                .decode_image()
                .map_err(|e| ExportError::Options(e.to_string()))?;
            let [r, g, b, a] = color.apply(image.rgba[..4].try_into().unwrap());
            return Ok(format!("rgba({r},{g},{b},{})", f64::from(a) / 255.));
        };
        let (mut left, mut top, mut right, mut bottom) = (0., 0., w, h);
        if !fill.repeat {
            let el = Element {
                kind: kind.clone(),
                ..Default::default()
            };
            if let Some((commands, _)) = el.to_path_commands()
                && let Some(bounds) =
                    path_bounds(&commands).map_err(|e| ExportError::Options(e.to_string()))?
            {
                for x in [bounds.x_min, bounds.x_max] {
                    for y in [bounds.y_min, bounds.y_max] {
                        let px = ia * x + ic * y + ix;
                        let py = ib * x + id * y + iy;
                        left = f64::min(left, px.floor());
                        right = right.max(px.ceil());
                        top = f64::min(top, py.floor());
                        bottom = bottom.max(py.ceil());
                    }
                }
            }
        }
        let key = serde_json::to_string(&(
            image_id,
            fill.matrix,
            fill.smoothing,
            color,
            [left, top, right, bottom],
        ))
        .map_err(|e| ExportError::Options(e.to_string()))?;
        if let Some(id) = self.patterns.get(&key) {
            return Ok(format!("url(#{id})"));
        }
        let id = format!("nf-pattern-{}", self.patterns.len());
        let _ = write!(
            defs,
            "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" x=\"{left}\" y=\"{top}\" width=\"{}\" height=\"{}\" patternTransform=\"matrix({a} {b} {c} {d} {tx} {ty})\" viewBox=\"{left} {top} {} {}\" preserveAspectRatio=\"none\">",
            right - left,
            bottom - top,
            right - left,
            bottom - top
        );
        if !color.is_identity() {
            self.filters += 1;
            let _ = write!(
                defs,
                "<filter id=\"{id}-ct\" color-interpolation-filters=\"sRGB\"><feComponentTransfer>"
            );
            for (i, ch) in ["R", "G", "B", "A"].iter().enumerate() {
                let _ = write!(
                    defs,
                    "<feFunc{ch} type=\"linear\" slope=\"{}\" intercept=\"{}\"/>",
                    color.multipliers[i],
                    color.offsets[i] / 255.
                );
            }
            let _ = write!(
                defs,
                "</feComponentTransfer></filter><g filter=\"url(#{id}-ct)\">"
            );
        }
        let rendering = if fill.smoothing { "auto" } else { "pixelated" };
        let _ = write!(
            defs,
            "<g image-rendering=\"{rendering}\"><use href=\"#{image_id}\"/>"
        );
        // Extend the outer texels into uncovered parts of a non-repeating brush.
        for (x, dx, sx, sw) in [
            (left, -left, 0., 1.),
            (0., w, 0., w),
            (w, right - w, w - 1., 1.),
        ] {
            for (y, dy, sy, sh) in [
                (top, -top, 0., 1.),
                (0., h, 0., h),
                (h, bottom - h, h - 1., 1.),
            ] {
                if dx > 0. && dy > 0. && !(x == 0. && y == 0. && dx == w && dy == h) {
                    let _ = write!(
                        defs,
                        "<svg x=\"{x}\" y=\"{y}\" width=\"{dx}\" height=\"{dy}\" viewBox=\"{sx} {sy} {sw} {sh}\" preserveAspectRatio=\"none\" overflow=\"hidden\"><use href=\"#{image_id}\"/></svg>"
                    );
                }
            }
        }
        defs.push_str("</g>");
        if !color.is_identity() {
            defs.push_str("</g>");
        }
        defs.push_str("</pattern>");
        self.patterns.insert(key, id.clone());
        Ok(format!("url(#{id})"))
    }
}
