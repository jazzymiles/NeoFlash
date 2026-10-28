//! Bounded AS2 gradient-noise kernel. Adapted from the W3C SVG feTurbulence
//! algorithm and Ruffle 07d3e430631333f80f754df01a968bdcd80d98c5's Rust port.
//! Copyright © 2015 W3C® (MIT, ERCIM, Keio, Beihang).
//! This software includes material derived from
//! https://www.w3.org/TR/SVG11/filters.html#feTurbulenceElement.
//! See THIRD_PARTY_NOTICES.txt for the W3C and Ruffle notices.
//!
//! NeoFlash changes: compact 256-entry tables (no duplicated sentinels), finite
//! input/work limits, floor-based negative coordinates, integral periodic lattice
//! wrapping and direct arena RGBA transfer. Exact Flash noise pixels are not claimed.
use super::{BitmapError, error};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerlinSettings {
    pub base: [f64; 2],
    pub octaves: u8,
    pub seed: i32,
    pub stitch: bool,
    pub fractal: bool,
    pub channels: u8,
    pub grayscale: bool,
    pub offsets: Vec<[f64; 2]>,
}
struct Noise {
    permutation: [u8; 256],
    gradient: [[[f64; 2]; 256]; 4],
}
impl Noise {
    fn new(seed: i32) -> Self {
        let modulus = 2147483647_i64;
        let mut state = i64::from(seed);
        if state <= 0 {
            state = -(state % (modulus - 1)) + 1;
        }
        state = state.min(modulus - 1);
        let mut random = || {
            state = state * 16807 % modulus;
            state
        };
        let mut result = Self {
            permutation: std::array::from_fn(|i| i as u8),
            gradient: [[[0.0; 2]; 256]; 4],
        };
        for channel in &mut result.gradient {
            for vector in channel {
                vector[0] = (random() % 512 - 256) as f64 / 256.0;
                vector[1] = (random() % 512 - 256) as f64 / 256.0;
                let length = vector[0].hypot(vector[1]);
                if length == 0.0 {
                    *vector = [1.0, 0.0];
                } else {
                    vector[0] /= length;
                    vector[1] /= length;
                }
            }
        }
        for i in (1..256).rev() {
            result.permutation.swap(i, (random() % 256) as usize);
        }
        result
    }
    fn sample(&self, channel: usize, x: f64, y: f64, period: [i64; 2]) -> f64 {
        let ix = x.floor() as i64;
        let iy = y.floor() as i64;
        let fx = x - ix as f64;
        let fy = y - iy as f64;
        let wrap = |value: i64, axis: usize| {
            if period[axis] > 0 {
                value.rem_euclid(period[axis])
            } else {
                value
            }
        };
        let dot = |dx: i64, dy: i64| {
            let x = wrap(ix + dx, 0).rem_euclid(256) as usize;
            let y = wrap(iy + dy, 1).rem_euclid(256) as usize;
            let vector = self.gradient[channel]
                [self.permutation[(usize::from(self.permutation[x]) + y) & 255] as usize];
            (fx - dx as f64) * vector[0] + (fy - dy as f64) * vector[1]
        };
        let ease = |v: f64| v * v * (3.0 - 2.0 * v);
        let mix = |a: f64, b: f64, t: f64| a + t * (b - a);
        mix(
            mix(dot(0, 0), dot(1, 0), ease(fx)),
            mix(dot(0, 1), dot(1, 1), ease(fx)),
            ease(fy),
        )
    }
}
pub(super) fn generate(
    width: u32,
    height: u32,
    settings: &PerlinSettings,
) -> Result<Vec<[u8; 4]>, BitmapError> {
    if settings.octaves > 16
        || settings.channels > 15
        || settings.offsets.len() > usize::from(settings.octaves)
        || settings
            .base
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0 || (*v > 0.0 && *v < 1.0 / 1024.0) || *v > 1e6)
        || settings
            .offsets
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err(error(
            "perlinNoise requires 0..16 octaves, channel mask0..15, bounded nonnegative bases and finite per-octave offsets",
        ));
    }
    let channels = if settings.grayscale {
        1 + usize::from(settings.channels & 8 != 0)
    } else {
        settings.channels.count_ones() as usize
    };
    if width as usize * height as usize * usize::from(settings.octaves.max(1)) * channels.max(1)
        > 64 * 1024 * 1024
    {
        return Err(error("perlinNoise exceeds the 64 Mi-sample work budget"));
    }
    let noise = Noise::new(settings.seed);
    let dimensions = [width, height];
    let mut frequency = settings
        .base
        .map(|base| if base == 0.0 { 0.0 } else { 1.0 / base });
    let mut period = [0_i64; 2];
    if settings.stitch {
        for axis in 0..2 {
            if frequency[axis] != 0.0 {
                period[axis] = (f64::from(dimensions[axis]) * frequency[axis])
                    .round()
                    .max(1.0) as i64;
                frequency[axis] = period[axis] as f64 / f64::from(dimensions[axis]);
            }
        }
    }
    let value = |x: u32, y: u32, channel: usize| {
        let mut sum = 0.0;
        let mut factor = 1.0;
        let mut stitch = period;
        for octave in 0..usize::from(settings.octaves) {
            let offset = settings.offsets.get(octave).copied().unwrap_or([0.0; 2]);
            let sample = noise.sample(
                channel,
                (f64::from(x) + offset[0]) * frequency[0] * factor,
                (f64::from(y) + offset[1]) * frequency[1] * factor,
                stitch,
            );
            sum += if settings.fractal {
                sample
            } else {
                sample.abs()
            } / factor;
            factor *= 2.0;
            stitch = stitch.map(|period| period * 2);
        }
        if settings.fractal {
            ((sum * 255.0 + 255.0 + 0.5) / 2.0) as u8
        } else {
            (sum * 255.0 + 0.5) as u8
        }
    };
    let mut output = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            let mut pixel = [0, 0, 0, 255];
            if settings.grayscale {
                pixel[..3].fill(value(x, y, 0));
                if settings.channels & 8 != 0 {
                    pixel[3] = value(x, y, 1);
                }
            } else {
                let mut selected = 0;
                for (channel, pixel) in pixel.iter_mut().enumerate() {
                    if settings.channels & (1 << channel) != 0 {
                        *pixel = value(x, y, selected);
                        selected += 1;
                    }
                }
            }
            output.push(pixel);
        }
    }
    Ok(output)
}
