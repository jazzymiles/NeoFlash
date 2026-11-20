use serde::{Deserialize, Serialize};

use super::graphemes::utf16_byte;
use super::layout::measured;
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextQuery {
    FirstCharInParagraph { char_index: i64 },
    ParagraphLength { char_index: i64 },
    LineIndexAtPoint { x: f64, y: f64 },
    CharBoundaries { char_index: i64 },
    CharIndexAtPoint { x: f64, y: f64 },
    GetFormat { begin: i64, end: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextCharBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn paragraph_range(text: &str, char_index: i64) -> Option<(u32, u32)> {
    if char_index < 0 {
        return None;
    }
    let wanted = char_index as u64;
    let mut para_start = 0u32;
    let mut offset = 0u32;
    for ch in text.chars() {
        offset += ch.len_utf16() as u32;
        if ch == '\n' {
            if wanted < u64::from(offset) {
                return Some((para_start, offset - para_start));
            }
            para_start = offset;
        }
    }
    if wanted < u64::from(offset) {
        Some((para_start, offset - para_start))
    } else {
        None
    }
}

struct LineCharSpan {
    start: u32,
    end: u32,
    glyph_x: f64,
    glyph_width: f64,
    cell_left: f64,
    cell_right: f64,
}

fn line_char_spans<M>(
    field: &TextFieldState,
    line: &TextLine,
    measure: &mut M,
) -> Result<Vec<LineCharSpan>, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    if line.text.is_empty() {
        return Ok(Vec::new());
    }
    let source_slice = {
        let start_byte = utf16_byte(&field.text, line.start)?;
        let end_byte = utf16_byte(&field.text, line.end)?;
        &field.text[start_byte..end_byte]
    };
    let source_chars: Vec<char> = source_slice
        .chars()
        .filter(|ch| !(field.multiline && *ch == '\n'))
        .collect();
    if field.format.tab_stops.is_empty() {
        let mut prefixes = Vec::new();
        let mut current = String::new();
        for ch in &source_chars {
            if field.display_as_password {
                for _ in 0..ch.len_utf16() {
                    current.push('*');
                }
            } else {
                current.push(*ch);
            }
            prefixes.push(current.clone());
        }
        let widths = measured(&prefixes, &field.format, measure)?;
        let mut spans = Vec::with_capacity(source_chars.len());
        let mut offset = line.start;
        for (idx, ch) in source_chars.iter().enumerate() {
            let len = ch.len_utf16() as u32;
            let prev = if idx == 0 { 0.0 } else { widths[idx - 1] };
            let next = widths[idx];
            let gap = if idx == 0 {
                0.0
            } else {
                field.format.letter_spacing
            };
            let glyph_left = (prev + gap).clamp(0.0, next);
            spans.push(LineCharSpan {
                start: offset,
                end: offset + len,
                glyph_x: line.x + glyph_left,
                glyph_width: (next - glyph_left).max(0.0),
                cell_left: line.x + prev.min(next),
                cell_right: line.x + next.max(prev),
            });
            offset += len;
        }
        return Ok(spans);
    }
    // Custom tabStops: measure cumulative prefixes within each non-tab run.
    let mut prefixes = Vec::new();
    let mut run_prefix_counts = Vec::new();
    let mut run_chars: Vec<Vec<char>> = vec![Vec::new()];
    for ch in &source_chars {
        if *ch == '\t' {
            run_chars.push(Vec::new());
        } else {
            run_chars.last_mut().expect("active run").push(*ch);
        }
    }
    for chars in &run_chars {
        let mut current = String::new();
        for ch in chars {
            if field.display_as_password {
                for _ in 0..ch.len_utf16() {
                    current.push('*');
                }
            } else {
                current.push(*ch);
            }
            prefixes.push(current.clone());
        }
        run_prefix_counts.push(chars.len());
    }
    let widths = if prefixes.is_empty() {
        Vec::new()
    } else {
        measured(&prefixes, &field.format, measure)?
    };
    let mut spans = Vec::with_capacity(source_chars.len());
    let mut offset = line.start;
    let mut width_cursor = 0usize;
    let mut x = 0.0_f64;
    for (run_idx, chars) in run_chars.iter().enumerate() {
        let count = run_prefix_counts[run_idx];
        let slice = &widths[width_cursor..width_cursor + count];
        width_cursor += count;
        for (idx, ch) in chars.iter().enumerate() {
            let len = ch.len_utf16() as u32;
            let prev = if idx == 0 { 0.0 } else { slice[idx - 1] };
            let next = slice[idx];
            let gap = if idx == 0 {
                0.0
            } else {
                field.format.letter_spacing
            };
            let glyph_left = (prev + gap).clamp(0.0, next);
            spans.push(LineCharSpan {
                start: offset,
                end: offset + len,
                glyph_x: line.x + x + glyph_left,
                glyph_width: (next - glyph_left).max(0.0),
                cell_left: line.x + x + prev.min(next),
                cell_right: line.x + x + next.max(prev),
            });
            offset += len;
        }
        if let Some(&last_w) = slice.last() {
            x += last_w;
        }
        if run_idx + 1 < run_chars.len() {
            let next_stop = field
                .format
                .tab_stops
                .iter()
                .copied()
                .find(|stop| *stop > x)
                .unwrap_or_else(|| {
                    let origin = field.format.tab_stops.last().copied().unwrap_or(0.);
                    let step = field.format.size * 4.;
                    origin + ((x - origin) / step).floor().max(0.) * step + step
                });
            spans.push(LineCharSpan {
                start: offset,
                end: offset + 1,
                glyph_x: line.x + x,
                glyph_width: (next_stop - x).max(0.0),
                cell_left: line.x + x,
                cell_right: line.x + next_stop.max(x),
            });
            offset += 1;
            x = next_stop;
        }
    }
    Ok(spans)
}

/// Lay out a text field once and return each character's glyph bounding box in field coordinates.
pub fn layout_char_boxes<M>(
    field: &TextFieldState,
    mut measure: M,
) -> Result<Vec<(char, TextCharBox)>, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    let layout = layout_text(field, &mut measure)?;
    let mut out = Vec::new();
    for line in &layout.lines {
        let spans = line_char_spans(field, line, &mut measure)?;
        for span in spans {
            let start_byte = utf16_byte(&field.text, span.start)?;
            let end_byte = utf16_byte(&field.text, span.end)?;
            if let Some(ch) = field.text[start_byte..end_byte].chars().next() {
                out.push((
                    ch,
                    TextCharBox {
                        x: span.glyph_x,
                        y: line.y,
                        width: span.glyph_width,
                        height: line.height,
                    },
                ));
            }
        }
    }
    Ok(out)
}

pub fn query_text<M>(
    field: &TextFieldState,
    query: &TextQuery,
    mut measure: M,
) -> Result<serde_json::Value, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    field.validate()?;
    match *query {
        TextQuery::FirstCharInParagraph { char_index } => Ok(serde_json::json!(
            paragraph_range(&field.text, char_index).map_or(-1, |(start, _)| start as i32)
        )),
        TextQuery::ParagraphLength { char_index } => Ok(serde_json::json!(
            paragraph_range(&field.text, char_index).map_or(-1, |(_, len)| len as i32)
        )),
        TextQuery::LineIndexAtPoint { x, y } => {
            if !x.is_finite() || !y.is_finite() || field.text.is_empty() {
                return Ok(serde_json::json!(-1));
            }
            if let Some(box_size) = field.box_size
                && (x < 0.0 || y < 0.0 || x > box_size.width || y > box_size.height)
            {
                return Ok(serde_json::json!(-1));
            }
            let layout = layout_text(field, &mut measure)?;
            let row_advance = layout.line_height.max(0.0);
            for (idx, line) in layout.lines.iter().enumerate() {
                let bottom = line.y + row_advance.max(line.height);
                let left = field.box_size.map_or(line.x, |_| 2.0_f64.min(line.x));
                let right = field.box_size.map_or(line.x + line.width, |b| {
                    (b.width - 2.0).max(line.x + line.width)
                });
                if y >= line.y && y < bottom && x >= left && x <= right {
                    return Ok(serde_json::json!(idx as i32));
                }
            }
            Ok(serde_json::json!(-1))
        }
        TextQuery::CharBoundaries { char_index } => {
            let total_utf16 = field.text.encode_utf16().count() as i64;
            if char_index < 0 || char_index >= total_utf16 {
                return Ok(serde_json::Value::Null);
            }
            let target = char_index as u32;
            let layout = layout_text(field, &mut measure)?;
            let Some(line) = layout
                .lines
                .iter()
                .find(|line| target >= line.start && target < line.end)
            else {
                return Ok(serde_json::Value::Null);
            };
            let spans = line_char_spans(field, line, &mut measure)?;
            if let Some(span) = spans
                .iter()
                .find(|span| target >= span.start && target < span.end)
            {
                return Ok(serde_json::json!(TextCharBox {
                    x: span.glyph_x,
                    y: line.y,
                    width: span.glyph_width,
                    height: line.height,
                }));
            }
            // Trailing newline character on a multiline line.
            Ok(serde_json::json!(TextCharBox {
                x: line.x + line.width,
                y: line.y,
                width: 0.0,
                height: line.height,
            }))
        }
        TextQuery::CharIndexAtPoint { x, y } => {
            if !x.is_finite() || !y.is_finite() || field.text.is_empty() {
                return Ok(serde_json::json!(-1));
            }
            if let Some(box_size) = field.box_size
                && (x < 0.0 || y < 0.0 || x > box_size.width || y > box_size.height)
            {
                return Ok(serde_json::json!(-1));
            }
            let layout = layout_text(field, &mut measure)?;
            let row_advance = layout.line_height.max(0.0);
            for line in &layout.lines {
                let bottom = line.y + row_advance.max(line.height);
                if y >= line.y && y < bottom {
                    let spans = line_char_spans(field, line, &mut measure)?;
                    for (idx, span) in spans.iter().enumerate() {
                        let hit = if idx + 1 == spans.len() {
                            x >= span.cell_left && x <= span.cell_right
                        } else {
                            x >= span.cell_left && x < span.cell_right
                        };
                        if hit {
                            return Ok(serde_json::json!(span.start as i32));
                        }
                    }
                    return Ok(serde_json::json!(-1));
                }
            }
            Ok(serde_json::json!(-1))
        }
        TextQuery::GetFormat { begin, end } => field.merged_format_json(begin, end),
    }
}
