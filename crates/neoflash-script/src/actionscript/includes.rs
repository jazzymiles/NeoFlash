//! AS1/AS2/AS3 textual includes resolve only immutable project resources, never disk/HTTP.
use crate::Diagnostic;
use neoflash_core::{ResourceUnit, Script, ScriptLanguage};
use std::collections::BTreeMap;

const MAX_BYTES: usize = 1_048_576;
pub(super) struct Expanded {
    pub script: Script,
    pub(super) origins: Vec<(String, usize)>,
    pub(super) sources: BTreeMap<String, String>,
}
impl Expanded {
    pub fn diagnostic(&self, mut error: Diagnostic) -> Diagnostic {
        if let Some((path, line)) = self.origins.get(error.line.saturating_sub(1)) {
            let end = self.origins.get(error.end_line.saturating_sub(1));
            error.line = *line;
            error.end_line = end
                .filter(|(end_path, _)| end_path == path)
                .map_or(*line, |(_, line)| *line);
            error.path = (!path.is_empty()).then(|| path.clone());
            if let Some(source) = self.sources.get(path) {
                error.span = (
                    crate::line_col_to_byte_offset(source, error.line, error.column),
                    crate::line_col_to_byte_offset(source, error.end_line, error.end_column),
                );
                error.fixes = crate::warnings::quick_fixes_for_diagnostic(&error, source);
            }
            if !path.is_empty() {
                error.message = format!("#include {path}: {}", error.message);
            }
        }
        error
    }
}

fn logical_path(parent: &str, requested: &str) -> Result<String, String> {
    if requested.is_empty() || requested.starts_with('/') || requested.contains(['\\', ':', '\0']) {
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
    if !result.ends_with(".as") {
        return Err("#include expects an .as project resource".into());
    }
    Ok(result)
}

fn scan_comments(line: &str, block: &mut bool) {
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

pub(super) fn expand(
    script: &Script,
    resources: &BTreeMap<String, ResourceUnit>,
) -> Result<Expanded, Diagnostic> {
    expand_at(script, "", resources)
}

pub(super) fn expand_at(
    script: &Script,
    base_path: &str,
    resources: &BTreeMap<String, ResourceUnit>,
) -> Result<Expanded, Diagnostic> {
    let mut expanded = Expanded {
        script: script.clone(),
        origins: vec![],
        sources: BTreeMap::new(),
    };
    expanded.script.source.clear();
    append(script, base_path, resources, &mut vec![], &mut expanded)?;
    Ok(expanded)
}

fn parse_as3_include_arg(trimmed: &str) -> Option<&str> {
    if let Some(rest) = trimmed.strip_prefix("#include")
        && (rest.is_empty()
            || rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\''))
    {
        return Some(rest.trim());
    }
    if let Some(rest) = trimmed.strip_prefix("include")
        && rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
    {
        return Some(rest.trim());
    }
    None
}

fn split_lines_with_endings(source: &str) -> impl Iterator<Item = &str> {
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

fn builtin_include_source(included: &str) -> Option<&'static str> {
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

fn append(
    script: &Script,
    path: &str,
    resources: &BTreeMap<String, ResourceUnit>,
    stack: &mut Vec<String>,
    output: &mut Expanded,
) -> Result<(), Diagnostic> {
    output
        .sources
        .entry(path.to_owned())
        .or_insert_with(|| script.source.clone());
    let is_as3 = script.language == ScriptLanguage::ActionScript3;
    let mut block = false;
    let mut offset = 0;
    for (line_number, line) in split_lines_with_endings(&script.source).enumerate() {
        let trimmed = line.trim();
        let directive_arg = if !block {
            if is_as3 {
                parse_as3_include_arg(trimmed)
            } else if trimmed.starts_with("#include") {
                Some(trimmed.strip_prefix("#include").unwrap().trim())
            } else {
                None
            }
        } else {
            None
        };
        if let Some(raw_arg) = directive_arg {
            let error = |message: String| {
                let mut result = Diagnostic::at(script, offset, message);
                if !path.is_empty() {
                    result.path = Some(path.to_owned());
                    result.message = format!("#include {path}: {}", result.message);
                }
                result
            };
            let argument = if is_as3 {
                let no_comment = raw_arg.split("//").next().unwrap_or(raw_arg).trim();
                no_comment.strip_suffix(';').map_or(no_comment, str::trim)
            } else {
                raw_arg.split("//").next().unwrap_or(raw_arg).trim()
            };
            let requested = argument
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| {
                    argument
                        .strip_prefix('\'')
                        .and_then(|v| v.strip_suffix('\''))
                })
                .filter(|v| !v.contains(['"', '\'', '\\']))
                .ok_or_else(|| {
                    if is_as3 {
                        error("Use include \"path.as\"; on its own line".into())
                    } else {
                        error("Use #include \"path.as\" on its own line without a semicolon".into())
                    }
                })?;
            let included = logical_path(path, requested).map_err(error)?;
            if stack.len() >= 32 || stack.contains(&included) {
                return Err(error(format!(
                    "Cyclic or over-deep #include: {included} (maximum 32 levels)"
                )));
            }
            let source = if let Some(resource) = resources.get(&included).or_else(|| {
                resources
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(&included))
                    .map(|(_, v)| v)
            }) {
                let bytes = resource.decoded_bytes().map_err(|e| error(e.to_string()))?;
                if bytes.len() > MAX_BYTES {
                    return Err(error("An included source is limited to 1 MiB".into()));
                }
                let raw_source = String::from_utf8(bytes).map_err(|_| {
                    error(format!(
                        "#include resource '{included}' must contain UTF-8 source"
                    ))
                })?;
                raw_source
                    .strip_prefix('\u{feff}')
                    .map_or(raw_source.clone(), str::to_owned)
            } else if !is_as3 && let Some(builtin) = builtin_include_source(&included) {
                builtin.to_owned()
            } else {
                return Err(error(format!(
                    "Missing #include resource '{included}'; add the .as file to Project Resource files"
                )));
            };
            stack.push(included.clone());
            append(
                &Script {
                    language: script.language,
                    source,
                },
                &included,
                resources,
                stack,
                output,
            )?;
            stack.pop();
        } else {
            let line_without_ending = line
                .strip_suffix("\r\n")
                .or_else(|| line.strip_suffix('\n'))
                .or_else(|| line.strip_suffix('\r'))
                .unwrap_or(line);
            output.script.source.push_str(line_without_ending);
            output.script.source.push('\n');
            output.origins.push((path.into(), line_number + 1));
            scan_comments(line_without_ending, &mut block);
        }
        if output.script.source.len() > MAX_BYTES {
            return Err(Diagnostic::at(
                script,
                offset,
                "Expanded AS2 source is limited to 1 MiB",
            ));
        }
        offset += line.len();
    }
    Ok(())
}
