//! Standard Source Map v3 generation, VLQ encoding/decoding, stack trace mapping,
//! and lightweight runtime debugging (`__nfDebugStep` / `__nfDebug`).

use neoflash_core::{Script, ScriptLanguage};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_char_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Encode raw bytes as standard Base64.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(BASE64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decode standard Base64 into raw bytes.
pub fn base64_decode(encoded: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = encoded
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if !clean.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity((clean.len() / 4) * 3);
    for chunk in clean.chunks(4) {
        let v0 = base64_char_value(chunk[0])? as u32;
        let v1 = base64_char_value(chunk[1])? as u32;
        let v2 = if chunk[2] == b'=' {
            0
        } else {
            base64_char_value(chunk[2])? as u32
        };
        let v3 = if chunk[3] == b'=' {
            0
        } else {
            base64_char_value(chunk[3])? as u32
        };
        let triple = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;
        out.push(((triple >> 16) & 0xFF) as u8);
        if chunk[2] != b'=' {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            out.push((triple & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Encode a single signed integer into Source Map v3 Base64 VLQ format.
pub fn encode_vlq(value: i64, out: &mut String) {
    let mut vlq = if value < 0 {
        ((-value) as u64) << 1 | 1
    } else {
        (value as u64) << 1
    };
    loop {
        let mut digit = (vlq & 0x1F) as u8;
        vlq >>= 5;
        if vlq > 0 {
            digit |= 0x20;
        }
        out.push(BASE64_CHARS[digit as usize] as char);
        if vlq == 0 {
            break;
        }
    }
}

/// Decode one signed integer from a Base64 VLQ slice, returning `(value, bytes_consumed)`.
pub fn decode_vlq(input: &str) -> Option<(i64, usize)> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    for (idx, byte) in input.bytes().enumerate() {
        let digit = base64_char_value(byte)? as u64;
        result |= (digit & 0x1F) << shift;
        shift += 5;
        if (digit & 0x20) == 0 {
            let is_negative = (result & 1) == 1;
            let magnitude = (result >> 1) as i64;
            let signed = if is_negative { -magnitude } else { magnitude };
            return Some((signed, idx + 1));
        }
    }
    None
}

/// A single decoded mapping segment (all indices 0-based, matching Source Map v3 spec).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMappingSegment {
    pub generated_line: usize,
    pub generated_column: usize,
    pub source_index: usize,
    pub original_line: usize,
    pub original_column: usize,
    pub name_index: Option<usize>,
}

/// A resolved 1-based source location in the author's original script.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginalLocation {
    pub source: String,
    /// 1-based line number in the original source.
    pub line: usize,
    /// 1-based column number in the original source.
    pub column: usize,
    pub name: Option<String>,
}

/// A stack frame mapped back to its original authored source location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappedStackFrame {
    pub function_name: Option<String>,
    pub source: String,
    /// 1-based original line.
    pub line: usize,
    /// 1-based original column.
    pub column: usize,
}

/// Decoded compiler mapping for hosts. Generated positions use JavaScript UTF-16 columns;
/// authored columns are Unicode characters, matching editor diagnostics. All positions are 1-based.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSourceLocation {
    pub generated_line: usize,
    pub generated_column: usize,
    pub source: String,
    pub line: usize,
    pub column: usize,
}

/// Standard Source Map v3 JSON structure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMap {
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(rename = "sourceRoot", default)]
    pub source_root: String,
    pub sources: Vec<String>,
    #[serde(rename = "sourcesContent", default)]
    pub sources_content: Vec<String>,
    pub names: Vec<String>,
    pub mappings: String,
}

impl SourceMap {
    /// Encode a list of 0-based `SourceMappingSegment`s into a Source Map v3 `mappings` string.
    pub fn from_segments(
        file: Option<String>,
        sources: Vec<String>,
        sources_content: Vec<String>,
        names: Vec<String>,
        mut segments: Vec<SourceMappingSegment>,
    ) -> Self {
        segments.sort_by_key(|s| (s.generated_line, s.generated_column));
        let max_line = segments.last().map_or(0, |s| s.generated_line);
        let mut mappings = String::new();
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;
        let mut prev_name: i64 = 0;
        let mut seg_idx = 0;

        for line in 0..=max_line {
            if line > 0 {
                mappings.push(';');
            }
            let mut prev_gen_col: i64 = 0;
            let mut first_in_line = true;
            while seg_idx < segments.len() && segments[seg_idx].generated_line == line {
                let seg = &segments[seg_idx];
                seg_idx += 1;
                if !first_in_line {
                    mappings.push(',');
                }
                first_in_line = false;

                encode_vlq(seg.generated_column as i64 - prev_gen_col, &mut mappings);
                prev_gen_col = seg.generated_column as i64;

                encode_vlq(seg.source_index as i64 - prev_source, &mut mappings);
                prev_source = seg.source_index as i64;

                encode_vlq(seg.original_line as i64 - prev_orig_line, &mut mappings);
                prev_orig_line = seg.original_line as i64;

                encode_vlq(seg.original_column as i64 - prev_orig_col, &mut mappings);
                prev_orig_col = seg.original_column as i64;

                if let Some(name_idx) = seg.name_index {
                    encode_vlq(name_idx as i64 - prev_name, &mut mappings);
                    prev_name = name_idx as i64;
                }
            }
        }

        Self {
            version: 3,
            file,
            source_root: String::new(),
            sources,
            sources_content,
            names,
            mappings,
        }
    }

    /// Decode VLQ `mappings` back into 0-based `SourceMappingSegment`s.
    pub fn decode_segments(&self) -> Vec<SourceMappingSegment> {
        let mut out = Vec::new();
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;
        let mut prev_name: i64 = 0;

        for (gen_line, line_str) in self.mappings.split(';').enumerate() {
            if line_str.is_empty() {
                continue;
            }
            let mut prev_gen_col: i64 = 0;
            for seg_str in line_str.split(',') {
                if seg_str.is_empty() {
                    continue;
                }
                let mut rest = seg_str;
                let Some((gen_col_delta, used)) = decode_vlq(rest) else {
                    break;
                };
                rest = &rest[used..];
                prev_gen_col += gen_col_delta;
                if rest.is_empty() {
                    continue;
                }
                let Some((src_delta, used)) = decode_vlq(rest) else {
                    break;
                };
                rest = &rest[used..];
                prev_source += src_delta;

                let Some((orig_line_delta, used)) = decode_vlq(rest) else {
                    break;
                };
                rest = &rest[used..];
                prev_orig_line += orig_line_delta;

                let Some((orig_col_delta, used)) = decode_vlq(rest) else {
                    break;
                };
                rest = &rest[used..];
                prev_orig_col += orig_col_delta;

                let name_index = if !rest.is_empty() {
                    if let Some((name_delta, _)) = decode_vlq(rest) {
                        prev_name += name_delta;
                        Some(prev_name.max(0) as usize)
                    } else {
                        None
                    }
                } else {
                    None
                };

                out.push(SourceMappingSegment {
                    generated_line: gen_line,
                    generated_column: prev_gen_col.max(0) as usize,
                    source_index: prev_source.max(0) as usize,
                    original_line: prev_orig_line.max(0) as usize,
                    original_column: prev_orig_col.max(0) as usize,
                    name_index,
                });
            }
        }
        out
    }

    /// Resolve a 1-based `(generated_line, generated_column)` to its 1-based `OriginalLocation`.
    pub fn lookup_original_position(
        &self,
        generated_line: usize,
        generated_column: usize,
    ) -> Option<OriginalLocation> {
        let target_line = generated_line.saturating_sub(1);
        let target_col = generated_column.saturating_sub(1);
        let segments = self.decode_segments();
        // Source Map v3 mappings never carry across generated lines. Compiler helpers
        // and wrapper code intentionally have no authored origin.
        let chosen = segments
            .iter()
            .rev()
            .find(|s| s.generated_line == target_line && s.generated_column <= target_col)?;
        let source = self
            .sources
            .get(chosen.source_index)
            .cloned()
            .unwrap_or_default();
        let name = chosen
            .name_index
            .and_then(|idx| self.names.get(idx).cloned());
        Some(OriginalLocation {
            source,
            line: chosen.original_line + 1,
            column: chosen.original_column + 1,
            name,
        })
    }

    pub fn runtime_locations(&self) -> Vec<RuntimeSourceLocation> {
        self.decode_segments()
            .into_iter()
            .filter_map(|segment| {
                let source = self.sources.get(segment.source_index)?.clone();
                let text = self
                    .sources_content
                    .get(segment.source_index)
                    .and_then(|text| text.lines().nth(segment.original_line))
                    .unwrap_or("");
                let mut units = 0;
                let column = text
                    .chars()
                    .take_while(|c| {
                        let keep = units < segment.original_column;
                        units += c.len_utf16();
                        keep
                    })
                    .count()
                    + 1;
                Some(RuntimeSourceLocation {
                    generated_line: segment.generated_line + 1,
                    generated_column: segment.generated_column + 1,
                    source,
                    line: segment.original_line + 1,
                    column,
                })
            })
            .collect()
    }

    /// Map a runtime JavaScript stack trace string back to authored source positions.
    pub fn map_stack_trace(&self, stack: &str) -> Vec<MappedStackFrame> {
        let mut frames = Vec::new();
        for raw_line in stack.lines() {
            let trimmed = raw_line.trim();
            if let Some((fn_name, gen_line, gen_col)) = parse_stack_line(trimmed)
                && let Some(orig) = self.lookup_original_position(gen_line, gen_col)
            {
                frames.push(MappedStackFrame {
                    function_name: orig.name.or(fn_name),
                    source: orig.source,
                    line: orig.line,
                    column: orig.column,
                });
            }
        }
        frames
    }

    /// Serialize this Source Map v3 as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("SourceMap serializes to valid JSON")
    }

    /// Base64-encoded JSON representation.
    pub fn to_base64(&self) -> String {
        base64_encode(self.to_json().as_bytes())
    }

    /// Data URL (`data:application/json;base64,...`) suitable for `sourceMappingURL`.
    pub fn to_data_url(&self) -> String {
        format!("data:application/json;base64,{}", self.to_base64())
    }

    /// Standard inline comment `//# sourceMappingURL=data:application/json;base64,...`.
    pub fn inline_comment(&self) -> String {
        format!("//# sourceMappingURL={}", self.to_data_url())
    }

    /// Parse an inline `//# sourceMappingURL=data:application/json;base64,...` comment from compiled JS.
    pub fn from_inline_comment(compiled_code: &str) -> Option<Self> {
        let marker = "//# sourceMappingURL=data:application/json";
        let idx = compiled_code.rfind(marker)?;
        let rest = &compiled_code[idx..];
        let line = rest.lines().next()?;
        let b64 = line.rsplit_once(";base64,")?.1.trim();
        let bytes = base64_decode(b64)?;
        serde_json::from_slice(&bytes).ok()
    }
}

fn parse_stack_line(line: &str) -> Option<(Option<String>, usize, usize)> {
    // Matches V8 / SpiderMonkey / JavaScriptCore stack lines:
    // `at foo (file.js:10:5)` or `at file.js:10:5` or `foo@file.js:10:5`
    let cleaned = line.strip_prefix("at ").unwrap_or(line).trim();
    let (fn_name, loc_part) = if let Some((before, after)) = cleaned.rsplit_once('(') {
        let name = before.trim();
        let loc = after.trim_end_matches(')');
        ((!name.is_empty()).then(|| name.to_owned()), loc)
    } else if let Some((before, after)) = cleaned.split_once('@') {
        let name = before.trim();
        ((!name.is_empty()).then(|| name.to_owned()), after.trim())
    } else {
        (None, cleaned)
    };

    let mut parts = loc_part.rsplitn(3, ':');
    let col_str = parts.next()?;
    let line_str = parts.next()?;
    let col: usize = col_str
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    let line_num: usize = line_str
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some((fn_name, line_num, col))
}

/// Extract identifier tokens from a line of code with their 0-based character column.
fn extract_ident_tokens(line: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut chars = line.char_indices().peekable();
    let mut byte_to_col = BTreeMap::new();
    for (char_col, (b, _)) in line.char_indices().enumerate() {
        byte_to_col.insert(b, char_col);
    }
    while let Some(&(start_byte, ch)) = chars.peek() {
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let col = *byte_to_col.get(&start_byte).unwrap_or(&0);
            let mut ident = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    ident.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if !is_js_or_as_keyword(&ident) {
                out.push((ident, col));
            }
        } else {
            chars.next();
        }
    }
    out
}

fn is_js_or_as_keyword(word: &str) -> bool {
    matches!(
        word,
        "var"
            | "let"
            | "const"
            | "function"
            | "return"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "new"
            | "class"
            | "extends"
            | "implements"
            | "interface"
            | "package"
            | "public"
            | "private"
            | "protected"
            | "internal"
            | "static"
            | "override"
            | "import"
            | "export"
            | "from"
            | "as"
            | "true"
            | "false"
            | "null"
            | "undefined"
            | "void"
            | "typeof"
            | "instanceof"
            | "in"
            | "of"
            | "__neoflash_handler__"
            | "__nfFrame"
            | "__nfProgram"
            | "__nfBuiltins"
            | "__nfDebugStep"
    )
}

/// Build a Source Map v3 mapping `compiled_code` back to `original_source` in `source_name`.
pub fn build_source_map(
    source_name: &str,
    original_source: &str,
    compiled_code: &str,
) -> SourceMap {
    let orig_lines: Vec<&str> = original_source.split('\n').collect();
    let gen_lines: Vec<&str> = compiled_code.split('\n').collect();

    let mut names: Vec<String> = Vec::new();
    let mut name_indices: BTreeMap<String, usize> = BTreeMap::new();
    let mut intern_name = |ident: &str| -> usize {
        if let Some(&idx) = name_indices.get(ident) {
            idx
        } else {
            let idx = names.len();
            names.push(ident.to_owned());
            name_indices.insert(ident.to_owned(), idx);
            idx
        }
    };

    // Index original lines by their non-keyword identifiers so transformed lines
    // match back to their exact original source line & column.
    let orig_line_tokens: Vec<Vec<(String, usize)>> =
        orig_lines.iter().map(|l| extract_ident_tokens(l)).collect();

    let mut segments = Vec::new();
    let mut last_matched_orig_line = 0usize;

    for (gen_line_idx, gen_line) in gen_lines.iter().enumerate() {
        let trimmed = gen_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//# sourceMappingURL=") {
            continue;
        }
        let gen_col = gen_line.chars().take_while(|c| c.is_whitespace()).count();
        let gen_tokens = extract_ident_tokens(gen_line);

        // Find the best matching original line for this generated line.
        let mut best_orig_line = None;
        let mut best_score = 0usize;

        // Check if there is an explicit `__nfDebugStep("...", line, ...)` marker on this line.
        if let Some(step_idx) = gen_line.find("__nfDebugStep(") {
            let after = &gen_line[step_idx + "__nfDebugStep(".len()..];
            if let Some((_, after_comma)) = after.split_once(',') {
                let line_digits: String = after_comma
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(one_based) = line_digits.parse::<usize>()
                    && one_based >= 1
                    && one_based <= orig_lines.len()
                {
                    best_orig_line = Some(one_based - 1);
                    best_score = 100;
                }
            }
        }

        if best_orig_line.is_none() && !gen_tokens.is_empty() {
            for (orig_idx, o_tokens) in orig_line_tokens.iter().enumerate() {
                if o_tokens.is_empty() {
                    continue;
                }
                let shared = gen_tokens
                    .iter()
                    .filter(|(gt, _)| o_tokens.iter().any(|(ot, _)| ot == gt))
                    .count();
                if shared > best_score
                    || (shared > 0
                        && shared == best_score
                        && orig_idx >= last_matched_orig_line
                        && best_orig_line.is_some_and(|b| b < last_matched_orig_line))
                {
                    best_score = shared;
                    best_orig_line = Some(orig_idx);
                }
            }
        }

        let orig_line_idx = best_orig_line.unwrap_or_else(|| {
            if orig_lines.is_empty() {
                0
            } else {
                last_matched_orig_line.min(orig_lines.len() - 1)
            }
        });
        last_matched_orig_line = orig_line_idx;

        let orig_line_str = orig_lines.get(orig_line_idx).copied().unwrap_or("");
        let orig_col = orig_line_str
            .chars()
            .take_while(|c| c.is_whitespace())
            .count();

        let first_name = orig_line_tokens
            .get(orig_line_idx)
            .and_then(|t| t.first())
            .map(|(ident, _)| intern_name(ident));

        segments.push(SourceMappingSegment {
            generated_line: gen_line_idx,
            generated_column: gen_col,
            source_index: 0,
            original_line: orig_line_idx,
            original_column: orig_col,
            name_index: first_name,
        });

        // Also emit token-level column mappings for shared identifiers on the matched line.
        if let Some(o_tokens) = orig_line_tokens.get(orig_line_idx) {
            for (g_ident, g_col) in &gen_tokens {
                if let Some((_, o_col)) = o_tokens.iter().find(|(o_ident, _)| o_ident == g_ident) {
                    let name_idx = intern_name(g_ident);
                    if *g_col != gen_col || *o_col != orig_col {
                        segments.push(SourceMappingSegment {
                            generated_line: gen_line_idx,
                            generated_column: *g_col,
                            source_index: 0,
                            original_line: orig_line_idx,
                            original_column: *o_col,
                            name_index: Some(name_idx),
                        });
                    }
                }
            }
        }
    }

    if segments.is_empty() {
        segments.push(SourceMappingSegment {
            generated_line: 0,
            generated_column: 0,
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
    }

    SourceMap::from_segments(
        Some(format!("{source_name}.js")),
        vec![source_name.to_owned()],
        vec![original_source.to_owned()],
        names,
        segments,
    )
}

/// A collision-free compiler comment prefix. Authored comments/strings cannot be mistaken
/// for emitted statement origins, including source that deliberately contains the base spelling.
pub(crate) fn marker_prefix(source: &str) -> String {
    let mut index = 0;
    loop {
        let marker = format!("/*__nf_origin_{index}:");
        if !source.contains(&marker) {
            return marker;
        }
        index += 1;
    }
}

/// Map only positions supplied by the parser/emitter, never identifier similarity.
/// Offsets in marker comments are byte offsets in the original source; v3 columns are UTF-16.
pub(crate) fn map_marked_source(name: &str, source: &str, code: &str, prefix: &str) -> SourceMap {
    let mut segments = Vec::new();
    for (line, text) in code.lines().enumerate() {
        for (start, _) in text.match_indices(prefix) {
            let rest = &text[start + prefix.len()..];
            let Some((offset, _)) = rest.split_once("*/") else {
                continue;
            };
            let Ok(offset) = offset.parse::<usize>() else {
                continue;
            };
            let Some(before) = source.get(..offset) else {
                continue;
            };
            let original_line = before.bytes().filter(|c| *c == b'\n').count();
            let original_column = before
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .encode_utf16()
                .count();
            let end = start + prefix.len() + rest.find("*/").unwrap() + 2;
            segments.push(SourceMappingSegment {
                generated_line: line,
                generated_column: text[..end].encode_utf16().count(),
                source_index: 0,
                original_line,
                original_column,
                name_index: None,
            });
        }
    }
    SourceMap::from_segments(
        None,
        vec![name.into()],
        vec![source.into()],
        vec![],
        segments,
    )
}

pub(crate) fn identity_source_map(name: &str, source: &str) -> SourceMap {
    let mut offsets: Vec<usize> = crate::warnings::scan_word_tokens(source)
        .into_iter()
        .map(|(_, start, _)| start)
        .collect();
    let mut start = 0;
    for text in source.split_inclusive('\n') {
        if let Some(first) = text.find(|c: char| !c.is_whitespace()) {
            offsets.push(start + first);
        }
        start += text.len();
    }
    offsets.sort_unstable();
    offsets.dedup();
    let segments = offsets
        .into_iter()
        .map(|offset| {
            let before = &source[..offset];
            let line = before.bytes().filter(|c| *c == b'\n').count();
            let column = before
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .encode_utf16()
                .count();
            SourceMappingSegment {
                generated_line: line,
                generated_column: column,
                source_index: 0,
                original_line: line,
                original_column: column,
                name_index: None,
            }
        })
        .collect();
    SourceMap::from_segments(
        None,
        vec![name.into()],
        vec![source.into()],
        vec![],
        segments,
    )
}

/// Rebase an exact map when its code is embedded in a generated wrapper.
pub(crate) fn shift_map(map: &SourceMap, line: usize, column: usize) -> SourceMap {
    let mut segments = map.decode_segments();
    for s in &mut segments {
        if s.generated_line == 0 {
            s.generated_column += column;
        }
        s.generated_line += line;
    }
    SourceMap::from_segments(
        map.file.clone(),
        map.sources.clone(),
        map.sources_content.clone(),
        map.names.clone(),
        segments,
    )
}

pub(crate) fn combine_maps(maps: &[SourceMap]) -> SourceMap {
    let mut sources = Vec::new();
    let mut contents = Vec::new();
    let mut names = Vec::new();
    let mut segments = Vec::new();
    for map in maps {
        let indices: Vec<_> = map
            .sources
            .iter()
            .enumerate()
            .map(|(i, name)| {
                sources.iter().position(|s| s == name).unwrap_or_else(|| {
                    sources.push(name.clone());
                    contents.push(map.sources_content.get(i).cloned().unwrap_or_default());
                    sources.len() - 1
                })
            })
            .collect();
        let name_start = names.len();
        names.extend(map.names.iter().cloned());
        segments.extend(map.decode_segments().into_iter().map(|mut s| {
            s.source_index = indices[s.source_index];
            s.name_index = s.name_index.map(|i| i + name_start);
            s
        }));
    }
    SourceMap::from_segments(None, sources, contents, names, segments)
}

/// Relocate Oxc mappings through explicit copied byte ranges produced by the module rewriter.
/// Removed imports and generated export shims have no source span; retained declarations do.
pub(crate) fn relocate_map(
    map: &SourceMap,
    before: &str,
    after: &str,
    ranges: &[(usize, usize, usize)],
) -> SourceMap {
    let mut segments = Vec::new();
    for mut segment in map.decode_segments() {
        let mut line_start = 0;
        let mut offset = None;
        for (line, text) in before.split_inclusive('\n').enumerate() {
            if line == segment.generated_line {
                let mut units = 0;
                for (byte, ch) in text.char_indices() {
                    if units == segment.generated_column {
                        offset = Some(line_start + byte);
                        break;
                    }
                    units += ch.len_utf16();
                }
                break;
            }
            line_start += text.len();
        }
        let Some(offset) = offset else {
            continue;
        };
        let Some((start, _, dest)) = ranges
            .iter()
            .find(|(start, end, _)| *start <= offset && offset < *end)
        else {
            continue;
        };
        let prefix = &after[..dest + offset - start];
        segment.generated_line = prefix.bytes().filter(|c| *c == b'\n').count();
        segment.generated_column = prefix
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .encode_utf16()
            .count();
        segments.push(segment);
    }
    SourceMap::from_segments(
        map.file.clone(),
        map.sources.clone(),
        map.sources_content.clone(),
        map.names.clone(),
        segments,
    )
}

/// Build a multi-source `SourceMap` for a bundle of compiled modules.
pub fn build_bundle_source_map(
    bundle_file: &str,
    modules: &[(String, String, String)], // (source_path, original_source, compiled_chunk)
    bundle_code: &str,
) -> SourceMap {
    let mut sources = Vec::new();
    let mut sources_content = Vec::new();
    let mut names = Vec::new();
    let mut name_map = BTreeMap::new();
    let mut segments = Vec::new();

    for (src_idx, (path, orig, chunk)) in modules.iter().enumerate() {
        sources.push(path.clone());
        sources_content.push(orig.clone());
        let sub_map = build_source_map(path, orig, chunk);
        let chunk_start_line = bundle_code.find(chunk.as_str()).map_or(0, |byte_idx| {
            bundle_code[..byte_idx].lines().count().saturating_sub(1)
        });
        for seg in sub_map.decode_segments() {
            let name_index = seg.name_index.and_then(|n_idx| {
                let name = sub_map.names.get(n_idx)?;
                if let Some(&existing) = name_map.get(name) {
                    Some(existing)
                } else {
                    let idx = names.len();
                    names.push(name.clone());
                    name_map.insert(name.clone(), idx);
                    Some(idx)
                }
            });
            segments.push(SourceMappingSegment {
                generated_line: chunk_start_line + seg.generated_line,
                generated_column: seg.generated_column,
                source_index: src_idx,
                original_line: seg.original_line,
                original_column: seg.original_column,
                name_index,
            });
        }
    }

    SourceMap::from_segments(
        Some(bundle_file.to_owned()),
        sources,
        sources_content,
        names,
        segments,
    )
}

/// Compiled script paired with its Source Map v3 and inline `sourceMappingURL`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledWithSourceMap {
    pub language: ScriptLanguage,
    pub source_name: String,
    /// Compiled JavaScript with trailing `//# sourceMappingURL=data:application/json;base64,...`.
    pub code: String,
    pub source_map: SourceMap,
}

/// Compiled debug-instrumented script with `__nfDebugStep(source, line, locals)` hooks
/// and Source Map v3 metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledDebugScript {
    pub language: ScriptLanguage,
    pub source_name: String,
    /// Compiled JavaScript instrumented with `__nfDebugStep` calls and inline source map.
    pub code: String,
    pub source_map: SourceMap,
    /// 1-based lines in the original source that have step/breakpoint hooks.
    pub breakable_lines: Vec<usize>,
    /// Local variable names discovered per 1-based line.
    pub locals_by_line: BTreeMap<usize, Vec<String>>,
}

/// Lightweight JavaScript runtime debug controller injected into `ProjectProgram` and debug builds.
pub fn debug_runtime_js() -> &'static str {
    r#"const __nfDebug = (function() {
  const existing = typeof globalThis !== "undefined" && globalThis.__nfDebug ? globalThis.__nfDebug : null;
  if (existing) return existing;
  const breakpoints = new Set();
  const conditions = new Map();
  const sourceMaps = new Map();
  let stepMode = false;
  let lastFrame = null;
  const traceLog = [];
  const pauseListeners = [];
  const api = {
    setBreakpoint(source, line, condition) {
      const key = `${source}:${Number(line)}`;
      breakpoints.add(key);
      if (typeof condition === "function" || typeof condition === "string") {
        conditions.set(key, condition);
      } else {
        conditions.delete(key);
      }
      return key;
    },
    removeBreakpoint(source, line) {
      const key = `${source}:${Number(line)}`;
      conditions.delete(key);
      return breakpoints.delete(key);
    },
    hasBreakpoint(source, line) {
      return breakpoints.has(`${source}:${Number(line)}`);
    },
    clearBreakpoints() {
      breakpoints.clear();
      conditions.clear();
    },
    setStepMode(enabled) {
      stepMode = Boolean(enabled);
    },
    onPause(listener) {
      if (typeof listener === "function") pauseListeners.push(listener);
      return () => {
        const idx = pauseListeners.indexOf(listener);
        if (idx >= 0) pauseListeners.splice(idx, 1);
      };
    },
    registerSourceMap(source, map) {
      sourceMaps.set(source, map);
    },
    getSourceMap(source) {
      return sourceMaps.get(source) || null;
    },
    getLocals() {
      return lastFrame ? { ...lastFrame.locals } : {};
    },
    getLastFrame() {
      return lastFrame;
    },
    getTrace() {
      return traceLog.slice();
    },
    clearTrace() {
      traceLog.length = 0;
      lastFrame = null;
    },
    step(source, line, localsOrThunk) {
      const locals = typeof localsOrThunk === "function" ? localsOrThunk() : (localsOrThunk || {});
      const frame = { source: String(source), line: Number(line), locals: { ...locals } };
      lastFrame = frame;
      if (traceLog.length < 1024) traceLog.push(frame);
      const key = `${frame.source}:${frame.line}`;
      let shouldPause = stepMode || breakpoints.has(key);
      if (shouldPause && conditions.has(key)) {
        const cond = conditions.get(key);
        try {
          shouldPause = typeof cond === "function" ? Boolean(cond(frame.locals, frame)) : Boolean(frame.locals[cond]);
        } catch (_) {
          shouldPause = false;
        }
      }
      if (shouldPause) {
        for (const listener of pauseListeners) {
          listener(frame);
        }
      }
      return frame;
    }
  };
  if (typeof globalThis !== "undefined") {
    globalThis.__nfDebug = api;
    globalThis.__nfDebugStep = (source, line, locals) => api.step(source, line, locals);
  }
  return api;
})();
const __nfDebugStep = (source, line, locals) => __nfDebug.step(source, line, locals);"#
}

/// Instrument a user script with `__nfDebugStep(source_name, line, { ...locals })` hooks
/// before compiling so every statement line is steppable and local variables are inspectable.
pub fn instrument_script_for_debug(
    script: &Script,
    source_name: &str,
) -> (Script, Vec<usize>, BTreeMap<usize, Vec<String>>) {
    let lines: Vec<&str> = script.source.split('\n').collect();
    let mut out_lines = Vec::with_capacity(lines.len());
    let mut breakable_lines = Vec::new();
    let mut locals_by_line = BTreeMap::new();
    let mut known_locals: Vec<String> = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();

        // Discover newly declared variables on this line (`var x`, `let x`, `const x`).
        for kw in ["var ", "let ", "const "] {
            if let Some(rest) = trimmed.strip_prefix(kw).or_else(|| {
                trimmed
                    .strip_prefix("export ")
                    .and_then(|r| r.trim_start().strip_prefix(kw))
            }) {
                let name: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() && !is_js_or_as_keyword(&name) && !known_locals.contains(&name)
                {
                    known_locals.push(name);
                }
            }
        }

        let is_executable = !trimmed.is_empty()
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("/*")
            && !trimmed.starts_with('*')
            && !trimmed.starts_with("import ")
            && !trimmed.starts_with("package")
            && !trimmed.starts_with("interface ")
            && !trimmed.starts_with("type ")
            && !trimmed.starts_with('#')
            && trimmed != "{"
            && trimmed != "}"
            && trimmed != "};"
            && !trimmed.starts_with("else")
            && !trimmed.starts_with("case ")
            && !trimmed.starts_with("default:")
            && !trimmed.starts_with("catch")
            && !trimmed.starts_with("finally");

        if is_executable
            && matches!(
                script.language,
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
            )
        {
            breakable_lines.push(line_no);
            locals_by_line.insert(line_no, known_locals.clone());
            let locals_obj = if known_locals.is_empty() {
                "{}".to_owned()
            } else {
                let pairs: Vec<String> = known_locals
                    .iter()
                    .map(|n| format!("{n}: (typeof {n} !== \"undefined\" ? {n} : undefined)"))
                    .collect();
                format!("{{ {} }}", pairs.join(", "))
            };
            if trimmed.starts_with("return") || trimmed.starts_with("throw") {
                out_lines.push(format!(
                    "__nfDebugStep({source_name:?}, {line_no}, {locals_obj}); {line}"
                ));
            } else {
                out_lines.push(format!(
                    "{line} __nfDebugStep({source_name:?}, {line_no}, {locals_obj});"
                ));
            }
        } else {
            if is_executable {
                breakable_lines.push(line_no);
                locals_by_line.insert(line_no, known_locals.clone());
            }
            out_lines.push((*line).to_owned());
        }
    }

    (
        Script {
            language: script.language,
            source: out_lines.join("\n"),
        },
        breakable_lines,
        locals_by_line,
    )
}

/// Host/editor debugger session for managing breakpoints, stepping, and inspecting locals.
#[derive(Clone, Debug, Default)]
pub struct DebuggerSession {
    pub breakpoints: BTreeSet<(String, usize)>,
    pub source_maps: BTreeMap<String, SourceMap>,
    pub paused_at: Option<(String, usize)>,
    pub locals: BTreeMap<String, serde_json::Value>,
    pub step_mode: bool,
}

impl DebuggerSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_breakpoint(&mut self, source: impl Into<String>, line: usize) {
        self.breakpoints.insert((source.into(), line));
    }

    pub fn remove_breakpoint(&mut self, source: &str, line: usize) -> bool {
        self.breakpoints.remove(&(source.to_owned(), line))
    }

    pub fn has_breakpoint(&self, source: &str, line: usize) -> bool {
        self.breakpoints.contains(&(source.to_owned(), line))
    }

    pub fn register_source_map(&mut self, source: impl Into<String>, map: SourceMap) {
        self.source_maps.insert(source.into(), map);
    }

    /// Record a runtime step event and return `true` if execution should pause at a breakpoint or step.
    pub fn on_step(
        &mut self,
        source: &str,
        line: usize,
        locals: BTreeMap<String, serde_json::Value>,
    ) -> bool {
        self.locals = locals;
        if self.step_mode || self.has_breakpoint(source, line) {
            self.paused_at = Some((source.to_owned(), line));
            true
        } else {
            false
        }
    }

    pub fn resume(&mut self) {
        self.paused_at = None;
        self.step_mode = false;
    }

    pub fn step_over(&mut self) {
        self.paused_at = None;
        self.step_mode = true;
    }
}
