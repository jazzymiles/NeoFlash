use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;

use super::*;

pub fn normalize_text_and_runs(
    inserted: &str,
    inserted_runs: &[TextFormatRun],
) -> (String, Vec<TextFormatRun>) {
    if !inserted.contains('\r') {
        return (inserted.to_owned(), inserted_runs.to_vec());
    }
    if inserted_runs.is_empty() {
        return (normalize_text(inserted), Vec::new());
    }
    let chars: Vec<char> = inserted.chars().collect();
    let mut old_to_new = Vec::with_capacity(inserted.encode_utf16().count() + 1);
    let mut norm = String::with_capacity(inserted.len());
    let mut new_u16 = 0u32;
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        let u16_len = ch.len_utf16();
        for _ in 0..u16_len {
            old_to_new.push(new_u16);
        }
        if ch == '\r' {
            if i + 1 < chars.len() && chars[i + 1] == '\n' {
                // \r\n collapses to a single \n: map both \r and \n starts consistently.
                old_to_new.push(new_u16);
                norm.push('\n');
                new_u16 += 1;
                i += 2;
                continue;
            }
            norm.push('\n');
            new_u16 += 1;
        } else {
            norm.push(ch);
            new_u16 += u16_len as u32;
        }
        i += 1;
    }
    old_to_new.push(new_u16);
    let map_idx = |pos: u32| -> u32 { old_to_new.get(pos as usize).copied().unwrap_or(new_u16) };
    let mut mapped = Vec::with_capacity(inserted_runs.len());
    for r in inserted_runs {
        let s = map_idx(r.start);
        let e = map_idx(r.end);
        if s < e {
            mapped.push(TextFormatRun {
                start: s,
                end: e,
                format: r.format.clone(),
            });
        }
    }
    (norm, mapped)
}
pub(super) fn clamp_utf16(text: &str, index: i64) -> u32 {
    snap_utf16_to_grapheme_floor(text, index)
}

/// Returns all UAX #29 extended grapheme cluster boundaries in UTF-16 code-unit offsets,
/// always starting with `0` and ending with `text.encode_utf16().count() as u32`.
pub fn grapheme_utf16_boundaries(text: &str) -> Vec<u32> {
    let mut boundaries = Vec::new();
    boundaries.push(0);
    let mut utf16_offset = 0u32;
    for cluster in text.graphemes(true) {
        utf16_offset += cluster.encode_utf16().count() as u32;
        boundaries.push(utf16_offset);
    }
    boundaries
}

/// Iterates over UAX #29 extended grapheme clusters in `text`, yielding
/// `(start_utf16, end_utf16, cluster_str)`.
pub fn grapheme_clusters_utf16(text: &str) -> Vec<(u32, u32, &str)> {
    if text.is_empty() {
        return Vec::new();
    }
    let bounds = grapheme_utf16_boundaries(text);
    let mut out = Vec::with_capacity(bounds.len().saturating_sub(1));
    let mut cur_u16 = 0u32;
    let mut bound_idx = 1usize;
    let mut cluster_start_u16 = 0u32;
    let mut cluster_start_byte = 0usize;
    for (byte_idx, ch) in text.char_indices() {
        cur_u16 += ch.len_utf16() as u32;
        let cur_byte = byte_idx + ch.len_utf8();
        if bound_idx < bounds.len() && cur_u16 == bounds[bound_idx] {
            out.push((
                cluster_start_u16,
                cur_u16,
                &text[cluster_start_byte..cur_byte],
            ));
            cluster_start_u16 = cur_u16;
            cluster_start_byte = cur_byte;
            bound_idx += 1;
        }
    }
    if cluster_start_byte < text.len() {
        out.push((cluster_start_u16, cur_u16, &text[cluster_start_byte..]));
    }
    out
}

pub fn is_grapheme_boundary_utf16(text: &str, utf16_offset: u32) -> bool {
    grapheme_utf16_boundaries(text)
        .binary_search(&utf16_offset)
        .is_ok()
}

pub fn snap_utf16_to_grapheme_floor(text: &str, index: i64) -> u32 {
    if index <= 0 || text.is_empty() {
        return 0;
    }
    let bounds = grapheme_utf16_boundaries(text);
    let total = *bounds.last().unwrap_or(&0);
    if index >= i64::from(total) {
        return total;
    }
    let wanted = index as u32;
    match bounds.binary_search(&wanted) {
        Ok(_) => wanted,
        Err(pos) => bounds[pos.saturating_sub(1)],
    }
}

pub fn snap_utf16_to_grapheme_ceil(text: &str, index: i64) -> u32 {
    if index <= 0 || text.is_empty() {
        return 0;
    }
    let bounds = grapheme_utf16_boundaries(text);
    let total = *bounds.last().unwrap_or(&0);
    if index >= i64::from(total) {
        return total;
    }
    let wanted = index as u32;
    match bounds.binary_search(&wanted) {
        Ok(_) => wanted,
        Err(pos) => bounds.get(pos).copied().unwrap_or(total),
    }
}

pub fn prev_grapheme_utf16(text: &str, offset: usize) -> usize {
    if offset == 0 || text.is_empty() {
        return 0;
    }
    let bounds = grapheme_utf16_boundaries(text);
    let clamped = offset.min(*bounds.last().unwrap_or(&0) as usize) as u32;
    match bounds.binary_search(&clamped) {
        Ok(0) => 0,
        Ok(idx) => bounds[idx - 1] as usize,
        Err(idx) => bounds[idx.saturating_sub(1)] as usize,
    }
}

pub fn next_grapheme_utf16(text: &str, offset: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    let bounds = grapheme_utf16_boundaries(text);
    let total = *bounds.last().unwrap_or(&0);
    if offset >= total as usize {
        return total as usize;
    }
    let clamped = offset as u32;
    match bounds.binary_search(&clamped) {
        Ok(idx) => bounds.get(idx + 1).copied().unwrap_or(total) as usize,
        Err(idx) => bounds.get(idx).copied().unwrap_or(total) as usize,
    }
}

fn is_word_grapheme(cluster: &str) -> bool {
    cluster.chars().any(|ch| ch.is_alphanumeric() || ch == '_')
}

pub fn prev_word_utf16(text: &str, offset: usize) -> usize {
    if offset == 0 || text.is_empty() {
        return 0;
    }
    let clusters = grapheme_clusters_utf16(text);
    let clamped = snap_utf16_to_grapheme_floor(text, offset as i64);
    let mut i = clusters.partition_point(|(start, _, _)| *start < clamped);
    while i > 0 && clusters[i - 1].2.chars().all(char::is_whitespace) {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let target_word = is_word_grapheme(clusters[i - 1].2);
    while i > 0 {
        let prev = clusters[i - 1].2;
        if prev.chars().all(char::is_whitespace) || is_word_grapheme(prev) != target_word {
            break;
        }
        i -= 1;
    }
    clusters[i].0 as usize
}

pub fn next_word_utf16(text: &str, offset: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    let clusters = grapheme_clusters_utf16(text);
    let total = clusters
        .last()
        .map(|(_, end, _)| *end as usize)
        .unwrap_or(0);
    if offset >= total {
        return total;
    }
    let clamped = snap_utf16_to_grapheme_floor(text, offset as i64);
    let mut i = clusters.partition_point(|(start, _, _)| *start < clamped);
    while i < clusters.len() && clusters[i].2.chars().all(char::is_whitespace) {
        i += 1;
    }
    if i >= clusters.len() {
        return total;
    }
    let target_word = is_word_grapheme(clusters[i].2);
    while i < clusters.len() {
        let cur = clusters[i].2;
        if cur.chars().all(char::is_whitespace) || is_word_grapheme(cur) != target_word {
            break;
        }
        i += 1;
    }
    clusters
        .get(i)
        .map(|(s, _, _)| *s as usize)
        .unwrap_or(total)
}

pub fn word_bounds_at_utf16(text: &str, offset: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let clusters = grapheme_clusters_utf16(text);
    if clusters.is_empty() {
        return (0, 0);
    }
    let snapped = snap_utf16_to_grapheme_floor(text, offset as i64);
    let mut idx = clusters.partition_point(|(start, _, _)| *start < snapped);
    if idx >= clusters.len() {
        idx = clusters.len() - 1;
    }
    let cur = clusters[idx].2;
    let is_ws = cur.chars().all(char::is_whitespace);
    let word = is_word_grapheme(cur);
    let same_cat = |s: &str| {
        if is_ws {
            s.chars().all(char::is_whitespace)
        } else {
            !s.chars().all(char::is_whitespace) && is_word_grapheme(s) == word
        }
    };
    let mut start = idx;
    while start > 0 && same_cat(clusters[start - 1].2) {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < clusters.len() && same_cat(clusters[end].2) {
        end += 1;
    }
    (clusters[start].0 as usize, clusters[end - 1].1 as usize)
}

pub fn utf16_slice_safe(text: &str, start_utf16: usize, end_utf16: usize) -> &str {
    let lo = snap_utf16_to_grapheme_floor(text, start_utf16.min(end_utf16) as i64);
    let hi = snap_utf16_to_grapheme_ceil(text, start_utf16.max(end_utf16) as i64);
    match (utf16_byte(text, lo), utf16_byte(text, hi)) {
        (Ok(b0), Ok(b1)) if b0 <= b1 && b1 <= text.len() => &text[b0..b1],
        _ => "",
    }
}
/// Compiled change points keep membership O(log ranges), including long pasted
/// text. The common restrict grammar is exact Unicode ranges/toggles/escapes;
/// automatic legacy case substitution and malformed ranges are not guessed.
pub(super) struct Restriction {
    points: BTreeMap<u32, bool>,
}
impl Restriction {
    pub(super) fn compile(pattern: &str) -> Result<Self, TextError> {
        if pattern.len() > 4096 {
            return Err(error("restrict exceeds 4096 UTF-8 bytes"));
        }
        #[derive(Clone, Copy)]
        enum Token {
            Char(char),
            Toggle,
            Dash,
        }
        let mut tokens = vec![];
        let mut chars = pattern.chars();
        while let Some(ch) = chars.next() {
            tokens.push(match ch {
                '\\' => match chars.next() {
                    Some(value @ ('\\' | '^' | '-')) => Token::Char(value),
                    _ => {
                        return Err(error(
                            "restrict accepts only escaped backslash, caret or hyphen",
                        ));
                    }
                },
                '^' => Token::Toggle,
                '-' => Token::Dash,
                value => Token::Char(value),
            });
        }
        let mut result = Self {
            points: BTreeMap::from([
                (0, matches!(tokens.first(), Some(Token::Toggle))),
                (0x110000, false),
            ]),
        };
        let mut include = true;
        let mut i = 0;
        let mut rules = 0;
        while i < tokens.len() {
            let start = match tokens[i] {
                Token::Toggle => {
                    include = !include;
                    i += 1;
                    continue;
                }
                Token::Dash => '-',
                Token::Char(ch) => ch,
            };
            let mut end = start;
            if let (Some(Token::Dash), Some(Token::Char(last))) =
                (tokens.get(i + 1), tokens.get(i + 2))
            {
                end = *last;
                i += 2;
            }
            if start > end {
                return Err(error("restrict range endpoints must be ordered"));
            }
            rules += 1;
            if rules > 512 {
                return Err(error("restrict exceeds 512 ranges/characters"));
            }
            let start = start as u32;
            let end = end as u32 + 1;
            let after = *result
                .points
                .range(..=end)
                .next_back()
                .expect("initial point")
                .1;
            let keys: Vec<_> = result
                .points
                .range(start..end)
                .map(|(key, _)| *key)
                .collect();
            for key in keys {
                result.points.remove(&key);
            }
            result.points.insert(start, include);
            result.points.insert(end, after);
            i += 1;
        }
        Ok(result)
    }
    pub(super) fn allows(&self, ch: char) -> bool {
        *self
            .points
            .range(..=ch as u32)
            .next_back()
            .expect("initial point")
            .1
    }
}
pub(super) fn utf16_byte(text: &str, index: u32) -> Result<usize, TextError> {
    let mut units = 0u32;
    for (byte, ch) in text.char_indices() {
        if units == index {
            return Ok(byte);
        }
        units += ch.len_utf16() as u32;
        if units > index {
            return Err(error("UTF-16 range splits a surrogate pair"));
        }
    }
    if units == index {
        Ok(text.len())
    } else {
        Err(error("UTF-16 range is outside the text"))
    }
}
