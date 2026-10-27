//! Bounded, original CPU display filters. The host rasterizes a complete display
//! subtree in logical stage pixels; Rust applies ordered filters to straight
//! RGBA bytes. Blur uses repeated separable box passes in premultiplied space.
//! This portable profile does not claim Flash's exact subpixel/rounding kernel.
use serde::{Deserialize, Serialize};

pub const MAX_FILTER_PIXELS: usize = 4 * 1024 * 1024;
pub const MAX_OBJECT_FILTERS: usize = 8;
const MAX_FILTER_WORK: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("Display filters: {0}")]
pub struct FilterError(pub String);
fn error(message: &str) -> FilterError {
    FilterError(message.into())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilterSpec {
    Blur {
        blur_x: f64,
        blur_y: f64,
        quality: u8,
    },
    Glow {
        color: u32,
        alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        inner: bool,
        knockout: bool,
    },
    DropShadow {
        distance: f64,
        angle: f64,
        color: u32,
        alpha: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        quality: u8,
        inner: bool,
        knockout: bool,
        hide_object: bool,
    },
    ColorMatrix {
        matrix: [f64; 20],
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterPadding {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl FilterSpec {
    pub fn validate(&self) -> Result<(), FilterError> {
        let finite =
            |value: f64, low: f64, high: f64| value.is_finite() && (low..=high).contains(&value);
        if let Self::ColorMatrix { matrix } = self {
            if matrix.iter().all(|v| finite(*v, -65536., 65536.)) {
                return Ok(());
            }
            return Err(error(
                "ColorMatrix requires 20 finite coefficients within +/-65536",
            ));
        }
        let (x, y, q) = self.blur();
        if !finite(x, 0., 255.) || !finite(y, 0., 255.) || q > 15 {
            return Err(error(
                "blurX/blurY must be finite in 0..255; quality must be an integer in 0..15",
            ));
        }
        if let Self::Glow {
            color,
            alpha,
            strength,
            ..
        }
        | Self::DropShadow {
            color,
            alpha,
            strength,
            ..
        } = self
            && (*color > 0xffffff || !finite(*alpha, 0., 1.) || !finite(*strength, 0., 255.))
        {
            return Err(error(
                "color must be 24-bit RGB, alpha in 0..1 and strength in 0..255",
            ));
        }
        if let Self::DropShadow {
            distance, angle, ..
        } = self
            && (!finite(*distance, -8192., 8192.) || !finite(*angle, -1e9, 1e9))
        {
            return Err(error(
                "shadow distance must be finite within +/-8192 and angle within +/-1e9 degrees",
            ));
        }
        Ok(())
    }
    fn blur(&self) -> (f64, f64, u8) {
        match self {
            Self::Blur {
                blur_x,
                blur_y,
                quality,
            }
            | Self::Glow {
                blur_x,
                blur_y,
                quality,
                ..
            }
            | Self::DropShadow {
                blur_x,
                blur_y,
                quality,
                ..
            } => (*blur_x, *blur_y, *quality),
            Self::ColorMatrix { .. } => (0., 0., 0),
        }
    }
    fn radii(&self) -> (usize, usize, usize) {
        let (x, y, q) = self.blur();
        (
            (x / 2.).ceil() as usize,
            (y / 2.).ceil() as usize,
            q as usize,
        )
    }
    fn work(&self) -> usize {
        let (x, y, q) = self.radii();
        let channels = if matches!(self, Self::Blur { .. }) {
            4
        } else {
            1
        };
        4 + (usize::from(x > 0) + usize::from(y > 0)) * q * channels
    }
}

pub fn validate_filters(filters: &[FilterSpec]) -> Result<(), FilterError> {
    if filters.len() > MAX_OBJECT_FILTERS {
        return Err(error("an object supports at most 8 ordered filters"));
    }
    for filter in filters {
        filter.validate()?;
    }
    Ok(())
}

/// Padding is additive across the ordered chain. The host must provide this
/// transparent border before calling filter_rgba; the kernel never resizes it.
pub fn filter_padding(filters: &[FilterSpec]) -> Result<FilterPadding, FilterError> {
    validate_filters(filters)?;
    let mut result = FilterPadding::default();
    for filter in filters {
        let (x, y, q) = filter.radii();
        let (x, y) = ((x * q) as f64, (y * q) as f64);
        let (dx, dy) = match filter {
            FilterSpec::DropShadow {
                distance, angle, ..
            } => {
                let a = angle.rem_euclid(360.).to_radians();
                (distance * a.cos(), distance * a.sin())
            }
            _ => (0., 0.),
        };
        if matches!(
            filter,
            FilterSpec::Glow { inner: true, .. }
                | FilterSpec::DropShadow { inner: true, .. }
                | FilterSpec::ColorMatrix { .. }
        ) {
            continue;
        }
        result.left += (x - dx).max(0.).ceil() as u32;
        result.right += (x + dx).max(0.).ceil() as u32;
        result.top += (y - dy).max(0.).ceil() as u32;
        result.bottom += (y + dy).max(0.).ceil() as u32;
    }
    Ok(result)
}

pub fn filter_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    filters: &[FilterSpec],
) -> Result<Vec<u8>, FilterError> {
    validate_filters(filters)?;
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| error("raster size overflow"))?;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || count > MAX_FILTER_PIXELS
        || rgba.len() != count * 4
    {
        return Err(error(
            "raster must be 1..8192 per side, at most 4194304 pixels, with exactly width*height*4 RGBA bytes",
        ));
    }
    let work = filters.iter().map(FilterSpec::work).sum::<usize>();
    if count.saturating_mul(work) > MAX_FILTER_WORK {
        return Err(error(
            "filter chain exceeds the 128 Mi channel-pass work budget; reduce raster size or quality",
        ));
    }
    let mut result = rgba.to_vec();
    for filter in filters {
        match filter {
            FilterSpec::ColorMatrix { matrix } => {
                for pixel in result.as_chunks_mut::<4>().0.iter_mut() {
                    let old = [
                        pixel[0] as f64,
                        pixel[1] as f64,
                        pixel[2] as f64,
                        pixel[3] as f64,
                    ];
                    for row in 0..4 {
                        let m = &matrix[row * 5..row * 5 + 5];
                        pixel[row] = byte(
                            m[0] * old[0] + m[1] * old[1] + m[2] * old[2] + m[3] * old[3] + m[4],
                        );
                    }
                    if pixel[3] == 0 {
                        pixel[..3].fill(0);
                    }
                }
            }
            FilterSpec::Blur { .. } => {
                let (rx, ry, q) = filter.radii();
                if q == 0 || rx + ry == 0 {
                    continue;
                }
                let values = result
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| {
                        let a = p[3] as u16;
                        [p[0] as u16 * a, p[1] as u16 * a, p[2] as u16 * a, a * 255]
                    })
                    .collect();
                let blurred = blur(values, width as usize, height as usize, 4, rx, ry, q);
                for (dst, src) in result
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(blurred.as_chunks::<4>().0.iter())
                {
                    let a = src[3] as f64;
                    for c in 0..3 {
                        dst[c] = if a > 0. {
                            byte(src[c] as f64 * 255. / a)
                        } else {
                            0
                        };
                    }
                    dst[3] = byte(a / 255.);
                    if dst[3] == 0 {
                        dst[..3].fill(0);
                    }
                }
            }
            FilterSpec::Glow {
                color,
                alpha,
                strength,
                inner,
                knockout,
                ..
            } => {
                shadow(
                    &mut result,
                    width as usize,
                    height as usize,
                    filter,
                    *color,
                    *alpha,
                    *strength,
                    *inner,
                    *knockout,
                    false,
                    0.,
                    0.,
                );
            }
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                strength,
                inner,
                knockout,
                hide_object,
                ..
            } => {
                let a = angle.rem_euclid(360.).to_radians();
                shadow(
                    &mut result,
                    width as usize,
                    height as usize,
                    filter,
                    *color,
                    *alpha,
                    *strength,
                    *inner,
                    *knockout,
                    *hide_object,
                    distance * a.cos(),
                    distance * a.sin(),
                );
            }
        }
    }
    Ok(result)
}
fn byte(value: f64) -> u8 {
    value.round().clamp(0., 255.) as u8
}

/// Constant-time-per-pixel sliding sums, independent of blur radius. Each pass
/// samples transparent outside the padded raster, never repeats edge colors.
#[allow(clippy::too_many_arguments)]
fn blur(
    mut source: Vec<u16>,
    w: usize,
    h: usize,
    channels: usize,
    rx: usize,
    ry: usize,
    quality: usize,
) -> Vec<u16> {
    if quality == 0 || rx + ry == 0 {
        return source;
    }
    let mut target = vec![0; source.len()];
    for _ in 0..quality {
        for (horizontal, radius) in [(true, rx), (false, ry)] {
            if radius == 0 {
                continue;
            }
            let (length, lines) = if horizontal { (w, h) } else { (h, w) };
            let denominator = (radius * 2 + 1) as u64;
            for line in 0..lines {
                for channel in 0..channels {
                    let index = |position: usize| {
                        if horizontal {
                            (line * w + position) * channels + channel
                        } else {
                            (position * w + line) * channels + channel
                        }
                    };
                    let mut sum: u64 = (0..=radius.min(length - 1))
                        .map(|p| source[index(p)] as u64)
                        .sum();
                    for position in 0..length {
                        target[index(position)] = ((sum + denominator / 2) / denominator) as u16;
                        if position >= radius {
                            sum -= source[index(position - radius)] as u64;
                        }
                        if position + radius + 1 < length {
                            sum += source[index(position + radius + 1)] as u64;
                        }
                    }
                }
            }
            std::mem::swap(&mut source, &mut target);
        }
    }
    source
}

#[allow(clippy::too_many_arguments)]
fn shadow(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    filter: &FilterSpec,
    color: u32,
    alpha: f64,
    strength: f64,
    inner: bool,
    knockout: bool,
    hide: bool,
    dx: f64,
    dy: f64,
) {
    let (rx, ry, q) = filter.radii();
    let mask = blur(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3] as u16 * 255)
            .collect(),
        w,
        h,
        1,
        rx,
        ry,
        q,
    );
    let sample = |x: i64, y: i64| -> f64 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0.
        } else {
            mask[y as usize * w + x as usize] as f64 / 65025.
        }
    };
    let color = [
        ((color >> 16) & 255) as f64,
        ((color >> 8) & 255) as f64,
        (color & 255) as f64,
    ];
    for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (index % w) as f64 - dx;
        let y = (index / w) as f64 - dy;
        let ix = x.floor() as i64;
        let iy = y.floor() as i64;
        let fx = x - ix as f64;
        let fy = y - iy as f64;
        let a = (sample(ix, iy) * (1. - fx) + sample(ix + 1, iy) * fx) * (1. - fy)
            + (sample(ix, iy + 1) * (1. - fx) + sample(ix + 1, iy + 1) * fx) * fy;
        let original = pixel[3] as f64 / 255.;
        let effect = ((if inner { 1. - a } else { a }) * alpha * strength).clamp(0., 1.);
        let (effect_alpha, original_weight) = if inner {
            (
                effect * original,
                if knockout || hide {
                    0.
                } else {
                    original * (1. - effect)
                },
            )
        } else if knockout {
            (effect * (1. - original), 0.)
        } else if hide {
            (effect, 0.)
        } else {
            (effect * (1. - original), original)
        };
        let out_alpha = effect_alpha + original_weight;
        for c in 0..3 {
            pixel[c] = if out_alpha > 0. {
                byte((color[c] * effect_alpha + pixel[c] as f64 * original_weight) / out_alpha)
            } else {
                0
            };
        }
        pixel[3] = byte(out_alpha * 255.);
        if pixel[3] == 0 {
            pixel[..3].fill(0);
        }
    }
}
