//! Shared ActionScript (`AS1`, `AS2`, `AS3`) `#include` / `include` directive scanner,
//! relative path normalizer, and bounded textual expander (`T-412`).
//!
//! Shared between `AuthoredPlayback` (in-editor `Play`), `neoflash-script` (`compile_project` /
//! `compile_with_program`), and `neoflash-editor` (external disk & open-tab hydration).

use super::AuthoredPlayback;
use crate::{Project, ResourceUnit, Script, ScriptLanguage};
use std::collections::BTreeMap;

pub(super) const MAX_INCLUDE_BYTES: usize = 1_048_576;
pub(super) const MAX_INCLUDE_DEPTH: usize = 32;

pub type ExpandedIncludesResult = (Script, Vec<(String, usize)>, BTreeMap<String, String>);
pub type IncludeExpandError = (Script, usize, String, bool, String);

#[derive(Debug, PartialEq, Eq)]
enum LineDirective<'a> {
    None,
    Include {
        closed_comment_prefix: Option<&'a str>,
        requested: &'a str,
        opens_trailing_block_comment: bool,
    },
    SyntaxError(&'static str),
}

fn syntax_error_msg(is_as3: bool) -> &'static str {
    if is_as3 {
        "Use include \"path.as\"; on its own line"
    } else {
        "Use #include \"path.as\" on its own line without a semicolon"
    }
}

pub(super) fn scan_comments_in_line(line: &str, block: &mut bool) {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut quote = None;
    while i < bytes.len() {
        if *block {
            if bytes[i..].starts_with(b"*/") {
                *block = false;
                i += 2;
            } else {
                i += 1;
            }
        } else if let Some(end) = quote {
            if bytes[i] == b'\\' {
                i = (i + 2).min(bytes.len());
            } else {
                if bytes[i] == end {
                    quote = None;
                }
                i += 1;
            }
        } else if bytes[i..].starts_with(b"//") {
            break;
        } else if bytes[i..].starts_with(b"/*") {
            *block = true;
            i += 2;
        } else {
            if matches!(bytes[i], b'\'' | b'"') {
                quote = Some(bytes[i]);
            }
            i += 1;
        }
    }
}

pub(super) fn split_lines_with_endings(source: &str) -> impl Iterator<Item = &str> {
    let bytes = source.as_bytes();
    let mut start = 0usize;
    std::iter::from_fn(move || {
        if start >= bytes.len() {
            return None;
        }
        let mut i = start;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    i += 1;
                    break;
                }
                b'\r' => {
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'\n' {
                        i += 1;
                    }
                    break;
                }
                _ => i += 1,
            }
        }
        let slice = &source[start..i];
        start = i;
        Some(slice)
    })
}

fn skip_ws_and_inline_comments(s: &str, mut i: usize) -> Result<(usize, bool), ()> {
    let bytes = s.as_bytes();
    while i < bytes.len() {
        let ch = s[i..].chars().next().unwrap();
        if ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            if let Some(rel_close) = s[i + 2..].find("*/") {
                i += 2 + rel_close + 2;
                continue;
            }
            // Unclosed `/*` runs to the end of the line.
            return Ok((bytes.len(), true));
        }
        break;
    }
    Ok((i, false))
}

fn parse_include_directive_line<'a>(
    line_without_ending: &'a str,
    is_first_line: bool,
    is_as3: bool,
    block: &mut bool,
) -> LineDirective<'a> {
    let s = if is_first_line {
        line_without_ending
            .strip_prefix('\u{feff}')
            .unwrap_or(line_without_ending)
    } else {
        line_without_ending
    };

    let initial_block = *block;
    let mut cursor = 0usize;
    let mut closed_prefix_end: Option<usize> = None;

    if *block {
        let Some(rel_close) = s.find("*/") else {
            return LineDirective::None;
        };
        *block = false;
        cursor = rel_close + 2;
        closed_prefix_end = Some(cursor);
    }

    // Skip leading whitespace and any closed `/* ... */` comments before a potential directive.
    loop {
        while cursor < s.len() {
            let ch = s[cursor..].chars().next().unwrap();
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }
        if s[cursor..].starts_with("/*") {
            if let Some(rel_close) = s[cursor + 2..].find("*/") {
                cursor += 2 + rel_close + 2;
                continue;
            }
            // Opens an unclosed block comment before any directive; let normal line handling emit it.
            *block = initial_block;
            return LineDirective::None;
        }
        break;
    }

    let candidate = &s[cursor..];
    let after_kw = if let Some(rest) = candidate.strip_prefix("#include") {
        let boundary_ok = rest.is_empty()
            || rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            || rest.starts_with("/*")
            || rest.starts_with("//");
        if boundary_ok {
            Some(rest)
        } else if !is_as3 {
            // Preserve legacy AS1/AS2 behavior for malformed `#include...` lines.
            Some(rest)
        } else {
            None
        }
    } else if is_as3
        && let Some(rest) = candidate.strip_prefix("include")
        && (rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            || rest.starts_with("/*"))
    {
        Some(rest)
    } else {
        None
    };

    let Some(after_kw) = after_kw else {
        *block = initial_block;
        return LineDirective::None;
    };

    let Ok((arg_pos, unclosed_before_arg)) = skip_ws_and_inline_comments(after_kw, 0) else {
        return LineDirective::SyntaxError(syntax_error_msg(is_as3));
    };
    if unclosed_before_arg || arg_pos >= after_kw.len() {
        return LineDirective::SyntaxError(syntax_error_msg(is_as3));
    }

    let arg_rest = &after_kw[arg_pos..];
    let quote_char = match arg_rest.as_bytes()[0] {
        b'"' => '"',
        b'\'' => '\'',
        _ => return LineDirective::SyntaxError(syntax_error_msg(is_as3)),
    };
    let after_open = &arg_rest[1..];
    let Some(close_idx) = after_open.find(quote_char) else {
        return LineDirective::SyntaxError(syntax_error_msg(is_as3));
    };
    let requested = &after_open[..close_idx];
    if requested.contains(['"', '\'', '\\']) {
        return LineDirective::SyntaxError(syntax_error_msg(is_as3));
    }

    let tail = &after_open[close_idx + 1..];
    let Ok((mut tail_pos, mut opens_trailing_block)) = skip_ws_and_inline_comments(tail, 0) else {
        return LineDirective::SyntaxError(syntax_error_msg(is_as3));
    };

    if !opens_trailing_block && is_as3 && tail[tail_pos..].starts_with(';') {
        tail_pos += 1;
        let Ok((next_pos, unclosed_after_semi)) = skip_ws_and_inline_comments(tail, tail_pos)
        else {
            return LineDirective::SyntaxError(syntax_error_msg(is_as3));
        };
        tail_pos = next_pos;
        opens_trailing_block = unclosed_after_semi;
    }

    if !opens_trailing_block && tail_pos < tail.len() && !tail[tail_pos..].starts_with("//") {
        return LineDirective::SyntaxError(syntax_error_msg(is_as3));
    }

    *block = opens_trailing_block;
    let closed_comment_prefix = if initial_block {
        closed_prefix_end.map(|end| &s[..end])
    } else {
        None
    };
    LineDirective::Include {
        closed_comment_prefix,
        requested,
        opens_trailing_block_comment: opens_trailing_block,
    }
}

pub(super) fn builtin_include_source(included: &str) -> Option<&'static str> {
    let file_name = included.rsplit_once('/').map_or(included, |(_, name)| name);
    if file_name.eq_ignore_ascii_case("NetServices.as") {
        Some(
            "if (_global.NetServices == undefined) {\n\
             _global.NetServices = {};\n\
             _global.NetServices.setDefaultGatewayUrl = function(url) { this.defaultGatewayUrl = url; };\n\
             _global.NetServices.setGatewayUrl = function(url) { this.gatewayUrl = url; };\n\
             _global.NetServices.createGatewayConnection = function(url) {\n\
                 var nc = new NetConnection();\n\
                 var targetUrl = (url != undefined) ? url : ((this.gatewayUrl != undefined) ? this.gatewayUrl : this.defaultGatewayUrl);\n\
                 if (targetUrl != undefined) { nc.connect(targetUrl); }\n\
                 nc.getService = function(serviceName, responder) {\n\
                     return { _serviceName: serviceName, _responder: responder, _nc: this };\n\
                 };\n\
                 return nc;\n\
             };\n\
             }\n\
             var NetServices = _global.NetServices;\n",
        )
    } else if file_name.eq_ignore_ascii_case("NetDebug.as") {
        Some(
            "if (_global.NetDebug == undefined) {\n\
             _global.NetDebug = {};\n\
             _global.NetDebug.initialize = function() { return true; };\n\
             _global.NetDebug.trace = function(obj) { trace(obj); };\n\
             _global.NetDebug.traceNetServices = function(who, severity, number, message) { trace(message); };\n\
             }\n\
             var NetDebug = _global.NetDebug;\n",
        )
    } else {
        None
    }
}

fn append_includes(
    script: &Script,
    path: &str,
    resources: &BTreeMap<String, ResourceUnit>,
    stack: &mut Vec<String>,
    output_source: &mut String,
    origins: &mut Vec<(String, usize)>,
    sources: &mut BTreeMap<String, String>,
) -> Result<(), IncludeExpandError> {
    sources
        .entry(path.to_owned())
        .or_insert_with(|| script.source.clone());
    let is_as3 = script.language == ScriptLanguage::ActionScript3;
    let mut block = false;
    let mut offset = 0usize;

    for (line_number, line) in split_lines_with_endings(&script.source).enumerate() {
        let line_without_ending = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .or_else(|| line.strip_suffix('\r'))
            .unwrap_or(line);

        let initial_block = block;
        match parse_include_directive_line(
            line_without_ending,
            line_number == 0,
            is_as3,
            &mut block,
        ) {
            LineDirective::Include {
                closed_comment_prefix,
                requested,
                opens_trailing_block_comment,
            } => {
                if let Some(prefix) = closed_comment_prefix {
                    output_source.push_str(prefix);
                    output_source.push('\n');
                    origins.push((path.to_owned(), line_number + 1));
                }

                let make_err = |msg: String| -> IncludeExpandError {
                    (script.clone(), offset, path.to_owned(), true, msg)
                };

                let included = AuthoredPlayback::resolve_include_logical_path(path, requested)
                    .map_err(make_err)?;
                if stack.len() >= MAX_INCLUDE_DEPTH || stack.iter().any(|p| p == &included) {
                    return Err(make_err(format!(
                        "Cyclic or over-deep #include: {included} (maximum 32 levels)"
                    )));
                }

                let source = if let Some(resource) = resources.get(&included).or_else(|| {
                    resources
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(&included))
                        .map(|(_, v)| v)
                }) {
                    let bytes = resource
                        .decoded_bytes()
                        .map_err(|e| make_err(e.to_string()))?;
                    if bytes.len() > MAX_INCLUDE_BYTES {
                        return Err(make_err("An included source is limited to 1 MiB".into()));
                    }
                    let raw_source = String::from_utf8(bytes).map_err(|_| {
                        make_err(format!(
                            "#include resource '{included}' must contain UTF-8 source"
                        ))
                    })?;
                    if let Some(stripped) = raw_source.strip_prefix('\u{feff}') {
                        stripped.to_owned()
                    } else {
                        raw_source
                    }
                } else if !is_as3 && let Some(builtin) = builtin_include_source(&included) {
                    builtin.to_owned()
                } else {
                    return Err(make_err(format!(
                        "Missing #include resource '{included}'; add the .as file to Project Resource files"
                    )));
                };

                stack.push(included.clone());
                append_includes(
                    &Script {
                        language: script.language,
                        source,
                    },
                    &included,
                    resources,
                    stack,
                    output_source,
                    origins,
                    sources,
                )?;
                stack.pop();

                if opens_trailing_block_comment {
                    output_source.push_str("/*\n");
                    origins.push((path.to_owned(), line_number + 1));
                }
            }
            LineDirective::SyntaxError(msg) => {
                return Err((
                    script.clone(),
                    offset,
                    path.to_owned(),
                    true,
                    msg.to_owned(),
                ));
            }
            LineDirective::None => {
                let emitted = if line_number == 0 {
                    line_without_ending
                        .strip_prefix('\u{feff}')
                        .unwrap_or(line_without_ending)
                } else {
                    line_without_ending
                };
                output_source.push_str(emitted);
                output_source.push('\n');
                origins.push((path.to_owned(), line_number + 1));
                block = initial_block;
                scan_comments_in_line(emitted, &mut block);
            }
        }

        if output_source.len() > MAX_INCLUDE_BYTES {
            return Err((
                script.clone(),
                offset,
                path.to_owned(),
                false,
                "Expanded AS2 source is limited to 1 MiB".into(),
            ));
        }
        offset += line.len();
    }
    Ok(())
}

impl AuthoredPlayback {
    /// Normalize a requested `#include` / `include` path relative to `parent` within the project root.
    pub fn resolve_include_logical_path(parent: &str, requested: &str) -> Result<String, String> {
        if requested.is_empty()
            || requested.starts_with('/')
            || requested.contains(['\\', ':', '\0'])
        {
            return Err("#include requires a relative project resource path using '/'".into());
        }
        let mut parts: Vec<_> = parent
            .rsplit_once('/')
            .map_or("", |(base, _)| base)
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        for part in requested.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err("#include path escapes the project resource root".into());
                    }
                }
                value => parts.push(value),
            }
        }
        let result = parts.join("/");
        let is_as_ext = result
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case("as"));
        if !is_as_ext {
            return Err("#include expects an .as project resource".into());
        }
        Ok(result)
    }

    /// Scan uncommented `#include` / AS3 `include` directives in `script`, returning raw requested
    /// path strings in source order. Non-ActionScript scripts return an empty list.
    pub fn scan_script_include_directives(script: &Script) -> Vec<String> {
        if !script.language.is_actionscript() {
            return Vec::new();
        }
        let is_as3 = script.language == ScriptLanguage::ActionScript3;
        let mut block = false;
        let mut found = Vec::new();
        for (line_number, line) in split_lines_with_endings(&script.source).enumerate() {
            let line_without_ending = line
                .strip_suffix("\r\n")
                .or_else(|| line.strip_suffix('\n'))
                .or_else(|| line.strip_suffix('\r'))
                .unwrap_or(line);
            let initial_block = block;
            match parse_include_directive_line(
                line_without_ending,
                line_number == 0,
                is_as3,
                &mut block,
            ) {
                LineDirective::Include { requested, .. } => {
                    found.push(requested.to_owned());
                }
                LineDirective::SyntaxError(_) => {}
                LineDirective::None => {
                    let emitted = if line_number == 0 {
                        line_without_ending
                            .strip_prefix('\u{feff}')
                            .unwrap_or(line_without_ending)
                    } else {
                        line_without_ending
                    };
                    block = initial_block;
                    scan_comments_in_line(emitted, &mut block);
                }
            }
        }
        found
    }

    /// True if `filename` matches a built-in Flash authoring include (`NetServices.as`, `NetDebug.as`).
    pub fn is_builtin_actionscript_include(language: ScriptLanguage, included: &str) -> bool {
        language != ScriptLanguage::ActionScript3 && builtin_include_source(included).is_some()
    }

    /// Expand `#include` / AS3 `include` directives against `resources` (bounded to 32 levels and 1 MiB).
    pub fn expand_script_includes(
        script: &Script,
        base_path: &str,
        resources: &BTreeMap<String, ResourceUnit>,
    ) -> Result<ExpandedIncludesResult, IncludeExpandError> {
        if !script.language.is_actionscript() {
            let origins = (1..=script.source.lines().count().max(1))
                .map(|line| (base_path.to_owned(), line))
                .collect();
            let mut sources = BTreeMap::new();
            sources.insert(base_path.to_owned(), script.source.clone());
            return Ok((script.clone(), origins, sources));
        }
        let clean_root = script.source.strip_prefix('\u{feff}').map_or_else(
            || script.clone(),
            |stripped| Script {
                language: script.language,
                source: stripped.to_owned(),
            },
        );
        let mut output_source = String::new();
        let mut origins = Vec::new();
        let mut sources = BTreeMap::new();
        append_includes(
            &clean_root,
            base_path,
            resources,
            &mut Vec::new(),
            &mut output_source,
            &mut origins,
            &mut sources,
        )?;
        Ok((
            Script {
                language: script.language,
                source: output_source,
            },
            origins,
            sources,
        ))
    }

    /// Build an include resource lookup map from `project.resources` and `.as` `project.sources`
    /// (`project.resources` takes precedence when both define the same path).
    pub fn build_project_include_map(project: &Project) -> BTreeMap<String, ResourceUnit> {
        let mut map = BTreeMap::new();
        for unit in &project.sources {
            let norm_path = unit.path.trim().replace('\\', "/");
            if norm_path
                .rsplit_once('.')
                .is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case("as"))
                && unit.source.len() <= MAX_INCLUDE_BYTES
                && let Ok(res) = ResourceUnit::from_bytes(
                    unit.id.clone(),
                    norm_path.clone(),
                    "text/plain",
                    unit.source.as_bytes(),
                )
            {
                map.insert(norm_path, res);
            }
        }
        for resource in &project.resources {
            map.insert(resource.path.clone(), resource.clone());
        }
        map
    }

    pub(super) fn expand_preview_script(
        &mut self,
        project: &Project,
        script: &Script,
        origin: &str,
    ) -> Option<Script> {
        if !script.language.is_actionscript() {
            return Some(script.clone());
        }
        if !script.source.contains("include") {
            return Some(script.clone());
        }
        let resources = Self::build_project_include_map(project);
        match Self::expand_script_includes(script, "", &resources) {
            Ok((expanded, _, _)) => Some(expanded),
            Err((failing_script, offset, failing_path, _, message)) => {
                let line = failing_script.source[..offset.min(failing_script.source.len())]
                    .bytes()
                    .filter(|&b| b == b'\n')
                    .count()
                    + 1;
                let diag_origin = if failing_path.is_empty() {
                    origin.to_owned()
                } else {
                    format!("{origin} (#include {failing_path})")
                };
                self.record_diagnostic(&diag_origin, line, &message);
                None
            }
        }
    }
}
