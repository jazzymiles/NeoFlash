//! Snapping math for stage gestures (NF2 ST-04): dragged and drawn points snap to ruler guides and
//! grid lines. Pure functions over timeline coordinates; tolerances arrive in stage units, already
//! converted from screen points by the caller, so snapping feels the same at every zoom.
//!
//! Rules (docs/NF2-RULERS-GUIDES-GRID.md "Snapping"): each axis snaps independently; a guide within
//! its tolerance wins over the grid; the nearest line wins; nothing snaps outside its tolerance.

/// Lines a gesture can snap to, in timeline coordinates.
/// Priority per axis: **1. Guides > 2. Objects > 3. Grid > 4. Pixels**.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Snapper {
    /// x of visible vertical guides.
    pub guides_x: Vec<f64>,
    /// y of visible horizontal guides.
    pub guides_y: Vec<f64>,
    /// Guide tolerance in timeline units for the x/y axes.  The Stage converts the
    /// screen-point tolerance through its current affine mapping before constructing
    /// the snapper, so non-uniformly scaled and skewed symbol contexts remain exact.
    pub guide_tolerance: [f64; 2],
    /// x of unselected object edges and centres on the current frame.
    pub objects_x: Vec<f64>,
    /// y of unselected object edges and centres on the current frame.
    pub objects_y: Vec<f64>,
    /// Object snap tolerance in timeline units for the x/y axes.
    pub object_tolerance: [f64; 2],
    /// Grid spacing (x, y) and per-axis tolerance in timeline units, when enabled.
    pub grid: Option<([f64; 2], [f64; 2])>,
    /// Per-axis pixel snap tolerance in timeline units (capped at `<= 0.5`).
    pub pixel_tolerance: Option<[f64; 2]>,
}

/// Outcome of snapping one point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Snapped {
    pub point: [f64; 2],
    /// Which axes moved onto a line.
    pub x: bool,
    pub y: bool,
}

impl Snapped {
    pub fn any(self) -> bool {
        self.x || self.y
    }
}

impl Snapper {
    /// No guides, objects, grid or pixels: snapping cannot change anything.
    pub fn is_empty(&self) -> bool {
        self.guides_x.is_empty()
            && self.guides_y.is_empty()
            && self.objects_x.is_empty()
            && self.objects_y.is_empty()
            && self.grid.is_none()
            && self.pixel_tolerance.is_none()
    }

    /// Offset that moves `value` onto the best line of one axis, if any is within tolerance.
    /// Priority: **1. Guides > 2. Objects > 3. Grid > 4. Pixels**.
    fn axis(
        &self,
        value: f64,
        guides: &[f64],
        objects: &[f64],
        axis: usize,
        multi_anchor: bool,
    ) -> Option<(u8, f64)> {
        if !value.is_finite() {
            return None;
        }
        let nearest = |candidates: &[f64], tol: f64| {
            candidates
                .iter()
                .map(|c| c - value)
                .filter(|d| d.is_finite() && d.abs() <= tol)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        };
        if let Some(d) = nearest(guides, self.guide_tolerance[axis]) {
            return Some((0, d));
        }
        if let Some(d) = nearest(objects, self.object_tolerance[axis]) {
            return Some((1, d));
        }
        if let Some((spacing, tolerance)) = self.grid {
            let s = spacing[axis];
            if s.is_finite() && s > 0.0 {
                let grid_tol = if multi_anchor {
                    (tolerance[axis] * 0.65).min(s * 0.16)
                } else {
                    tolerance[axis]
                };
                let d = (value / s).round() * s - value;
                if d.is_finite() && d.abs() <= grid_tol {
                    return Some((2, d));
                }
            }
        }
        if let Some(pixel_tol) = self.pixel_tolerance
            && pixel_tol[axis].is_finite()
            && pixel_tol[axis] >= 0.0
        {
            let d = value.round() - value;
            if d.is_finite() && d.abs() <= pixel_tol[axis] {
                return Some((3, d));
            }
        }
        None
    }

    pub fn snap_x(&self, x: f64) -> Option<f64> {
        self.axis(x, &self.guides_x, &self.objects_x, 0, false)
            .map(|(_, d)| x + d)
    }

    pub fn snap_y(&self, y: f64) -> Option<f64> {
        self.axis(y, &self.guides_y, &self.objects_y, 1, false)
            .map(|(_, d)| y + d)
    }

    /// True when any candidate coordinate on `axis` is within `factor * tolerance` of a discrete
    /// guide or object snap target (used to show the unsnapped approach ring near real targets
    /// without hovering across empty space or inside every periodic grid/pixel cell).
    pub fn near_discrete_axis(&self, values: &[f64], axis: usize, factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let (guides, objects) = if axis == 0 {
            (&self.guides_x, &self.objects_x)
        } else {
            (&self.guides_y, &self.objects_y)
        };
        let guide_limit = self.guide_tolerance[axis] * factor;
        let object_limit = self.object_tolerance[axis] * factor;
        values.iter().copied().filter(|v| v.is_finite()).any(|v| {
            (guide_limit > 0.0 && guides.iter().any(|g| (g - v).abs() <= guide_limit))
                || (object_limit > 0.0 && objects.iter().any(|o| (o - v).abs() <= object_limit))
        })
    }

    /// Snap one point (pen anchor, shape corner, path handle).
    pub fn point(&self, p: [f64; 2]) -> Snapped {
        let x = self.snap_x(p[0]);
        let y = self.snap_y(p[1]);
        Snapped {
            point: [x.unwrap_or(p[0]), y.unwrap_or(p[1])],
            x: x.is_some(),
            y: y.is_some(),
        }
    }

    /// Best correction for a moving selection whose candidate edges are `xs` / `ys` (left, centre,
    /// right; top, centre, bottom): per axis, preserve target priority across all candidates, then
    /// choose the smallest offset within that target family.
    /// Returns the offset and the candidate indices that snapped.
    pub fn offset(&self, xs: &[f64], ys: &[f64]) -> ([f64; 2], [Option<usize>; 2]) {
        let best = |values: &[f64], guides: &[f64], objects: &[f64], axis: usize| {
            let multi_anchor = values
                .first()
                .zip(values.last())
                .is_some_and(|(a, b)| values.len() > 1 && (*b - *a).abs() > 1e-6);
            values
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    self.axis(*v, guides, objects, axis, multi_anchor)
                        .map(|(priority, d)| (i, priority, d))
                })
                .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.abs().total_cmp(&b.2.abs())))
                .map(|(i, _, d)| (i, d))
        };
        let x = best(xs, &self.guides_x, &self.objects_x, 0);
        let y = best(ys, &self.guides_y, &self.objects_y, 1);
        (
            [x.map_or(0.0, |v| v.1), y.map_or(0.0, |v| v.1)],
            [x.map(|v| v.0), y.map(|v| v.0)],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapper() -> Snapper {
        Snapper {
            guides_x: vec![105.0],
            guides_y: vec![-30.0],
            guide_tolerance: [8.0; 2],
            grid: Some(([20.0, 10.0], [4.0; 2])),
            ..Default::default()
        }
    }

    #[test]
    fn grid_snaps_within_tolerance_per_axis() {
        let s = Snapper {
            grid: Some(([20.0, 20.0], [4.0; 2])),
            ..Default::default()
        };
        assert_eq!(
            s.point([43.0, 51.0]),
            Snapped {
                point: [40.0, 51.0],
                x: true,
                y: false
            }
        );
        assert_eq!(s.point([-21.0, 59.0]).point, [-20.0, 60.0]);
        assert!(!s.point([10.0, 10.0]).any());
    }

    #[test]
    fn guides_win_over_the_grid() {
        let s = snapper();
        // 100 is a grid line 1 away, the guide at 105 is 4 away: the guide wins.
        assert_eq!(s.snap_x(101.0), Some(105.0));
        // Outside guide tolerance, the grid still applies.
        assert_eq!(s.snap_x(141.0), Some(140.0));
        assert_eq!(s.snap_y(-36.0), Some(-30.0));
        assert_eq!(s.snap_y(12.0), Some(10.0));
        assert_eq!(s.snap_x(130.0), None);
    }

    #[test]
    fn tolerance_scales_with_zoom() {
        // 8 screen points at 200 % = 4 stage px; at 25 % = 32 stage px.
        let at = |zoom: f64| Snapper {
            guides_x: vec![100.0],
            guide_tolerance: [8.0 / zoom; 2],
            ..Default::default()
        };
        assert_eq!(at(2.0).snap_x(110.0), None);
        assert_eq!(at(2.0).snap_x(103.0), Some(100.0));
        assert_eq!(at(0.25).snap_x(130.0), Some(100.0));
    }

    #[test]
    fn moving_bounds_snap_the_nearest_edge() {
        let s = Snapper {
            guides_x: vec![200.0],
            guides_y: vec![],
            guide_tolerance: [6.0; 2],
            ..Default::default()
        };
        // Left 150, centre 175, right 197: the right edge is 3 from the guide.
        let (offset, which) = s.offset(&[150.0, 175.0, 197.0], &[0.0, 10.0, 20.0]);
        assert_eq!(offset, [3.0, 0.0]);
        assert_eq!(which, [Some(2), None]);
        assert!(Snapper::default().is_empty());

        // Priority applies to the selection as a whole, even when a different edge/centre is
        // already on a lower-priority target. Each axis still chooses independently.
        let mut s = Snapper {
            guides_x: vec![205.0],
            guide_tolerance: [8.0; 2],
            objects_x: vec![201.0],
            objects_y: vec![205.0],
            object_tolerance: [8.0; 2],
            grid: Some(([20.0, 20.0], [8.0; 2])),
            pixel_tolerance: Some([0.5; 2]),
            ..Default::default()
        };
        let candidates = [160.0, 181.0, 201.0];
        assert_eq!(
            s.offset(&candidates, &candidates),
            ([4.0, 4.0], [Some(2), Some(2)]),
            "guide/object targets beat another anchor already on the grid"
        );
        s.guides_x.clear();
        assert_eq!(s.offset(&candidates, &[]).1[0], Some(2));
        s.objects_x.clear();
        assert_eq!(
            s.offset(&[161.0, 181.0, 201.0], &[]).0[0],
            -1.0,
            "grid beats an anchor already on an integer pixel"
        );
        s.grid = None;
        assert_eq!(s.offset(&[161.25], &[]).0[0], -0.25);
    }

    #[test]
    fn degenerate_grid_never_snaps() {
        let s = Snapper {
            grid: Some(([0.0, f64::NAN], [10.0; 2])),
            ..Default::default()
        };
        assert!(!s.point([3.0, 3.0]).any());
    }

    #[test]
    fn multi_anchor_grid_drag_preserves_free_movement_while_point_bounds_keep_full_tolerance() {
        let s = Snapper {
            grid: Some(([20.0, 20.0], [4.5; 2])),
            ..Default::default()
        };
        // A 53 px wide box has left, centre, right spaced by 26.5 px across a 20 px grid.
        // At least part of every 20 px cell must remain unsnapped so dragging is never 100% sticky.
        let mut free_positions = 0;
        for step in 0..40 {
            let left = step as f64 * 0.5;
            let xs = [left, left + 26.5, left + 53.0];
            if s.offset(&xs, &[10.0, 10.0, 10.0]).1[0].is_none() {
                free_positions += 1;
            }
        }
        assert!(
            free_positions >= 6,
            "3-anchor box drag must leave smooth free-movement deadbands (got {free_positions}/40)"
        );

        // Degenerate 0x0 bounds (empty MovieClip registration point) have identical [x, x, x]
        // and therefore keep single-point grid tolerance (`4.0 <= 4.5`).
        let (offset, which) = s.offset(&[104.0, 104.0, 104.0], &[56.0, 56.0, 56.0]);
        assert_eq!(offset, [-4.0, 4.0]);
        assert_eq!(which, [Some(0), Some(0)]);
    }

    #[test]
    fn near_discrete_axis_ignores_grid_and_pixels_and_detects_guides_and_objects() {
        let s = Snapper {
            guides_x: vec![100.0],
            guide_tolerance: [4.5; 2],
            objects_y: vec![200.0],
            object_tolerance: [4.0; 2],
            grid: Some(([20.0, 20.0], [4.5; 2])),
            pixel_tolerance: Some([0.5; 2]),
            ..Default::default()
        };
        assert!(!s.near_discrete_axis(&[40.0], 0, 1.6));
        assert!(s.near_discrete_axis(&[93.0], 0, 1.6));
        assert!(!s.near_discrete_axis(&[90.0], 0, 1.6));
        assert!(s.near_discrete_axis(&[206.0], 1, 1.6));
        assert!(!s.near_discrete_axis(&[210.0], 1, 1.6));
    }
}
