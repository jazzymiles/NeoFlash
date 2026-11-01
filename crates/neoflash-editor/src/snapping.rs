//! Snapping math for stage gestures (NF2 ST-04): dragged and drawn points snap to ruler guides and
//! grid lines. Pure functions over timeline coordinates; tolerances arrive in stage units, already
//! converted from screen points by the caller, so snapping feels the same at every zoom.
//!
//! Rules (docs/NF2-RULERS-GUIDES-GRID.md "Snapping"): each axis snaps independently; a guide within
//! its tolerance wins over the grid; the nearest line wins; nothing snaps outside its tolerance.

/// Lines a gesture can snap to, in timeline coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Snapper {
    /// x of visible vertical guides.
    pub guides_x: Vec<f64>,
    /// y of visible horizontal guides.
    pub guides_y: Vec<f64>,
    /// Guide tolerance in stage units.
    pub guide_tolerance: f64,
    /// Grid spacing (x, y) and tolerance in stage units, when grid snapping is on.
    pub grid: Option<([f64; 2], f64)>,
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
    /// No guides and no grid: snapping cannot change anything.
    pub fn is_empty(&self) -> bool {
        self.guides_x.is_empty() && self.guides_y.is_empty() && self.grid.is_none()
    }

    /// Offset that moves `value` onto the best line of one axis, if any is within tolerance.
    fn axis(&self, value: f64, guides: &[f64], axis: usize) -> Option<f64> {
        let guide = guides
            .iter()
            .map(|g| g - value)
            .filter(|d| d.abs() <= self.guide_tolerance)
            .min_by(|a, b| a.abs().total_cmp(&b.abs()));
        guide.or_else(|| {
            let (spacing, tolerance) = self.grid?;
            let s = spacing[axis];
            if !(s.is_finite() && s > 0.0) {
                return None;
            }
            let d = (value / s).round() * s - value;
            (d.abs() <= tolerance).then_some(d)
        })
    }

    pub fn snap_x(&self, x: f64) -> Option<f64> {
        self.axis(x, &self.guides_x, 0).map(|d| x + d)
    }

    pub fn snap_y(&self, y: f64) -> Option<f64> {
        self.axis(y, &self.guides_y, 1).map(|d| y + d)
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
    /// right; top, centre, bottom): per axis, the smallest offset that puts any candidate on a line.
    /// Returns the offset and the candidate indices that snapped.
    pub fn offset(&self, xs: &[f64], ys: &[f64]) -> ([f64; 2], [Option<usize>; 2]) {
        let best = |values: &[f64], guides: &[f64], axis: usize| {
            values
                .iter()
                .enumerate()
                .filter_map(|(i, v)| self.axis(*v, guides, axis).map(|d| (i, d)))
                .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        };
        let x = best(xs, &self.guides_x, 0);
        let y = best(ys, &self.guides_y, 1);
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
            guide_tolerance: 8.0,
            grid: Some(([20.0, 10.0], 4.0)),
        }
    }

    #[test]
    fn grid_snaps_within_tolerance_per_axis() {
        let s = Snapper {
            grid: Some(([20.0, 20.0], 4.0)),
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
            guide_tolerance: 8.0 / zoom,
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
            guide_tolerance: 6.0,
            grid: None,
        };
        // Left 150, centre 175, right 197: the right edge is 3 from the guide.
        let (offset, which) = s.offset(&[150.0, 175.0, 197.0], &[0.0, 10.0, 20.0]);
        assert_eq!(offset, [3.0, 0.0]);
        assert_eq!(which, [Some(2), None]);
        assert!(Snapper::default().is_empty());
    }

    #[test]
    fn degenerate_grid_never_snaps() {
        let s = Snapper {
            grid: Some(([0.0, f64::NAN], 10.0)),
            ..Default::default()
        };
        assert!(!s.point([3.0, 3.0]).any());
    }
}
