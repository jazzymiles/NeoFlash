use crate::{CompiledScript, HtmlPublishConfig};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PublishSourceMapShift {
    pub(crate) generated_lines: isize,
    pub(crate) first_line_columns: isize,
}

pub(crate) fn begins_with_use_strict_directive(code: &str) -> bool {
    let trimmed = code.trim_start();
    ["\"use strict\"", "'use strict'"]
        .into_iter()
        .any(|directive| {
            trimmed.strip_prefix(directive).is_some_and(|tail| {
                tail.is_empty()
                    || tail.starts_with(';')
                    || tail.starts_with('\n')
                    || tail.starts_with('\r')
            })
        })
}

pub(crate) fn strip_generated_use_strict(code: &str) -> Option<(String, PublishSourceMapShift)> {
    for directive in ["\"use strict\";", "'use strict';"] {
        if let Some(rest) = code.strip_prefix(&format!("{directive}\r\n")) {
            // Preserve the line break so existing generated line positions remain exact.
            return Some((format!("\r\n{rest}"), PublishSourceMapShift::default()));
        }
        if let Some(rest) = code.strip_prefix(&format!("{directive}\n")) {
            // Preserve the line break so existing generated line positions remain exact.
            return Some((format!("\n{rest}"), PublishSourceMapShift::default()));
        }
        if let Some(rest) = code.strip_prefix(&format!("{directive} ")) {
            return Some((
                rest.to_owned(),
                PublishSourceMapShift {
                    generated_lines: 0,
                    first_line_columns: -(directive.encode_utf16().count() as isize + 1),
                },
            ));
        }
        if let Some(rest) = code.strip_prefix(directive) {
            return Some((
                rest.to_owned(),
                PublishSourceMapShift {
                    generated_lines: 0,
                    first_line_columns: -(directive.encode_utf16().count() as isize),
                },
            ));
        }
    }
    None
}

/// Apply only transformations that are safe without reparsing executable JavaScript.
///
/// `minify` still compacts the surrounding JSON bundle, but executable program and handler code
/// is preserved byte-for-byte. The previous line-oriented whitespace/comment remover could change
/// token boundaries (`x + +y` -> `x++y`) and interpreted `//` inside multiline templates as a
/// comment. Strict mode is inserted on its own generated line so Source Map v3 positions can be
/// rebased exactly.
pub(crate) fn apply_js_publish_transforms(
    code: &str,
    config: Option<&HtmlPublishConfig>,
) -> (String, PublishSourceMapShift) {
    let Some(cfg) = config else {
        return (code.to_owned(), PublishSourceMapShift::default());
    };

    if cfg.strict_mode {
        if begins_with_use_strict_directive(code) {
            (code.to_owned(), PublishSourceMapShift::default())
        } else {
            (
                format!("\"use strict\";\n{code}"),
                PublishSourceMapShift {
                    generated_lines: 1,
                    first_line_columns: 0,
                },
            )
        }
    } else {
        strip_generated_use_strict(code)
            .unwrap_or_else(|| (code.to_owned(), PublishSourceMapShift::default()))
    }
}

pub(crate) fn apply_publish_source_map_shift(
    map: &neoflash_script::SourceMap,
    shift: PublishSourceMapShift,
) -> neoflash_script::SourceMap {
    if shift == PublishSourceMapShift::default() {
        return map.clone();
    }

    let removed_lines = shift.generated_lines.unsigned_abs();
    let segments = map
        .decode_segments()
        .into_iter()
        .filter_map(|mut segment| {
            let original_generated_line = segment.generated_line;
            if shift.generated_lines >= 0 {
                segment.generated_line = segment
                    .generated_line
                    .saturating_add(shift.generated_lines as usize);
            } else if segment.generated_line < removed_lines {
                return None;
            } else {
                segment.generated_line -= removed_lines;
            }

            if original_generated_line == 0 && shift.first_line_columns != 0 {
                if shift.first_line_columns >= 0 {
                    segment.generated_column = segment
                        .generated_column
                        .saturating_add(shift.first_line_columns as usize);
                } else {
                    segment.generated_column = segment
                        .generated_column
                        .saturating_sub(shift.first_line_columns.unsigned_abs());
                }
            }
            Some(segment)
        })
        .collect();
    neoflash_script::SourceMap::from_segments(
        map.file.clone(),
        map.sources.clone(),
        map.sources_content.clone(),
        map.names.clone(),
        segments,
    )
}

pub(crate) fn build_fallback_bundle_source_map(
    scripts: &[CompiledScript],
    program_code: &str,
) -> neoflash_script::SourceMap {
    if !scripts.is_empty() {
        let modules: Vec<(String, String, String)> = scripts
            .iter()
            .map(|s| (s.location.clone(), s.source.clone(), s.code.clone()))
            .collect();
        let combined_code = scripts
            .iter()
            .map(|s| s.code.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        neoflash_script::build_bundle_source_map("neoflash-project.js", &modules, &combined_code)
    } else {
        neoflash_script::build_source_map("neoflash-project.js", program_code, program_code)
    }
}
