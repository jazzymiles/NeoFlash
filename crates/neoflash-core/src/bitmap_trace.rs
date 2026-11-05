//! Bounded raster-to-vector authoring. Weighted median-cut colour quantization is followed by
//! pixel-edge contour walking, polygon simplification and optional quadratic corner rounding.
//! Geometry is local to the original bitmap; callers own placement, history and cancellation.
use crate::{Color, DecodedImage, FillRule, PathCommand};
use std::collections::{BTreeMap, HashMap};

pub const MAX_TRACE_DIMENSION: u32 = 4096;
pub const MAX_TRACE_PATHS: usize = 20_000;
const MAX_TRACE_EDGES: usize = 1_000_000;
const TRANSPARENT: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceMode {
    Colour,
    Greyscale,
    BlackAndWhite,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracePalette {
    Limited,
    Full,
    Automatic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceMethod {
    Abutting,
    Overlapping,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TraceOptions {
    pub mode: TraceMode,
    pub palette: TracePalette,
    pub colours: u8,
    pub paths: f64,
    pub corners: f64,
    /// Regions smaller than this many source pixels merge into a neighbouring colour;
    /// isolated speckles surrounded by transparency are discarded.
    pub noise: u32,
    pub method: TraceMethod,
    pub fills: bool,
    pub strokes: bool,
    pub stroke_width: f64,
    pub snap_curves: bool,
    pub ignore_white: bool,
}
impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            mode: TraceMode::Colour,
            palette: TracePalette::Limited,
            colours: 16,
            paths: 50.,
            corners: 50.,
            noise: 8,
            method: TraceMethod::Overlapping,
            fills: true,
            strokes: false,
            stroke_width: 10.,
            snap_curves: false,
            ignore_white: false,
        }
    }
}
impl TraceOptions {
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=30).contains(&self.colours)
            || !(1..=100).contains(&self.noise)
            || !self.paths.is_finite()
            || !(0.0..=100.).contains(&self.paths)
            || !self.corners.is_finite()
            || !(0.0..=100.).contains(&self.corners)
            || !self.stroke_width.is_finite()
            || !(0.01..=1000.).contains(&self.stroke_width)
        {
            return Err("Trace options are outside their supported ranges.".into());
        }
        if !self.fills && !self.strokes {
            return Err("Select Fills, Strokes, or both.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct TracedShape {
    pub commands: Vec<PathCommand>,
    pub fill_rule: FillRule,
    pub colour: Color,
}
#[derive(Clone, Debug)]
pub struct BitmapTrace {
    pub width: u32,
    pub height: u32,
    pub shapes: Vec<TracedShape>,
    pub path_count: usize,
}

/// Returns `Ok(None)` on cancellation. `checkpoint` receives monotone progress in 0..=1 and must
/// return false to cancel; it runs at bounded intervals throughout all large loops.
pub fn trace_bitmap(
    image: &DecodedImage,
    options: &TraceOptions,
    mut checkpoint: impl FnMut(f32) -> bool,
) -> Result<Option<BitmapTrace>, String> {
    options.validate()?;
    let (w, h) = (image.width as usize, image.height as usize);
    if w == 0
        || h == 0
        || image.width > MAX_TRACE_DIMENSION
        || image.height > MAX_TRACE_DIMENSION
        || image.rgba.len() != w * h * 4
    {
        return Err("Trace accepts images from 1 × 1 through 4096 × 4096 pixels.".into());
    }
    if !checkpoint(0.) {
        return Ok(None);
    }
    let Some((palette, mut labels)) = quantize(image, options, &mut checkpoint)? else {
        return Ok(None);
    };
    if options.noise > 1 && !remove_noise(&mut labels, w, h, options.noise, &mut checkpoint) {
        return Ok(None);
    }
    let mut edges: Vec<Vec<Edge>> = vec![Vec::new(); palette.len()];
    let mut count = 0usize;
    for y in 0..h {
        if y % 8 == 0 && !checkpoint(0.4 + 0.25 * y as f32 / h as f32) {
            return Ok(None);
        }
        for x in 0..w {
            let a = labels[y * w + x];
            if a == TRANSPARENT {
                continue;
            }
            for (b, px, py, dir) in [
                (
                    if y > 0 {
                        labels[(y - 1) * w + x]
                    } else {
                        TRANSPARENT
                    },
                    x,
                    y,
                    0,
                ),
                (
                    if x + 1 < w {
                        labels[y * w + x + 1]
                    } else {
                        TRANSPARENT
                    },
                    x + 1,
                    y,
                    1,
                ),
                (
                    if y + 1 < h {
                        labels[(y + 1) * w + x]
                    } else {
                        TRANSPARENT
                    },
                    x + 1,
                    y + 1,
                    2,
                ),
                (
                    if x > 0 {
                        labels[y * w + x - 1]
                    } else {
                        TRANSPARENT
                    },
                    x,
                    y + 1,
                    3,
                ),
            ] {
                if a == b {
                    continue;
                }
                // Overlapping paints use the most common opaque colour as an underpaint. All other
                // colours sit above it; transparent pixels and translucent regions remain holes.
                let under_a = options.method == TraceMethod::Overlapping
                    && palette[0][3] == 255
                    && palette[a as usize][3] == 255;
                let under_b = b != TRANSPARENT
                    && options.method == TraceMethod::Overlapping
                    && palette[0][3] == 255
                    && palette[b as usize][3] == 255;
                if a != 0 || !under_a || !under_b {
                    edges[a as usize].push(Edge::new(px, py, dir));
                    count += 1;
                }
                if a != 0 && under_a && !under_b {
                    edges[0].push(Edge::new(px, py, dir));
                    count += 1;
                }
                if count > MAX_TRACE_EDGES {
                    return Err("Trace exceeds 1,000,000 boundary edges. Reduce Colours, increase Noise, or resize the bitmap.".into());
                }
            }
        }
    }
    let mut shapes = Vec::new();
    let mut path_count = 0usize;
    let mut done = 0usize;
    for (colour, mut edges) in palette.into_iter().zip(edges) {
        if edges.is_empty() {
            continue;
        }
        edges.sort_unstable();
        edges.dedup();
        let mut visited = vec![false; edges.len()];
        let mut commands = Vec::new();
        for first in 0..edges.len() {
            if visited[first] {
                continue;
            }
            path_count += 1;
            if path_count > MAX_TRACE_PATHS {
                return Err("Trace exceeds 20,000 paths. Increase Noise, reduce Colours, or resize the bitmap.".into());
            }
            let mut polygon = Vec::new();
            let mut i = first;
            loop {
                if visited[i] {
                    break;
                }
                visited[i] = true;
                let edge = edges[i];
                polygon.push(edge.point());
                done += 1;
                if done.is_multiple_of(1024)
                    && !checkpoint(0.65 + 0.3 * done as f32 / count.max(1) as f32)
                {
                    return Ok(None);
                }
                let end = edge.end();
                if end == edges[first].point() {
                    break;
                }
                // Follow the rightmost continuation at diagonal contacts, keeping disconnected
                // islands and the opposite winding of hole boundaries separate.
                let mut next = None;
                for dir in [
                    (edge.dir + 1) % 4,
                    edge.dir,
                    (edge.dir + 3) % 4,
                    (edge.dir + 2) % 4,
                ] {
                    let candidate = Edge::new(end[0] as usize, end[1] as usize, dir);
                    if let Ok(index) = edges.binary_search(&candidate)
                        && !visited[index]
                    {
                        next = Some(index);
                        break;
                    }
                }
                let Some(next) = next else {
                    return Err(
                        "Could not close a traced contour. Try Abutting or a lower colour count."
                            .into(),
                    );
                };
                i = next;
            }
            let progress = 0.65 + 0.3 * done as f32 / count.max(1) as f32;
            let Some(polygon) = simplify(
                &polygon,
                (1. - options.paths / 100.).powi(2) * 4.,
                &mut || checkpoint(progress),
            ) else {
                return Ok(None);
            };
            append_contour(&mut commands, &polygon, options);
            if commands.len() > crate::MAX_PATH_COMMANDS {
                return Err("A traced colour exceeds the path-command limit. Reduce Colours or resize the bitmap.".into());
            }
        }
        if !commands.is_empty() {
            shapes.push(TracedShape {
                commands,
                fill_rule: FillRule::EvenOdd,
                colour,
            });
        }
    }
    if !checkpoint(1.) {
        return Ok(None);
    }
    if shapes.is_empty() {
        return Err("No paths remain. Lower Noise or turn off Ignore white.".into());
    }
    Ok(Some(BitmapTrace {
        width: image.width,
        height: image.height,
        shapes,
        path_count,
    }))
}

fn mapped(mut p: Color, mode: TraceMode) -> Color {
    if mode != TraceMode::Colour {
        let y = ((u32::from(p[0]) * 54 + u32::from(p[1]) * 183 + u32::from(p[2]) * 19 + 128) / 256)
            as u8;
        let y = if mode == TraceMode::BlackAndWhite {
            if y < 128 { 0 } else { 255 }
        } else {
            y
        };
        p[..3].fill(y);
    }
    p
}
fn key(p: Color, exact: bool) -> u32 {
    if exact {
        u32::from_be_bytes(p)
    } else {
        u32::from(p[0] >> 3) << 14
            | u32::from(p[1] >> 3) << 9
            | u32::from(p[2] >> 3) << 4
            | u32::from(p[3] >> 4)
    }
}
#[derive(Clone)]
struct Bin {
    key: u32,
    count: u64,
    sums: [u64; 4],
}
impl Bin {
    fn colour(&self) -> Color {
        self.sums.map(|v| ((v + self.count / 2) / self.count) as u8)
    }
}
type Quantized = (Vec<Color>, Vec<u16>);
fn quantize(
    image: &DecodedImage,
    options: &TraceOptions,
    check: &mut impl FnMut(f32) -> bool,
) -> Result<Option<Quantized>, String> {
    let exact = options.palette == TracePalette::Full;
    let mut histogram = BTreeMap::<u32, Bin>::new();
    let n = image.rgba.len() / 4;
    for (i, raw) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
        if i % 4096 == 0 && !check(0.15 * i as f32 / n as f32) {
            return Ok(None);
        }
        let p = mapped(*raw, options.mode);
        if p[3] == 0 || (options.ignore_white && p[..3].iter().all(|c| *c >= 250)) {
            continue;
        }
        let k = key(p, exact);
        let bin = histogram.entry(k).or_insert(Bin {
            key: k,
            count: 0,
            sums: [0; 4],
        });
        bin.count += 1;
        for (sum, c) in bin.sums.iter_mut().zip(p) {
            *sum += u64::from(c);
        }
        if exact && histogram.len() > MAX_TRACE_PATHS {
            return Err("Full palette exceeds 20,000 colours. Choose Limited or Automatic.".into());
        }
    }
    let bins: Vec<_> = histogram.into_values().collect();
    if bins.is_empty() {
        return Err("The bitmap has no visible pixels after Ignore white.".into());
    }
    let target = if exact {
        bins.len()
    } else if options.palette == TracePalette::Automatic {
        ((bins.len() as f64).sqrt().ceil() as usize).clamp(2, 30)
    } else {
        options.colours as usize
    };
    let mut boxes = if exact {
        (0..bins.len()).map(|i| vec![i]).collect()
    } else {
        vec![(0..bins.len()).collect::<Vec<_>>()]
    };
    while boxes.len() < target {
        let split = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let ranges: [u8; 4] = std::array::from_fn(|c| {
                    let min = b.iter().map(|i| bins[*i].colour()[c]).min().unwrap();
                    let max = b.iter().map(|i| bins[*i].colour()[c]).max().unwrap();
                    max - min
                });
                let channel = (0..4).max_by_key(|c| ranges[*c]).unwrap();
                (
                    i,
                    channel,
                    u64::from(ranges[channel]) * b.iter().map(|i| bins[*i].count).sum::<u64>(),
                )
            })
            .max_by_key(|(_, _, score)| *score);
        let Some((index, channel, _)) = split else {
            break;
        };
        let mut b = boxes.swap_remove(index);
        b.sort_by_key(|i| (bins[*i].colour()[channel], bins[*i].key));
        let half = b.iter().map(|i| bins[*i].count).sum::<u64>() / 2;
        let mut count = 0;
        let split = b
            .iter()
            .position(|i| {
                count += bins[*i].count;
                count >= half
            })
            .unwrap_or(0)
            .saturating_add(1)
            .min(b.len() - 1);
        let right = b.split_off(split);
        boxes.push(b);
        boxes.push(right);
        if !check(0.15) {
            return Ok(None);
        }
    }
    boxes.sort_by_key(|b| std::cmp::Reverse(b.iter().map(|i| bins[*i].count).sum::<u64>()));
    let mut palette = Vec::new();
    let mut lookup = HashMap::with_capacity(bins.len());
    for (index, b) in boxes.iter().enumerate() {
        let mut count = 0u64;
        let mut sums = [0u64; 4];
        for i in b {
            count += bins[*i].count;
            for (sum, v) in sums.iter_mut().zip(bins[*i].sums) {
                *sum += v;
            }
            lookup.insert(bins[*i].key, index as u16);
        }
        palette.push(sums.map(|v| ((v + count / 2) / count) as u8));
    }
    let mut labels = Vec::with_capacity(n);
    for (i, raw) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
        if i % 4096 == 0 && !check(0.15 + 0.15 * i as f32 / n as f32) {
            return Ok(None);
        }
        let p = mapped(*raw, options.mode);
        labels.push(
            if p[3] == 0 || (options.ignore_white && p[..3].iter().all(|c| *c >= 250)) {
                TRANSPARENT
            } else {
                lookup[&key(p, exact)]
            },
        );
    }
    Ok(Some((palette, labels)))
}
fn remove_noise(
    labels: &mut [u16],
    w: usize,
    h: usize,
    threshold: u32,
    check: &mut impl FnMut(f32) -> bool,
) -> bool {
    let mut visited = vec![false; labels.len()];
    let mut todo = Vec::<usize>::new();
    let mut small = Vec::with_capacity(threshold as usize);
    let mut seen = 0usize;
    for start in 0..labels.len() {
        if visited[start] || labels[start] == TRANSPARENT {
            continue;
        }
        let colour = labels[start];
        visited[start] = true;
        todo.push(start);
        small.clear();
        let mut count = 0;
        let mut neighbours = BTreeMap::<u16, usize>::new();
        while let Some(i) = todo.pop() {
            count += 1;
            seen += 1;
            if count < threshold {
                small.push(i);
            }
            if seen.is_multiple_of(4096) && !check(0.3 + 0.1 * seen as f32 / labels.len() as f32) {
                return false;
            }
            let (x, y) = (i % w, i / w);
            for next in [
                if x > 0 { Some(i - 1) } else { None },
                if x + 1 < w { Some(i + 1) } else { None },
                if y > 0 { Some(i - w) } else { None },
                if y + 1 < h { Some(i + w) } else { None },
            ]
            .into_iter()
            .flatten()
            {
                if count < threshold && labels[next] != colour && labels[next] != TRANSPARENT {
                    *neighbours.entry(labels[next]).or_default() += 1;
                }
                if !visited[next] && labels[next] == colour {
                    visited[next] = true;
                    todo.push(next);
                }
            }
        }
        if count < threshold {
            let replacement = neighbours
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .map(|(colour, _)| colour)
                .unwrap_or(TRANSPARENT);
            for i in &small {
                labels[*i] = replacement;
            }
        }
    }
    true
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    point: u32,
    dir: u8,
}
impl Edge {
    fn new(x: usize, y: usize, dir: u8) -> Self {
        Self {
            point: ((y as u32) << 16) | x as u32,
            dir,
        }
    }
    fn point(self) -> [f64; 2] {
        [(self.point & 0xffff) as f64, (self.point >> 16) as f64]
    }
    fn end(self) -> [f64; 2] {
        let mut p = self.point();
        match self.dir {
            0 => p[0] += 1.,
            1 => p[1] += 1.,
            2 => p[0] -= 1.,
            _ => p[1] -= 1.,
        };
        p
    }
}
fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0] * d[0] + d[1] * d[1];
    let t = if length > 0. {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length
    } else {
        0.
    }
    .clamp(0., 1.);
    (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
}
fn simplify(
    points: &[[f64; 2]],
    tolerance: f64,
    check: &mut impl FnMut() -> bool,
) -> Option<Vec<[f64; 2]>> {
    let mut corners = Vec::new();
    for i in 0..points.len() {
        if i % 4096 == 0 && !check() {
            return None;
        }
        let a = points[(i + points.len() - 1) % points.len()];
        let b = points[i];
        let c = points[(i + 1) % points.len()];
        if (b[0] - a[0]) * (c[1] - b[1]) != (b[1] - a[1]) * (c[0] - b[0]) {
            corners.push(b);
        }
    }
    if tolerance == 0. || corners.len() < 5 {
        return Some(corners);
    }
    // Two open halves avoid the degenerate coincident endpoints of a closed RDP call.
    let half = (1..corners.len())
        .max_by(|a, b| {
            distance(corners[*a], corners[0], corners[0]).total_cmp(&distance(
                corners[*b],
                corners[0],
                corners[0],
            ))
        })
        .unwrap();
    corners.push(corners[0]);
    let mut keep = vec![false; corners.len()];
    keep[0] = true;
    keep[half] = true;
    keep[corners.len() - 1] = true;
    let mut ranges = vec![(0, half), (half, corners.len() - 1)];
    let mut work = 0usize;
    while let Some((a, b)) = ranges.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut i, mut d) = (a + 1, 0.0);
        for at in a + 1..b {
            work += 1;
            if work.is_multiple_of(4096) && !check() {
                return None;
            }
            // RDP has quadratic pathological cases. Preserve the exact compressed boundary
            // instead of spending unbounded time on simplification of a sawtooth contour.
            if work > (corners.len() * 64).min(8_000_000) {
                corners.pop();
                return Some(corners);
            }
            let next = distance(corners[at], corners[a], corners[b]);
            if next > d {
                i = at;
                d = next;
            }
        }
        if d > tolerance {
            keep[i] = true;
            ranges.push((a, i));
            ranges.push((i, b));
        }
    }
    let result: Vec<_> = corners[..corners.len() - 1]
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect();
    if result.len() < 3 {
        corners.pop();
        Some(corners)
    } else {
        Some(result)
    }
}
fn append_contour(commands: &mut Vec<PathCommand>, points: &[[f64; 2]], options: &TraceOptions) {
    if points.len() < 3 {
        return;
    }
    let mut rounded = Vec::with_capacity(points.len());
    for i in 0..points.len() {
        let a = points[(i + points.len() - 1) % points.len()];
        let b = points[i];
        let c = points[(i + 1) % points.len()];
        let before = [b[0] - a[0], b[1] - a[1]];
        let after = [c[0] - b[0], c[1] - b[1]];
        let cosine = ((before[0] * after[0] + before[1] * after[1])
            / (before[0].hypot(before[1]) * after[0].hypot(after[1])))
        .clamp(-1., 1.);
        let smooth = !options.snap_curves
            && cosine.acos() + 1e-8 < (1. - options.corners / 100.) * std::f64::consts::PI;
        let t = if smooth { 0.25 } else { 0. };
        rounded.push((
            [b[0] - before[0] * t, b[1] - before[1] * t],
            b,
            [b[0] + after[0] * t, b[1] + after[1] * t],
            smooth,
        ));
    }
    commands.push(PathCommand::MoveTo {
        x: rounded[0].0[0],
        y: rounded[0].0[1],
    });
    for (i, (entry, corner, exit, smooth)) in rounded.iter().enumerate() {
        if i > 0 {
            commands.push(PathCommand::LineTo {
                x: entry[0],
                y: entry[1],
            });
        }
        if *smooth {
            commands.push(PathCommand::QuadraticTo {
                cx: corner[0],
                cy: corner[1],
                x: exit[0],
                y: exit[1],
            });
        }
    }
    commands.push(PathCommand::Close);
}
