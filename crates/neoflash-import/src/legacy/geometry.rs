//! Versioned binary shape records, retained without a rendering interpretation.
//!
//! Integers and flags are archive values. In particular, this module does not
//! invent fill topology, alpha rules, pixel units, or procedural stroke output.
//! Format references and provenance are documented in docs/IMPORTS.md.
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ITEMS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryShapeRecord {
    pub shape_revision: u8,
    pub matrix_words: [i32; 6],
    pub data_revision: u8,
    /// An archive hint, not a trusted allocation size or an edge terminator.
    pub metadata: u32,
    pub fills: Vec<BinaryFill>,
    pub strokes: Vec<BinaryStroke>,
    pub edges: Vec<BinaryEdge>,
    pub auxiliary: Vec<BinaryAuxiliaryCurve>,
    /// Exact record bytes, including flags whose visual semantics are unresolved.
    pub original_bytes: Vec<u8>,
}

impl BinaryShapeRecord {
    pub(super) fn item_count(&self) -> usize {
        1 + self.edges.len()
            + self.fills.iter().map(BinaryFill::item_count).sum::<usize>()
            + self
                .strokes
                .iter()
                .map(|s| 1 + s.extension.as_ref().map_or(0, |e| e.paint.item_count()))
                .sum::<usize>()
            + self.auxiliary.len()
            + self
                .auxiliary
                .iter()
                .filter_map(|c| c.entries.as_ref())
                .map(Vec::len)
                .sum::<usize>()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatedBinaryShape {
    /// CPicShape body offset inside the containing stream, after its CPicObj base.
    pub offset: usize,
    pub record: BinaryShapeRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryFill {
    pub packed_value: u32,
    pub encoding: BinaryFillEncoding,
}
impl BinaryFill {
    fn item_count(&self) -> usize {
        1 + match &self.encoding {
            BinaryFillEncoding::Extended {
                payload: BinaryFillPayload::Gradient { stops, .. },
                ..
            } => stops.len(),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding", rename_all = "snake_case")]
pub enum BinaryFillEncoding {
    Legacy {
        word: u16,
    },
    Extended {
        flags: u8,
        secondary_flags: u8,
        payload: BinaryFillPayload,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BinaryFillPayload {
    Plain,
    Gradient {
        matrix_words: [i32; 6],
        extension: Option<BinaryGradientExtension>,
        stops: Vec<BinaryGradientStop>,
    },
    /// Known byte layout, unresolved paint semantics. Never treated as solid.
    Unresolved {
        matrix_words: [i32; 6],
        value: u32,
        words: [u16; 4],
    },
    Bitmap {
        matrix_words: [i32; 6],
        media_id: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryGradientExtension {
    pub focal_point: i32,
    pub flags: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryGradientStop {
    pub ratio: u8,
    pub color_word: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryStroke {
    pub color_word: u32,
    pub width_word: u16,
    pub style_words: [u16; 2],
    pub extension: Option<BinaryStrokeExtension>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryStrokeExtension {
    pub pixel_hinting: u8,
    pub scale_mode: u8,
    pub cap_style: u8,
    pub join_style: u8,
    pub miter_limit_word: u16,
    pub paint: BinaryFill,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryContourStyles {
    /// Zero means no style; bit 15 is contour selection, not a table index.
    pub stroke: u16,
    pub fill_0: u16,
    pub fill_1: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryEdge {
    pub encoding_flags: u8,
    pub styles: BinaryContourStyles,
    pub start_delta: [i32; 2],
    pub control_delta: [i32; 2],
    pub end_delta: [i32; 2],
    /// Absolute archive coordinates, using signed 32-bit wrapping addition.
    pub start: [i32; 2],
    pub control: Option<[i32; 2]>,
    pub end: [i32; 2],
    /// Present on straight edges from CPicShape revision 4 onwards.
    pub line_flags: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryAuxiliaryCurve {
    pub point_words: [i32; 8],
    pub entries: Option<Vec<BinaryAuxiliaryPoint>>,
    pub boundary_flags: Option<u8>,
    pub boundary_start: Option<[i32; 2]>,
    pub boundary_end: Option<[i32; 2]>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryAuxiliaryPoint {
    pub point: [i32; 2],
    pub control_flag: u8,
    pub value: u8,
}

/// Aggregate progress only; inspecting a private file does not expose geometry.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GeometryCounts {
    pub complete_shape_blocks: usize,
    pub nonempty_shape_blocks: usize,
    pub fills: usize,
    pub strokes: usize,
    pub straight_edges: usize,
    pub quadratic_edges: usize,
    pub auxiliary_curves: usize,
    pub unresolved_fill_payloads: usize,
}
impl GeometryCounts {
    pub(super) fn add(&mut self, shape: &BinaryShapeRecord) {
        self.complete_shape_blocks += 1;
        self.nonempty_shape_blocks += usize::from(!shape.edges.is_empty());
        self.fills += shape.fills.len();
        self.strokes += shape.strokes.len();
        self.quadratic_edges += shape.edges.iter().filter(|e| e.control.is_some()).count();
        self.straight_edges += shape.edges.iter().filter(|e| e.control.is_none()).count();
        self.auxiliary_curves += shape.auxiliary.len();
        self.unresolved_fill_payloads += shape
            .fills
            .iter()
            .chain(
                shape
                    .strokes
                    .iter()
                    .filter_map(|s| s.extension.as_ref().map(|e| &e.paint)),
            )
            .filter(|f| {
                matches!(
                    f.encoding,
                    BinaryFillEncoding::Extended {
                        payload: BinaryFillPayload::Unresolved { .. },
                        ..
                    }
                )
            })
            .count();
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{code} at shape byte {offset}: {message}")]
pub struct BinaryGeometryError {
    pub code: String,
    pub offset: usize,
    pub message: String,
}
type Result<T> = std::result::Result<T, BinaryGeometryError>;

/// Decode exactly one CPicShape body (revision + matrix + shape data).
/// The inherited CPicObj header and any CPicFrame fields are not part of this
/// slice. Trailing bytes are rejected. This is preservation, not project import.
pub fn decode_binary_shape(data: &[u8]) -> Result<BinaryShapeRecord> {
    let shape = decode_prefix(data)?;
    if shape.original_bytes.len() != data.len() {
        return Err(BinaryGeometryError {
            code: "fla_shape_trailing_data".into(),
            offset: shape.original_bytes.len(),
            message: "Bytes remain after the complete shape body.".into(),
        });
    }
    Ok(shape)
}

pub(super) fn decode_prefix(data: &[u8]) -> Result<BinaryShapeRecord> {
    let mut r = Cursor {
        data,
        pos: 0,
        items: 0,
    };
    if data.len() > MAX_BYTES {
        return r.fail("fla_shape_limit", "Shape input exceeds 64 MiB.");
    }
    let shape_revision = r.u8()?;
    if shape_revision > 6 {
        return r.fail(
            "fla_record_revision_unsupported",
            "Unsupported CPicShape revision.",
        );
    }
    let matrix_words = r.ints()?;
    let data_revision = r.u8()?;
    if data_revision > 5 {
        return r.fail(
            "fla_record_revision_unsupported",
            "Unsupported shape-data revision.",
        );
    }
    let metadata = r.u32()?;
    let fill_count = usize::from(r.u16()?);
    r.budget(fill_count, 6)?;
    let mut fills = Vec::with_capacity(fill_count);
    for _ in 0..fill_count {
        fills.push(if data_revision < 3 {
            BinaryFill {
                packed_value: r.u32()?,
                encoding: BinaryFillEncoding::Legacy { word: r.u16()? },
            }
        } else {
            r.fill(shape_revision)?
        });
    }
    let stroke_count = usize::from(r.u16()?);
    r.budget(stroke_count, if shape_revision >= 3 { 22 } else { 10 })?;
    let mut strokes = Vec::with_capacity(stroke_count);
    for _ in 0..stroke_count {
        let color_word = r.u32()?;
        let width_word = r.u16()?;
        let style_words = [r.u16()?, r.u16()?];
        let extension = if shape_revision >= 3 {
            Some(BinaryStrokeExtension {
                pixel_hinting: r.u8()?,
                scale_mode: r.u8()?,
                cap_style: r.u8()?,
                join_style: r.u8()?,
                miter_limit_word: r.u16()?,
                paint: r.fill(shape_revision)?,
            })
        } else {
            None
        };
        strokes.push(BinaryStroke {
            color_word,
            width_word,
            style_words,
            extension,
        });
    }
    let mut edges = Vec::new();
    let mut previous = [0_i32; 2];
    let mut styles = BinaryContourStyles::default();
    if data_revision >= 2 {
        loop {
            let flags = r.u8()?;
            if flags == 0 {
                break;
            }
            r.budget(1, 0)?;
            if flags & 0x40 != 0 {
                let mut indices = [0; 3];
                for v in &mut indices {
                    *v = if flags & 0x80 != 0 {
                        u16::from(r.u8()?)
                    } else {
                        r.u16()?
                    };
                }
                if usize::from(indices[0] & 0x7fff) > strokes.len()
                    || indices[1..]
                        .iter()
                        .any(|v| usize::from(v & 0x7fff) > fills.len())
                {
                    return r.fail(
                        "fla_shape_style_index",
                        "Edge refers to a missing shape-local style.",
                    );
                }
                styles = BinaryContourStyles {
                    stroke: indices[0],
                    fill_0: indices[1],
                    fill_1: indices[2],
                };
            }
            let start_delta = r.vector(flags & 3)?;
            let control_delta = r.vector((flags >> 2) & 3)?;
            let end_delta = r.vector((flags >> 4) & 3)?;
            let start = add(previous, start_delta);
            let control = (flags & 0x0c != 0).then(|| add(start, control_delta));
            let end = add(start, end_delta);
            let line_flags = if control.is_none() && shape_revision >= 4 {
                Some(r.u8()?)
            } else {
                None
            };
            edges.push(BinaryEdge {
                encoding_flags: flags,
                styles,
                start_delta,
                control_delta,
                end_delta,
                start,
                control,
                end,
                line_flags,
            });
            previous = end;
        }
    }
    let mut auxiliary = Vec::new();
    if data_revision >= 5 {
        let count = r.u32()? as usize;
        r.budget(
            count,
            32 + usize::from(shape_revision >= 5) + usize::from(shape_revision >= 6),
        )?;
        for _ in 0..count {
            let point_words = r.ints()?;
            let entries = if shape_revision >= 5 {
                let count = usize::from(r.u8()?);
                r.budget(count, 10)?;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    entries.push(BinaryAuxiliaryPoint {
                        point: r.ints()?,
                        control_flag: r.u8()?,
                        value: r.u8()?,
                    });
                }
                Some(entries)
            } else {
                None
            };
            let boundary_flags = if shape_revision >= 6 {
                Some(r.u8()?)
            } else {
                None
            };
            let boundary_start = if boundary_flags.is_some_and(|f| f & 1 != 0) {
                Some(r.ints()?)
            } else {
                None
            };
            let boundary_end = if boundary_flags.is_some_and(|f| f & 2 != 0) {
                Some(r.ints()?)
            } else {
                None
            };
            auxiliary.push(BinaryAuxiliaryCurve {
                point_words,
                entries,
                boundary_flags,
                boundary_start,
                boundary_end,
            });
        }
    }
    Ok(BinaryShapeRecord {
        shape_revision,
        matrix_words,
        data_revision,
        metadata,
        fills,
        strokes,
        edges,
        auxiliary,
        original_bytes: data[..r.pos].to_vec(),
    })
}

/// Morph style tables: counted fills then counted strokes, both in the
/// extended encoding without the revision >= 3 gradient/stroke extensions.
/// Returns the tables and the number of bytes consumed.
pub(super) fn decode_morph_styles(
    data: &[u8],
) -> Result<(Vec<BinaryFill>, Vec<BinaryStroke>, usize)> {
    let mut r = Cursor {
        data,
        pos: 0,
        items: 0,
    };
    if data.len() > MAX_BYTES {
        return r.fail("fla_shape_limit", "Morph style input exceeds 64 MiB.");
    }
    let fill_count = usize::from(r.u16()?);
    r.budget(fill_count, 6)?;
    let mut fills = Vec::with_capacity(fill_count);
    for _ in 0..fill_count {
        fills.push(r.fill(0)?);
    }
    let stroke_count = usize::from(r.u16()?);
    r.budget(stroke_count, 10)?;
    let mut strokes = Vec::with_capacity(stroke_count);
    for _ in 0..stroke_count {
        strokes.push(BinaryStroke {
            color_word: r.u32()?,
            width_word: r.u16()?,
            style_words: [r.u16()?, r.u16()?],
            extension: None,
        });
    }
    Ok((fills, strokes, r.pos))
}

fn add(a: [i32; 2], b: [i32; 2]) -> [i32; 2] {
    [a[0].wrapping_add(b[0]), a[1].wrapping_add(b[1])]
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    items: usize,
}
impl Cursor<'_> {
    fn fail<T>(&self, code: &str, message: &str) -> Result<T> {
        Err(BinaryGeometryError {
            code: code.into(),
            offset: self.pos,
            message: message.into(),
        })
    }
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        if N > self.data.len() - self.pos {
            return self.fail(
                "fla_truncated_record",
                "Truncated shape data; no boundary recovery attempted.",
            );
        }
        let v = self.data[self.pos..self.pos + N].try_into().unwrap();
        self.pos += N;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes()?))
    }
    fn ints<const N: usize>(&mut self) -> Result<[i32; N]> {
        let mut result = [0; N];
        for v in &mut result {
            *v = self.i32()?;
        }
        Ok(result)
    }
    fn budget(&mut self, count: usize, minimum_bytes: usize) -> Result<()> {
        if count > MAX_ITEMS - self.items {
            return self.fail(
                "fla_shape_limit",
                "Shape exceeds 100000 geometry/style items.",
            );
        }
        if count.saturating_mul(minimum_bytes) > self.data.len() - self.pos {
            return self.fail(
                "fla_truncated_record",
                "Shape collection exceeds remaining input.",
            );
        }
        self.items += count;
        Ok(())
    }
    fn vector(&mut self, mode: u8) -> Result<[i32; 2]> {
        let mut point = [0; 2];
        for v in &mut point {
            *v = match mode {
                0 => 0,
                2 => self.i32()?,
                _ => i32::from(i16::from_le_bytes(self.bytes()?)) << if mode == 3 { 7 } else { 0 },
            };
        }
        Ok(point)
    }
    fn fill(&mut self, shape_revision: u8) -> Result<BinaryFill> {
        let packed_value = self.u32()?;
        let flags = self.u8()?;
        let secondary_flags = self.u8()?;
        let payload = if flags & 0x10 != 0 {
            let matrix_words = self.ints()?;
            let count = usize::from(self.u8()?);
            if count > 15 {
                return self.fail(
                    "fla_gradient_stop_limit",
                    "Gradient exceeds native capacity of 15 stops.",
                );
            }
            let extension = if shape_revision >= 3 {
                Some(BinaryGradientExtension {
                    focal_point: self.i32()?,
                    flags: self.u32()?,
                })
            } else {
                None
            };
            self.budget(count, 5)?;
            let mut stops = Vec::with_capacity(count);
            for _ in 0..count {
                stops.push(BinaryGradientStop {
                    ratio: self.u8()?,
                    color_word: self.u32()?,
                });
            }
            BinaryFillPayload::Gradient {
                matrix_words,
                extension,
                stops,
            }
        } else if flags & 0x20 != 0 {
            BinaryFillPayload::Unresolved {
                matrix_words: self.ints()?,
                value: self.u32()?,
                words: [self.u16()?, self.u16()?, self.u16()?, self.u16()?],
            }
        } else if flags & 0x40 != 0 {
            BinaryFillPayload::Bitmap {
                matrix_words: self.ints()?,
                media_id: self.u16()?,
            }
        } else {
            BinaryFillPayload::Plain
        };
        Ok(BinaryFill {
            packed_value,
            encoding: BinaryFillEncoding::Extended {
                flags,
                secondary_flags,
                payload,
            },
        })
    }
}
