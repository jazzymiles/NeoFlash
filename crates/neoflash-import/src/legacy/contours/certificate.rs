//! Conservative topology certificate. Integer/dyadic bounds can prove disjoint
//! curves; failure to prove within bounded work/depth is an error, never a snap.
use super::{Edge, Result, Work, at_edge, error};

type P = [i128; 2];
const SCALE: i128 = 1 << 24;
const MAX_DEPTH: u8 = 12;

#[derive(Clone, Copy)]
struct Bounds {
    min: P,
    max: P,
}
impl Bounds {
    fn of(points: &[P]) -> Self {
        let mut b = Self {
            min: points[0],
            max: points[0],
        };
        for p in &points[1..] {
            for (axis, &value) in p.iter().enumerate() {
                b.min[axis] = b.min[axis].min(value);
                b.max[axis] = b.max[axis].max(value);
            }
        }
        b
    }
    fn overlap(self, other: Self) -> Option<Self> {
        let b = Self {
            min: [self.min[0].max(other.min[0]), self.min[1].max(other.min[1])],
            max: [self.max[0].min(other.max[0]), self.max[1].min(other.max[1])],
        };
        (b.min[0] <= b.max[0] && b.min[1] <= b.max[1]).then_some(b)
    }
    fn contains(self, p: P) -> bool {
        (0..2).all(|i| self.min[i] <= p[i] && p[i] <= self.max[i])
    }
    fn size(self) -> i128 {
        (self.max[0] - self.min[0]).max(self.max[1] - self.min[1])
    }
}
#[derive(Clone, Copy)]
struct Piece {
    points: [P; 3],
    line: bool,
    depth: u8,
}
fn middle(a: P, b: P) -> P {
    [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]
}
fn cross(a: P, b: P, c: P) -> i128 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn determinant(a: P, b: P) -> i128 {
    a[0] * b[1] - a[1] * b[0]
}
fn scaled(p: [i32; 2]) -> P {
    p.map(|v| i128::from(v) * SCALE)
}
impl Piece {
    fn from_edge(edge: &Edge) -> Result<Self> {
        let a = scaled(edge.start);
        let b = scaled(edge.end);
        let c = edge.control.map(scaled).unwrap_or_else(|| middle(a, b));
        let collinear = cross(a, b, c) == 0;
        if collinear && !Bounds::of(&[a, b]).contains(c) {
            return Err(at_edge(
                error(
                    "fla_contour_degenerate",
                    "A collinear quadratic backtracks beyond its endpoints.",
                ),
                edge.original,
            ));
        }
        Ok(Self {
            points: [a, c, b],
            line: collinear,
            depth: 0,
        })
    }
    fn bounds(self) -> Bounds {
        Bounds::of(&self.points)
    }
    fn split(self) -> [Self; 2] {
        // At most 12 subdivisions consume the original 24 dyadic bits. All
        // midpoints remain exact integers; no float/epsilon decides topology.
        let a = middle(self.points[0], self.points[1]);
        let b = middle(self.points[1], self.points[2]);
        let c = middle(a, b);
        [
            Self {
                points: [self.points[0], a, c],
                depth: self.depth + 1,
                ..self
            },
            Self {
                points: [c, b, self.points[2]],
                depth: self.depth + 1,
                ..self
            },
        ]
    }
}

pub(super) fn certify(edges: &[Edge], cycles: &[Vec<usize>], work: &mut Work) -> Result<()> {
    let pieces = edges
        .iter()
        .map(Piece::from_edge)
        .collect::<Result<Vec<_>>>()?;
    let bounds = pieces.iter().map(|p| p.bounds()).collect::<Vec<_>>();
    let mut order = (0..edges.len()).collect::<Vec<_>>();
    order.sort_by_key(|&i| (bounds[i].min[0], i));
    for (position, &a) in order.iter().enumerate() {
        for &b in &order[position + 1..] {
            if bounds[b].min[0] > bounds[a].max[0] {
                break;
            }
            work.charge()?;
            if bounds[a].overlap(bounds[b]).is_none() {
                continue;
            }
            let common = [pieces[a].points[0], pieces[a].points[2]]
                .into_iter()
                .filter(|p| *p == pieces[b].points[0] || *p == pieces[b].points[2])
                .collect::<Vec<_>>();
            disjoint(pieces[a], pieces[b], &common, work)
                .map_err(|e| at_edge(e, edges[b].original))?;
        }
    }
    let mut signs = vec![];
    let mut cycle_bounds = vec![];
    for cycle in cycles {
        let mut area = 0i128;
        let mut points = vec![];
        for &index in cycle {
            let edge = edges[index];
            let a = edge.start.map(i128::from);
            let b = edge.end.map(i128::from);
            area += if let Some(c) = edge.control.map(|p| p.map(i128::from)) {
                2 * determinant(a, c) + 2 * determinant(c, b) + determinant(a, b)
            } else {
                3 * determinant(a, b)
            };
            points.extend(pieces[index].points);
        }
        if area == 0 {
            return Err(error(
                "fla_contour_degenerate",
                "A contour has zero signed area.",
            ));
        }
        signs.push(area.signum());
        cycle_bounds.push(Bounds::of(&points));
    }
    // Certify that nonzero and evenodd agree for every region. Boundary
    // disjointness makes one vertex enough to classify a whole inner contour.
    for (i, cycle) in cycles.iter().enumerate() {
        let query = pieces[cycle[0]].points[0];
        let mut outside = 0i128;
        for (j, other) in cycles.iter().enumerate() {
            if i == j {
                continue;
            }
            work.charge()?;
            if !cycle_bounds[j].contains(query) {
                continue;
            }
            let mut winding = 0i32;
            for &index in other {
                winding += ray_crossings(pieces[index], query, work)?;
            }
            if winding != 0 {
                outside += signs[j];
            }
        }
        if outside.abs() > 1 || (outside + signs[i]).abs() > 1 {
            return Err(error(
                "fla_contour_winding_unsupported",
                "Nested oriented contours disagree between nonzero and evenodd; fill semantics require further evidence.",
            ));
        }
    }
    Ok(())
}

fn disjoint(a: Piece, b: Piece, allowed: &[P], work: &mut Work) -> Result<()> {
    let mut pending = vec![(a, b)];
    while let Some((a, b)) = pending.pop() {
        work.charge()?;
        let Some(overlap) = a.bounds().overlap(b.bounds()) else {
            continue;
        };
        if overlap.min == overlap.max && allowed.contains(&overlap.min) {
            continue;
        }
        // A Bezier lies strictly inside a supporting half-plane for 0<t<1
        // unless every control point lies on its boundary. This proves shared
        // endpoints of a lens without endlessly subdividing touching boxes.
        if let Some(axis) = (0..2).find(|&axis| {
            let plane = overlap.min[axis];
            overlap.min[axis] == overlap.max[axis]
                && (plane == a.bounds().min[axis] || plane == a.bounds().max[axis])
                && (plane == b.bounds().min[axis] || plane == b.bounds().max[axis])
        }) {
            let plane = overlap.min[axis];
            let contacts = |piece: Piece| -> Option<Vec<P>> {
                if piece.points.iter().all(|p| p[axis] == plane) {
                    None
                } else {
                    Some(
                        [piece.points[0], piece.points[2]]
                            .into_iter()
                            .filter(|p| p[axis] == plane)
                            .collect(),
                    )
                }
            };
            let ac = contacts(a);
            let bc = contacts(b);
            if let Some(points) = ac.as_ref().or(bc.as_ref()) {
                let other = if ac.is_some() { b } else { a };
                let other_contacts = if ac.is_some() {
                    bc.as_ref()
                } else {
                    ac.as_ref()
                };
                if points.iter().any(|p| {
                    !allowed.contains(p)
                        && other.bounds().contains(*p)
                        && other_contacts.is_none_or(|values| values.contains(p))
                }) {
                    return Err(error(
                        "fla_contour_intersection",
                        "Boundaries touch away from a shared endpoint.",
                    ));
                }
                continue;
            }
        }
        if a.line && b.line {
            let [a0, _, a1] = a.points;
            let [b0, _, b1] = b.points;
            let ac = [cross(a0, a1, b0), cross(a0, a1, b1)];
            let bc = [cross(b0, b1, a0), cross(b0, b1, a1)];
            if ac[0].signum() * ac[1].signum() > 0 || bc[0].signum() * bc[1].signum() > 0 {
                continue;
            }
            // Non-collinear adjacent lines have exactly their shared endpoint.
            if !allowed.is_empty() && (ac[0] != 0 || ac[1] != 0) {
                continue;
            }
            return Err(error(
                "fla_contour_intersection",
                "Boundaries intersect, touch away from a shared endpoint, or overlap.",
            ));
        }
        let split_a = !a.line
            && (b.line
                || a.depth < MAX_DEPTH
                    && (b.depth == MAX_DEPTH || a.bounds().size() >= b.bounds().size()));
        let piece = if split_a { a } else { b };
        if piece.depth == MAX_DEPTH {
            return Err(error(
                "fla_contour_intersection_uncertain",
                "Curve separation could not be certified within 12 exact subdivisions.",
            ));
        }
        for half in piece.split() {
            pending.push(if split_a { (half, b) } else { (a, half) });
        }
    }
    Ok(())
}

fn ray_crossings(piece: Piece, query: P, work: &mut Work) -> Result<i32> {
    let mut pending = vec![piece];
    let mut total = 0;
    while let Some(piece) = pending.pop() {
        work.charge()?;
        let bounds = piece.bounds();
        if bounds.max[1] <= query[1] || bounds.min[1] > query[1] || bounds.max[0] <= query[0] {
            continue;
        }
        let [a, c, b] = piece.points;
        let direction = if a[1] <= query[1] && query[1] < b[1] {
            1
        } else if b[1] <= query[1] && query[1] < a[1] {
            -1
        } else {
            0
        };
        if piece.line {
            let side = cross(a, b, query);
            if direction > 0 && side > 0 || direction < 0 && side < 0 {
                total += direction;
            }
        } else if bounds.min[0] > query[0] && c[1] >= a[1].min(b[1]) && c[1] <= a[1].max(b[1]) {
            total += direction;
        } else {
            if piece.depth == MAX_DEPTH {
                return Err(error(
                    "fla_contour_nesting_uncertain",
                    "Contour nesting could not be certified within 12 exact subdivisions.",
                ));
            }
            pending.extend(piece.split());
        }
    }
    Ok(total)
}
