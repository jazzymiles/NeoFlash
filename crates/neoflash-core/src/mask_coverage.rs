//! Prepared, renderer-independent mask coverage for native previews and picking.
//! Evaluation remains pure: immutable image decoding shares the resource cache.
use crate::{
    ColorTransform, DecodedImage, FillRule, FlattenOptions, FlattenedContour, GradientPaint,
    PathBounds, Project, RuntimeClip, flatten_path, path_bounds,
};
use std::{collections::HashMap, sync::Arc};

const MAX_POINTS: usize = 1_000_000;
const MAX_DEPTH: usize = 64;
const MAX_RASTER_PIXELS: usize = 4_194_304;
const MAX_SAMPLE_WORK: usize = 128_000_000;
type Edge = ([f64; 2], [f64; 2]);
type EdgeRows = Vec<Vec<Edge>>;

struct Component {
    edges: EdgeRows,
    bounds: Option<PathBounds>,
    inverse: Option<[f64; 6]>,
    rule: FillRule,
    alpha: f64,
    gradient: Option<GradientPaint>,
    bitmap: Option<(Arc<DecodedImage>, u32, u32, bool, ColorTransform)>,
    clips: Vec<usize>,
}
struct Clip {
    alpha: bool,
    components: Vec<Component>,
}

/// Geometry unions are boolean; alpha sources use source-over union. Referenced
/// masks intersect each source before it joins its enclosing mask.
pub struct MaskCoverage {
    indices: HashMap<String, usize>,
    clips: Vec<Clip>,
    points: usize,
    work_per_point: usize,
    alpha_clips: Vec<bool>,
}
impl MaskCoverage {
    pub fn new(project: &Project, clips: &[RuntimeClip]) -> Result<Self, String> {
        let indices: HashMap<_, _> = clips
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id.clone(), i))
            .collect();
        if indices.len() != clips.len() || clips.len() > 4096 {
            return Err("Mask coverage has duplicate IDs or exceeds 4096 masks".into());
        }
        let mut result = Self {
            indices,
            clips: Vec::with_capacity(clips.len()),
            points: 0,
            work_per_point: 0,
            alpha_clips: Vec::new(),
        };
        let mut edge_entries = 0;
        for clip in clips {
            let mut components = Vec::new();
            let sources: Vec<_> = if let Some(alpha) = &clip.alpha_components {
                alpha
                    .iter()
                    .map(|c| {
                        (
                            &c.commands,
                            c.fill_rule,
                            c.matrix,
                            &c.clip_ids,
                            c.alpha,
                            c.fill_gradient.clone(),
                            c.resource_bitmap.as_ref(),
                            c.fill_bitmap.is_some(),
                        )
                    })
                    .collect()
            } else {
                clip.components
                    .iter()
                    .map(|c| {
                        (
                            &c.commands,
                            c.fill_rule,
                            c.matrix,
                            &c.clip_ids,
                            1.,
                            None,
                            None,
                            false,
                        )
                    })
                    .collect()
            };
            for (commands, rule, matrix, ids, alpha, gradient, bitmap, runtime_bitmap) in sources {
                if runtime_bitmap {
                    return Err("Native authored mask requires a portable image resource, not runtime bitmap pixels".into());
                }
                let contours = flatten_path(
                    commands,
                    FlattenOptions {
                        tolerance: 0.1,
                        ..Default::default()
                    },
                )
                .map_err(|error| error.to_string())?;
                result.points += contours.iter().map(|c| c.points.len()).sum::<usize>();
                if result.points > MAX_POINTS {
                    return Err(
                        "Mask coverage exceeds one million flattened points; simplify the mask"
                            .into(),
                    );
                }
                let bitmap = bitmap
                    .map(|paint| {
                        let resource = project
                            .resources
                            .iter()
                            .find(|r| r.id == paint.resource_id)
                            .ok_or_else(|| {
                                format!("Mask image '{}' is missing", paint.resource_id)
                            })?;
                        let decoded = resource.decode_image().map_err(|e| e.to_string())?;
                        Ok::<_, String>((
                            decoded,
                            paint.width,
                            paint.height,
                            paint.smoothing,
                            paint.color_transform,
                        ))
                    })
                    .transpose()?;
                let mut clips =
                    ids.iter()
                        .map(|id| {
                            result.indices.get(id).copied().ok_or_else(|| {
                                format!("Mask coverage references missing mask '{id}'")
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                clips.sort_unstable();
                clips.dedup();
                let bounds = path_bounds(commands).map_err(|e| e.to_string())?;
                let edges = edge_rows(&contours, bounds, &mut edge_entries)?;
                result.work_per_point +=
                    edges.iter().map(Vec::len).max().unwrap_or(0) + clips.len() + 1;
                components.push(Component {
                    edges,
                    bounds,
                    inverse: inverse(matrix),
                    rule,
                    alpha: alpha.clamp(0., 1.),
                    gradient,
                    bitmap,
                    clips,
                });
            }
            result.clips.push(Clip {
                alpha: clip.alpha_components.is_some(),
                components,
            });
        }
        fn visit(
            index: usize,
            clips: &[Clip],
            active: &mut Vec<usize>,
            done: &mut [bool],
        ) -> Result<(), String> {
            if active.contains(&index) || active.len() >= MAX_DEPTH {
                return Err("Mask coverage contains a cycle or exceeds 64 nested masks".into());
            }
            if done[index] {
                return Ok(());
            }
            active.push(index);
            for next in clips[index].components.iter().flat_map(|c| &c.clips) {
                visit(*next, clips, active, done)?;
            }
            active.pop();
            done[index] = true;
            Ok(())
        }
        let mut done = vec![false; result.clips.len()];
        for index in 0..result.clips.len() {
            visit(index, &result.clips, &mut Vec::new(), &mut done)?;
        }
        fn has_alpha(index: usize, clips: &[Clip], cache: &mut [Option<bool>]) -> bool {
            if let Some(value) = cache[index] {
                return value;
            }
            let value = clips[index].alpha
                || clips[index]
                    .components
                    .iter()
                    .flat_map(|c| &c.clips)
                    .any(|next| has_alpha(*next, clips, cache));
            cache[index] = Some(value);
            value
        }
        let mut cache = vec![None; result.clips.len()];
        result.work_per_point += result.clips.len();
        result.alpha_clips = (0..result.clips.len())
            .map(|i| has_alpha(i, &result.clips, &mut cache))
            .collect();
        Ok(result)
    }

    pub fn has_alpha(&self, ids: &[String]) -> bool {
        ids.iter()
            .filter_map(|id| self.indices.get(id))
            .any(|i| self.alpha_clips[*i])
    }

    fn resolve(&self, ids: &[String]) -> Option<Vec<usize>> {
        let mut indices = Vec::new();
        for id in ids {
            let index = *self.indices.get(id)?;
            if !indices.contains(&index) {
                indices.push(index);
            }
        }
        Some(indices)
    }

    pub fn alpha(&self, ids: &[String], point: [f64; 2]) -> f64 {
        let Some(indices) = self.resolve(ids) else {
            return 0.;
        };
        let mut cache = vec![f64::NAN; self.clips.len()];
        indices
            .iter()
            .map(|i| self.clip_alpha(*i, point, &mut cache))
            .product()
    }

    /// Pixel centers cover the given world rectangle, with a bounded work budget.
    pub fn raster(
        &self,
        ids: &[String],
        origin: [f64; 2],
        step: [f64; 2],
        width: usize,
        height: usize,
    ) -> Result<Vec<u8>, String> {
        let count = width
            .checked_mul(height)
            .ok_or("Mask raster size overflow")?;
        if count > MAX_RASTER_PIXELS
            || count
                .saturating_mul(self.work_per_point.max(1))
                .saturating_mul(4)
                > MAX_SAMPLE_WORK
        {
            return Err(
                "Mask preview exceeds its pixel/work budget; reduce zoom or simplify the mask"
                    .into(),
            );
        }
        if !origin.into_iter().chain(step).all(f64::is_finite) || step.iter().any(|v| *v <= 0.) {
            return Err("Mask raster has invalid coordinates".into());
        }
        let indices = self
            .resolve(ids)
            .ok_or("Mask raster references a missing mask")?;
        let mut cache = vec![f64::NAN; self.clips.len()];
        let mut pixels = Vec::with_capacity(count);
        for y in 0..height {
            for x in 0..width {
                let mut alpha = 0.;
                for dy in [0.25, 0.75] {
                    for dx in [0.25, 0.75] {
                        cache.fill(f64::NAN);
                        let point = [
                            origin[0] + (x as f64 + dx) * step[0],
                            origin[1] + (y as f64 + dy) * step[1],
                        ];
                        alpha += indices
                            .iter()
                            .map(|i| self.clip_alpha(*i, point, &mut cache))
                            .product::<f64>()
                            * 0.25;
                    }
                }
                pixels.push((alpha * 255.).round() as u8);
            }
        }
        Ok(pixels)
    }

    fn clip_alpha(&self, index: usize, point: [f64; 2], cache: &mut [f64]) -> f64 {
        if !cache[index].is_nan() {
            return cache[index];
        }
        let clip = &self.clips[index];
        let mut result = 0.;
        for component in &clip.components {
            let Some(m) = component.inverse else {
                continue;
            };
            let local = [
                m[0] * point[0] + m[2] * point[1] + m[4],
                m[1] * point[0] + m[3] * point[1] + m[5],
            ];
            if !component.bounds.is_some_and(|b| b.contains(local))
                || !contains(
                    &component.edges,
                    component.bounds.unwrap(),
                    component.rule,
                    local,
                )
            {
                continue;
            }
            let mut alpha = component.alpha;
            if let Some(gradient) = &component.gradient {
                alpha *= f64::from(gradient.sample(local[0], local[1])[3]) / 255.;
            }
            if let Some((image, width, height, smoothing, color)) = &component.bitmap {
                if *width == 0 || *height == 0 {
                    continue;
                }
                let x = local[0] * f64::from(image.width) / f64::from(*width);
                let y = local[1] * f64::from(image.height) / f64::from(*height);
                let a = bitmap_alpha(image, x, y, *smoothing);
                alpha *= (a * color.multipliers[3] + color.offsets[3] / 255.).clamp(0., 1.);
            }
            for id in &component.clips {
                alpha *= self.clip_alpha(*id, point, cache);
            }
            if clip.alpha {
                result += alpha * (1. - result);
            } else {
                result = result.max(alpha);
            }
            if result >= 1. {
                break;
            }
        }
        let result = result.clamp(0., 1.);
        cache[index] = result;
        result
    }
}
fn inverse(m: [f64; 6]) -> Option<[f64; 6]> {
    let d = m[0] * m[3] - m[1] * m[2];
    if !d.is_finite() || d == 0. {
        return None;
    }
    Some([
        m[3] / d,
        -m[1] / d,
        -m[2] / d,
        m[0] / d,
        (m[2] * m[5] - m[3] * m[4]) / d,
        (m[1] * m[4] - m[0] * m[5]) / d,
    ])
}
fn edge_rows(
    contours: &[FlattenedContour],
    bounds: Option<PathBounds>,
    entries: &mut usize,
) -> Result<EdgeRows, String> {
    let Some(bounds) = bounds.filter(|b| b.height() > 0.) else {
        return Ok(vec![]);
    };
    let mut rows = vec![Vec::new(); 64];
    for contour in contours {
        for i in 0..contour.points.len() {
            let a = contour.points[i];
            let b = contour.points[(i + 1) % contour.points.len()];
            if a[1] == b[1] {
                continue;
            }
            let first = row(a[1].min(b[1]), bounds, rows.len());
            let last = row(a[1].max(b[1]), bounds, rows.len());
            *entries += last - first + 1;
            if *entries > MAX_POINTS * 2 {
                return Err(
                    "Mask coverage edge index exceeds its memory budget; simplify the mask".into(),
                );
            }
            for bucket in &mut rows[first..=last] {
                bucket.push((a, b));
            }
        }
    }
    Ok(rows)
}
fn row(y: f64, bounds: PathBounds, count: usize) -> usize {
    (((y - bounds.y_min) / bounds.height() * count as f64).floor() as usize).min(count - 1)
}
fn contains(edges: &[Vec<Edge>], bounds: PathBounds, rule: FillRule, p: [f64; 2]) -> bool {
    if edges.is_empty() {
        return false;
    }
    let mut winding = 0i64;
    for &(a, b) in &edges[row(p[1], bounds, edges.len())] {
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if x > p[0] {
                winding += if b[1] > a[1] { 1 } else { -1 };
            }
        }
    }
    match rule {
        FillRule::EvenOdd => winding % 2 != 0,
        FillRule::NonZero => winding != 0,
    }
}
fn bitmap_alpha(image: &DecodedImage, x: f64, y: f64, smoothing: bool) -> f64 {
    let pixel = |x: i64, y: i64| {
        let x = x.clamp(0, i64::from(image.width) - 1) as usize;
        let y = y.clamp(0, i64::from(image.height) - 1) as usize;
        f64::from(image.rgba[(y * image.width as usize + x) * 4 + 3]) / 255.
    };
    if !smoothing {
        return pixel(x.floor() as i64, y.floor() as i64);
    }
    let (x, y) = (x - 0.5, y - 0.5);
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let a = pixel(ix as i64, iy as i64) * (1. - fx) + pixel(ix as i64 + 1, iy as i64) * fx;
    let b = pixel(ix as i64, iy as i64 + 1) * (1. - fx) + pixel(ix as i64 + 1, iy as i64 + 1) * fx;
    a * (1. - fy) + b * fy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AlphaClipComponent, ClipComponent, PathCommand};
    fn commands() -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::LineTo { x: 10., y: 0. },
            PathCommand::LineTo { x: 10., y: 10. },
            PathCommand::LineTo { x: 0., y: 10. },
            PathCommand::Close,
        ]
    }
    #[test]
    fn prepared_coverage_intersects_nested_masks_and_rejects_cycles_and_excessive_rasters() {
        let alpha = AlphaClipComponent {
            commands: commands(),
            fill_rule: FillRule::NonZero,
            matrix: [1., 0., 0., 1., 0., 0.],
            alpha: 0.5,
            clip_ids: vec![],
            resource_bitmap: None,
            fill_gradient: None,
            fill_bitmap: None,
        };
        let mut clips = vec![
            RuntimeClip {
                id: "alpha".into(),
                components: vec![],
                alpha_components: Some(vec![alpha.clone(), alpha]),
            },
            RuntimeClip {
                id: "outer".into(),
                components: vec![ClipComponent {
                    commands: commands(),
                    fill_rule: FillRule::NonZero,
                    matrix: [1., 0., 0., 1., 5., 0.],
                    clip_ids: vec!["alpha".into()],
                }],
                alpha_components: None,
            },
        ];
        let coverage = MaskCoverage::new(&Project::blank(), &clips).unwrap();
        assert_eq!(coverage.alpha(&["outer".into()], [6., 5.]), 0.75);
        assert_eq!(coverage.alpha(&["outer".into()], [2., 5.]), 0.);
        assert_eq!(coverage.alpha(&["outer".into()], [12., 5.]), 0.);
        assert!(coverage.has_alpha(&["outer".into()]));
        assert_eq!(
            coverage
                .raster(&["outer".into()], [5., 0.], [1., 1.], 2, 2)
                .unwrap(),
            vec![191; 4]
        );
        assert!(
            coverage
                .raster(&["outer".into()], [0.; 2], [1.; 2], 10_000, 10_000)
                .is_err()
        );
        clips[0].alpha_components.as_mut().unwrap()[0]
            .clip_ids
            .push("outer".into());
        assert!(MaskCoverage::new(&Project::blank(), &clips).is_err());
    }
}
