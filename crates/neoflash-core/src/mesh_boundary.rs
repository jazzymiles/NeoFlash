//! Recover editable exterior contours from an already tessellated, non-overlapping fill.
use crate::{MAX_PATH_COMMANDS, PathCommand};
use std::collections::{BTreeMap, HashMap};

/// Removes shared triangle edges, retaining outer islands and holes with their winding.
/// Coordinate-identical mesh vertices are welded before counting edges. Invalid or non-manifold
/// meshes return `None`; output is deterministic and bounded by the ordinary path command limit.
pub fn fill_mesh_boundary(vertices: &[[f64; 2]], indices: &[u32]) -> Option<Vec<PathCommand>> {
    if !indices.len().is_multiple_of(3)
        || vertices.len() > MAX_PATH_COMMANDS
        || indices.len() > MAX_PATH_COMMANDS * 6
    {
        return None;
    }
    let mut intern = HashMap::new();
    let mut points = Vec::new();
    let mut canonical = Vec::with_capacity(vertices.len());
    for &point in vertices {
        if !point.iter().all(|value| value.is_finite()) {
            return None;
        }
        let key = point.map(|v| if v == 0. { 0 } else { v.to_bits() });
        let id = *intern.entry(key).or_insert_with(|| {
            points.push(point);
            points.len() - 1
        });
        canonical.push(id);
    }
    let mut edges = BTreeMap::<_, (usize, (usize, usize))>::new();
    for triangle in indices.as_chunks::<3>().0 {
        let [a, mut b, mut c] = [
            *canonical.get(triangle[0] as usize)?,
            *canonical.get(triangle[1] as usize)?,
            *canonical.get(triangle[2] as usize)?,
        ];
        let [p, q, r] = [points[a], points[b], points[c]];
        let area = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
        if area == 0. {
            continue;
        }
        if area < 0. {
            std::mem::swap(&mut b, &mut c);
        }
        for (from, to) in [(a, b), (b, c), (c, a)] {
            let entry = edges
                .entry((from.min(to), from.max(to)))
                .or_insert((0, (from, to)));
            entry.0 += 1;
            if entry.0 > 2 {
                return None;
            }
        }
    }
    let mut outgoing = BTreeMap::<usize, Vec<usize>>::new();
    for (_, (count, (from, to))) in edges {
        if count == 1 {
            outgoing.entry(from).or_default().push(to);
        }
    }
    let mut commands = Vec::new();
    while let Some((&start, _)) = outgoing.first_key_value() {
        let contour_start = commands.len();
        let [x, y] = points[start];
        commands.push(PathCommand::MoveTo { x, y });
        let mut at = start;
        let mut area = 0.;
        loop {
            let next = outgoing.get_mut(&at)?;
            let to = next.pop()?;
            if next.is_empty() {
                outgoing.remove(&at);
            }
            let [a, b] = [points[at], points[to]];
            area += (a[0] - x) * (b[1] - y) - (b[0] - x) * (a[1] - y);
            if to == start {
                break;
            }
            let [x, y] = points[to];
            commands.push(PathCommand::LineTo { x, y });
            if commands.len() >= MAX_PATH_COMMANDS {
                return None;
            }
            at = to;
        }
        // Tessellators may leave collinear zero-area cycles at tip intersections.
        // They enclose no fill and must not become visible interior strokes.
        if area == 0. {
            commands.truncate(contour_start);
            continue;
        }
        commands.push(PathCommand::Close);
        if commands.len() > MAX_PATH_COMMANDS {
            return None;
        }
    }
    crate::validate_path_commands(&commands).ok()?;
    Some(commands)
}
