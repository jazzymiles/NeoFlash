//! Snapshot-based affine gestures. Points and bounds use the edited symbol's coordinates.
//!
//! A single resize works in the placement's own axes, including skew; it never decomposes a
//! world AABB. Multi-selection scales only uniformly and rotates, which composes with every
//! existing affine pose without changing its skew. Edge skew (`skew_many`) is the one gesture
//! that shears in stage space and decomposes the result into scale, rotation and skew.

use neoflash_core::{
    Element, ElementKind, PathCommand, Transform, affine_multiply, invert_affine_matrix,
    transform_matrix,
};

pub(crate) type Point = [f64; 2];
const EPSILON: f64 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub min: Point,
    pub max: Point,
}

impl Bounds {
    pub(crate) fn valid(self) -> bool {
        finite_point(self.min)
            && finite_point(self.max)
            && self.min[0] <= self.max[0]
            && self.min[1] <= self.max[1]
            && (self.max[0] - self.min[0]).is_finite()
            && (self.max[1] - self.min[1]).is_finite()
    }

    pub(crate) fn center(self) -> Point {
        [
            self.min[0] * 0.5 + self.max[0] * 0.5,
            self.min[1] * 0.5 + self.max[1] * 0.5,
        ]
    }

    #[cfg(test)]
    pub(crate) fn contains(self, point: Point) -> bool {
        self.valid()
            && finite_point(point)
            && (self.min[0]..=self.max[0]).contains(&point[0])
            && (self.min[1]..=self.max[1]).contains(&point[1])
    }

    fn resizable(self) -> bool {
        self.valid() && self.max[0] - self.min[0] > EPSILON && self.max[1] - self.min[1] > EPSILON
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResizeHandle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

impl ResizeHandle {
    pub(crate) const ALL: [Self; 8] = [
        Self::NorthWest,
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
    ];

    fn axes(self) -> [i8; 2] {
        match self {
            Self::NorthWest => [-1, -1],
            Self::North => [0, -1],
            Self::NorthEast => [1, -1],
            Self::East => [1, 0],
            Self::SouthEast => [1, 1],
            Self::South => [0, 1],
            Self::SouthWest => [-1, 1],
            Self::West => [-1, 0],
        }
    }

    pub(crate) fn opposite(self) -> Self {
        match self {
            Self::NorthWest => Self::SouthEast,
            Self::North => Self::South,
            Self::NorthEast => Self::SouthWest,
            Self::East => Self::West,
            Self::SouthEast => Self::NorthWest,
            Self::South => Self::North,
            Self::SouthWest => Self::NorthEast,
            Self::West => Self::East,
        }
    }

    pub(crate) fn is_corner(self) -> bool {
        let axes = self.axes();
        axes[0] != 0 && axes[1] != 0
    }

    pub(crate) fn point(self, bounds: Bounds) -> Point {
        let center = bounds.center();
        let axes = self.axes();
        std::array::from_fn(|i| match axes[i] {
            -1 => bounds.min[i],
            1 => bounds.max[i],
            _ => center[i],
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResizeOptions {
    pub from_center: bool,
    pub preserve_aspect: bool,
}

fn finite_point(point: Point) -> bool {
    point.iter().all(|x| x.is_finite())
}

/// Mirrors the document's numeric transform limits, so a gesture cannot produce
/// a pose that its eventual project commit would reject.
fn valid_transform(t: Transform) -> bool {
    [t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.skew, t.alpha]
        .iter()
        .all(|x| x.is_finite())
        && t.x.abs() <= 1e12
        && t.y.abs() <= 1e12
        && t.scale_x.abs() <= 1e4
        && t.scale_y.abs() <= 1e4
        && t.rotation.abs() <= 1e9
        && t.skew.abs() <= 1e9
        && (0.0..=1.0).contains(&t.alpha)
}

fn nonsingular(t: Transform) -> bool {
    let [a, b, c, d, _, _] = transform_matrix(t);
    t.scale_x.abs() > EPSILON
        && t.scale_y.abs() > EPSILON
        && (a * d - b * c).abs() > EPSILON * t.scale_x.abs() * t.scale_y.abs()
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}

fn subtract(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn rotate_vector(v: Point, degrees: f64) -> Point {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [cos * v[0] - sin * v[1], sin * v[0] + cos * v[1]]
}

/// Uses the core's complete affine transform. Gesture functions validate inputs/results;
/// this elementary helper deliberately leaves validation to its caller.
pub(crate) fn transformed_point(t: Transform, local: Point) -> Point {
    let [a, b, c, d, x, y] = transform_matrix(t);
    [
        a * local[0] + c * local[1] + x,
        b * local[0] + d * local[1] + y,
    ]
}

/// Signed least-squares uniform scale. Normalizing first avoids squaring large
/// stage coordinates; a negative result deliberately flips both axes.
fn projected_ratio(base: Point, target: Point) -> Option<f64> {
    if !finite_point(base) || !finite_point(target) {
        return None;
    }
    let magnitude = base[0].abs().max(base[1].abs());
    if magnitude <= EPSILON {
        return None;
    }
    let a = [base[0] / magnitude, base[1] / magnitude];
    let b = [target[0] / magnitude, target[1] / magnitude];
    let factor = (a[0] * b[0] + a[1] * b[1]) / (a[0] * a[0] + a[1] * a[1]);
    factor.is_finite().then_some(factor)
}

/// Resize from the original snapshot. `press` can be offset from the actual
/// handle; only pointer displacement changes the pose, avoiding a press-time jump.
/// Unconstrained crossing flips the crossed axis. Aspect-constrained crossing
/// uses one signed factor, flipping both axes; no sign clamp or hysteresis.
/// Zero scale is a valid intermediate output, but cannot start a new resize.
pub(crate) fn resize_single(
    initial: Transform,
    bounds: Bounds,
    handle: ResizeHandle,
    press: Point,
    current: Point,
    options: ResizeOptions,
) -> Option<Transform> {
    resize_single_with_pivot(initial, bounds, handle, press, current, options, None)
}

pub(crate) fn resize_single_with_pivot(
    initial: Transform,
    bounds: Bounds,
    handle: ResizeHandle,
    press: Point,
    current: Point,
    options: ResizeOptions,
    custom_pivot: Option<Point>,
) -> Option<Transform> {
    if !valid_transform(initial)
        || !nonsingular(initial)
        || !bounds.resizable()
        || !finite_point(press)
        || !finite_point(current)
    {
        return None;
    }
    if press == current {
        return Some(initial);
    }
    let anchor = if options.from_center {
        custom_pivot
            .filter(|p| finite_point(*p))
            .unwrap_or_else(|| bounds.center())
    } else {
        handle.opposite().point(bounds)
    };
    let local_span = subtract(handle.point(bounds), anchor);
    let [a, b, c, d, _, _] = transform_matrix(initial);
    let delta = subtract(current, press);
    let axes = handle.axes();
    let mut factors = [1.0; 2];
    if options.preserve_aspect {
        // Project in world space, where a skewed local basis is not orthogonal.
        let span = [
            a * local_span[0] + c * local_span[1],
            b * local_span[0] + d * local_span[1],
        ];
        let factor = projected_ratio(span, add(span, delta))?;
        factors = [factor; 2];
    } else {
        let determinant = a * d - b * c;
        let local_delta = [
            (d * delta[0] - c * delta[1]) / determinant,
            (a * delta[1] - b * delta[0]) / determinant,
        ];
        for i in 0..2 {
            if axes[i] != 0 {
                if local_span[i].abs() <= EPSILON {
                    return None;
                }
                factors[i] = 1.0 + local_delta[i] / local_span[i];
            }
        }
    }
    let mut result = initial;
    result.scale_x *= factors[0];
    result.scale_y *= factors[1];
    // Recover translation by enforcing the frozen anchor, including a local
    // bounds origin that is different from the symbol registration point.
    let fixed = transformed_point(initial, anchor);
    let offset = transformed_point(
        Transform {
            x: 0.,
            y: 0.,
            ..result
        },
        anchor,
    );
    [result.x, result.y] = subtract(fixed, offset);
    valid_transform(result).then_some(result)
}

/// Shortest signed angle from the frozen press ray, in degrees. Wraps correctly
/// across ±180°. The UI accumulates successive deltas for multiple revolutions.
pub(crate) fn rotation_delta(
    pivot: Point,
    press: Point,
    current: Point,
    snap_15: bool,
) -> Option<f64> {
    if !finite_point(pivot) || !finite_point(press) || !finite_point(current) {
        return None;
    }
    let normalize = |point: Point| {
        let vector = subtract(point, pivot);
        let length = vector[0].hypot(vector[1]);
        (length.is_finite() && length > EPSILON).then(|| [vector[0] / length, vector[1] / length])
    };
    let a = normalize(press)?;
    let b = normalize(current)?;
    let mut angle = (a[0] * b[1] - a[1] * b[0])
        .atan2(a[0] * b[0] + a[1] * b[1])
        .to_degrees();
    if snap_15 {
        angle = (angle / 15.0).round() * 15.0;
    }
    angle.is_finite().then_some(angle)
}

fn rotated_about(initial: Transform, pivot: Point, delta: f64) -> Option<Transform> {
    let mut result = initial;
    [result.x, result.y] = add(
        pivot,
        rotate_vector(subtract([initial.x, initial.y], pivot), delta),
    );
    result.rotation += delta;
    valid_transform(result).then_some(result)
}

/// Rotate around the transformed local bounds center, preserving its position.
/// Shift snaps the final single-object angle (not only the delta) to 15°.
#[cfg(test)]
pub(crate) fn rotate_single(
    initial: Transform,
    bounds: Bounds,
    press: Point,
    current: Point,
    snap_15: bool,
) -> Option<Transform> {
    if !valid_transform(initial) || !bounds.valid() {
        return None;
    }
    let pivot = transformed_point(initial, bounds.center());
    let delta = rotation_delta(pivot, press, current, false)?;
    rotate_single_by(initial, bounds, delta, snap_15)
}

/// Apply an unwrapped gesture angle to the original pose. Full turns remain in
/// `rotation` for timeline interpolation; only the visual position is periodic.
/// A zero delta is an exact no-op, including when snapping is enabled.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn rotate_single_by(
    initial: Transform,
    bounds: Bounds,
    delta_degrees: f64,
    snap_15: bool,
) -> Option<Transform> {
    if !valid_transform(initial) || !bounds.valid() || !delta_degrees.is_finite() {
        return None;
    }
    let pivot = transformed_point(initial, bounds.center());
    rotate_single_about_by(initial, pivot, delta_degrees, snap_15)
}

/// Rotate a single placement about an arbitrary world-space `pivot`.
/// Shift snaps the final single-object angle (not only the delta) to 15°.
pub(crate) fn rotate_single_about_by(
    initial: Transform,
    pivot: Point,
    delta_degrees: f64,
    snap_15: bool,
) -> Option<Transform> {
    if !valid_transform(initial) || !finite_point(pivot) || !delta_degrees.is_finite() {
        return None;
    }
    if delta_degrees == 0.0 {
        return Some(initial);
    }
    let mut delta = delta_degrees;
    if snap_15 {
        delta = ((initial.rotation + delta) / 15.0).round() * 15.0 - initial.rotation;
    }
    rotated_about(initial, pivot, delta)
}

/// Uniform scale of multiple placement origins and scales about a shared pivot.
/// A negative factor preserves every placement's reflection parity while
/// flipping both of its scale signs. No rotation decomposition or shear occurs.
#[cfg(test)]
pub(crate) fn uniform_scale_many(
    initial: &[Transform],
    pivot: Point,
    press: Point,
    current: Point,
) -> Option<Vec<Transform>> {
    let factor = uniform_scale_factor(pivot, press, current)?;
    uniform_scale_many_by(initial, pivot, factor)
}

/// Signed uniform scale implied by a pointer ray in one coordinate space.
/// Callers whose chrome lives in a mapped parent context compute the factor in
/// that context, then apply it about the inverse-mapped pivot with
/// [`uniform_scale_many_by`].
pub(crate) fn uniform_scale_factor(pivot: Point, press: Point, current: Point) -> Option<f64> {
    projected_ratio(subtract(press, pivot), subtract(current, pivot))
}

/// Apply an already-resolved uniform scale in the edited timeline's space.
/// A scalar scale commutes with every affine parent linear transform; only the
/// pivot needs to be inverse-mapped by the caller.
pub(crate) fn uniform_scale_many_by(
    initial: &[Transform],
    pivot: Point,
    factor: f64,
) -> Option<Vec<Transform>> {
    if initial.is_empty()
        || !finite_point(pivot)
        || !factor.is_finite()
        || initial
            .iter()
            .any(|t| !valid_transform(*t) || !nonsingular(*t))
    {
        return None;
    }
    if factor == 1.0 {
        return Some(initial.to_vec());
    }
    initial
        .iter()
        .map(|&original| {
            let mut result = original;
            result.x = pivot[0] + (original.x - pivot[0]) * factor;
            result.y = pivot[1] + (original.y - pivot[1]) * factor;
            result.scale_x *= factor;
            result.scale_y *= factor;
            valid_transform(result).then_some(result)
        })
        .collect()
}

/// Rotate all placements by the same delta, retaining their relative angles
/// and their individual nonuniform/mirrored scales. Shift snaps that delta.
#[cfg(test)]
pub(crate) fn rotate_many(
    initial: &[Transform],
    pivot: Point,
    press: Point,
    current: Point,
    snap_15: bool,
) -> Option<Vec<Transform>> {
    let delta = rotation_delta(pivot, press, current, false)?;
    rotate_many_by(initial, pivot, delta, snap_15)
}

/// Apply the same unwrapped angle to every original pose. Unlike single-object
/// snapping, group snapping constrains the delta to retain relative angles.
pub(crate) fn rotate_many_by(
    initial: &[Transform],
    pivot: Point,
    delta_degrees: f64,
    snap_15: bool,
) -> Option<Vec<Transform>> {
    if initial.is_empty()
        || !finite_point(pivot)
        || !delta_degrees.is_finite()
        || initial.iter().any(|t| !valid_transform(*t))
    {
        return None;
    }
    let delta = if snap_15 {
        (delta_degrees / 15.0).round() * 15.0
    } else {
        delta_degrees
    };
    if delta == 0.0 {
        return Some(initial.to_vec());
    }
    initial
        .iter()
        .map(|&t| rotated_about(t, pivot, delta))
        .collect()
}

/// Free Transform edge drag whose screen-axis box lives in an affine parent
/// context. The context-space shear is conjugated back into the edited
/// timeline before it is applied to each authored placement. The sheared
/// matrix is decomposed with rotation/skew kept on the turn nearest the source.
pub(crate) fn skew_many_in_context(
    initial: &[Transform],
    edit_to_context: [f64; 6],
    horizontal: bool,
    edge: f64,
    anchor: f64,
    displacement: f64,
) -> Option<Vec<Transform>> {
    let shear = shear_matrix(horizontal, edge, anchor, displacement)?;
    let context_to_edit = invert_affine_matrix(edit_to_context)?;
    let edit_shear = affine_multiply(affine_multiply(context_to_edit, shear), edit_to_context);
    transform_many_by_affine(initial, edit_shear)
}

fn shear_matrix(horizontal: bool, edge: f64, anchor: f64, displacement: f64) -> Option<[f64; 6]> {
    let span = edge - anchor;
    if !span.is_finite() || span.abs() <= EPSILON || !displacement.is_finite() {
        return None;
    }
    if displacement == 0.0 {
        return Some([1., 0., 0., 1., 0., 0.]);
    }
    let k = displacement / span;
    Some(if horizontal {
        [1.0, 0.0, k, 1.0, -k * anchor, 0.0]
    } else {
        [1.0, k, 0.0, 1.0, 0.0, -k * anchor]
    })
}

fn transform_many_by_affine(initial: &[Transform], affine: [f64; 6]) -> Option<Vec<Transform>> {
    if initial.is_empty()
        || affine.iter().any(|value| !value.is_finite())
        || initial
            .iter()
            .any(|t| !valid_transform(*t) || !nonsingular(*t))
    {
        return None;
    }
    if affine == [1., 0., 0., 1., 0., 0.] {
        return Some(initial.to_vec());
    }
    initial
        .iter()
        .map(|&t| {
            let matrix = affine_multiply(affine, transform_matrix(t));
            let decomposed = Transform::from_matrix(matrix)?;
            let mut result = Transform {
                alpha: t.alpha,
                color_transform: t.color_transform,
                pivot: t.pivot,
                primitive: t.primitive,
                ..decomposed
            };
            result.rotation += 360.0 * ((t.rotation - result.rotation) / 360.0).round();
            result.skew += 360.0 * ((t.skew - result.skew) / 360.0).round();
            (valid_transform(result) && nonsingular(result)).then_some(result)
        })
        .collect()
}

/// Classic Flash 4-corner `Distort` quad in context space `[TL, TR, BR, BL]`.
///
/// Dragging a corner moves that corner independently; holding `Shift` (or `Shift+Cmd` in default
/// Free Transform mode) performs Classic Flash **perspective tapering** by moving the adjoining
/// corner on the dominant drag axis in the opposite direction by the same distance.
/// Dragging an edge midpoint moves both endpoints of that edge together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DistortQuad {
    pub corners: [Point; 4],
}

impl DistortQuad {
    pub(crate) fn from_bounds(bounds: Bounds) -> Self {
        Self {
            corners: [
                bounds.min,
                [bounds.max[0], bounds.min[1]],
                bounds.max,
                [bounds.min[0], bounds.max[1]],
            ],
        }
    }

    pub(crate) fn valid(self) -> bool {
        self.corners.into_iter().all(finite_point)
    }

    pub(crate) fn handle_point(self, handle: ResizeHandle) -> Point {
        let [tl, tr, br, bl] = self.corners;
        match handle {
            ResizeHandle::NorthWest => tl,
            ResizeHandle::North => [tl[0] * 0.5 + tr[0] * 0.5, tl[1] * 0.5 + tr[1] * 0.5],
            ResizeHandle::NorthEast => tr,
            ResizeHandle::East => [tr[0] * 0.5 + br[0] * 0.5, tr[1] * 0.5 + br[1] * 0.5],
            ResizeHandle::SouthEast => br,
            ResizeHandle::South => [br[0] * 0.5 + bl[0] * 0.5, br[1] * 0.5 + bl[1] * 0.5],
            ResizeHandle::SouthWest => bl,
            ResizeHandle::West => [bl[0] * 0.5 + tl[0] * 0.5, bl[1] * 0.5 + tl[1] * 0.5],
        }
    }

    pub(crate) fn center(self) -> Point {
        let [tl, tr, br, bl] = self.corners;
        [
            (tl[0] + tr[0] + br[0] + bl[0]) * 0.25,
            (tl[1] + tr[1] + br[1] + bl[1]) * 0.25,
        ]
    }

    pub(crate) fn dragged(
        self,
        handle: ResizeHandle,
        press: Point,
        current: Point,
        perspective_taper: bool,
    ) -> Option<Self> {
        if !self.valid() || !finite_point(press) || !finite_point(current) {
            return None;
        }
        let delta = [current[0] - press[0], current[1] - press[1]];
        if !finite_point(delta) {
            return None;
        }
        let mut corners = self.corners;
        match handle {
            ResizeHandle::NorthWest
            | ResizeHandle::NorthEast
            | ResizeHandle::SouthEast
            | ResizeHandle::SouthWest => {
                let idx = match handle {
                    ResizeHandle::NorthWest => 0,
                    ResizeHandle::NorthEast => 1,
                    ResizeHandle::SouthEast => 2,
                    ResizeHandle::SouthWest => 3,
                    _ => unreachable!(),
                };
                if perspective_taper {
                    // Classic Flash Shift-drag perspective taper: move the dragged corner along
                    // the dominant drag axis and move the opposite corner on the same edge in the
                    // opposite direction by the same distance.
                    if delta[0].abs() >= delta[1].abs() {
                        let pair = idx ^ 1; // 0<->1 (top edge), 2<->3 (bottom edge)
                        corners[idx][0] += delta[0];
                        corners[pair][0] -= delta[0];
                    } else {
                        let pair = 3 - idx; // 0<->3 (left edge), 1<->2 (right edge)
                        corners[idx][1] += delta[1];
                        corners[pair][1] -= delta[1];
                    }
                } else {
                    corners[idx] = add(corners[idx], delta);
                }
            }
            ResizeHandle::North => {
                corners[0] = add(corners[0], delta);
                corners[1] = add(corners[1], delta);
            }
            ResizeHandle::East => {
                corners[1] = add(corners[1], delta);
                corners[2] = add(corners[2], delta);
            }
            ResizeHandle::South => {
                corners[2] = add(corners[2], delta);
                corners[3] = add(corners[3], delta);
            }
            ResizeHandle::West => {
                corners[3] = add(corners[3], delta);
                corners[0] = add(corners[0], delta);
            }
        }
        let next = Self { corners };
        next.valid().then_some(next)
    }

    /// Bilinear interpolation of normalized `(u, v)` in `[0, 1]^2` across the 4 corners.
    pub(crate) fn eval(self, u: f64, v: f64) -> Point {
        let [tl, tr, br, bl] = self.corners;
        let top = lerp_point(tl, tr, u);
        let bottom = lerp_point(bl, br, u);
        lerp_point(top, bottom, v)
    }
}

/// Interactive handle on the 24-point Classic Flash `Envelope` modifier cage:
/// 8 square anchor points (`■`) and 16 circular tangent handles (`●`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnvelopeHandle {
    Anchor(usize),
    TangentPrev(usize),
    TangentNext(usize),
}

/// Classic Flash `Envelope` cage in context space:
/// - 8 anchor points (`■`) clockwise around the perimeter:
///   `0 = TL`, `1 = TC`, `2 = TR`, `3 = RC`, `4 = BR`, `5 = BC`, `6 = BL`, `7 = LC`
/// - `tangent_prev[i]`: incoming cubic Bézier tangent handle (`●`) attached to `anchors[i]`
///   along the perimeter segment from `(i + 7) % 8` to `i`
/// - `tangent_next[i]`: outgoing cubic Bézier tangent handle (`●`) attached to `anchors[i]`
///   along the perimeter segment from `i` to `(i + 1) % 8`
///
/// Each of the 4 sides therefore has 3 square anchors (`■`) and 4 circular tangent handles (`●`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EnvelopeCage {
    pub bounds: Bounds,
    pub anchors: [Point; 8],
    pub tangent_prev: [Point; 8],
    pub tangent_next: [Point; 8],
}

impl EnvelopeCage {
    pub(crate) fn from_bounds(bounds: Bounds) -> Self {
        let c = bounds.center();
        let anchors = [
            bounds.min,                     // 0: TL
            [c[0], bounds.min[1]],          // 1: TC
            [bounds.max[0], bounds.min[1]], // 2: TR
            [bounds.max[0], c[1]],          // 3: RC
            bounds.max,                     // 4: BR
            [c[0], bounds.max[1]],          // 5: BC
            [bounds.min[0], bounds.max[1]], // 6: BL
            [bounds.min[0], c[1]],          // 7: LC
        ];
        let mut tangent_prev = [[0.0; 2]; 8];
        let mut tangent_next = [[0.0; 2]; 8];
        for i in 0..8 {
            let prev = anchors[(i + 7) % 8];
            let curr = anchors[i];
            let next = anchors[(i + 1) % 8];
            tangent_prev[i] = lerp_point(curr, prev, 1.0 / 3.0);
            tangent_next[i] = lerp_point(curr, next, 1.0 / 3.0);
        }
        Self {
            bounds,
            anchors,
            tangent_prev,
            tangent_next,
        }
    }

    pub(crate) fn valid(&self) -> bool {
        self.bounds.valid()
            && self.anchors.into_iter().all(finite_point)
            && self.tangent_prev.into_iter().all(finite_point)
            && self.tangent_next.into_iter().all(finite_point)
    }

    pub(crate) fn dragged(
        &self,
        handle: EnvelopeHandle,
        press: Point,
        current: Point,
    ) -> Option<Self> {
        if !self.valid() || !finite_point(press) || !finite_point(current) {
            return None;
        }
        let delta = [current[0] - press[0], current[1] - press[1]];
        if !finite_point(delta) {
            return None;
        }
        let mut next = *self;
        match handle {
            EnvelopeHandle::Anchor(i) if i < 8 => {
                // Moving an anchor point translates its two attached tangent handles with it.
                next.anchors[i] = add(next.anchors[i], delta);
                next.tangent_prev[i] = add(next.tangent_prev[i], delta);
                next.tangent_next[i] = add(next.tangent_next[i], delta);
            }
            EnvelopeHandle::TangentPrev(i) if i < 8 => {
                next.tangent_prev[i] = add(next.tangent_prev[i], delta);
            }
            EnvelopeHandle::TangentNext(i) if i < 8 => {
                next.tangent_next[i] = add(next.tangent_next[i], delta);
            }
            _ => return None,
        }
        next.valid().then_some(next)
    }

    /// Evaluates one of the 8 clockwise perimeter cubic Bézier segments `seg` in `0..8` at `t`.
    pub(crate) fn boundary_segment(&self, seg: usize, t: f64) -> Point {
        let i = seg % 8;
        let j = (i + 1) % 8;
        eval_cubic(
            self.anchors[i],
            self.tangent_next[i],
            self.tangent_prev[j],
            self.anchors[j],
            t,
        )
    }

    fn top_curve(&self, u: f64) -> Point {
        if u <= 0.5 {
            self.boundary_segment(0, u * 2.0)
        } else {
            self.boundary_segment(1, (u - 0.5) * 2.0)
        }
    }

    fn right_curve(&self, v: f64) -> Point {
        if v <= 0.5 {
            self.boundary_segment(2, v * 2.0)
        } else {
            self.boundary_segment(3, (v - 0.5) * 2.0)
        }
    }

    fn bottom_curve(&self, u: f64) -> Point {
        // Segments 4 (BR -> BC) and 5 (BC -> BL) run right-to-left, so invert `u`.
        if u <= 0.5 {
            self.boundary_segment(5, 1.0 - u * 2.0)
        } else {
            self.boundary_segment(4, 1.0 - (u - 0.5) * 2.0)
        }
    }

    fn left_curve(&self, v: f64) -> Point {
        // Segments 6 (BL -> LC) and 7 (LC -> TL) run bottom-to-top, so invert `v`.
        if v <= 0.5 {
            self.boundary_segment(7, 1.0 - v * 2.0)
        } else {
            self.boundary_segment(6, 1.0 - (v - 0.5) * 2.0)
        }
    }

    /// Bicubic Coons patch evaluation at normalized `(u, v)` in `[0, 1]^2` with smoothly blended
    /// internal midpoint tangents (`TC`, `RC`, `BC`, `LC`) so dragging any of the 8 anchors or 16
    /// tangent handles warps the interior as well as the boundary.
    pub(crate) fn eval(&self, u: f64, v: f64) -> Point {
        let top = self.top_curve(u);
        let bottom = self.bottom_curve(u);
        let left = self.left_curve(v);
        let right = self.right_curve(v);

        let lc = lerp_point(top, bottom, v);
        let ld = lerp_point(left, right, u);

        let [tl, _, tr, _, br, _, bl, _] = self.anchors;
        let bilinear = lerp_point(lerp_point(tl, tr, u), lerp_point(bl, br, u), v);
        [lc[0] + ld[0] - bilinear[0], lc[1] + ld[1] - bilinear[1]]
    }
}

fn lerp_point(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn eval_quad(p0: Point, p1: Point, p2: Point, t: f64) -> Point {
    let mt = 1.0 - t;
    let w0 = mt * mt;
    let w1 = 2.0 * mt * t;
    let w2 = t * t;
    [
        w0 * p0[0] + w1 * p1[0] + w2 * p2[0],
        w0 * p0[1] + w1 * p1[1] + w2 * p2[1],
    ]
}

fn eval_cubic(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;
    let w0 = mt2 * mt;
    let w1 = 3.0 * mt2 * t;
    let w2 = 3.0 * mt * t2;
    let w3 = t2 * t;
    [
        w0 * p0[0] + w1 * p1[0] + w2 * p2[0] + w3 * p3[0],
        w0 * p0[1] + w1 * p1[1] + w2 * p2[1] + w3 * p3[1],
    ]
}

/// Fits a cubic Bézier `CubicTo` segment whose curve passes through `q0` at `t = 0`,
/// `q1` at `t = 1/3`, `q2` at `t = 2/3`, and `q3` at `t = 1`.
fn fit_cubic_through_four_points(q0: Point, q1: Point, q2: Point, q3: Point) -> PathCommand {
    let cx1 = (-5.0 * q0[0] + 18.0 * q1[0] - 9.0 * q2[0] + 2.0 * q3[0]) / 6.0;
    let cy1 = (-5.0 * q0[1] + 18.0 * q1[1] - 9.0 * q2[1] + 2.0 * q3[1]) / 6.0;
    let cx2 = (2.0 * q0[0] - 9.0 * q1[0] + 18.0 * q2[0] - 5.0 * q3[0]) / 6.0;
    let cy2 = (2.0 * q0[1] - 9.0 * q1[1] + 18.0 * q2[1] - 5.0 * q3[1]) / 6.0;
    PathCommand::CubicTo {
        cx1,
        cy1,
        cx2,
        cy2,
        x: q3[0],
        y: q3[1],
    }
}

fn apply_affine(m: [f64; 6], p: Point) -> Point {
    [
        m[0] * p[0] + m[2] * p[1] + m[4],
        m[1] * p[0] + m[3] * p[1] + m[5],
    ]
}

fn deform_element_commands(
    element: &Element,
    element_to_context: [f64; 6],
    context_to_element: [f64; 6],
    subdivide_lines: usize,
    map_context_point: &impl Fn(Point) -> Point,
) -> Option<Element> {
    let (source_commands, fill_rule) = element.to_path_commands()?;
    let map_local = |local: Point| -> Point {
        let ctx_pt = apply_affine(element_to_context, local);
        let warped_ctx = map_context_point(ctx_pt);
        apply_affine(context_to_element, warped_ctx)
    };
    let mut commands = Vec::with_capacity(source_commands.len() * subdivide_lines.max(1));
    let mut current = [0.0, 0.0];
    let mut subpath_start = [0.0, 0.0];

    let emit_subdivided_segment =
        |commands: &mut Vec<PathCommand>,
         subdivisions: usize,
         sample_local: &dyn Fn(f64) -> Point| {
            let n = subdivisions.max(1);
            for i in 0..n {
                let t0 = i as f64 / n as f64;
                let t3 = (i + 1) as f64 / n as f64;
                let t1 = t0 + (t3 - t0) / 3.0;
                let t2 = t0 + (t3 - t0) * (2.0 / 3.0);
                let q0 = map_local(sample_local(t0));
                let q1 = map_local(sample_local(t1));
                let q2 = map_local(sample_local(t2));
                let q3 = map_local(sample_local(t3));
                if subdivide_lines <= 1 {
                    commands.push(PathCommand::LineTo { x: q3[0], y: q3[1] });
                } else {
                    commands.push(fit_cubic_through_four_points(q0, q1, q2, q3));
                }
            }
        };

    for cmd in source_commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                let point = [x, y];
                current = point;
                subpath_start = point;
                let mapped = map_local(point);
                commands.push(PathCommand::MoveTo {
                    x: mapped[0],
                    y: mapped[1],
                });
            }
            PathCommand::LineTo { x, y } => {
                let p0 = current;
                let p1 = [x, y];
                if subdivide_lines <= 1 {
                    let mapped = map_local(p1);
                    commands.push(PathCommand::LineTo {
                        x: mapped[0],
                        y: mapped[1],
                    });
                } else {
                    emit_subdivided_segment(&mut commands, subdivide_lines, &|t| {
                        lerp_point(p0, p1, t)
                    });
                }
                current = p1;
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let p0 = current;
                let control = [cx, cy];
                let to = [x, y];
                emit_subdivided_segment(&mut commands, subdivide_lines.clamp(1, 2), &|t| {
                    eval_quad(p0, control, to, t)
                });
                current = to;
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let p0 = current;
                let control_1 = [cx1, cy1];
                let control_2 = [cx2, cy2];
                let to = [x, y];
                emit_subdivided_segment(&mut commands, subdivide_lines.clamp(1, 2), &|t| {
                    eval_cubic(p0, control_1, control_2, to, t)
                });
                current = to;
            }
            PathCommand::Close => {
                if (current[0] - subpath_start[0]).hypot(current[1] - subpath_start[1]) > 1e-6
                    && subdivide_lines > 1
                {
                    let p0 = current;
                    let p1 = subpath_start;
                    emit_subdivided_segment(&mut commands, subdivide_lines, &|t| {
                        lerp_point(p0, p1, t)
                    });
                }
                current = subpath_start;
                commands.push(PathCommand::Close);
            }
        }
    }

    for cmd in &commands {
        match *cmd {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                if !finite_point([x, y]) {
                    return None;
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if !finite_point([cx, cy]) || !finite_point([x, y]) {
                    return None;
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if !finite_point([cx1, cy1]) || !finite_point([cx2, cy2]) || !finite_point([x, y]) {
                    return None;
                }
            }
            PathCommand::Close => {}
        }
    }

    let mut updated = element.clone();
    updated.transform.primitive = None;
    updated.kind = match &element.kind {
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
        }
        | ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        } => ElementKind::Path {
            commands,
            fill_rule,
            fill: *fill,
            stroke: *stroke,
            stroke_width: *stroke_width,
        },
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill: *fill,
            stroke: *stroke,
            fill_gradient: fill_gradient.clone(),
            stroke_gradient: stroke_gradient.clone(),
            stroke_width: *stroke_width,
        },
        _ => return None,
    };
    Some(updated)
}

pub(crate) fn element_supports_mesh_deform(element: &Element) -> bool {
    matches!(
        element.kind,
        ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. }
    )
}

/// Deforms a vector shape (`Rectangle`, `Ellipse`, `Path`, `PaintedPath`) using the 4-corner
/// `DistortQuad` over `source_bounds` in context space. Straight lines are subdivided so non-affine
/// bilinear quad warps bend smoothly.
pub(crate) fn deform_element_quad(
    element: &Element,
    edit_to_context: [f64; 6],
    source_bounds: Bounds,
    quad: DistortQuad,
) -> Option<Element> {
    if !source_bounds.resizable() || !quad.valid() || !element_supports_mesh_deform(element) {
        return None;
    }
    let element_to_context = affine_multiply(edit_to_context, transform_matrix(element.transform));
    let context_to_element = invert_affine_matrix(element_to_context)?;
    let span_x = source_bounds.max[0] - source_bounds.min[0];
    let span_y = source_bounds.max[1] - source_bounds.min[1];
    deform_element_commands(element, element_to_context, context_to_element, 2, &|pt| {
        let u = (pt[0] - source_bounds.min[0]) / span_x;
        let v = (pt[1] - source_bounds.min[1]) / span_y;
        quad.eval(u, v)
    })
}

/// Deforms a vector shape (`Rectangle`, `Ellipse`, `Path`, `PaintedPath`) using the 24-handle
/// `EnvelopeCage` in context space. Straight lines are subdivided into 4 cubic Bézier segments
/// (hitting the midpoint anchors at `0.5`) so dragging any of the 8 square anchors or 16 circular
/// tangent handles bends edges into smooth cubic curves.
pub(crate) fn deform_element_envelope(
    element: &Element,
    edit_to_context: [f64; 6],
    cage: &EnvelopeCage,
) -> Option<Element> {
    if !cage.valid() || !cage.bounds.resizable() || !element_supports_mesh_deform(element) {
        return None;
    }
    let element_to_context = affine_multiply(edit_to_context, transform_matrix(element.transform));
    let context_to_element = invert_affine_matrix(element_to_context)?;
    let span_x = cage.bounds.max[0] - cage.bounds.min[0];
    let span_y = cage.bounds.max[1] - cage.bounds.min[1];
    deform_element_commands(element, element_to_context, context_to_element, 4, &|pt| {
        let u = (pt[0] - cage.bounds.min[0]) / span_x;
        let v = (pt[1] - cage.bounds.min[1]) / span_y;
        cage.eval(u, v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Bounds = Bounds {
        min: [-20., 15.],
        max: [80., 65.],
    };

    fn close(a: f64, b: f64) {
        assert!(
            (a - b).abs() <= 1e-8 * a.abs().max(b.abs()).max(1.),
            "{a} != {b}"
        );
    }

    fn point_close(a: Point, b: Point) {
        close(a[0], b[0]);
        close(a[1], b[1]);
    }

    fn pose() -> Transform {
        Transform {
            x: 312.,
            y: -125.,
            scale_x: -2.5,
            scale_y: 0.7,
            rotation: 37.,
            alpha: 0.6,
            ..Default::default()
        }
    }

    fn pointer_delta(initial: Transform, local_axes_delta: Point) -> Point {
        transformed_point(
            Transform {
                x: 0.,
                y: 0.,
                scale_x: 1.,
                scale_y: 1.,
                ..initial
            },
            local_axes_delta,
        )
    }

    #[test]
    fn all_eight_handles_keep_opposite_anchor_for_rotated_nonuniform_mirror() {
        for initial in [
            pose(),
            Transform::from_matrix([-1.8, 0.4, 0.7, 1.2, 312., -125.]).unwrap(),
        ] {
            for handle in ResizeHandle::ALL {
                let press = add(transformed_point(initial, handle.point(BOUNDS)), [2., -3.]);
                let current = add(press, pointer_delta(initial, [19., -11.]));
                let result = resize_single(
                    initial,
                    BOUNDS,
                    handle,
                    press,
                    current,
                    ResizeOptions::default(),
                )
                .unwrap();
                let anchor = handle.opposite().point(BOUNDS);
                point_close(
                    transformed_point(initial, anchor),
                    transformed_point(result, anchor),
                );
                close(result.rotation, initial.rotation);
                close(result.skew, initial.skew);
                close(result.alpha, initial.alpha);
                let axes = handle.axes();
                let old_handle = transformed_point(initial, handle.point(BOUNDS));
                let mut used_delta = [19., -11.];
                for i in 0..2 {
                    if axes[i] == 0 {
                        used_delta[i] = 0.;
                    }
                }
                point_close(
                    transformed_point(result, handle.point(BOUNDS)),
                    add(old_handle, pointer_delta(initial, used_delta)),
                );
                if axes[0] == 0 {
                    close(result.scale_x, initial.scale_x);
                }
                if axes[1] == 0 {
                    close(result.scale_y, initial.scale_y);
                }
            }
        }
    }

    #[test]
    fn option_center_stays_fixed_and_grab_offset_does_not_jump() {
        let initial = pose();
        for handle in ResizeHandle::ALL {
            let press = [123., 456.];
            let options = ResizeOptions {
                from_center: true,
                preserve_aspect: false,
            };
            assert_eq!(
                resize_single(initial, BOUNDS, handle, press, press, options),
                Some(initial)
            );
            let result = resize_single(
                initial,
                BOUNDS,
                handle,
                press,
                add(press, [40., -35.]),
                options,
            )
            .unwrap();
            point_close(
                transformed_point(result, BOUNDS.center()),
                transformed_point(initial, BOUNDS.center()),
            );
        }
    }

    #[test]
    fn shift_corner_projects_to_uniform_diagonal_and_side_ignores_tangent() {
        let initial = pose();
        for center in [false, true] {
            let options = ResizeOptions {
                from_center: center,
                preserve_aspect: true,
            };
            for handle in ResizeHandle::ALL {
                let anchor = if center {
                    BOUNDS.center()
                } else {
                    handle.opposite().point(BOUNDS)
                };
                let span = subtract(
                    transformed_point(initial, handle.point(BOUNDS)),
                    transformed_point(initial, anchor),
                );
                let result = resize_single(
                    initial,
                    BOUNDS,
                    handle,
                    [0., 0.],
                    [span[0] * 0.4, span[1] * 0.4],
                    options,
                )
                .unwrap();
                close(result.scale_x / initial.scale_x, 1.4);
                close(result.scale_y / initial.scale_y, 1.4);
                point_close(
                    transformed_point(result, anchor),
                    transformed_point(initial, anchor),
                );
            }
        }
        let east = resize_single(
            initial,
            BOUNDS,
            ResizeHandle::East,
            [0., 0.],
            pointer_delta(initial, [0., 200.]),
            ResizeOptions {
                preserve_aspect: true,
                ..Default::default()
            },
        )
        .unwrap();
        close(east.scale_x, initial.scale_x);
        close(east.scale_y, initial.scale_y);
    }

    #[test]
    fn crossing_fixed_anchor_can_pass_through_zero_then_reflect() {
        for initial in [
            pose(),
            Transform::from_matrix([-1.8, 0.4, 0.7, 1.2, 312., -125.]).unwrap(),
        ] {
            let handle = ResizeHandle::East;
            let span = subtract(
                transformed_point(initial, handle.point(BOUNDS)),
                transformed_point(initial, handle.opposite().point(BOUNDS)),
            );
            let zero = resize_single(
                initial,
                BOUNDS,
                handle,
                [0., 0.],
                [-span[0], -span[1]],
                ResizeOptions::default(),
            )
            .unwrap();
            close(zero.scale_x, 0.);
            let crossed = resize_single(
                initial,
                BOUNDS,
                handle,
                [0., 0.],
                [-2. * span[0], -2. * span[1]],
                ResizeOptions::default(),
            )
            .unwrap();
            close(crossed.scale_x, -initial.scale_x);
            close(crossed.scale_y, initial.scale_y);
            point_close(
                transformed_point(crossed, handle.opposite().point(BOUNDS)),
                transformed_point(initial, handle.opposite().point(BOUNDS)),
            );
            assert!(
                resize_single(
                    zero,
                    BOUNDS,
                    handle,
                    [0., 0.],
                    [1., 0.],
                    ResizeOptions::default()
                )
                .is_none()
            );
        }
    }

    #[test]
    fn single_rotation_keeps_visual_center_for_mirrored_nonuniform_pose() {
        let initial = pose();
        let pivot = transformed_point(initial, BOUNDS.center());
        let press = add(pivot, [50., 0.]);
        let current = add(pivot, [0., 50.]);
        let result = rotate_single(initial, BOUNDS, press, current, false).unwrap();
        close(result.rotation, 127.);
        point_close(transformed_point(result, BOUNDS.center()), pivot);
        close(result.scale_x, initial.scale_x);
        close(result.scale_y, initial.scale_y);
        for p in [BOUNDS.min, BOUNDS.max, [0., 0.]] {
            point_close(
                transformed_point(result, p),
                add(
                    pivot,
                    rotate_vector(subtract(transformed_point(initial, p), pivot), 90.),
                ),
            );
        }
    }

    #[test]
    fn single_shift_snaps_final_angle_and_press_without_move_is_exact_noop() {
        let initial = Transform {
            rotation: 13.,
            ..pose()
        };
        let pivot = transformed_point(initial, BOUNDS.center());
        let press = add(pivot, [100., 0.]);
        let current = add(pivot, rotate_vector([100., 0.], -12.));
        let result = rotate_single(initial, BOUNDS, press, current, true).unwrap();
        close(result.rotation, 0.);
        point_close(transformed_point(result, BOUNDS.center()), pivot);
        assert_eq!(
            rotate_single(initial, BOUNDS, press, press, true),
            Some(initial)
        );
    }

    #[test]
    fn rotation_wraps_across_branch_cut_and_snaps_delta() {
        let ray = |degrees| rotate_vector([20., 0.], degrees);
        close(
            rotation_delta([0., 0.], ray(179.), ray(-179.), false).unwrap(),
            2.,
        );
        close(
            rotation_delta([0., 0.], ray(-179.), ray(179.), false).unwrap(),
            -2.,
        );
        close(
            rotation_delta([0., 0.], ray(170.), ray(-172.), true).unwrap(),
            15.,
        );
        assert!(rotation_delta([0., 0.], [0., 0.], [1., 0.], false).is_none());
        assert!(rotation_delta([0., 0.], [1., 0.], [0., 0.], false).is_none());
    }

    #[test]
    fn multi_uniform_scale_preserves_exact_affine_images_and_reflections() {
        let initial = [
            pose(),
            Transform {
                x: -65.,
                y: 42.,
                rotation: -76.,
                scale_x: 0.6,
                scale_y: -3.2,
                ..pose()
            },
        ];
        let pivot = [10., 90.];
        let press = add(pivot, [100., 40.]);
        for factor in [2.25, 0., -1.5] {
            let result = uniform_scale_many(
                &initial,
                pivot,
                press,
                add(pivot, [100. * factor, 40. * factor]),
            )
            .unwrap();
            for (before, after) in initial.iter().zip(result) {
                close(after.rotation, before.rotation);
                close(after.scale_x, before.scale_x * factor);
                close(after.scale_y, before.scale_y * factor);
                close(after.alpha, before.alpha);
                for p in [BOUNDS.min, BOUNDS.max, [0., 0.], [17., -13.]] {
                    let original = transformed_point(*before, p);
                    point_close(
                        transformed_point(after, p),
                        [
                            pivot[0] + (original[0] - pivot[0]) * factor,
                            pivot[1] + (original[1] - pivot[1]) * factor,
                        ],
                    );
                }
            }
        }
    }

    #[test]
    fn multi_rotation_preserves_relative_angles_scales_and_shared_pivot() {
        let initial = [
            pose(),
            Transform {
                x: -50.,
                y: 30.,
                rotation: -18.,
                scale_x: 1.3,
                scale_y: -4.,
                ..pose()
            },
        ];
        let pivot = [80., -90.];
        let result = rotate_many(
            &initial,
            pivot,
            add(pivot, [50., 0.]),
            add(pivot, rotate_vector([50., 0.], 17.)),
            true,
        )
        .unwrap();
        for (before, after) in initial.iter().zip(&result) {
            close(after.rotation - before.rotation, 15.);
            close(after.scale_x, before.scale_x);
            close(after.scale_y, before.scale_y);
            for p in [BOUNDS.min, BOUNDS.max, [0., 0.]] {
                point_close(
                    transformed_point(*after, p),
                    add(
                        pivot,
                        rotate_vector(subtract(transformed_point(*before, p), pivot), 15.),
                    ),
                );
            }
        }
        close(
            result[0].rotation - result[1].rotation,
            initial[0].rotation - initial[1].rotation,
        );
    }

    #[test]
    fn invalid_degenerate_and_document_limit_inputs_are_rejected_atomically() {
        let resize = |t, b, p| {
            resize_single(
                t,
                b,
                ResizeHandle::SouthEast,
                [0., 0.],
                p,
                ResizeOptions::default(),
            )
        };
        assert!(
            resize(
                pose(),
                Bounds {
                    min: [0., 0.],
                    max: [0., 40.]
                },
                [2., 2.]
            )
            .is_none()
        );
        assert!(
            resize(
                Transform {
                    scale_y: 0.,
                    ..pose()
                },
                BOUNDS,
                [2., 2.]
            )
            .is_none()
        );
        assert!(resize(pose(), BOUNDS, [f64::NAN, 2.]).is_none());
        assert!(
            resize(
                Transform {
                    scale_x: f64::INFINITY,
                    ..pose()
                },
                BOUNDS,
                [2., 2.]
            )
            .is_none()
        );
        assert!(
            resize(
                pose(),
                Bounds {
                    min: [3., 0.],
                    max: [2., 40.]
                },
                [2., 2.]
            )
            .is_none()
        );
        assert!(resize(pose(), BOUNDS, [1e200, 0.]).is_none());
        assert!(
            uniform_scale_many(
                &[
                    pose(),
                    Transform {
                        scale_x: 0.,
                        ..pose()
                    }
                ],
                [0., 0.],
                [1., 0.],
                [2., 0.]
            )
            .is_none()
        );
        assert!(uniform_scale_many(&[pose()], [0., 0.], [0., 0.], [2., 0.]).is_none());
        assert!(uniform_scale_many(&[pose()], [0., 0.], [1., 0.], [1e10, 0.]).is_none());
        assert!(rotate_many(&[pose()], [f64::NAN, 0.], [1., 0.], [0., 1.], false).is_none());
        assert!(rotate_many(&[], [0., 0.], [1., 0.], [0., 1.], false).is_none());
        assert!(BOUNDS.valid());
        assert!(BOUNDS.contains(BOUNDS.center()));
        assert!(!BOUNDS.contains([f64::NAN, 0.]));
        assert!(!BOUNDS.contains([1e10, 0.]));
    }

    #[test]
    fn outputs_validate_in_a_real_document_and_snapshots_do_not_accumulate() {
        let initial = pose();
        let press = [40., 40.];
        let current = [100., 70.];
        let resized = resize_single(
            initial,
            BOUNDS,
            ResizeHandle::NorthWest,
            press,
            current,
            ResizeOptions::default(),
        )
        .unwrap();
        assert_eq!(
            resize_single(
                initial,
                BOUNDS,
                ResizeHandle::NorthWest,
                press,
                current,
                ResizeOptions::default()
            ),
            Some(resized)
        );
        let pivot = transformed_point(initial, BOUNDS.center());
        let rotated = rotate_single(
            initial,
            BOUNDS,
            add(pivot, [30., 0.]),
            add(pivot, [0., 30.]),
            true,
        )
        .unwrap();
        for transform in [resized, rotated] {
            let mut project = neoflash_core::Project::demo();
            project.symbols[0].timeline.layers[0].keyframes[0].elements[0].transform = transform;
            assert!(project.validate().is_ok());
        }
        assert!(ResizeHandle::NorthWest.is_corner());
        assert!(!ResizeHandle::North.is_corner());
    }

    #[test]
    fn unwrapped_single_retains_full_turns_and_snaps_around_initial_angle() {
        let initial = Transform {
            rotation: 13.,
            ..pose()
        };
        let pivot = transformed_point(initial, BOUNDS.center());
        for delta in [720., -450.] {
            let result = rotate_single_by(initial, BOUNDS, delta, false).unwrap();
            close(result.rotation, initial.rotation + delta);
            point_close(transformed_point(result, BOUNDS.center()), pivot);
            for point in [BOUNDS.min, BOUNDS.max] {
                point_close(
                    transformed_point(result, point),
                    add(
                        pivot,
                        rotate_vector(subtract(transformed_point(initial, point), pivot), delta),
                    ),
                );
            }
            let snapped = rotate_single_by(initial, BOUNDS, delta, true).unwrap();
            close(
                snapped.rotation,
                ((initial.rotation + delta) / 15.).round() * 15.,
            );
            point_close(transformed_point(snapped, BOUNDS.center()), pivot);
            let mut project = neoflash_core::Project::demo();
            project.symbols[0].timeline.layers[0].keyframes[0].elements[0].transform = snapped;
            assert!(project.validate().is_ok());
        }
        assert_eq!(rotate_single_by(initial, BOUNDS, 0., true), Some(initial));
        close(
            rotate_single_by(initial, BOUNDS, 720., true)
                .unwrap()
                .rotation,
            735.,
        );
        close(
            rotate_single_by(initial, BOUNDS, -450., true)
                .unwrap()
                .rotation,
            -435.,
        );
    }

    #[test]
    fn unwrapped_multi_preserves_winding_relative_angles_and_reflections() {
        let initial = [
            pose(),
            Transform {
                rotation: -18.,
                scale_y: -3.,
                x: -70.,
                ..pose()
            },
        ];
        let pivot = [60., -12.];
        for delta in [720., -450., 721., -449.] {
            for snap in [false, true] {
                let applied = if snap {
                    (delta / 15.0_f64).round() * 15.
                } else {
                    delta
                };
                let result = rotate_many_by(&initial, pivot, delta, snap).unwrap();
                for (original, transformed) in initial.iter().zip(&result) {
                    close(transformed.rotation, original.rotation + applied);
                    close(transformed.scale_x, original.scale_x);
                    close(transformed.scale_y, original.scale_y);
                    for point in [BOUNDS.min, BOUNDS.max] {
                        point_close(
                            transformed_point(*transformed, point),
                            add(
                                pivot,
                                rotate_vector(
                                    subtract(transformed_point(*original, point), pivot),
                                    applied,
                                ),
                            ),
                        );
                    }
                }
                close(
                    result[0].rotation - result[1].rotation,
                    initial[0].rotation - initial[1].rotation,
                );
            }
        }
        assert_eq!(
            rotate_many_by(&initial, pivot, 0., true),
            Some(initial.to_vec())
        );
    }

    #[test]
    fn unwrapped_rotation_rejects_nonfinite_and_out_of_range_results() {
        for delta in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e10, -1e10] {
            for snap in [false, true] {
                assert!(rotate_single_by(pose(), BOUNDS, delta, snap).is_none());
                assert!(rotate_many_by(&[pose()], [0., 0.], delta, snap).is_none());
            }
        }
        assert!(rotate_many_by(&[pose()], [f64::NAN, 0.], 0., true).is_none());
        assert!(
            rotate_single_by(
                pose(),
                Bounds {
                    min: [f64::NAN, 0.],
                    max: [2., 3.]
                },
                0.,
                true
            )
            .is_none()
        );
        let near_limit = Transform {
            rotation: 1e9,
            ..pose()
        };
        assert!(rotate_many_by(&[pose(), near_limit], [0., 0.], 1., false).is_none());
    }

    #[test]
    fn distort_quad_moves_single_corner_and_shift_perspective_tapers_opposite_corner() {
        let quad = DistortQuad::from_bounds(BOUNDS);
        let moved = quad
            .dragged(
                ResizeHandle::NorthWest,
                BOUNDS.min,
                [BOUNDS.min[0] + 15.0, BOUNDS.min[1] + 5.0],
                false,
            )
            .unwrap();
        point_close(
            moved.corners[0],
            [BOUNDS.min[0] + 15.0, BOUNDS.min[1] + 5.0],
        );
        point_close(moved.corners[1], quad.corners[1]);
        point_close(moved.corners[2], quad.corners[2]);
        point_close(moved.corners[3], quad.corners[3]);

        // Shift-drag horizontal taper on NorthWest moves NorthWest.x by +15 and NorthEast.x by -15.
        let tapered = quad
            .dragged(
                ResizeHandle::NorthWest,
                BOUNDS.min,
                [BOUNDS.min[0] + 15.0, BOUNDS.min[1] + 5.0],
                true,
            )
            .unwrap();
        point_close(tapered.corners[0], [BOUNDS.min[0] + 15.0, BOUNDS.min[1]]);
        point_close(tapered.corners[1], [BOUNDS.max[0] - 15.0, BOUNDS.min[1]]);
    }

    #[test]
    fn envelope_cage_preserves_identity_and_bends_straight_rect_into_cubic_curves() {
        let cage = EnvelopeCage::from_bounds(BOUNDS);
        point_close(cage.eval(0.0, 0.0), BOUNDS.min);
        point_close(cage.eval(1.0, 1.0), BOUNDS.max);
        point_close(cage.eval(0.5, 0.5), BOUNDS.center());

        let warped = cage
            .dragged(
                EnvelopeHandle::Anchor(1),
                cage.anchors[1],
                [cage.anchors[1][0], cage.anchors[1][1] - 20.0],
            )
            .unwrap();
        point_close(
            warped.eval(0.5, 0.0),
            [cage.anchors[1][0], cage.anchors[1][1] - 20.0],
        );

        let rect = Element {
            id: "rect".to_owned(),
            name: "Rect".to_owned(),
            transform: Transform {
                x: BOUNDS.min[0],
                y: BOUNDS.min[1],
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: BOUNDS.max[0] - BOUNDS.min[0],
                height: BOUNDS.max[1] - BOUNDS.min[1],
                fill: [255, 204, 0, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        let deformed =
            deform_element_envelope(&rect, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &warped).unwrap();
        let ElementKind::Path { commands, .. } = deformed.kind else {
            panic!("expected path");
        };
        assert!(
            commands
                .iter()
                .any(|cmd| matches!(cmd, PathCommand::CubicTo { .. }))
        );
    }
}
