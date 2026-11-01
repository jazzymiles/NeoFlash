//! Snapshot-based affine gestures. Points and bounds use the edited symbol's coordinates.
//!
//! A single resize works in the placement's own axes, including skew; it never decomposes a
//! world AABB. Multi-selection scales only uniformly and rotates, which composes with every
//! existing affine pose without changing its skew. Edge skew (`skew_many`) is the one gesture
//! that shears in stage space and decomposes the result into scale, rotation and skew.

use neoflash_core::{Transform, transform_matrix};

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
        bounds.center()
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
    if !finite_point(pivot) {
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
pub(crate) fn uniform_scale_many(
    initial: &[Transform],
    pivot: Point,
    press: Point,
    current: Point,
) -> Option<Vec<Transform>> {
    if initial.is_empty()
        || !finite_point(pivot)
        || initial
            .iter()
            .any(|t| !valid_transform(*t) || !nonsingular(*t))
    {
        return None;
    }
    let factor = projected_ratio(subtract(press, pivot), subtract(current, pivot))?;
    if press == current {
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

/// Free Transform edge drag: shear every placement in stage space. `horizontal` drags a top or
/// bottom edge sideways (x shear), otherwise a left or right edge vertically (y shear). The line at
/// `anchor` (stage y for x shear, stage x for y shear) stays fixed; the dragged `edge` moves by
/// `displacement`. The sheared matrix is decomposed into scale, rotation and the single skew angle,
/// with rotation and skew unwrapped to the turn nearest the original so tweens keep their direction.
pub(crate) fn skew_many(
    initial: &[Transform],
    horizontal: bool,
    edge: f64,
    anchor: f64,
    displacement: f64,
) -> Option<Vec<Transform>> {
    let span = edge - anchor;
    if initial.is_empty()
        || !span.is_finite()
        || span.abs() <= EPSILON
        || !displacement.is_finite()
        || initial
            .iter()
            .any(|t| !valid_transform(*t) || !nonsingular(*t))
    {
        return None;
    }
    if displacement == 0.0 {
        return Some(initial.to_vec());
    }
    let k = displacement / span;
    let shear = if horizontal {
        [1.0, 0.0, k, 1.0, -k * anchor, 0.0]
    } else {
        [1.0, k, 0.0, 1.0, 0.0, -k * anchor]
    };
    initial
        .iter()
        .map(|&t| {
            let matrix = neoflash_core::affine_multiply(shear, transform_matrix(t));
            let decomposed = Transform::from_matrix(matrix)?;
            let mut result = Transform {
                alpha: t.alpha,
                color_transform: t.color_transform,
                ..decomposed
            };
            result.rotation += 360.0 * ((t.rotation - result.rotation) / 360.0).round();
            result.skew += 360.0 * ((t.skew - result.skew) / 360.0).round();
            (valid_transform(result) && nonsingular(result)).then_some(result)
        })
        .collect()
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
}
