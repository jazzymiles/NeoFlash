//! AS2 static TextSnapshot: captured text/metrics, owned selection, live paint.
//! Chunk/range/search semantics adapt Ruffle MIT v0.7.1 display_object/text.rs.
use super::*;
use serde_json::{Value, json};
const MAX_UNITS: usize = 16_384;
#[derive(Clone)]
pub(super) struct SnapshotGlyph {
    pub source: ObjectRef,
    pub revision: String,
    pub unit: u16,
    pub local: [f64; 4],
    pub matrix: [f64; 6],
    pub font: String,
    pub color: u32,
    pub height: f64,
    pub selected: bool,
    pub continuation: bool,
}
pub(super) struct TextSnapshot {
    pub glyphs: Vec<SnapshotGlyph>,
    pub boundaries: BTreeSet<usize>,
    pub color: u32,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Count,
    GetText {
        start: i64,
        end: Option<i64>,
        #[serde(default)]
        include_line_endings: bool,
    },
    Find {
        start: i64,
        query: Vec<u16>,
        case_sensitive: bool,
    },
    SetSelected {
        start: i64,
        end: i64,
        selected: bool,
    },
    GetSelected {
        start: i64,
        end: Option<i64>,
    },
    SelectedText {
        #[serde(default)]
        include_line_endings: bool,
    },
    SelectColor {
        color: u32,
    },
    RunInfo {
        start: i64,
        end: i64,
    },
    HitTest {
        x: f64,
        y: f64,
        #[serde(default)]
        distance: f64,
    },
}
fn range(count: usize, start: i64, end: Option<i64>) -> std::ops::Range<usize> {
    if count == 0 {
        return 0..0;
    }
    let start = (start.max(0) as usize).min(count - 1);
    let end = (end.unwrap_or(count as i64).max(0) as usize)
        .min(count)
        .max(start + 1);
    start..end
}
fn point(matrix: [f64; 6], x: f64, y: f64) -> [f64; 2] {
    [
        matrix[0] * x + matrix[2] * y + matrix[4],
        matrix[1] * x + matrix[3] * y + matrix[5],
    ]
}
impl TextSnapshot {
    fn text(&self, range: std::ops::Range<usize>, selected: bool, newlines: bool) -> Vec<u16> {
        let mut result = vec![];
        let mut previous_chunk = None;
        for index in range {
            let glyph = &self.glyphs[index];
            if selected && !glyph.selected {
                continue;
            }
            let chunk = self
                .boundaries
                .range(..=index)
                .next_back()
                .copied()
                .unwrap_or(0);
            if newlines
                && !result.is_empty()
                && previous_chunk.is_some_and(|previous| previous != chunk)
            {
                result.push(10);
            }
            result.push(glyph.unit);
            previous_chunk = Some(chunk);
        }
        result
    }
    fn execute(&mut self, command: Command) -> Result<Value, DisplayError> {
        let count = self.glyphs.len();
        match command {
            Command::Count => Ok(json!(count)),
            Command::GetText {
                start,
                end,
                include_line_endings,
            } => Ok(json!({"units":self.text(range(count,start,end),false,include_line_endings)})),
            Command::SelectedText {
                include_line_endings,
            } => Ok(json!({"units":self.text(0..count,true,include_line_endings)})),
            Command::Find {
                start,
                mut query,
                case_sensitive,
            } => {
                if query.len() > MAX_UNITS {
                    return Err(error("TextSnapshot search exceeds 16384 UTF-16 units"));
                }
                if start < 0 || start as usize >= count || query.is_empty() {
                    return Ok(json!(-1));
                }
                let mut haystack: Vec<_> = self.glyphs[start as usize..]
                    .iter()
                    .map(|g| g.unit)
                    .collect();
                if !case_sensitive {
                    for unit in haystack.iter_mut().chain(query.iter_mut()) {
                        if (65..=90).contains(unit) {
                            *unit += 32;
                        }
                    }
                }
                let found = haystack
                    .windows(query.len())
                    .position(|window| window == query)
                    .map_or(-1, |index| start + index as i64);
                Ok(json!(found))
            }
            Command::SetSelected {
                start,
                end,
                selected,
            } => {
                for index in range(count, start, Some(end)) {
                    self.glyphs[index].selected = selected;
                }
                Ok(Value::Null)
            }
            Command::GetSelected { start, end } => Ok(json!(
                range(count, start, end).any(|index| self.glyphs[index].selected)
            )),
            Command::SelectColor { color } => {
                if color > 0xffffff {
                    return Err(error("TextSnapshot selection color requires 24-bit RGB"));
                }
                self.color = color;
                Ok(Value::Null)
            }
            Command::RunInfo { start, end } => {
                let range = range(count, start, Some(end));
                if range.len() > 4096 {
                    return Err(error(
                        "TextSnapshot.getTextRunInfo supports at most 4096 characters per request",
                    ));
                }
                Ok(Value::Array(range.map(|index|{let g=&self.glyphs[index];let [x,y,w,h]=g.local;let matrix=affine_multiply(g.matrix,[1.,0.,0.,1.,x,y]);let p0=point(g.matrix,x,y);let p1=point(g.matrix,x+w,y);let p2=point(g.matrix,x+w,y+h);let p3=point(g.matrix,x,y+h);json!({"indexInRun":index,"selected":g.selected,"font":g.font,"color":g.color,"height":g.height,"matrix_a":matrix[0],"matrix_b":matrix[1],"matrix_c":matrix[2],"matrix_d":matrix[3],"matrix_tx":matrix[4],"matrix_ty":matrix[5],"corner0x":p0[0],"corner0y":p0[1],"corner1x":p1[0],"corner1y":p1[1],"corner2x":p2[0],"corner2y":p2[1],"corner3x":p3[0],"corner3y":p3[1]})}).collect()))
            }
            Command::HitTest { x, y, distance } => {
                if ![x, y, distance].iter().all(|v| v.is_finite()) || distance < 0. {
                    return Err(error(
                        "TextSnapshot hit test requires finite coordinates and nonnegative distance",
                    ));
                }
                let mut found = -1;
                let mut nearest = f64::INFINITY;
                for (index, g) in self.glyphs.iter().enumerate() {
                    if g.continuation {
                        continue;
                    }
                    let [a, b, c, d, tx, ty] = g.matrix;
                    let determinant = a * d - b * c;
                    if determinant.abs() < 1e-12 {
                        continue;
                    }
                    let local = [
                        (d * (x - tx) - c * (y - ty)) / determinant,
                        (-b * (x - tx) + a * (y - ty)) / determinant,
                    ];
                    let [gx, gy, w, h] = g.local;
                    if local[0] >= gx && local[0] <= gx + w && local[1] >= gy && local[1] <= gy + h
                    {
                        return Ok(json!(index));
                    }
                    if distance > 0. {
                        let center = point(g.matrix, gx + w / 2., gy + h / 2.);
                        let delta = (center[0] - x).hypot(center[1] - y);
                        if delta <= distance && delta < nearest {
                            nearest = delta;
                            found = index as i64;
                        }
                    }
                }
                Ok(json!(found))
            }
        }
    }
}
impl Player {
    pub fn as2_snapshot_create<M>(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        mut measure: M,
    ) -> Result<String, DisplayError>
    where
        M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
    {
        if !self.graph.alive(target) {
            return Err(error("TextSnapshot target is stale or foreign"));
        }
        if self.graph.as2_snapshots.len() >= 64 {
            return Err(error(
                "Publication supports at most 64 TextSnapshot captures before reset",
            ));
        }
        let scene = self.runtime_snapshot(project);
        let mut glyphs = vec![];
        let mut boundaries = BTreeSet::new();
        // AVM1 captures direct static-text children, not nested/dynamic fields.
        for reference in self.graph.children(target) {
            let Some(node) = scene
                .objects
                .iter()
                .find(|node| node.object_ref == reference)
            else {
                continue;
            };
            if !matches!(node.display.kind, Some(ElementKind::Text { .. })) {
                continue;
            }
            let Some(field) = &node.text else {
                continue;
            };
            if glyphs.len() + field.text.encode_utf16().count() > MAX_UNITS {
                return Err(error("TextSnapshot capture exceeds 16384 UTF-16 units"));
            }
            boundaries.insert(glyphs.len());
            let source_units: Vec<_> = field.text.encode_utf16().collect();
            let layout = layout_text(field, &mut measure).map_err(|e| error(&e.to_string()))?;
            let chars: BTreeSet<_> = field.text.chars().filter(|ch| *ch != '\n').collect();
            let strings: Vec<_> = chars.iter().map(char::to_string).collect();
            let widths = measure(&strings, &field.format).map_err(|e| error(&e.to_string()))?;
            if widths.len() != strings.len()
                || widths.iter().any(|v| !v.is_finite() || *v < 0. || *v > 1e9)
            {
                return Err(error(
                    "TextSnapshot font measurement returned invalid widths",
                ));
            }
            let advances: HashMap<_, _> = chars.into_iter().zip(widths).collect();
            let matrix = node.content_local_matrix;
            for line in layout.lines {
                let sum: f64 = line
                    .text
                    .chars()
                    .map(|ch| advances.get(&ch).copied().unwrap_or(0.))
                    .sum();
                let scale = if sum > 0. { line.width / sum } else { 1. };
                let mut x = line.x;
                for ch in line.text.chars() {
                    let width = advances.get(&ch).copied().unwrap_or(0.) * scale;
                    let mut units = [0; 2];
                    for (index, unit) in ch.encode_utf16(&mut units).iter().enumerate() {
                        glyphs.push(SnapshotGlyph {
                            source: reference.clone(),
                            revision: field.revision.clone(),
                            unit: *unit,
                            local: [x, line.y, width, line.height],
                            matrix,
                            font: field.format.font.clone(),
                            color: (u32::from(field.alpha) << 24) | field.format.color,
                            height: field.format.size,
                            selected: false,
                            continuation: index > 0,
                        });
                    }
                    x += width;
                }
                // Layout consumes explicit newlines but omits them from line.text.
                if line.end > line.start
                    && source_units.get(line.end.saturating_sub(1) as usize) == Some(&10)
                {
                    glyphs.push(SnapshotGlyph {
                        source: reference.clone(),
                        revision: field.revision.clone(),
                        unit: 10,
                        local: [x, line.y, 0., line.height],
                        matrix,
                        font: field.format.font.clone(),
                        color: (u32::from(field.alpha) << 24) | field.format.color,
                        height: field.format.size,
                        selected: false,
                        continuation: false,
                    });
                }
            }
        }
        let total: usize = self
            .graph
            .as2_snapshots
            .values()
            .map(|snapshot| snapshot.glyphs.len())
            .sum();
        if total + glyphs.len() > 65_536 {
            return Err(error(
                "Publication TextSnapshot storage exceeds 65536 UTF-16 units",
            ));
        }
        self.graph.as2_snapshot_next = self
            .graph
            .as2_snapshot_next
            .checked_add(1)
            .ok_or_else(|| error("TextSnapshot identity exhausted"))?;
        let handle = format!(
            "{}:{:016}",
            self.graph.generation, self.graph.as2_snapshot_next
        );
        self.graph.as2_snapshots.insert(
            handle.clone(),
            TextSnapshot {
                glyphs,
                boundaries,
                color: 0xffff00,
            },
        );
        Ok(handle)
    }
    pub fn as2_snapshot_command(
        &mut self,
        handle: &str,
        request: Value,
    ) -> Result<Value, DisplayError> {
        let command: Command = serde_json::from_value(request)
            .map_err(|e| error(&format!("Invalid TextSnapshot request: {e}")))?;
        let changes_selection = matches!(
            command,
            Command::SetSelected { .. } | Command::SelectColor { .. }
        );
        let snapshot = self
            .graph
            .as2_snapshots
            .get_mut(handle)
            .ok_or_else(|| error("TextSnapshot was reset or belongs to another publication"))?;
        let result = snapshot.execute(command)?;
        if changes_selection {
            for glyph in &snapshot.glyphs {
                self.graph
                    .as2_snapshot_selection
                    .insert(glyph.source.clone(), handle.into());
            }
        }
        Ok(result)
    }
}
impl RuntimeGraph {
    pub(super) fn as2_selection_rectangles(
        &self,
        source: &ObjectRef,
        revision: &str,
    ) -> Vec<([f64; 4], u32)> {
        let Some(handle) = self.as2_snapshot_selection.get(source) else {
            return vec![];
        };
        let Some(snapshot) = self.as2_snapshots.get(handle) else {
            return vec![];
        };
        snapshot
            .glyphs
            .iter()
            .filter(|glyph| {
                &glyph.source == source
                    && glyph.revision == revision
                    && glyph.selected
                    && !glyph.continuation
                    && glyph.local[2] > 0.
            })
            .map(|glyph| (glyph.local, snapshot.color))
            .collect()
    }
}
