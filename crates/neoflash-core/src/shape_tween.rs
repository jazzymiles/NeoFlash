//! Flash-style shape tweens between the shapes of two keyframes on one layer.
//!
//! Shapes (`Rectangle`, `Ellipse`, `Path`, `PaintedPath`) are converted to
//! commands in their parent space and morphed into a `Path` (or `PaintedPath`
//! when a gradient is involved) with a geometrically identity transform.
//! Command lists with equal topology keep the plain per-command interpolation;
//! everything else is morphed contour by contour after arc-length resampling to
//! a common cubic segment count and a start-point/direction alignment.
//!
//! The resampling and alignment are written for this project: lines and
//! quadratics are elevated to cubics, every source segment keeps at least one
//! output segment (so corners survive), extra segments are distributed by arc
//! length and produced by exact de Casteljau subdivision, so t=0 and t=1
//! reproduce the keyframe geometry.
use crate::evaluate::{affine_multiply, transform_matrix};
use crate::geometry::{interpolate_commands, paths_have_same_topology, validate_path_commands};
use crate::runtime_graph::clipping::vector_path;
use crate::{
    Color, ColorTransform, Element, ElementKind, FillRule, GradientPaint, GradientStop,
    PathCommand, Transform,
};

/// Upper bound of cubic segments in one tweened shape. A pair needing more is
/// held at its `from` element instead of producing a partial morph.
pub const MAX_SHAPE_TWEEN_SEGMENTS: usize = 4096;
const MIN_CONTOUR_SEGMENTS: usize = 8;
const MAX_CONTOUR_SEGMENTS: usize = 256;
/// Polyline samples per cubic used to estimate arc length.
const ARC_SAMPLES: usize = 32;
/// Maximum start rotations tried while aligning one contour pair.
const MAX_ROTATION_CANDIDATES: usize = 256;

type P = [f64; 2];
/// Cubic segment: first control, second control, end. The start is the
/// previous segment's end (or the contour start).
type Seg = [P; 3];

#[derive(Clone, Debug)]
struct Contour {
    start: P,
    segs: Vec<Seg>,
    closed: bool,
}

#[derive(Clone, Debug)]
struct Paint {
    fill: Color,
    stroke: Color,
    fill_gradient: Option<GradientPaint>,
    stroke_gradient: Option<GradientPaint>,
    stroke_width: f64,
    fill_rule: FillRule,
}

struct Shape {
    contours: Vec<Contour>,
    commands: Vec<PathCommand>,
    paint: Paint,
}

/// Shape tween between the shapes of two keyframes on one layer, at eased progress `t` (0..=1).
/// Returns the elements to draw at that frame.
///
/// Shapes pair by equal `id` first, then by order. Non-shape elements of `from`
/// are held unchanged. Unpaired `from` shapes shrink to the centre of their
/// bounds (omitted at t >= 1); unpaired `to` shapes grow from theirs (omitted
/// at t <= 0) and are appended after the `from` elements.
pub fn shape_tween_elements(from: &[Element], to: &[Element], t: f64) -> Vec<Element> {
    let t = if t.is_finite() { t } else { 0. };
    let from_shapes: Vec<usize> = (0..from.len())
        .filter(|&i| is_shape(&from[i].kind))
        .collect();
    let to_shapes: Vec<usize> = (0..to.len()).filter(|&i| is_shape(&to[i].kind)).collect();
    let mut partner: Vec<Option<usize>> = vec![None; from.len()];
    let mut to_used = vec![false; to.len()];
    for &fi in &from_shapes {
        if let Some(&ti) = to_shapes
            .iter()
            .find(|&&ti| !to_used[ti] && to[ti].id == from[fi].id)
        {
            partner[fi] = Some(ti);
            to_used[ti] = true;
        }
    }
    let free_to: Vec<usize> = to_shapes
        .iter()
        .copied()
        .filter(|&ti| !to_used[ti])
        .collect();
    let mut free_to = free_to.into_iter();
    for &fi in &from_shapes {
        if partner[fi].is_none()
            && let Some(ti) = free_to.next()
        {
            partner[fi] = Some(ti);
            to_used[ti] = true;
        }
    }

    let mut out = Vec::with_capacity(from.len() + to.len());
    for (fi, element) in from.iter().enumerate() {
        if !is_shape(&element.kind) {
            out.push(element.clone());
            continue;
        }
        match partner[fi] {
            Some(ti) => out.push(morph_pair(element, &to[ti], t)),
            // Over the segment cap the shape is held unchanged.
            None if t < 1. => {
                out.push(collapse_shape(element, t).unwrap_or_else(|| element.clone()))
            }
            None => {}
        }
    }
    if t > 0. {
        for &ti in &to_shapes {
            if !to_used[ti] {
                // Over the segment cap a growing shape stays hidden until t = 1.
                out.extend(collapse_shape(&to[ti], 1. - t));
            }
        }
    }
    out
}

fn is_shape(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. }
    )
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn lerp_p(a: P, b: P, t: f64) -> P {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]
}
fn color_lerp(a: Color, b: Color, t: f64) -> Color {
    std::array::from_fn(|i| lerp(a[i] as f64, b[i] as f64, t).clamp(0., 255.).round() as u8)
}
fn dist2(a: P, b: P) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}
fn map_point(m: [f64; 6], x: f64, y: f64) -> P {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/// Commands, contours and paint of a shape element in its parent space.
fn parent_shape(element: &Element) -> Option<Shape> {
    let local = match &element.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. } => {
            vector_path(&element.kind).map_or_else(
                // Zero-sized boxes stay drawable as a degenerate outline so they
                // can grow into (or shrink from) their partner.
                || degenerate_box(width.max(0.), height.max(0.)),
                |(commands, _)| commands,
            )
        }
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            commands.clone()
        }
        _ => return None,
    };
    let m = transform_matrix(element.transform);
    let commands: Vec<PathCommand> = local
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo { x, y } => {
                let [x, y] = map_point(m, x, y);
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let [x, y] = map_point(m, x, y);
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [cx, cy] = map_point(m, cx, cy);
                let [x, y] = map_point(m, x, y);
                PathCommand::QuadraticTo { cx, cy, x, y }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let [cx1, cy1] = map_point(m, cx1, cy1);
                let [cx2, cy2] = map_point(m, cx2, cy2);
                let [x, y] = map_point(m, x, y);
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect();
    validate_path_commands(&commands).ok()?;
    // Strokes scale with the placement; non-uniform scale is approximated by
    // the geometric mean of the axis scales.
    let stroke_scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
    let to_parent = |g: &Option<GradientPaint>| {
        g.as_ref().map(|g| GradientPaint {
            matrix: affine_multiply(m, g.matrix),
            ..g.clone()
        })
    };
    let paint = match &element.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        } => Paint {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: *stroke_width * stroke_scale,
            fill_rule: FillRule::NonZero,
        },
        ElementKind::Path {
            fill_rule,
            fill,
            stroke,
            stroke_width,
            ..
        } => Paint {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: *stroke_width * stroke_scale,
            fill_rule: *fill_rule,
        },
        ElementKind::PaintedPath {
            fill_rule,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => Paint {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: to_parent(fill_gradient),
            stroke_gradient: to_parent(stroke_gradient),
            stroke_width: *stroke_width * stroke_scale,
            fill_rule: *fill_rule,
        },
        _ => return None,
    };
    let contours = contours(&commands);
    Some(Shape {
        contours,
        commands,
        paint,
    })
}

fn degenerate_box(width: f64, height: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: width, y: 0. },
        PathCommand::LineTo {
            x: width,
            y: height,
        },
        PathCommand::LineTo { x: 0., y: height },
        PathCommand::Close,
    ]
}

fn line(a: P, b: P) -> Seg {
    [lerp_p(a, b, 1. / 3.), lerp_p(a, b, 2. / 3.), b]
}

/// Split validated commands into cubic contours. A closed contour whose pen is
/// not at its start gets the explicit closing line. Move-only contours are dropped.
fn contours(commands: &[PathCommand]) -> Vec<Contour> {
    let mut out = Vec::new();
    let mut current: Option<Contour> = None;
    let mut pen = [0., 0.];
    let finish = |c: Option<Contour>, out: &mut Vec<Contour>| {
        if let Some(c) = c
            && !c.segs.is_empty()
        {
            out.push(c);
        }
    };
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                finish(current.take(), &mut out);
                current = Some(Contour {
                    start: [x, y],
                    segs: Vec::new(),
                    closed: false,
                });
                pen = [x, y];
            }
            PathCommand::LineTo { x, y } => {
                if let Some(c) = current.as_mut() {
                    c.segs.push(line(pen, [x, y]));
                }
                pen = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(c) = current.as_mut() {
                    let (q, e) = ([cx, cy], [x, y]);
                    c.segs
                        .push([lerp_p(pen, q, 2. / 3.), lerp_p(e, q, 2. / 3.), e]);
                }
                pen = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if let Some(c) = current.as_mut() {
                    c.segs.push([[cx1, cy1], [cx2, cy2], [x, y]]);
                }
                pen = [x, y];
            }
            PathCommand::Close => {
                if let Some(mut c) = current.take() {
                    if pen != c.start {
                        c.segs.push(line(pen, c.start));
                    }
                    c.closed = true;
                    pen = c.start;
                    finish(Some(c), &mut out);
                }
            }
        }
    }
    finish(current, &mut out);
    out
}

impl Contour {
    fn end(&self) -> P {
        self.segs.last().map_or(self.start, |s| s[2])
    }
    /// Ends where it starts, so any segment may begin the outline.
    fn cyclic(&self) -> bool {
        self.closed || self.end() == self.start
    }
    fn make_cyclic(&mut self) {
        if self.end() != self.start {
            let s = line(self.end(), self.start);
            self.segs.push(s);
        }
    }
    /// On-curve points: the start plus every segment end.
    fn vertices(&self) -> Vec<P> {
        std::iter::once(self.start)
            .chain(self.segs.iter().map(|s| s[2]))
            .collect()
    }
    fn reversed(&self) -> Contour {
        let mut segs = Vec::with_capacity(self.segs.len());
        for i in (0..self.segs.len()).rev() {
            let start = if i == 0 {
                self.start
            } else {
                self.segs[i - 1][2]
            };
            segs.push([self.segs[i][1], self.segs[i][0], start]);
        }
        Contour {
            start: self.end(),
            segs,
            closed: self.closed,
        }
    }
    /// Start at vertex `r` of a cyclic contour.
    fn rotated(&self, r: usize) -> Contour {
        if r == 0 {
            return self.clone();
        }
        let mut segs = self.segs.clone();
        segs.rotate_left(r);
        Contour {
            start: self.segs[r - 1][2],
            segs,
            closed: self.closed,
        }
    }
    fn bounds_centre(&self) -> P {
        bounds_centre(self.vertices().into_iter())
    }
}

fn bounds_centre(points: impl Iterator<Item = P>) -> P {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in points {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    if lo[0] > hi[0] {
        return [0., 0.];
    }
    [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2.]
}

fn cubic_point(p0: P, s: &Seg, u: f64) -> P {
    let v = 1. - u;
    let (a, b, c, d) = (v * v * v, 3. * v * v * u, 3. * v * u * u, u * u * u);
    [
        a * p0[0] + b * s[0][0] + c * s[1][0] + d * s[2][0],
        a * p0[1] + b * s[0][1] + c * s[1][1] + d * s[2][1],
    ]
}

/// Cumulative polyline lengths at u = i / ARC_SAMPLES.
fn arc_table(p0: P, s: &Seg) -> [f64; ARC_SAMPLES + 1] {
    let mut table = [0.; ARC_SAMPLES + 1];
    let (mut prev, mut length) = (p0, 0.);
    for (i, slot) in table.iter_mut().enumerate().skip(1) {
        let p = cubic_point(p0, s, i as f64 / ARC_SAMPLES as f64);
        length += dist2(prev, p).sqrt();
        *slot = length;
        prev = p;
    }
    table
}

/// de Casteljau split at `u`; returns (left, right) with the right part's
/// start being the left part's end.
fn split(p0: P, s: &Seg, u: f64) -> (Seg, Seg) {
    let p01 = lerp_p(p0, s[0], u);
    let p12 = lerp_p(s[0], s[1], u);
    let p23 = lerp_p(s[1], s[2], u);
    let p012 = lerp_p(p01, p12, u);
    let p123 = lerp_p(p12, p23, u);
    let mid = lerp_p(p012, p123, u);
    ([p01, p012, mid], [p123, p23, s[2]])
}

/// Split one cubic into `k` pieces of (approximately) equal arc length.
fn split_equal(p0: P, s: &Seg, k: usize, out: &mut Vec<Seg>) {
    if k <= 1 {
        out.push(*s);
        return;
    }
    let table = arc_table(p0, s);
    let total = table[ARC_SAMPLES];
    let mut rest = *s;
    let mut rest_start = p0;
    let mut prev_u = 0.;
    for j in 1..k {
        let u = if total > 0. {
            let target = total * j as f64 / k as f64;
            let i = table.partition_point(|&l| l < target).clamp(1, ARC_SAMPLES);
            let span = table[i] - table[i - 1];
            let f = if span > 0. {
                (target - table[i - 1]) / span
            } else {
                0.
            };
            (i as f64 - 1. + f) / ARC_SAMPLES as f64
        } else {
            j as f64 / k as f64
        };
        let remaining = 1. - prev_u;
        let local = if remaining > 0. {
            ((u - prev_u) / remaining).clamp(0., 1.)
        } else {
            0.
        };
        let (left, right) = split(rest_start, &rest, local);
        out.push(left);
        rest_start = left[2];
        rest = right;
        prev_u = u;
    }
    out.push(rest);
}

/// Distribute `n >= lengths.len()` segments: one per source segment, extras by
/// arc length with the largest-remainder rule (ties to the earlier segment).
fn distribute(lengths: &[f64], n: usize) -> Vec<usize> {
    let m = lengths.len();
    let extra = n - m;
    let total: f64 = lengths.iter().sum();
    if !(total.is_finite() && total > 0.) {
        return (0..m)
            .map(|i| 1 + extra / m + usize::from(i < extra % m))
            .collect();
    }
    let ideal: Vec<f64> = lengths.iter().map(|l| extra as f64 * l / total).collect();
    let mut counts: Vec<usize> = ideal.iter().map(|v| 1 + v.floor() as usize).collect();
    let assigned: usize = counts.iter().sum::<usize>() - m;
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| {
        (ideal[b] - ideal[b].floor())
            .total_cmp(&(ideal[a] - ideal[a].floor()))
            .then(a.cmp(&b))
    });
    for &i in order.iter().take(extra.saturating_sub(assigned)) {
        counts[i] += 1;
    }
    counts
}

fn resample(c: &Contour, n: usize) -> Contour {
    let mut lengths = Vec::with_capacity(c.segs.len());
    let mut p0 = c.start;
    for s in &c.segs {
        lengths.push(arc_table(p0, s)[ARC_SAMPLES]);
        p0 = s[2];
    }
    let counts = distribute(&lengths, n);
    let mut segs = Vec::with_capacity(n);
    let mut p0 = c.start;
    for (s, &k) in c.segs.iter().zip(&counts) {
        split_equal(p0, s, k, &mut segs);
        p0 = s[2];
    }
    Contour {
        start: c.start,
        segs,
        closed: c.closed,
    }
}

fn contour_segments(a: &Contour, b: &Contour) -> usize {
    let most = a.segs.len().max(b.segs.len());
    (2 * most)
        .clamp(MIN_CONTOUR_SEGMENTS, MAX_CONTOUR_SEGMENTS)
        .max(most)
}

/// Choose the start vertex (cyclic only) and direction of `dst` that minimise
/// the summed squared distance to the vertices of `src` (same segment count).
fn align(src: &Contour, dst: &Contour, cyclic: bool, allow_reverse: bool) -> Contour {
    let n = src.segs.len();
    let sv = src.vertices();
    let mut best: Option<(f64, Contour)> = None;
    let mut candidates = vec![dst.clone()];
    if allow_reverse {
        candidates.push(dst.reversed());
    }
    let stride = if cyclic {
        n.div_ceil(MAX_ROTATION_CANDIDATES).max(1)
    } else {
        n + 1
    };
    for candidate in candidates {
        let dv = candidate.vertices();
        let rotations = if cyclic { n } else { 1 };
        let mut r = 0;
        while r < rotations {
            let cost: f64 = if cyclic {
                (0..n).map(|i| dist2(sv[i], dv[(i + r) % n])).sum()
            } else {
                sv.iter().zip(&dv).map(|(a, b)| dist2(*a, *b)).sum()
            };
            if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                best = Some((cost, candidate.rotated(r)));
            }
            r += stride;
        }
    }
    best.map_or_else(|| dst.clone(), |(_, c)| c)
}

fn emit(commands: &mut Vec<PathCommand>, start: P, segs: impl Iterator<Item = Seg>, close: bool) {
    commands.push(PathCommand::MoveTo {
        x: start[0],
        y: start[1],
    });
    for s in segs {
        commands.push(PathCommand::CubicTo {
            cx1: s[0][0],
            cy1: s[0][1],
            cx2: s[1][0],
            cy2: s[1][1],
            x: s[2][0],
            y: s[2][1],
        });
    }
    if close {
        commands.push(PathCommand::Close);
    }
}

/// Emit a contour moved toward `centre` by `amount` (0 = unchanged, 1 = point).
fn emit_toward(commands: &mut Vec<PathCommand>, c: &Contour, centre: P, amount: f64) {
    let m = |p: P| lerp_p(p, centre, amount);
    emit(
        commands,
        m(c.start),
        c.segs.iter().map(|s| [m(s[0]), m(s[1]), m(s[2])]),
        c.closed,
    );
}

/// Contour-by-contour morph; `None` when the segment budget is exceeded.
fn morph_contours(a: &Shape, b: &Shape, t: f64) -> Option<Vec<PathCommand>> {
    let pairs = a.contours.len().min(b.contours.len());
    let mut budget = 0usize;
    for i in 0..pairs {
        budget += contour_segments(&a.contours[i], &b.contours[i]);
    }
    budget += a.contours[pairs..]
        .iter()
        .chain(&b.contours[pairs..])
        .map(|c| c.segs.len())
        .sum::<usize>();
    if budget > MAX_SHAPE_TWEEN_SEGMENTS {
        return None;
    }
    // Direction only changes the fill of a multi-contour nonzero target.
    let allow_reverse = b.contours.len() == 1 || b.paint.fill_rule == FillRule::EvenOdd;
    let mut commands = Vec::with_capacity(budget + 2 * (a.contours.len() + b.contours.len()));
    for i in 0..pairs {
        // The source decides: a cyclic source makes the target cyclic too.
        // Closed contours already end at their start (see `contours`).
        let ca = &a.contours[i];
        let mut cb = b.contours[i].clone();
        let cyclic = ca.cyclic();
        if cyclic {
            cb.make_cyclic();
        }
        let n = contour_segments(ca, &cb);
        let ra = resample(ca, n);
        let rb = align(&ra, &resample(&cb, n), cyclic, allow_reverse);
        let close = if t < 0.5 { ca.closed } else { cb.closed };
        emit(
            &mut commands,
            lerp_p(ra.start, rb.start, t),
            ra.segs.iter().zip(&rb.segs).map(|(x, y)| {
                [
                    lerp_p(x[0], y[0], t),
                    lerp_p(x[1], y[1], t),
                    lerp_p(x[2], y[2], t),
                ]
            }),
            close,
        );
    }
    if t < 1. {
        for c in &a.contours[pairs..] {
            emit_toward(&mut commands, c, c.bounds_centre(), t);
        }
    }
    if t > 0. {
        for c in &b.contours[pairs..] {
            emit_toward(&mut commands, c, c.bounds_centre(), 1. - t);
        }
    }
    Some(commands)
}

fn compatible(a: &GradientPaint, b: &GradientPaint) -> bool {
    a.kind == b.kind && a.stops.len() == b.stops.len()
}

/// The gradient `like` repainted in one solid colour.
fn solid_like(like: &GradientPaint, color: Color) -> GradientPaint {
    GradientPaint {
        stops: like
            .stops
            .iter()
            .map(|s| GradientStop {
                ratio: s.ratio,
                color,
            })
            .collect(),
        ..like.clone()
    }
}

fn lerp_gradient(a: &GradientPaint, b: &GradientPaint, t: f64) -> GradientPaint {
    GradientPaint {
        kind: a.kind,
        matrix: std::array::from_fn(|i| lerp(a.matrix[i], b.matrix[i], t)),
        stops: a
            .stops
            .iter()
            .zip(&b.stops)
            .map(|(x, y)| GradientStop {
                ratio: lerp(x.ratio as f64, y.ratio as f64, t)
                    .clamp(0., 255.)
                    .round() as u8,
                color: color_lerp(x.color, y.color, t),
            })
            .collect(),
        focal_point_ratio: lerp(a.focal_point_ratio, b.focal_point_ratio, t),
    }
}

/// Solid ↔ gradient morphs through the gradient repainted in the solid colour;
/// structurally different gradients (kind or stop count) switch at t = 0.5.
fn paint_gradient(
    a: Option<&GradientPaint>,
    a_color: Color,
    b: Option<&GradientPaint>,
    b_color: Color,
    t: f64,
) -> Option<GradientPaint> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(lerp_gradient(a, &solid_like(a, b_color), t)),
        (None, Some(b)) => Some(lerp_gradient(&solid_like(b, a_color), b, t)),
        (Some(a), Some(b)) if compatible(a, b) => Some(lerp_gradient(a, b, t)),
        (Some(a), Some(b)) => Some(if t < 0.5 { a.clone() } else { b.clone() }),
    }
}

fn lerp_paint(a: &Paint, b: &Paint, t: f64) -> Paint {
    Paint {
        fill: color_lerp(a.fill, b.fill, t),
        stroke: color_lerp(a.stroke, b.stroke, t),
        fill_gradient: paint_gradient(
            a.fill_gradient.as_ref(),
            a.fill,
            b.fill_gradient.as_ref(),
            b.fill,
            t,
        ),
        stroke_gradient: paint_gradient(
            a.stroke_gradient.as_ref(),
            a.stroke,
            b.stroke_gradient.as_ref(),
            b.stroke,
            t,
        ),
        stroke_width: lerp(a.stroke_width, b.stroke_width, t).max(0.),
        fill_rule: if t < 0.5 { a.fill_rule } else { b.fill_rule },
    }
}

/// Geometrically identity placement that keeps the non-geometric channels.
fn placement(a: &Transform, b: &Transform, t: f64) -> Transform {
    Transform {
        alpha: lerp(a.alpha, b.alpha, t),
        color_transform: ColorTransform {
            multipliers: std::array::from_fn(|i| {
                lerp(
                    a.color_transform.multipliers[i],
                    b.color_transform.multipliers[i],
                    t,
                )
            }),
            offsets: std::array::from_fn(|i| {
                lerp(
                    a.color_transform.offsets[i],
                    b.color_transform.offsets[i],
                    t,
                )
            }),
        },
        ..Transform::default()
    }
}

fn build(
    template: &Element,
    commands: Vec<PathCommand>,
    paint: Paint,
    transform: Transform,
) -> Element {
    let mut element = template.clone();
    element.transform = transform;
    element.kind = if paint.fill_gradient.is_some() || paint.stroke_gradient.is_some() {
        ElementKind::PaintedPath {
            commands,
            fill_rule: paint.fill_rule,
            fill: paint.fill,
            stroke: paint.stroke,
            fill_gradient: paint.fill_gradient,
            stroke_gradient: paint.stroke_gradient,
            stroke_width: paint.stroke_width,
        }
    } else {
        ElementKind::Path {
            commands,
            fill_rule: paint.fill_rule,
            fill: paint.fill,
            stroke: paint.stroke,
            stroke_width: paint.stroke_width,
        }
    };
    element
}

fn morph_pair(a: &Element, b: &Element, t: f64) -> Element {
    let (Some(sa), Some(sb)) = (parent_shape(a), parent_shape(b)) else {
        return a.clone();
    };
    let commands = if paths_have_same_topology(&sa.commands, &sb.commands) {
        interpolate_commands(&sa.commands, &sb.commands, t)
    } else {
        match morph_contours(&sa, &sb, t) {
            Some(commands) => commands,
            None => return a.clone(),
        }
    };
    build(
        a,
        commands,
        lerp_paint(&sa.paint, &sb.paint, t),
        placement(&a.transform, &b.transform, t),
    )
}

/// An unpaired shape moved toward the centre of its bounds by `amount`.
fn collapse_shape(element: &Element, amount: f64) -> Option<Element> {
    let shape = parent_shape(element)?;
    if shape.contours.iter().map(|c| c.segs.len()).sum::<usize>() > MAX_SHAPE_TWEEN_SEGMENTS {
        return None;
    }
    let centre = bounds_centre(shape.contours.iter().flat_map(|c| c.vertices()));
    let mut commands = Vec::new();
    for c in &shape.contours {
        emit_toward(&mut commands, c, centre, amount);
    }
    Some(build(
        element,
        commands,
        shape.paint,
        placement(&element.transform, &element.transform, 0.),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{FlattenOptions, hit_test_path, path_bounds};

    fn element(id: &str, kind: ElementKind, transform: Transform) -> Element {
        Element {
            id: id.into(),
            name: format!("{id} name"),
            transform,
            kind,
            scripts: Vec::new(),
            accessibility: None,
        }
    }
    fn at(x: f64, y: f64) -> Transform {
        Transform {
            x,
            y,
            ..Transform::default()
        }
    }
    fn rect(width: f64, height: f64) -> ElementKind {
        ElementKind::Rectangle {
            width,
            height,
            fill: [200, 0, 0, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.,
        }
    }
    fn ellipse(width: f64, height: f64) -> ElementKind {
        ElementKind::Ellipse {
            width,
            height,
            fill: [0, 0, 200, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 4.,
        }
    }
    fn polygon(contours: &[&[P]]) -> ElementKind {
        let mut commands = Vec::new();
        for points in contours {
            commands.push(PathCommand::MoveTo {
                x: points[0][0],
                y: points[0][1],
            });
            for p in &points[1..] {
                commands.push(PathCommand::LineTo { x: p[0], y: p[1] });
            }
            commands.push(PathCommand::Close);
        }
        ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill: [10, 20, 30, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        }
    }
    fn commands(e: &Element) -> &[PathCommand] {
        match &e.kind {
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                commands
            }
            other => panic!("expected a path, got {other:?}"),
        }
    }
    fn bounds(e: &Element) -> [f64; 4] {
        let b = path_bounds(commands(e)).unwrap().unwrap();
        [b.x_min, b.y_min, b.x_max, b.y_max]
    }
    fn near(a: [f64; 4], b: [f64; 4], tolerance: f64) {
        assert!(
            a.iter().zip(&b).all(|(x, y)| (x - y).abs() < tolerance),
            "{a:?} != {b:?}"
        );
    }
    fn inside(e: &Element, p: P) -> bool {
        hit_test_path(
            commands(e),
            FillRule::NonZero,
            p,
            true,
            0.,
            FlattenOptions::default(),
        )
        .unwrap()
    }
    fn on_curve(e: &Element) -> Vec<P> {
        commands(e)
            .iter()
            .filter_map(|c| match *c {
                PathCommand::MoveTo { x, y }
                | PathCommand::LineTo { x, y }
                | PathCommand::QuadraticTo { x, y, .. }
                | PathCommand::CubicTo { x, y, .. } => Some([x, y]),
                PathCommand::Close => None,
            })
            .collect()
    }
    fn contour_count(e: &Element) -> usize {
        commands(e)
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
            .count()
    }

    #[test]
    fn rectangle_to_ellipse_reproduces_both_keys_and_morphs_linearly() {
        let from = [element("a", rect(100., 60.), at(10., 20.))];
        let to = [element("b", ellipse(100., 60.), at(10., 20.))];
        let start = &shape_tween_elements(&from, &to, 0.)[0];
        let mid = &shape_tween_elements(&from, &to, 0.5)[0];
        let end = &shape_tween_elements(&from, &to, 1.)[0];
        near(bounds(start), [10., 20., 110., 80.], 1e-9);
        near(bounds(end), [10., 20., 110., 80.], 1e-6);
        for [x, y] in on_curve(start) {
            let on_edge = (x - 10.).abs() < 1e-9
                || (x - 110.).abs() < 1e-9
                || (y - 20.).abs() < 1e-9
                || (y - 80.).abs() < 1e-9;
            assert!(on_edge, "({x}, {y}) is not on the rectangle");
        }
        for [x, y] in on_curve(end) {
            let r = ((x - 60.) / 50.).powi(2) + ((y - 50.) / 30.).powi(2);
            assert!((r - 1.).abs() < 1e-3, "({x}, {y}) is not on the ellipse");
        }
        assert!(inside(start, [11., 21.]));
        assert!(!inside(end, [11., 21.]));
        assert!(inside(end, [60., 50.]) && inside(mid, [60., 50.]));
        // One resampling for every t: the midpoint is the average of both keys.
        for ((a, b), m) in on_curve(start).iter().zip(on_curve(end)).zip(on_curve(mid)) {
            assert!((lerp(a[0], b[0], 0.5) - m[0]).abs() < 1e-9);
            assert!((lerp(a[1], b[1], 0.5) - m[1]).abs() < 1e-9);
        }
        let ElementKind::Path {
            fill, stroke_width, ..
        } = &mid.kind
        else {
            panic!("expected a path")
        };
        assert_eq!(*fill, [100, 0, 100, 255]);
        assert_eq!(*stroke_width, 3.);
        assert_eq!(mid.id, "a");
        assert_eq!(
            shape_tween_elements(&from, &to, 0.37),
            shape_tween_elements(&from, &to, 0.37)
        );
    }

    #[test]
    fn same_topology_paths_keep_plain_command_interpolation() {
        let a = polygon(&[&[[0., 0.], [10., 0.], [10., 10.], [0., 10.]]]);
        let b = polygon(&[&[[5., 5.], [30., 0.], [40., 25.], [0., 30.]]]);
        let from = [element("p", a.clone(), Transform::default())];
        let to = [element("p", b.clone(), Transform::default())];
        let out = shape_tween_elements(&from, &to, 0.3);
        let (ElementKind::Path { commands: ca, .. }, ElementKind::Path { commands: cb, .. }) =
            (&a, &b)
        else {
            unreachable!()
        };
        let mut expected = a.clone();
        if let ElementKind::Path { commands, .. } = &mut expected {
            *commands = interpolate_commands(ca, cb, 0.3);
        }
        assert_eq!(out[0].kind, expected);
    }

    #[test]
    fn triangle_morphs_into_square_with_different_command_counts() {
        let from = [element(
            "t",
            polygon(&[&[[0., 100.], [50., 0.], [100., 100.]]]),
            Transform::default(),
        )];
        let to = [element(
            "s",
            polygon(&[&[[0., 0.], [100., 0.], [100., 100.], [0., 100.]]]),
            Transform::default(),
        )];
        let start = &shape_tween_elements(&from, &to, 0.)[0];
        let end = &shape_tween_elements(&from, &to, 1.)[0];
        near(bounds(start), [0., 0., 100., 100.], 1e-9);
        near(bounds(end), [0., 0., 100., 100.], 1e-9);
        assert!(!inside(start, [5., 5.]) && inside(end, [5., 5.]));
        assert!(inside(start, [50., 60.]) && inside(end, [50., 60.]));
        assert_eq!(commands(start).len(), commands(end).len());
        assert_eq!(commands(start).len(), 1 + MIN_CONTOUR_SEGMENTS + 1);
        for corner in [[0., 0.], [100., 0.], [100., 100.], [0., 100.]] {
            assert!(on_curve(end).iter().any(|p| dist2(*p, corner) < 1e-12));
        }
    }

    #[test]
    fn extra_contours_collapse_to_their_centre() {
        let square = |x: f64| [[x, 0.], [x + 20., 0.], [x + 20., 20.], [x, 20.]];
        let from = [element(
            "two",
            polygon(&[&square(0.), &square(100.)]),
            Transform::default(),
        )];
        let to = [element(
            "one",
            polygon(&[&square(40.)]),
            Transform::default(),
        )];
        let start = &shape_tween_elements(&from, &to, 0.)[0];
        assert!(inside(start, [10., 10.]) && inside(start, [110., 10.]));
        let mid = &shape_tween_elements(&from, &to, 0.5)[0];
        assert_eq!(contour_count(mid), 2);
        assert!(inside(mid, [110., 10.]) && !inside(mid, [104., 4.]));
        let end = &shape_tween_elements(&from, &to, 1.)[0];
        assert_eq!(contour_count(end), 1);
        near(bounds(end), [40., 0., 60., 20.], 1e-9);
        // The reverse direction grows the missing contour from its centre.
        let grow = &shape_tween_elements(&to, &from, 0.5)[0];
        assert!(inside(grow, [110., 10.]) && !inside(grow, [104., 4.]));
        assert_eq!(contour_count(&shape_tween_elements(&to, &from, 0.)[0]), 1);
    }

    #[test]
    fn unpaired_shapes_shrink_or_grow_and_other_elements_hold() {
        let video = element(
            "video",
            ElementKind::Video {
                width: 40.,
                height: 30.,
            },
            at(5., 5.),
        );
        let from = [
            element("a", rect(10., 10.), Transform::default()),
            video.clone(),
            element("b", rect(20., 20.), at(100., 100.)),
        ];
        let to = [element("a", rect(30., 30.), Transform::default())];
        let mid = shape_tween_elements(&from, &to, 0.5);
        assert_eq!(mid.len(), 3);
        assert_eq!(mid[1], video);
        near(bounds(&mid[0]), [0., 0., 20., 20.], 1e-9);
        near(bounds(&mid[2]), [105., 105., 115., 115.], 1e-9);
        assert_eq!(shape_tween_elements(&from, &to, 1.).len(), 2);
        // Swapped keys: the extra target shape is appended and grows.
        let grow = shape_tween_elements(&to, &from, 0.25);
        assert_eq!(grow.len(), 2);
        assert_eq!(grow[1].id, "b");
        near(bounds(&grow[1]), [107.5, 107.5, 112.5, 112.5], 1e-9);
        assert_eq!(shape_tween_elements(&to, &from, 0.).len(), 1);
    }

    #[test]
    fn shapes_pair_by_id_before_order() {
        let from = [
            element("a", rect(10., 10.), Transform::default()),
            element("b", rect(10., 10.), at(100., 0.)),
        ];
        let to = [
            element("b", ellipse(10., 10.), at(200., 0.)),
            element("a", ellipse(10., 10.), at(0., 50.)),
        ];
        let end = shape_tween_elements(&from, &to, 1.);
        assert_eq!(end[0].id, "a");
        near(bounds(&end[0]), [0., 50., 10., 60.], 1e-6);
        near(bounds(&end[1]), [200., 0., 210., 10.], 1e-6);
    }

    #[test]
    fn output_is_a_path_with_identity_geometry_transform() {
        let mut from_transform = at(50., 50.);
        from_transform.rotation = 90.;
        from_transform.scale_x = 2.;
        from_transform.alpha = 0.5;
        let from = element("r", rect(10., 20.), from_transform);
        let to = element("e", ellipse(10., 10.), at(0., 0.));
        let start = &shape_tween_elements(std::slice::from_ref(&from), &[to], 0.)[0];
        let t = start.transform;
        assert_eq!(
            (t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.skew),
            (0., 0., 1., 1., 0., 0.)
        );
        assert_eq!(t.alpha, 0.5);
        assert_eq!((&start.id, &start.name), (&from.id, &from.name));
        // 90 degrees maps local x to parent y and local y to parent -x.
        near(bounds(start), [30., 50., 50., 70.], 1e-9);
        let ElementKind::Path { stroke_width, .. } = start.kind else {
            panic!("expected a path")
        };
        assert!((stroke_width - 2. * 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn solid_fill_morphs_into_a_gradient() {
        let gradient = GradientPaint {
            kind: crate::GradientKind::Linear,
            matrix: [0.01, 0., 0., 0.01, 5., 5.],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 255, 255, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 0, 255],
                },
            ],
            focal_point_ratio: 0.,
        };
        let painted = ElementKind::PaintedPath {
            commands: degenerate_box(10., 10.),
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            fill_gradient: Some(gradient.clone()),
            stroke_gradient: None,
            stroke_width: 0.,
        };
        let from = [element("a", rect(10., 10.), Transform::default())];
        let to = [element("a", painted, at(10., 0.))];
        let gradient_at = |t: f64| match shape_tween_elements(&from, &to, t).remove(0).kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(g),
                ..
            } => g,
            other => panic!("expected a painted path, got {other:?}"),
        };
        assert!(
            gradient_at(0.)
                .stops
                .iter()
                .all(|s| s.color == [200, 0, 0, 255])
        );
        let end = gradient_at(1.);
        assert_eq!(end.stops, gradient.stops);
        assert_eq!(end.matrix, [0.01, 0., 0., 0.01, 15., 5.]);
    }

    #[test]
    fn segment_cap_holds_the_from_shape() {
        let points: Vec<P> = (0..=MAX_SHAPE_TWEEN_SEGMENTS)
            .map(|i| {
                let a = i as f64 / MAX_SHAPE_TWEEN_SEGMENTS as f64 * std::f64::consts::TAU;
                [100. * a.cos(), 100. * a.sin()]
            })
            .collect();
        let from = [element("big", polygon(&[&points]), at(3., 4.))];
        let to = [element("small", rect(10., 10.), Transform::default())];
        assert_eq!(shape_tween_elements(&from, &to, 0.5), from.to_vec());
    }
}
