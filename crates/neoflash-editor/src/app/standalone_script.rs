//! Standalone ActionScript (`.as`) document session tabs and recursive `#include` / `include`
//! hydration across open `.as` tabs, `project.resources`, `project.sources`, and relative disk files (`T-412`).

use super::*;
use std::borrow::Cow;
use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::Read as _;

const MAX_STANDALONE_AS_BYTES: usize = 1_048_576;
const MAX_HYDRATED_INCLUDE_DEPTH: usize = 32;

/// Strip UTF-8 BOM, comments (`/* ... */`, `// ...`) and string literals (`"..."`, `'...'`)
/// for lightweight AS3 vs AS2 dialect auto-detection when opening a `.as` file.
pub(crate) fn strip_comments_and_strings_for_detection(source: &str) -> String {
    let s = source.strip_prefix('\u{feff}').unwrap_or(source);
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut block_comment = false;
    let mut line_comment = false;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if block_comment {
            if b == b'\n' {
                out.push('\n');
            }
            if bytes[i..].starts_with(b"*/") {
                block_comment = false;
                out.push(' ');
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if line_comment {
            if b == b'\n' {
                line_comment = false;
                out.push('\n');
            }
            i += 1;
            continue;
        }
        if let Some(q) = quote {
            if b == b'\n' {
                out.push('\n');
            }
            if b == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if b == q {
                quote = None;
                out.push('"');
            }
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            block_comment = true;
            out.push(' ');
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            line_comment = true;
            i += 2;
            continue;
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
            out.push('"');
            i += 1;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn has_unpackaged_as2_dotted_class(skeleton: &str) -> bool {
    for line in skeleton.lines() {
        let mut t = line.trim_start();
        for kw in ["dynamic ", "intrinsic ", "public "] {
            if let Some(rest) = t.strip_prefix(kw) {
                t = rest.trim_start();
            }
        }
        if let Some(rest) = t.strip_prefix("class ") {
            let head = rest
                .split(|c: char| c.is_whitespace() || c == '{')
                .next()
                .unwrap_or("");
            if head.contains('.') {
                return true;
            }
        }
    }
    false
}

/// Auto-detect `ScriptLanguage::ActionScript3` vs `ScriptLanguage::ActionScript2` for a `.as` file.
pub(crate) fn detect_actionscript_file_language(source: &str) -> ScriptLanguage {
    let skeleton = strip_comments_and_strings_for_detection(source);
    let mut brace_depth = 0usize;
    for line in skeleton.lines() {
        let trimmed = line.trim();
        if brace_depth == 0
            && let Some(rest) = trimmed.strip_prefix("package")
            && (rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == '{'))
        {
            return ScriptLanguage::ActionScript3;
        }
        if trimmed.starts_with("include \"") || trimmed.starts_with("include\t") {
            return ScriptLanguage::ActionScript3;
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    let as2_dotted_class = has_unpackaged_as2_dotted_class(&skeleton);
    if !as2_dotted_class && skeleton.contains("import flash.") {
        return ScriptLanguage::ActionScript3;
    }

    for marker in [
        "override ",
        "internal class ",
        "internal function ",
        "internal interface ",
        "final class ",
        "final public class ",
        "public final class ",
        "const ",
        ":int",
        ":uint",
        ":void",
        "Vector.<",
        "...rest",
    ] {
        if skeleton.contains(marker) {
            return ScriptLanguage::ActionScript3;
        }
    }

    ScriptLanguage::ActionScript2
}

fn sanitize_source_unit_filename(path: &Path) -> String {
    let raw = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Script.as");
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        "Script.as".to_owned()
    } else if !trimmed
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("as"))
    {
        format!("{trimmed}.as")
    } else {
        trimmed.to_owned()
    }
}

const CP1251_HIGH: [char; 128] = [
    '\u{0402}', '\u{0403}', '\u{201a}', '\u{0453}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{20ac}', '\u{2030}', '\u{0409}', '\u{2039}', '\u{040a}', '\u{040c}', '\u{040b}', '\u{040f}',
    '\u{0452}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{fffd}', '\u{2122}', '\u{0459}', '\u{203a}', '\u{045a}', '\u{045c}', '\u{045b}', '\u{045f}',
    '\u{00a0}', '\u{040e}', '\u{045e}', '\u{0408}', '\u{00a4}', '\u{0490}', '\u{00a6}', '\u{00a7}',
    '\u{0401}', '\u{00a9}', '\u{0404}', '\u{00ab}', '\u{00ac}', '\u{00ad}', '\u{00ae}', '\u{0407}',
    '\u{00b0}', '\u{00b1}', '\u{0406}', '\u{0456}', '\u{0491}', '\u{00b5}', '\u{00b6}', '\u{00b7}',
    '\u{0451}', '\u{2116}', '\u{0454}', '\u{00bb}', '\u{0458}', '\u{0405}', '\u{0455}', '\u{0457}',
    '\u{0410}', '\u{0411}', '\u{0412}', '\u{0413}', '\u{0414}', '\u{0415}', '\u{0416}', '\u{0417}',
    '\u{0418}', '\u{0419}', '\u{041a}', '\u{041b}', '\u{041c}', '\u{041d}', '\u{041e}', '\u{041f}',
    '\u{0420}', '\u{0421}', '\u{0422}', '\u{0423}', '\u{0424}', '\u{0425}', '\u{0426}', '\u{0427}',
    '\u{0428}', '\u{0429}', '\u{042a}', '\u{042b}', '\u{042c}', '\u{042d}', '\u{042e}', '\u{042f}',
    '\u{0430}', '\u{0431}', '\u{0432}', '\u{0433}', '\u{0434}', '\u{0435}', '\u{0436}', '\u{0437}',
    '\u{0438}', '\u{0439}', '\u{043a}', '\u{043b}', '\u{043c}', '\u{043d}', '\u{043e}', '\u{043f}',
    '\u{0440}', '\u{0441}', '\u{0442}', '\u{0443}', '\u{0444}', '\u{0445}', '\u{0446}', '\u{0447}',
    '\u{0448}', '\u{0449}', '\u{044a}', '\u{044b}', '\u{044c}', '\u{044d}', '\u{044e}', '\u{044f}',
];

const CP1252_C1: [char; 32] = [
    '\u{20ac}', '\u{fffd}', '\u{201a}', '\u{0192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02c6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{fffd}', '\u{017d}', '\u{fffd}',
    '\u{fffd}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02dc}', '\u{2122}', '\u{0161}', '\u{203a}', '\u{0153}', '\u{fffd}', '\u{017e}', '\u{0178}',
];

fn normalize_actionscript_line_endings(text: String) -> String {
    if !text.contains('\r') {
        return text;
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            out.push('\n');
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn prefers_cp1251_over_cp1252(bytes: &[u8]) -> bool {
    if bytes.contains(&0x98) {
        return false;
    }
    if bytes
        .iter()
        .any(|b| matches!(*b, 0x81 | 0x8d | 0x8f | 0x90 | 0x9d))
    {
        return true;
    }
    let is_cyr_byte = |b: u8| matches!(b, 0xc0..=0xff | 0xa8 | 0xb8);
    let mut cyr_letters = 0usize;
    let mut adjacent_cyr_pairs = 0usize;
    let mut latin_adj_high = 0usize;
    for (idx, &b) in bytes.iter().enumerate() {
        if is_cyr_byte(b) {
            cyr_letters += 1;
            if idx + 1 < bytes.len() && is_cyr_byte(bytes[idx + 1]) {
                adjacent_cyr_pairs += 1;
            }
            let prev_latin = idx > 0 && bytes[idx - 1].is_ascii_alphabetic();
            let next_latin = idx + 1 < bytes.len() && bytes[idx + 1].is_ascii_alphabetic();
            if prev_latin || next_latin {
                latin_adj_high += 1;
            }
        }
    }
    adjacent_cyr_pairs > latin_adj_high || (cyr_letters >= 2 && latin_adj_high == 0)
}

pub(crate) fn decode_actionscript_source_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<String, String> {
    if bytes.starts_with(b"\x00\x05\x16\x00") {
        return Err(format!(
            "{} is a binary AppleSingle archive (magic 0x00051600), not an ActionScript (.as) source file",
            path.display()
        ));
    }
    if bytes.starts_with(b"\x00\x05\x16\x07") {
        return Err(format!(
            "{} is a binary AppleDouble resource-fork file (magic 0x00051607), not an ActionScript (.as) source file",
            path.display()
        ));
    }
    if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
        let little_endian = bytes.starts_with(b"\xff\xfe");
        let payload = &bytes[2..];
        if !payload.len().is_multiple_of(2) {
            return Err(format!(
                "{} contains a truncated UTF-16 byte sequence",
                path.display()
            ));
        }
        let units = payload.as_chunks::<2>().0.iter().map(|&pair| {
            if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        let mut decoded = String::with_capacity(payload.len() / 2);
        for res in char::decode_utf16(units) {
            let ch = res.map_err(|_| {
                format!(
                    "{} contains invalid UTF-16 ActionScript text",
                    path.display()
                )
            })?;
            if ch == '\0' {
                return Err(format!(
                    "{} appears to be a binary file (contains NUL bytes), not ActionScript text",
                    path.display()
                ));
            }
            decoded.push(ch);
        }
        let stripped = decoded
            .strip_prefix('\u{feff}')
            .map_or(decoded.as_str(), |s| s)
            .to_owned();
        return Ok(normalize_actionscript_line_endings(stripped));
    }
    if bytes.contains(&0) {
        return Err(format!(
            "{} appears to be a binary file (contains NUL bytes), not ActionScript text",
            path.display()
        ));
    }
    let body = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if let Ok(utf8) = std::str::from_utf8(body) {
        let clean = utf8.strip_prefix('\u{feff}').unwrap_or(utf8).to_owned();
        return Ok(normalize_actionscript_line_endings(clean));
    }

    let legacy_body = body.strip_suffix(b"\x1a").unwrap_or(body);
    if legacy_body
        .iter()
        .any(|&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
    {
        return Err(format!(
            "{} appears to be a binary file, not valid ActionScript text",
            path.display()
        ));
    }
    let use_cp1251 = prefers_cp1251_over_cp1252(legacy_body);
    let mut decoded = String::with_capacity(legacy_body.len() * 2);
    for &b in legacy_body {
        let ch = if b < 0x80 {
            char::from(b)
        } else if use_cp1251 {
            CP1251_HIGH[(b - 0x80) as usize]
        } else if b < 0xa0 {
            CP1252_C1[(b - 0x80) as usize]
        } else {
            char::from(b)
        };
        if ch == '\u{fffd}' {
            return Err(format!(
                "{} contains undecodable byte 0x{b:02X} for ActionScript text",
                path.display()
            ));
        }
        decoded.push(ch);
    }
    Ok(normalize_actionscript_line_endings(decoded))
}

fn read_utf8_as_file_bounded(path: &Path, max_bytes: usize) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if meta.len() > max_bytes as u64 {
        return Err(format!(
            "{} exceeds the 1 MiB ActionScript source file limit",
            path.display()
        ));
    }
    let mut bytes = Vec::with_capacity((meta.len() as usize).min(max_bytes));
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "{} exceeds the 1 MiB ActionScript source file limit",
            path.display()
        ));
    }
    decode_actionscript_source_bytes(path, &bytes)
}

pub(crate) fn has_actionscript_include_extension(path: &Path) -> bool {
    path.file_stem().is_some_and(|stem| !stem.is_empty())
        && path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("as") || ext.eq_ignore_ascii_case("asi"))
}

pub(crate) fn resolve_include_logical_path_as_or_asi(
    parent: &str,
    requested: &str,
) -> Result<String, String> {
    if requested.is_empty() || requested.starts_with('/') || requested.contains(['\\', ':', '\0']) {
        return Err("#include requires a relative project resource path using '/'".into());
    }
    let mut parts: Vec<&str> = parent
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
    if !has_actionscript_include_extension(Path::new(&result)) {
        return Err("#include expects an .as or .asi project resource".into());
    }
    Ok(result)
}

fn normalize_relative_include_logical_path(logical_path: &str) -> Option<String> {
    let normalized = logical_path.trim().replace('\\', "/");
    resolve_include_logical_path_as_or_asi("", &normalized).ok()
}

/// Returns `true` if any existing path segment of `clean_rel` inside `base_dir` is a symlink
/// (allowing `base_dir` itself to be a symlinked root such as `/tmp -> private/tmp`).
fn path_has_symlink_segment_under(base_dir: &Path, clean_rel: &str) -> bool {
    let canon_base = match std::fs::canonicalize(base_dir) {
        Ok(canon) => {
            if !canon.is_dir() {
                return true;
            }
            canon
        }
        Err(_) => {
            return std::fs::symlink_metadata(base_dir).is_ok();
        }
    };
    let mut current = canon_base;
    for segment in clean_rel.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return true;
        }
        let exact = current.join(segment);
        match std::fs::symlink_metadata(&exact) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return true;
                }
                if !meta.is_dir() {
                    return false;
                }
                current = exact;
            }
            Err(_) => {
                let Some(entry) = std::fs::read_dir(&current).ok().and_then(|entries| {
                    entries.filter_map(Result::ok).find(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(segment)
                    })
                }) else {
                    return false;
                };
                if entry.file_type().is_ok_and(|ft| ft.is_symlink()) {
                    return true;
                }
                let next_path = entry.path();
                let Ok(meta) = std::fs::symlink_metadata(&next_path) else {
                    return true;
                };
                if meta.file_type().is_symlink() {
                    return true;
                }
                if !meta.is_dir() {
                    return false;
                }
                current = next_path;
            }
        }
    }
    false
}

fn read_raw_include_bytes_from_disk(
    base_dir: &Path,
    logical_path: &str,
) -> Option<(PathBuf, Vec<u8>)> {
    let resolved = resolve_relative_include_disk_path(base_dir, logical_path)?;
    let lmeta = std::fs::symlink_metadata(&resolved).ok()?;
    if lmeta.file_type().is_symlink()
        || !lmeta.is_file()
        || !has_actionscript_include_extension(&resolved)
    {
        return None;
    }
    let canon_base = std::fs::canonicalize(base_dir).ok()?;
    let canon_resolved = std::fs::canonicalize(&resolved).ok()?;
    if !canon_resolved.starts_with(&canon_base)
        || !has_actionscript_include_extension(&canon_resolved)
    {
        return None;
    }
    let file = std::fs::File::open(&resolved).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    // Cap read to 1 MiB + 1 byte so `includes::expand` emits its exact 1 MiB diagnostic if oversized.
    let cap = MAX_STANDALONE_AS_BYTES + 1;
    let mut bytes = Vec::with_capacity((meta.len() as usize).min(cap));
    file.take(cap as u64).read_to_end(&mut bytes).ok()?;
    if bytes.len() <= MAX_STANDALONE_AS_BYTES
        && let Ok(decoded) = decode_actionscript_source_bytes(&resolved, &bytes)
    {
        return Some((resolved, decoded.into_bytes()));
    }
    Some((resolved, bytes))
}

/// Resolve a relative `.as` / `.asi` path (`"figures.as"` or `"sub/rotate.as"`) to an existing
/// non-symlink disk path canonically contained within `base_dir` (allowing `base_dir` itself to
/// be a symlinked directory such as `/tmp` on macOS while rejecting any symlink inside it).
pub(crate) fn resolve_relative_include_disk_path(
    base_dir: &Path,
    logical_path: &str,
) -> Option<PathBuf> {
    let clean = normalize_relative_include_logical_path(logical_path)?;
    let canon_base = std::fs::canonicalize(base_dir).ok()?;
    if !canon_base.is_dir() {
        return None;
    }
    let segments: Vec<&str> = clean.split('/').collect();
    let mut current = canon_base.clone();
    for (idx, segment) in segments.iter().enumerate() {
        if segment.is_empty() || *segment == "." || *segment == ".." {
            return None;
        }
        let exact = current.join(segment);
        let next_path = match std::fs::symlink_metadata(&exact) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return None;
                }
                exact
            }
            Err(_) => {
                let entry = std::fs::read_dir(&current)
                    .ok()?
                    .filter_map(Result::ok)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(segment)
                    })?;
                if entry.file_type().ok()?.is_symlink() {
                    return None;
                }
                entry.path()
            }
        };
        let meta = std::fs::symlink_metadata(&next_path).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        if idx + 1 < segments.len() {
            if !meta.is_dir() {
                return None;
            }
        } else if !meta.is_file() || !has_actionscript_include_extension(&next_path) {
            return None;
        }
        current = next_path;
    }
    let canon_resolved = std::fs::canonicalize(&current).ok()?;
    if !canon_resolved.starts_with(&canon_base)
        || !has_actionscript_include_extension(&canon_resolved)
    {
        return None;
    }
    let canon_meta = std::fs::symlink_metadata(&canon_resolved).ok()?;
    if canon_meta.file_type().is_symlink() || !canon_meta.is_file() {
        return None;
    }
    Some(current)
}

fn strip_keyword_ws<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    rest.starts_with(char::is_whitespace)
        .then(|| rest.trim_start())
}

fn declared_package_segments(unit: &SourceUnit) -> (Vec<String>, Option<String>) {
    let skeleton = strip_comments_and_strings_for_detection(&unit.source);
    let mut brace_depth = 0usize;
    for line in skeleton.lines() {
        let trimmed = line.trim();
        if brace_depth == 0 {
            if unit.language == ScriptLanguage::ActionScript3
                && let Some(rest) = trimmed.strip_prefix("package")
                && (rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == '{'))
            {
                let pkg = rest.split('{').next().unwrap_or("").trim();
                if pkg.is_empty() {
                    return (Vec::new(), None);
                }
                let segs: Vec<String> = pkg
                    .split('.')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                return (segs, None);
            }
            if unit.language == ScriptLanguage::ActionScript2 {
                let mut decl = trimmed;
                loop {
                    let mut stripped = false;
                    for kw in ["public", "dynamic", "intrinsic"] {
                        if let Some(next) = strip_keyword_ws(decl, kw) {
                            decl = next;
                            stripped = true;
                        }
                    }
                    if !stripped {
                        break;
                    }
                }
                if let Some(after_kw) =
                    strip_keyword_ws(decl, "class").or_else(|| strip_keyword_ws(decl, "interface"))
                {
                    let qname = after_kw
                        .split(|c: char| c.is_whitespace() || c == '{')
                        .next()
                        .unwrap_or("")
                        .trim();
                    let parts: Vec<&str> = qname.split('.').filter(|s| !s.is_empty()).collect();
                    if !parts.is_empty() {
                        let expected_as2_path = format!("{}.as", parts.join("/"));
                        let pkg_segs = parts[..parts.len() - 1]
                            .iter()
                            .map(|s| (*s).to_owned())
                            .collect();
                        return (pkg_segs, Some(expected_as2_path));
                    }
                }
            }
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    (Vec::new(), None)
}

fn required_parent_hops_for_script(language: ScriptLanguage, source: &str) -> usize {
    let script = Script {
        language,
        source: source.to_owned(),
    };
    let mut max_hops = 0usize;
    for requested in AuthoredPlayback::scan_script_include_directives(&script) {
        let mut depth = 0isize;
        let mut min_depth = 0isize;
        for part in requested.trim().replace('\\', "/").split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    depth -= 1;
                    min_depth = min_depth.min(depth);
                }
                _ => depth += 1,
            }
        }
        max_hops = max_hops.max((-min_depth) as usize);
    }
    max_hops.min(16)
}

fn matched_trailing_dir_hops(file_dir: &Path, pkg_segments: &[String]) -> usize {
    if pkg_segments.is_empty() {
        return 0;
    }
    let mut cur = file_dir;
    for expected in pkg_segments.iter().rev() {
        let Some(name) = cur.file_name().and_then(|s| s.to_str()) else {
            return 0;
        };
        if !name.eq_ignore_ascii_case(expected) {
            return 0;
        }
        let Some(parent) = cur.parent().filter(|p| !p.as_os_str().is_empty()) else {
            return 0;
        };
        if parent.parent().is_none() {
            return 0;
        }
        cur = parent;
    }
    pkg_segments.len()
}

fn walk_up_non_symlink_ancestors(
    file_dir: &Path,
    file_name: &str,
    hops: usize,
) -> (PathBuf, String) {
    let mut cur = file_dir.to_path_buf();
    let mut rel_dirs: Vec<String> = Vec::new();
    for _ in 0..hops {
        let Ok(cur_meta) = std::fs::symlink_metadata(&cur) else {
            break;
        };
        if cur_meta.file_type().is_symlink() || !cur_meta.is_dir() {
            break;
        }
        let Some(dir_name) = cur
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
        else {
            break;
        };
        let Some(parent) = cur.parent().filter(|p| !p.as_os_str().is_empty()) else {
            break;
        };
        // Never anchor at filesystem root `/`.
        if parent.parent().is_none() {
            break;
        }
        if !parent.is_dir() {
            break;
        }
        let (Ok(canon_cur), Ok(canon_parent)) =
            (std::fs::canonicalize(&cur), std::fs::canonicalize(parent))
        else {
            break;
        };
        if !canon_cur.starts_with(&canon_parent) {
            break;
        }
        rel_dirs.push(dir_name.to_owned());
        cur = parent.to_path_buf();
    }
    if rel_dirs.is_empty() {
        (cur, file_name.to_owned())
    } else {
        rel_dirs.reverse();
        (cur, format!("{}/{file_name}", rel_dirs.join("/")))
    }
}

fn relative_include_from_source_path(to_logical_path: &str, target_root_rel: &str) -> String {
    let to_parent_parts: Vec<&str> = to_logical_path
        .rsplit_once('/')
        .map_or("", |(base, _)| base)
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    let target_parts: Vec<&str> = target_root_rel
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    let target_dir_len = target_parts.len().saturating_sub(1);
    let common = to_parent_parts
        .iter()
        .zip(&target_parts[..target_dir_len])
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<&str> =
        Vec::with_capacity((to_parent_parts.len() - common) + (target_parts.len() - common));
    out.extend(std::iter::repeat_n("..", to_parent_parts.len() - common));
    out.extend_from_slice(&target_parts[common..]);
    out.join("/")
}

fn advance_block_comment_in_line(line: &str, block: &mut bool) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
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

fn split_lines_preserving_endings(source: &str) -> impl Iterator<Item = (&str, &str)> {
    let bytes = source.as_bytes();
    let mut start = 0usize;
    std::iter::from_fn(move || {
        if start >= bytes.len() {
            return None;
        }
        let mut i = start;
        let mut ending_start = start;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    ending_start = i;
                    i += 1;
                    break;
                }
                b'\r' => {
                    ending_start = i;
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'\n' {
                        i += 1;
                    }
                    break;
                }
                _ => {
                    i += 1;
                    ending_start = i;
                }
            }
        }
        let body = &source[start..ending_start];
        let ending = &source[ending_start..i];
        start = i;
        Some((body, ending))
    })
}

/// Rewrite `#include "..."` / AS3 `include "..."` directives resolved from `from_logical_path`
/// so they resolve to the identical root-relative `ResourceUnit` paths from `to_logical_path`,
/// preserving exact line numbers and line endings.
pub(crate) fn rewrite_includes_between_logical_paths(
    language: ScriptLanguage,
    source: &str,
    from_logical_path: &str,
    to_logical_path: &str,
) -> String {
    let from_parent = from_logical_path.rsplit_once('/').map_or("", |(b, _)| b);
    let to_parent = to_logical_path.rsplit_once('/').map_or("", |(b, _)| b);
    if from_parent == to_parent || !source.contains("include") || !language.is_actionscript() {
        return source.to_owned();
    }
    let is_as3 = language == ScriptLanguage::ActionScript3;
    let mut in_block_comment = false;
    let mut out = String::with_capacity(source.len());

    for (line_idx, (body, ending)) in split_lines_preserving_endings(source).enumerate() {
        let mut cursor = if line_idx == 0 && body.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };

        let mut rewritten_line: Option<String> = None;
        'parse: {
            if in_block_comment {
                let Some(rel_close) = body[cursor..].find("*/") else {
                    break 'parse;
                };
                cursor += rel_close + 2;
            }
            loop {
                while cursor < body.len() {
                    let ch = body[cursor..].chars().next().unwrap();
                    if ch.is_whitespace() {
                        cursor += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                if body[cursor..].starts_with("/*") {
                    let Some(rel_close) = body[cursor + 2..].find("*/") else {
                        break 'parse;
                    };
                    cursor += 2 + rel_close + 2;
                    continue;
                }
                break;
            }

            let candidate = &body[cursor..];
            let kw_len = if let Some(rest) = candidate.strip_prefix("#include") {
                let boundary_ok = rest.is_empty()
                    || rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    || rest.starts_with("/*")
                    || rest.starts_with("//");
                if boundary_ok || !is_as3 {
                    Some("#include".len())
                } else {
                    None
                }
            } else if is_as3
                && let Some(rest) = candidate.strip_prefix("include")
                && (rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    || rest.starts_with("/*"))
            {
                Some("include".len())
            } else {
                None
            };
            let Some(kw_len) = kw_len else {
                break 'parse;
            };
            cursor += kw_len;

            loop {
                while cursor < body.len() {
                    let ch = body[cursor..].chars().next().unwrap();
                    if ch.is_whitespace() {
                        cursor += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                if body[cursor..].starts_with("/*") {
                    let Some(rel_close) = body[cursor + 2..].find("*/") else {
                        break 'parse;
                    };
                    cursor += 2 + rel_close + 2;
                    continue;
                }
                break;
            }
            if cursor >= body.len() {
                break 'parse;
            }

            let quote_ch = match body.as_bytes()[cursor] {
                b'"' => '"',
                b'\'' => '\'',
                _ => break 'parse,
            };
            let req_start = cursor + 1;
            let Some(close_rel) = body[req_start..].find(quote_ch) else {
                break 'parse;
            };
            let req_end = req_start + close_rel;
            let raw_req = &body[req_start..req_end];
            if raw_req.contains(['"', '\'', '\\']) {
                break 'parse;
            }
            if let Ok(target_root_rel) =
                resolve_include_logical_path_as_or_asi(from_logical_path, raw_req)
            {
                let rel_req = relative_include_from_source_path(to_logical_path, &target_root_rel);
                let mut line_buf =
                    String::with_capacity(body.len() - raw_req.len() + rel_req.len());
                line_buf.push_str(&body[..req_start]);
                line_buf.push_str(&rel_req);
                line_buf.push_str(&body[req_end..]);
                rewritten_line = Some(line_buf);
            }
        }

        advance_block_comment_in_line(body, &mut in_block_comment);
        out.push_str(rewritten_line.as_deref().unwrap_or(body));
        out.push_str(ending);
    }

    out
}

/// Infer `(ancestor_base_dir, logical_source_path)` for a standalone `.as` file on disk so
/// package-nested classes (`package com.foo`, `class com.foo.Player`) and parent-relative
/// `#include "../shared/consts.as"` directives resolve cleanly within non-symlink parent folders
/// without mutating `self.project.sources[0].path`.
pub(crate) fn standalone_script_include_context(
    disk_path: &Path,
    unit: &SourceUnit,
) -> Option<(PathBuf, String)> {
    let file_dir = disk_path.parent().filter(|p| !p.as_os_str().is_empty())?;
    let file_name = sanitize_source_unit_filename(disk_path);
    let (pkg_segments, _) = declared_package_segments(unit);
    let pkg_hops = matched_trailing_dir_hops(file_dir, &pkg_segments);
    let inc_hops = required_parent_hops_for_script(unit.language, &unit.source);
    let desired_hops = pkg_hops.max(inc_hops);
    Some(walk_up_non_symlink_ancestors(
        file_dir,
        &file_name,
        desired_hops,
    ))
}

/// Recursively resolve `#include` / AS3 `include` directives across all `FrameScript`s,
/// `ObjectScript`s, and `.as` `SourceUnit`s in `project` using the 4-tier priority order:
/// 1. Open standalone `.as` session tabs (`open_as_tabs`: `(disk_path, live_source)`),
/// 2. Existing `project.resources`,
/// 3. Matching `.as` `project.sources`,
/// 4. Relative `.as` files on disk under `base_dir` (up to 32 levels, `<= 1 MiB`).
///
/// Returns `(hydrated_project, hydration_fingerprint)` without mutating the authored `Project`.
pub(crate) fn hydrate_project_includes_with_overrides<'a>(
    project: &'a Project,
    base_dir: Option<&Path>,
    open_as_tabs: &[(PathBuf, String)],
) -> (Cow<'a, Project>, u64) {
    let mut queue: VecDeque<(String, Script, usize)> = VecDeque::new();
    let mut any_include_candidate = false;

    for sym in &project.symbols {
        for fs in &sym.timeline.scripts {
            if fs.script.language.is_actionscript() && fs.script.source.contains("include") {
                any_include_candidate = true;
                queue.push_back((String::new(), fs.script.clone(), 0));
            }
        }
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        if os.script.language.is_actionscript()
                            && os.script.source.contains("include")
                        {
                            any_include_candidate = true;
                            queue.push_back((String::new(), os.script.clone(), 0));
                        }
                    }
                }
            }
        }
    }
    for unit in &project.sources {
        if unit.language.is_actionscript() && unit.source.contains("include") {
            any_include_candidate = true;
            queue.push_back((
                unit.path.clone(),
                Script {
                    language: unit.language,
                    source: unit.source.clone(),
                },
                0,
            ));
        }
    }

    if !any_include_candidate {
        return (Cow::Borrowed(project), 0);
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut staged_overrides: Vec<(String, Vec<u8>)> = Vec::new();
    let mut fp_hasher = std::collections::hash_map::DefaultHasher::new();

    let match_open_tab = |logical: &str| -> Option<&str> {
        if open_as_tabs.is_empty() {
            return None;
        }
        if let Some(dir) = base_dir {
            if !path_has_symlink_segment_under(dir, logical) {
                let expected = dir.join(logical);
                if let Some((_, src)) = open_as_tabs
                    .iter()
                    .find(|(p, _)| crate::document_sessions::same_path(p, &expected))
                {
                    return Some(src.as_str());
                }
            }
            if let Some(disk_existing) = resolve_relative_include_disk_path(dir, logical)
                && let Some((_, src)) = open_as_tabs
                    .iter()
                    .find(|(p, _)| crate::document_sessions::same_path(p, &disk_existing))
            {
                return Some(src.as_str());
            }
        } else if !logical.contains('/') {
            let mut matches = open_as_tabs.iter().filter(|(p, _)| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case(logical))
            });
            if let Some((_, first_src)) = matches.next()
                && matches.next().is_none()
            {
                return Some(first_src.as_str());
            }
        }
        None
    };

    while let Some((parent, script, depth)) = queue.pop_front() {
        if depth >= MAX_HYDRATED_INCLUDE_DEPTH {
            continue;
        }
        for requested in AuthoredPlayback::scan_script_include_directives(&script) {
            let Ok(logical) = resolve_include_logical_path_as_or_asi(&parent, &requested) else {
                continue;
            };
            let visit_key = logical.to_ascii_lowercase();
            if !visited.insert(visit_key) {
                continue;
            }

            // Tier 1: Open standalone `.as` tab matching the resolved disk path.
            if let Some(tab_source) = match_open_tab(&logical) {
                let bytes = tab_source.as_bytes().to_vec();
                "tab".hash(&mut fp_hasher);
                logical.hash(&mut fp_hasher);
                bytes.hash(&mut fp_hasher);
                staged_overrides.push((logical.clone(), bytes));
                if tab_source.contains("include") {
                    queue.push_back((
                        logical,
                        Script {
                            language: script.language,
                            source: tab_source.to_owned(),
                        },
                        depth + 1,
                    ));
                }
                continue;
            }

            // Tier 2: Existing `project.resources`.
            if let Some(existing_res) = project
                .resources
                .iter()
                .find(|r| r.path == logical || r.path.eq_ignore_ascii_case(&logical))
            {
                "res".hash(&mut fp_hasher);
                logical.hash(&mut fp_hasher);
                existing_res.data_base64.hash(&mut fp_hasher);
                if let Ok(bytes) = existing_res.decoded_bytes()
                    && bytes.len() <= MAX_STANDALONE_AS_BYTES
                    && let Ok(text) = String::from_utf8(bytes)
                    && text.contains("include")
                {
                    queue.push_back((
                        logical,
                        Script {
                            language: script.language,
                            source: text,
                        },
                        depth + 1,
                    ));
                }
                continue;
            }

            // Tier 3: Matching `.as` `project.sources`.
            if let Some(unit) = project.sources.iter().find(|u| {
                let norm = u.path.trim().replace('\\', "/");
                norm == logical || norm.eq_ignore_ascii_case(&logical)
            }) {
                let bytes = unit.source.as_bytes().to_vec();
                "src".hash(&mut fp_hasher);
                logical.hash(&mut fp_hasher);
                bytes.hash(&mut fp_hasher);
                staged_overrides.push((logical.clone(), bytes));
                if unit.source.contains("include") {
                    queue.push_back((
                        logical,
                        Script {
                            language: script.language,
                            source: unit.source.clone(),
                        },
                        depth + 1,
                    ));
                }
                continue;
            }

            // Tier 4: Relative `.as` file on disk under `base_dir`.
            if let Some(dir) = base_dir
                && let Some((_, bytes)) = read_raw_include_bytes_from_disk(dir, &logical)
            {
                "disk".hash(&mut fp_hasher);
                logical.hash(&mut fp_hasher);
                bytes.hash(&mut fp_hasher);
                if bytes.len() <= MAX_STANDALONE_AS_BYTES
                    && let Ok(text) = std::str::from_utf8(&bytes)
                    && text.contains("include")
                {
                    queue.push_back((
                        logical.clone(),
                        Script {
                            language: script.language,
                            source: text.to_owned(),
                        },
                        depth + 1,
                    ));
                }
                staged_overrides.push((logical, bytes));
            } else {
                "missing".hash(&mut fp_hasher);
                logical.hash(&mut fp_hasher);
            }
        }
    }

    let fingerprint = fp_hasher.finish();
    if staged_overrides.is_empty() {
        return (Cow::Borrowed(project), fingerprint);
    }

    let mut hydrated = project.clone();
    if hydrated.format_version < 3 {
        hydrated.format_version = 3;
    }
    for (idx, (logical, bytes)) in staged_overrides.into_iter().enumerate() {
        hydrated
            .resources
            .retain(|r| r.path != logical && !r.path.eq_ignore_ascii_case(&logical));
        if let Ok(res) = ResourceUnit::from_bytes(
            format!("hydrated-include-{idx}"),
            logical,
            "text/plain",
            &bytes,
        ) {
            hydrated.resources.push(res);
        }
    }
    (Cow::Owned(hydrated), fingerprint)
}

/// Convenience path-based helper for CLI (`--headless`) and batch exports.
pub(crate) fn hydrate_project_includes_from_disk<'a>(
    project: &'a Project,
    base_dir: Option<&Path>,
) -> Cow<'a, Project> {
    hydrate_project_includes_with_overrides(project, base_dir, &[]).0
}

const MAX_HYDRATED_SIBLING_SOURCES: usize = 256;

fn is_declarative_as3_metadata_name(name: &str) -> bool {
    // Keep `[Embed]` untouched so asset-embedding metadata continues to produce an explicit diagnostic.
    !name.eq_ignore_ascii_case("Embed")
        && matches!(
            name,
            "SWF"
                | "Event"
                | "Bindable"
                | "Style"
                | "Inspectable"
                | "DefaultProperty"
                | "ExcludeClass"
                | "Exclude"
                | "Deprecated"
                | "HostComponent"
                | "SkinState"
                | "SkinPart"
                | "ArrayElementType"
                | "InstanceType"
                | "Transient"
                | "IconFile"
                | "RemoteClass"
                | "NonCommittingChangeEvent"
                | "PercentProxy"
                | "RichTextContent"
                | "CollapseWhiteSpace"
        )
}

/// Blank declarative AS3 metadata brackets (`[SWF(...)]`, `[Event(...)]`, `[Bindable]`, etc.) at
/// package/class/member declaration level, replacing every non-newline byte with `' '` so exact
/// line and column numbers stay 100% preserved.
fn blank_declarative_as3_metadata(source: &str) -> String {
    if !source.contains('[') {
        return source.to_owned();
    }
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0usize;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut block_comment = false;
    let mut line_comment = false;
    let mut quote: Option<u8> = None;
    let mut stmt_start = true;

    while i < bytes.len() {
        let b = bytes[i];
        if block_comment {
            if bytes[i..].starts_with(b"*/") {
                block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if line_comment {
            if b == b'\n' {
                line_comment = false;
            }
            i += 1;
            continue;
        }
        if let Some(q) = quote {
            if b == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            block_comment = true;
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            line_comment = true;
            i += 2;
            continue;
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
            stmt_start = false;
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'[' && paren_depth == 0 && brace_depth <= 2 && stmt_start {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let ident_start = j;
            if j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                let meta_name = &source[ident_start..j];
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let mut valid = false;
                if j < bytes.len() && bytes[j] == b']' {
                    j += 1;
                    valid = true;
                } else if j < bytes.len() && bytes[j] == b'(' {
                    let mut pdepth = 1usize;
                    let mut q2: Option<u8> = None;
                    j += 1;
                    while j < bytes.len() && pdepth > 0 {
                        let c2 = bytes[j];
                        if let Some(qq) = q2 {
                            if c2 == b'\\' {
                                j = (j + 2).min(bytes.len());
                                continue;
                            }
                            if c2 == qq {
                                q2 = None;
                            }
                        } else if c2 == b'"' || c2 == b'\'' {
                            q2 = Some(c2);
                        } else if c2 == b'(' {
                            pdepth += 1;
                        } else if c2 == b')' {
                            pdepth -= 1;
                        }
                        j += 1;
                    }
                    if pdepth == 0 {
                        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b']' {
                            j += 1;
                            valid = true;
                        }
                    }
                }
                if valid && is_declarative_as3_metadata_name(meta_name) {
                    let mut end = j;
                    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t') {
                        end += 1;
                    }
                    if end < bytes.len() && bytes[end] == b';' {
                        j = end + 1;
                    }
                    for slot in &mut out[i..j] {
                        if *slot != b'\n' {
                            *slot = b' ';
                        }
                    }
                    i = j;
                    stmt_start = true;
                    continue;
                }
            }
        }
        match b {
            b'{' => {
                brace_depth += 1;
                stmt_start = true;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                stmt_start = true;
            }
            b';' => {
                stmt_start = true;
            }
            b'(' => {
                paren_depth += 1;
                stmt_start = false;
            }
            b')' => {
                paren_depth = paren_depth.saturating_sub(1);
                stmt_start = false;
            }
            _ => {
                stmt_start = false;
            }
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| source.to_owned())
}

/// Split an AS3 `package ... { ... }` unit that also declares file-private helper classes/interfaces
/// after the package block (e.g. `internal class RoupePoint extends Point` in `Step05SmoothCurve.as`)
/// into `(primary_package_source, vec![padded_helper_package_sources...])`.
fn split_as3_package_and_helpers(source: &str, pkg: &str) -> (String, Vec<String>) {
    let bytes = source.as_bytes();
    let mut i = 0usize;
    let mut brace_depth = 0usize;
    let mut saw_pkg_open = false;
    let mut pkg_close_end: Option<usize> = None;
    let mut block_comment = false;
    let mut line_comment = false;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if block_comment {
            if bytes[i..].starts_with(b"*/") {
                block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if line_comment {
            if b == b'\n' {
                line_comment = false;
            }
            i += 1;
            continue;
        }
        if let Some(q) = quote {
            if b == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            block_comment = true;
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            line_comment = true;
            i += 2;
            continue;
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
            i += 1;
            continue;
        }
        if b == b'{' {
            brace_depth += 1;
            saw_pkg_open = true;
        } else if b == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
            if saw_pkg_open && brace_depth == 0 {
                pkg_close_end = Some(i + 1);
                break;
            }
        }
        i += 1;
    }
    let Some(split_at) = pkg_close_end else {
        return (source.to_owned(), Vec::new());
    };
    let tail = &source[split_at..];
    let tail_skeleton = strip_comments_and_strings_for_detection(tail);
    if tail_skeleton.trim().is_empty() {
        return (source.to_owned(), Vec::new());
    }

    // Collect top-level `import ...;` statements in the out-of-package tail, and slice each
    // top-level helper class/interface block `{ ... }` into its own line-aligned companion unit.
    let mut tail_imports = Vec::new();
    let mut d = 0usize;
    for line in tail_skeleton.lines() {
        let trimmed = line.trim();
        if d == 0
            && let Some(rest) = trimmed.strip_prefix("import")
            && rest.starts_with(char::is_whitespace)
        {
            let target = rest.trim().trim_end_matches(';').trim();
            if !target.is_empty() {
                tail_imports.push(format!("import {target};"));
            }
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => d += 1,
                '}' => d = d.saturating_sub(1),
                _ => {}
            }
        }
    }

    let mut helpers = Vec::new();
    let tail_bytes = tail.as_bytes();
    let mut ti = 0usize;
    let mut td = 0usize;
    let mut t_block = false;
    let mut t_line = false;
    let mut t_quote: Option<u8> = None;
    let mut decl_start: Option<usize> = None;
    while ti < tail_bytes.len() {
        let b = tail_bytes[ti];
        if t_block {
            if tail_bytes[ti..].starts_with(b"*/") {
                t_block = false;
                ti += 2;
            } else {
                ti += 1;
            }
            continue;
        }
        if t_line {
            if b == b'\n' {
                t_line = false;
            }
            ti += 1;
            continue;
        }
        if let Some(q) = t_quote {
            if b == b'\\' {
                ti = (ti + 2).min(tail_bytes.len());
                continue;
            }
            if b == q {
                t_quote = None;
            }
            ti += 1;
            continue;
        }
        if tail_bytes[ti..].starts_with(b"/*") {
            t_block = true;
            ti += 2;
            continue;
        }
        if tail_bytes[ti..].starts_with(b"//") {
            t_line = true;
            ti += 2;
            continue;
        }
        if b == b'"' || b == b'\'' {
            t_quote = Some(b);
            ti += 1;
            continue;
        }
        if td == 0 && decl_start.is_none() && (b.is_ascii_alphabetic() || b == b'_') {
            let word_end = tail[ti..]
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .map_or(tail.len(), |off| ti + off);
            let word = &tail[ti..word_end];
            if word == "import" {
                while ti < tail_bytes.len() && tail_bytes[ti] != b';' && tail_bytes[ti] != b'\n' {
                    ti += 1;
                }
                if ti < tail_bytes.len() {
                    ti += 1;
                }
                continue;
            }
            if matches!(
                word,
                "internal" | "public" | "dynamic" | "final" | "class" | "interface"
            ) {
                decl_start = Some(ti);
            }
        }
        if b == b'{' {
            td += 1;
        } else if b == b'}' {
            td = td.saturating_sub(1);
            if td == 0
                && let Some(start_rel) = decl_start.take()
            {
                let end_rel = ti + 1;
                let abs_start = split_at + start_rel;
                let newlines_before = source[..abs_start].bytes().filter(|&c| c == b'\n').count();
                let pkg_open = if pkg.is_empty() {
                    "package {".to_owned()
                } else {
                    format!("package {pkg} {{")
                };
                let imports_inline = tail_imports.join(" ");
                let mut buf = String::with_capacity(end_rel - start_rel + newlines_before + 128);
                buf.push_str(&pkg_open);
                if !imports_inline.is_empty() {
                    buf.push(' ');
                    buf.push_str(&imports_inline);
                }
                for _ in 0..newlines_before {
                    buf.push('\n');
                }
                if newlines_before == 0 {
                    buf.push(' ');
                }
                buf.push_str(&tail[start_rel..end_rel]);
                buf.push_str("\n}\n");
                helpers.push(buf);
            }
        }
        ti += 1;
    }

    if helpers.is_empty() {
        return (source.to_owned(), Vec::new());
    }
    (source[..split_at].to_owned(), helpers)
}

/// Build a fast, side-effect-free signature stub for an external hydrated sibling `.as` class or
/// interface (`sources[1..]`), replacing member `{ ... }` bodies with `{}` so sibling method-body
/// constructs or style warnings never bleed into the active `.as` tab.
fn stub_external_sibling_class_source(language: ScriptLanguage, source: &str) -> String {
    let skeleton = strip_comments_and_strings_for_detection(source);
    let is_packaged = skeleton.lines().any(|line| {
        let t = line.trim();
        t.strip_prefix("package")
            .is_some_and(|r| r.is_empty() || r.starts_with(|c: char| c.is_whitespace() || c == '{'))
    });
    let target_body_depth = if is_packaged { 3usize } else { 2usize };

    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len() / 2 + 64);
    let mut i = 0usize;
    let mut brace_depth = 0usize;
    let mut block_comment = false;
    let mut line_comment = false;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if block_comment {
            if bytes[i..].starts_with(b"*/") {
                block_comment = false;
                if brace_depth < target_body_depth {
                    out.push(' ');
                }
                i += 2;
            } else {
                if b == b'\n' && brace_depth < target_body_depth {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        if line_comment {
            if b == b'\n' {
                line_comment = false;
                if brace_depth < target_body_depth {
                    out.push('\n');
                }
            }
            i += 1;
            continue;
        }
        if let Some(q) = quote {
            if b == b'\\' {
                let next = (i + 2).min(bytes.len());
                if brace_depth < target_body_depth {
                    out.push_str(&source[i..next]);
                }
                i = next;
                continue;
            }
            if b == q {
                quote = None;
            }
            if brace_depth < target_body_depth {
                let ch = source[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            } else {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            block_comment = true;
            if brace_depth < target_body_depth {
                out.push(' ');
            }
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            line_comment = true;
            i += 2;
            continue;
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
            if brace_depth < target_body_depth {
                out.push(char::from(b));
            }
            i += 1;
            continue;
        }
        if b == b'{' {
            brace_depth += 1;
            if brace_depth == target_body_depth {
                let head_start = out.rfind([';', '{', '}']).map_or(0, |idx| idx + 1);
                let member_head = out[head_start..].trim();
                let needs_throw_stub = language == ScriptLanguage::ActionScript3
                    && member_head.contains("function")
                    && member_head
                        .rsplit_once(')')
                        .and_then(|(_, after)| after.trim().strip_prefix(':'))
                        .map(str::trim)
                        .is_some_and(|ret_ty| {
                            !ret_ty.is_empty() && ret_ty != "void" && ret_ty != "Void"
                        });
                if needs_throw_stub {
                    out.push_str("{ throw null; }");
                } else {
                    out.push_str("{}");
                }
            } else if brace_depth < target_body_depth {
                out.push('{');
            }
            i += 1;
            continue;
        }
        if b == b'}' {
            if brace_depth < target_body_depth {
                out.push('}');
            }
            brace_depth = brace_depth.saturating_sub(1);
            i += 1;
            continue;
        }
        if brace_depth < target_body_depth {
            let ch = source[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        } else {
            i += 1;
        }
    }

    // Strip strictly class-private AS3 member declarations and `override` re-declarations from
    // external sibling stubs: AS3 `private` traits are never visible outside their own class, and
    // `override` members already exist with identical signatures on the base class (avoiding
    // `redeclaring native member 'x'` errors when a sibling overrides `Sprite.x` / `toString`).
    if language == ScriptLanguage::ActionScript3 {
        let mut filtered = String::with_capacity(out.len());
        for line in out.lines() {
            let trimmed = line.trim();
            let is_member_stub_line = trimmed.ends_with(';')
                || trimmed.ends_with("{}")
                || trimmed.ends_with("{ throw null; }");
            let is_private_or_override = trimmed.starts_with("private ")
                || trimmed.starts_with("static private ")
                || trimmed.starts_with("override ")
                || trimmed.starts_with("public override ")
                || trimmed.starts_with("protected override ")
                || trimmed.starts_with("internal override ")
                || trimmed.starts_with("final override ");
            if is_member_stub_line && is_private_or_override {
                continue;
            }
            filtered.push_str(line);
            filtered.push('\n');
        }
        out = filtered;
    }

    // If an AS3 sibling class extends a non-subclassable native leaf display type (such as
    // `TextField` in `GridInput extends TextField` or `Bitmap`), rewrite the sibling stub header
    // to `dynamic ... extends flash.display.Sprite` so callers can pass it to `addChild(...)` and
    // read/write leaf properties cleanly without affecting checks on `GridInput.as` itself.
    if language == ScriptLanguage::ActionScript3 {
        for leaf in [
            "flash.text.TextField",
            "TextField",
            "flash.display.Bitmap",
            "Bitmap",
        ] {
            let needle = format!("extends {leaf}");
            if let Some(pos) = out.find(&needle) {
                let after = &out[pos + needle.len()..];
                if after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == '{') {
                    out.replace_range(pos..pos + needle.len(), "extends flash.display.Sprite");
                    if let Some(class_pos) = out[..pos].rfind("class ") {
                        let prefix = &out[..class_pos];
                        if !prefix
                            .split_whitespace()
                            .rev()
                            .take(3)
                            .any(|w| w == "dynamic")
                        {
                            out.insert_str(class_pos, "dynamic ");
                        }
                    }
                    if let Some(pkg_brace) = out.find('{') {
                        out.insert_str(pkg_brace + 1, "\nimport flash.display.Sprite;\n");
                    }
                    break;
                }
            }
        }
    }

    // Prune exact `import pkg.Type;` lines from the external sibling stub when `Type` was only
    // referenced inside stripped method bodies/private fields, preventing `[unused-import]` lints
    // on hydrated sibling stubs.
    let mut non_import_body = String::with_capacity(out.len());
    for line in out.lines() {
        let t = line.trim();
        let is_import_line = t
            .strip_prefix("import")
            .is_some_and(|r| r.starts_with(char::is_whitespace));
        if !is_import_line {
            non_import_body.push_str(line);
            non_import_body.push('\n');
        }
    }
    let (_, _, used_idents) = extract_imports_and_identifiers(language, &non_import_body);
    let mut seen_imports: HashSet<String> = HashSet::new();
    let mut pruned = String::with_capacity(out.len());
    for line in out.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("import")
            && rest.starts_with(char::is_whitespace)
        {
            let target = rest.trim().trim_end_matches(';').trim();
            if !target.ends_with(".*") {
                let short = target.rsplit('.').next().unwrap_or("");
                if short.is_empty()
                    || !used_idents.contains(short)
                    || !seen_imports.insert(target.to_owned())
                {
                    continue;
                }
            }
        }
        pruned.push_str(line);
        pruned.push('\n');
    }
    normalize_sibling_stub_signatures(&pruned)
}

fn normalize_sibling_stub_signatures(stub: &str) -> String {
    // Extract the stub's own short class name so legacy `function ClassName(...) : void {}`
    // constructor return annotations are stripped from external sibling stubs.
    let mut class_short = String::new();
    for line in stub.lines() {
        let mut t = line.trim();
        for kw in ["public ", "internal ", "dynamic ", "final ", "intrinsic "] {
            while let Some(rest) = t.strip_prefix(kw) {
                t = rest.trim_start();
            }
        }
        if let Some(after) = t.strip_prefix("class ") {
            class_short = after
                .split(|c: char| c.is_whitespace() || c == '{')
                .next()
                .unwrap_or("")
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_owned();
            break;
        }
    }

    let ctor_marker = (!class_short.is_empty()).then(|| format!("function {class_short}"));
    let mut out = String::with_capacity(stub.len() + 32);
    for line in stub.lines() {
        let mut rewritten = line.to_owned();
        if let Some(marker) = ctor_marker.as_deref()
            && let Some(fn_idx) = rewritten.find(marker)
            && rewritten[fn_idx + marker.len()..]
                .trim_start()
                .starts_with('(')
            && let Some(close_paren_rel) = rewritten[fn_idx..].rfind(')')
        {
            let close_paren = fn_idx + close_paren_rel;
            if let Some(brace_rel) = rewritten[close_paren + 1..].find('{') {
                let between = rewritten[close_paren + 1..close_paren + 1 + brace_rel].trim();
                if between
                    .strip_prefix(':')
                    .map(str::trim)
                    .is_some_and(|ret| ret.eq_ignore_ascii_case("void"))
                {
                    rewritten.replace_range(close_paren + 1..close_paren + 1 + brace_rel, " ");
                }
            }
        }
        if (rewritten.contains("function get ") || rewritten.contains("function set "))
            && let Some(paren_idx) = rewritten.find('(')
            && !rewritten[..paren_idx].ends_with("*/")
        {
            out.push_str(&rewritten[..paren_idx]);
            out.push_str("/**/");
            out.push_str(&rewritten[paren_idx..]);
            out.push('\n');
        } else {
            out.push_str(&rewritten);
            out.push('\n');
        }
    }
    out
}

fn extract_imports_and_identifiers(
    language: ScriptLanguage,
    source: &str,
) -> (Vec<String>, Vec<String>, HashSet<String>) {
    let skeleton = strip_comments_and_strings_for_detection(source);
    let mut explicit_imports = Vec::new();
    let mut wildcard_imports = Vec::new();
    for line in skeleton.lines() {
        let mut rest = line.trim();
        while let Some(idx) = rest.find("import") {
            let before_ok = idx == 0
                || rest[..idx]
                    .chars()
                    .next_back()
                    .is_some_and(|c| !c.is_ascii_alphanumeric() && c != '_');
            let after = &rest[idx + "import".len()..];
            if before_ok && after.starts_with(char::is_whitespace) {
                let stmt_end = after.find([';', '{', '\n']).unwrap_or(after.len());
                let target: String = after[..stmt_end]
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                if let Some(pkg) = target.strip_suffix(".*") {
                    if !pkg.is_empty() {
                        wildcard_imports.push(pkg.to_owned());
                    }
                } else if target.contains('.')
                    || (language == ScriptLanguage::ActionScript3 && !target.is_empty())
                {
                    explicit_imports.push(target);
                }
                rest = &after[stmt_end..];
            } else {
                rest = after;
            }
        }
    }

    let mut idents = HashSet::new();
    let mut cur = String::new();
    for ch in skeleton.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            cur.push(ch);
        } else if !cur.is_empty() {
            if !cur.as_bytes()[0].is_ascii_digit() {
                idents.insert(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if !cur.is_empty() && !cur.as_bytes()[0].is_ascii_digit() {
        idents.insert(cur);
    }
    (explicit_imports, wildcard_imports, idents)
}

fn list_safe_as_files_in_package_dir(
    ancestor_dir: &Path,
    pkg_rel_dir: &str,
) -> Vec<(String, String)> {
    let canon_base = match std::fs::canonicalize(ancestor_dir) {
        Ok(c) if c.is_dir() => c,
        _ => return Vec::new(),
    };
    let target_dir = if pkg_rel_dir.is_empty() {
        canon_base.clone()
    } else {
        if path_has_symlink_segment_under(&canon_base, pkg_rel_dir) {
            return Vec::new();
        }
        let joined = canon_base.join(pkg_rel_dir);
        let Ok(lmeta) = std::fs::symlink_metadata(&joined) else {
            return Vec::new();
        };
        if lmeta.file_type().is_symlink() || !lmeta.is_dir() {
            return Vec::new();
        }
        let Ok(canon_joined) = std::fs::canonicalize(&joined) else {
            return Vec::new();
        };
        if !canon_joined.starts_with(&canon_base) {
            return Vec::new();
        }
        canon_joined
    };

    let Ok(entries) = std::fs::read_dir(&target_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        if entry
            .file_type()
            .is_ok_and(|ft| ft.is_symlink() || !ft.is_file())
        {
            continue;
        }
        let path = entry.path();
        let is_as = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("as"));
        if !is_as {
            continue;
        }
        let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let rel_logical = if pkg_rel_dir.is_empty() {
            fname.to_owned()
        } else {
            format!("{pkg_rel_dir}/{fname}")
        };
        out.push((stem.to_owned(), rel_logical));
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn declared_primary_qualified_name(unit: &SourceUnit) -> Option<String> {
    if let Some(scanned) = crate::script_api::scan_class(unit) {
        return Some(scanned.qualified);
    }
    let skeleton = strip_comments_and_strings_for_detection(&unit.source);
    let mut pkg = String::new();
    let mut brace_depth = 0usize;
    for line in skeleton.lines() {
        let trimmed = line.trim();
        if brace_depth == 0
            && unit.language == ScriptLanguage::ActionScript3
            && let Some(rest) = trimmed.strip_prefix("package")
            && (rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == '{'))
        {
            pkg = rest.split('{').next().unwrap_or("").trim().to_owned();
        }
        if brace_depth <= 1 {
            let mut decl = trimmed;
            loop {
                let mut stripped = false;
                for kw in ["public", "internal", "dynamic", "final", "intrinsic"] {
                    if let Some(next) = strip_keyword_ws(decl, kw) {
                        decl = next;
                        stripped = true;
                    }
                }
                if !stripped {
                    break;
                }
            }
            if let Some(after) =
                strip_keyword_ws(decl, "class").or_else(|| strip_keyword_ws(decl, "interface"))
            {
                let name = after
                    .split(|c: char| c.is_whitespace() || c == '{')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !name.is_empty() {
                    return Some(if pkg.is_empty() || name.contains('.') {
                        name.to_owned()
                    } else {
                        format!("{pkg}.{name}")
                    });
                }
            }
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    None
}

impl Editor {
    fn hydrate_standalone_class_dependencies(
        ancestor_dir: &Path,
        primary_logical_path: &str,
        primary_original_unit: &SourceUnit,
        hydrated_owned: &mut Project,
        open_tabs: &[(PathBuf, String)],
        fp_hasher: &mut std::collections::hash_map::DefaultHasher,
    ) {
        let primary_lang = primary_original_unit.language;
        let primary_pkg_segs = declared_package_segments(primary_original_unit).0;
        let primary_pkg = primary_pkg_segs.join(".");

        let mut seen_qnames: HashSet<String> = HashSet::new();
        if let Some(qn) = declared_primary_qualified_name(primary_original_unit) {
            seen_qnames.insert(qn);
        }

        // Normalize AS3 declarative metadata & split any out-of-package file-private helper classes
        // on the active standalone `.as` unit (`sources[0]`) without mutating `self.project`.
        let mut primary_helpers: Vec<SourceUnit> = Vec::new();
        if primary_lang == ScriptLanguage::ActionScript3
            && let Some(first_mut) = hydrated_owned.sources.first_mut()
        {
            let blanked = blank_declarative_as3_metadata(&first_mut.source);
            let (pkg_body, helpers) = split_as3_package_and_helpers(&blanked, &primary_pkg);
            let owner_id = first_mut.id.clone();
            let owner_path = first_mut.path.clone();
            first_mut.source = pkg_body;
            for helper_src in helpers {
                let helper_unit = SourceUnit {
                    id: owner_id.clone(),
                    path: owner_path.clone(),
                    language: ScriptLanguage::ActionScript3,
                    source: helper_src,
                };
                if let Some(hqn) = declared_primary_qualified_name(&helper_unit)
                    && seen_qnames.insert(hqn)
                {
                    primary_helpers.push(helper_unit);
                }
            }
        }
        hydrated_owned.sources.extend(primary_helpers);

        let load_sibling = |logical: &str| -> Option<String> {
            if !path_has_symlink_segment_under(ancestor_dir, logical) {
                let expected = ancestor_dir.join(logical);
                if let Some((_, tab_src)) = open_tabs
                    .iter()
                    .find(|(p, _)| crate::document_sessions::same_path(p, &expected))
                {
                    return Some(tab_src.clone());
                }
            }
            let disk = resolve_relative_include_disk_path(ancestor_dir, logical)?;
            if let Some((_, tab_src)) = open_tabs
                .iter()
                .find(|(p, _)| crate::document_sessions::same_path(p, &disk))
            {
                return Some(tab_src.clone());
            }
            read_utf8_as_file_bounded(&disk, MAX_STANDALONE_AS_BYTES).ok()
        };

        let mut dir_cache: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();
        let mut list_pkg_cached = |pkg_dir: &str| -> Vec<(String, String)> {
            dir_cache
                .entry(pkg_dir.to_owned())
                .or_insert_with(|| list_safe_as_files_in_package_dir(ancestor_dir, pkg_dir))
                .clone()
        };

        let mut visited_paths: HashSet<String> = HashSet::new();
        visited_paths.insert(primary_logical_path.to_ascii_lowercase());

        let mut bfs: VecDeque<(String, ScriptLanguage, String)> = VecDeque::new();
        bfs.push_back((
            primary_logical_path.to_owned(),
            primary_lang,
            primary_original_unit.source.clone(),
        ));

        let mut sibling_idx = 0usize;
        while let Some((cur_logical, cur_lang, cur_src)) = bfs.pop_front() {
            if sibling_idx >= MAX_HYDRATED_SIBLING_SOURCES {
                break;
            }
            let (explicit_imports, wildcard_imports, idents) =
                extract_imports_and_identifiers(cur_lang, &cur_src);
            let mut candidates: Vec<String> = Vec::new();

            // 1. Explicit `import pkg.Type;` -> `pkg/Type.as`.
            for imp in explicit_imports {
                let rel = format!("{}.as", imp.replace('.', "/"));
                if let Some(clean) = normalize_relative_include_logical_path(&rel) {
                    candidates.push(clean);
                }
            }

            // 2. Wildcard `import pkg.*;` -> referenced stems in `pkg/`, plus at least 1 valid class
            //    in `pkg/` so unused `import pkg.*;` never triggers an empty-wildcard error.
            for wpkg in wildcard_imports {
                let pkg_dir = wpkg.replace('.', "/");
                if pkg_dir.contains("..") || pkg_dir.starts_with('/') {
                    continue;
                }
                let entries = list_pkg_cached(&pkg_dir);
                let mut matched_any = false;
                for (stem, rel_logical) in &entries {
                    if idents.contains(stem) {
                        candidates.push(rel_logical.clone());
                        matched_any = true;
                    }
                }
                if !matched_any && let Some((_, first_rel)) = entries.first() {
                    candidates.push(first_rel.clone());
                }
            }

            // 3. Same-package sibling `.as` classes/interfaces referenced by identifier.
            let cur_pkg_dir = cur_logical.rsplit_once('/').map_or("", |(d, _)| d);
            for (stem, rel_logical) in list_pkg_cached(cur_pkg_dir) {
                if idents.contains(&stem) {
                    candidates.push(rel_logical);
                }
            }

            for cand_logical in candidates {
                if sibling_idx >= MAX_HYDRATED_SIBLING_SOURCES {
                    break;
                }
                if !visited_paths.insert(cand_logical.to_ascii_lowercase()) {
                    continue;
                }
                let Some(cand_src) = load_sibling(&cand_logical) else {
                    continue;
                };
                let cand_lang = if cur_lang == ScriptLanguage::ActionScript2 {
                    let det = detect_actionscript_file_language(&cand_src);
                    if det == ScriptLanguage::ActionScript3 {
                        ScriptLanguage::ActionScript3
                    } else {
                        ScriptLanguage::ActionScript2
                    }
                } else {
                    detect_actionscript_file_language(&cand_src)
                };
                let probe = SourceUnit {
                    id: String::new(),
                    path: cand_logical.clone(),
                    language: cand_lang,
                    source: cand_src.clone(),
                };
                if !Self::is_standalone_class_or_package_unit(&probe) {
                    continue;
                }
                "sibling".hash(fp_hasher);
                cand_logical.hash(fp_hasher);
                cand_src.hash(fp_hasher);

                bfs.push_back((cand_logical.clone(), cand_lang, cand_src.clone()));

                let cand_pkg = declared_package_segments(&probe).0.join(".");
                let units_to_stub: Vec<String> = if cand_lang == ScriptLanguage::ActionScript3 {
                    let blanked = blank_declarative_as3_metadata(&cand_src);
                    let (p_body, helpers) = split_as3_package_and_helpers(&blanked, &cand_pkg);
                    let mut all = Vec::with_capacity(1 + helpers.len());
                    all.push(p_body);
                    all.extend(helpers);
                    all
                } else {
                    vec![cand_src]
                };

                for sub_src in units_to_stub {
                    let stub_src = stub_external_sibling_class_source(cand_lang, &sub_src);
                    let stub_unit = SourceUnit {
                        id: format!("hydrated-sibling-{sibling_idx}"),
                        path: cand_logical.clone(),
                        language: cand_lang,
                        source: stub_src,
                    };
                    if let Some(qn) = declared_primary_qualified_name(&stub_unit)
                        && seen_qnames.insert(qn)
                    {
                        hydrated_owned.sources.push(stub_unit);
                        sibling_idx += 1;
                    }
                }
            }
        }
    }

    pub(crate) fn is_actionscript_source_path(path: &Path) -> bool {
        !path.is_dir()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("as"))
    }

    pub fn is_standalone_script_session(&self) -> bool {
        self.has_document() && self.standalone_script
    }

    pub fn standalone_script_language(&self) -> Option<ScriptLanguage> {
        if !self.is_standalone_script_session() {
            return None;
        }
        self.project
            .sources
            .first()
            .map(|u| u.language)
            .or(self.project.script_language)
    }

    pub fn standalone_script_source(&self) -> Option<&str> {
        if !self.is_standalone_script_session() {
            return None;
        }
        self.project.sources.first().map(|u| u.source.as_str())
    }

    pub fn set_standalone_script_language(&mut self, language: ScriptLanguage) {
        if !self.is_standalone_script_session() || !language.is_actionscript() {
            return;
        }
        let changed = self.project.script_language != Some(language)
            || self
                .project
                .sources
                .first()
                .is_some_and(|u| u.language != language);
        if !changed {
            return;
        }
        self.project.script_language = Some(language);
        if let Some(unit) = self.project.sources.first_mut() {
            unit.language = language;
        }
        crate::timeline_commands::upgrade_all_format_versions(&mut self.project);
        // Keep `saved_project` dialect metadata aligned so inspecting under AS1/AS2/AS3 without
        // editing file bytes does not falsely mark the raw `.as` buffer dirty.
        if let Some(saved) = self.saved_project.as_mut() {
            saved.script_language = Some(language);
            if let Some(saved_unit) = saved.sources.first_mut() {
                saved_unit.language = language;
            }
            crate::timeline_commands::upgrade_all_format_versions(saved);
        }
        self.invalidate_dirty_cache();
        self.code_feedback = None;
        self.compiler_errors.fingerprint = None;
    }

    pub(crate) fn load_standalone_script_project_from_path(path: &Path) -> Result<Project, String> {
        let source = read_utf8_as_file_bounded(path, MAX_STANDALONE_AS_BYTES)?;
        let file_name = sanitize_source_unit_filename(path);
        let language = detect_actionscript_file_language(&source);
        let mut project = Project::blank();
        project.format_version = 2;
        project.name = file_name.clone();
        project.script_language = Some(language);
        let root_id = project.root.clone();
        if let Some(root) = project.symbol_mut(&root_id) {
            root.timeline.frame_count = 1;
        }
        project.sources = vec![SourceUnit {
            id: new_id("source"),
            path: file_name,
            language,
            source,
        }];
        crate::timeline_commands::upgrade_all_format_versions(&mut project);
        project.validate().map_err(|e| e.to_string())?;
        Ok(project)
    }

    pub(crate) fn configure_active_standalone_script_state(&mut self) {
        self.standalone_script = true;
        self.script_target = ScriptTarget::Source;
        self.selected_source = self.project.sources.first().map(|u| u.id.clone());
        self.actions_ui.sidebar = self.workspace.code_editor.actions_sidebar;
        self.actions_ui.active_pinned = None;
        self.actions_ui.pinned_tabs.clear();
        // Clear runtime viewport tracking of detached authoring panels without mutating WorkspacePrefs!
        self.detached = crate::detached::DetachedState::default();
    }

    pub(crate) fn open_standalone_script_session(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
    ) -> bool {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return false;
        }
        if let Some(index) = self.session_for_open_path(&path) {
            self.switch_session_tab(ctx, index);
            return true;
        }
        let preserved_publish = self.workspace.publish.clone();
        match Self::load_standalone_script_project_from_path(&path) {
            Ok(project) => {
                self.open_new_session_tab(ctx, project, Some(path.clone()));
                self.workspace.publish = preserved_publish;
                self.configure_active_standalone_script_state();
                self.saved_project = Some(self.project.clone());
                self.invalidate_dirty_cache();
                self.sync_disk_mtime();
                self.workspace.general.push_recent(&path);
                self.sync_session_tabs_to_prefs();
                true
            }
            Err(error) => {
                self.message(format!("Open failed: {error}"));
                false
            }
        }
    }

    pub(crate) fn default_standalone_script_save_target(&self) -> (Option<PathBuf>, String) {
        if let Some(path) = &self.path {
            let dir = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf);
            let name = sanitize_source_unit_filename(path);
            return (dir, name);
        }
        let fallback = self
            .project
            .sources
            .first()
            .map(|u| sanitize_source_unit_filename(Path::new(&u.path)))
            .unwrap_or_else(|| "Script.as".to_owned());
        (None, fallback)
    }

    /// Atomically write raw UTF-8 `.as` source bytes to `path` (NEVER `.nflash` JSON!).
    pub(crate) fn save_standalone_script_to_path(&mut self, path: PathBuf) -> bool {
        if !self.is_standalone_script_session() {
            return false;
        }
        let path = if !path.is_dir() && path.extension().is_none() {
            path.with_extension("as")
        } else {
            path
        };
        if !Self::is_actionscript_source_path(&path) {
            let bad_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| path.display().to_string());
            self.notify_with_output(
                neoflash_ui::components::ToastType::Error,
                format!(
                    "Save failed: standalone ActionScript tab cannot overwrite non-.as file “{bad_name}”. Save As “.as”."
                ),
            );
            return false;
        }
        if self
            .session_for_path(&path)
            .is_some_and(|index| index != self.active_session)
        {
            self.notify_with_output(
                neoflash_ui::components::ToastType::Error,
                "Save failed: this file is already open in another document. Choose a different path.",
            );
            return false;
        }
        if !self.guard_save_on_external_conflict(Some(&path)) {
            return false;
        }
        self.finish_edits();

        let format_on_save = self.workspace.code_editor.format_on_save;
        let tab_size = self.workspace.code_editor.tab_size;
        if format_on_save {
            self.history
                .begin_named(&self.project, "Format scripts on save");
            Self::format_project_scripts(&mut self.project, tab_size);
        }

        let new_file_name = sanitize_source_unit_filename(&path);
        let source_bytes = self
            .project
            .sources
            .first()
            .map(|u| u.source.as_bytes().to_vec())
            .unwrap_or_default();

        match atomic_save_file(&path, &source_bytes) {
            Ok(()) => {
                self.project.name = new_file_name.clone();
                if let Some(unit) = self.project.sources.first_mut() {
                    unit.path = new_file_name.clone();
                }
                self.history
                    .sync_standalone_script_identity(&new_file_name, &new_file_name);
                if format_on_save {
                    self.commit_history();
                }
                self.workspace.general.push_recent(&path);
                self.saved_project = Some(self.project.clone());
                self.invalidate_dirty_cache();
                self.path = Some(path);
                self.clear_and_retarget_recovery();
                self.sync_disk_mtime();
                self.sync_session_tabs_to_prefs();
                self.invalidate_include_dependents_across_sessions();
                self.message("ActionScript file saved.");
                true
            }
            Err(e) => {
                if format_on_save {
                    self.history.cancel(&mut self.project);
                }
                self.notify_with_output(
                    neoflash_ui::components::ToastType::Error,
                    format!("Save failed: {e}"),
                );
                false
            }
        }
    }

    /// Invalidate cached compiler feedback, Compiler Errors fingerprints, and live `AuthoredPlayback`
    /// previews across all open sessions so edits/saves/reloads to `.as` files take effect immediately.
    pub(crate) fn invalidate_include_dependents_across_sessions(&mut self) {
        self.code_feedback = None;
        self.compiler_errors.fingerprint = None;
        self.authored_preview = None;
        for session in self.sessions.iter_mut().flatten() {
            session.code_feedback = None;
            session.compiler_errors.fingerprint = None;
        }
    }

    /// Collect `(disk_path, source_text)` for all currently open standalone `.as` tabs (active + background).
    pub(crate) fn open_standalone_as_tab_buffers(&self) -> Vec<(PathBuf, String)> {
        if !self.has_document() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for index in 0..self.sessions.len() {
            if index == self.active_session {
                if self.standalone_script
                    && let Some(path) = self.tracked_disk_path()
                    && let Some(unit) = self.project.sources.first()
                {
                    out.push((path.to_path_buf(), unit.source.clone()));
                }
            } else if let Some(session) = self.sessions[index].as_ref()
                && session.standalone_script
                && let Some(path) = session.tracked_disk_path()
                && let Some(unit) = session.project.sources.first()
            {
                out.push((path.to_path_buf(), unit.source.clone()));
            }
        }
        out
    }

    /// Base directory on disk used to resolve relative `#include "foo.as"` paths for the active session.
    pub(crate) fn active_project_include_base_dir(&self) -> Option<PathBuf> {
        let tracked = self.tracked_disk_path()?;
        if self.standalone_script
            && !tracked.is_dir()
            && let Some(unit) = self.project.sources.first()
            && let Some((ancestor_dir, _)) = standalone_script_include_context(tracked, unit)
        {
            return Some(ancestor_dir);
        }
        if tracked.is_dir() {
            return Some(tracked.to_path_buf());
        }
        if tracked
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("DOMDocument.xml"))
        {
            return tracked.parent().map(Path::to_path_buf);
        }
        tracked
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
    }

    /// Return `(hydrated_project, include_hydration_fingerprint)` for the active session.
    pub(crate) fn hydrated_project_and_fingerprint(&self) -> (Cow<'_, Project>, u64) {
        self.hydrate_arbitrary_project_with_editor_context(&self.project)
    }

    pub(crate) fn project_with_hydrated_includes(&self) -> Cow<'_, Project> {
        self.hydrated_project_and_fingerprint().0
    }

    pub(crate) fn hydrate_arbitrary_project_with_editor_context<'a>(
        &self,
        target_project: &'a Project,
    ) -> (Cow<'a, Project>, u64) {
        let open_tabs = self.open_standalone_as_tab_buffers();
        if self.is_standalone_script_session()
            && target_project.sources.len() == 1
            && let Some(tracked) = self.tracked_disk_path()
            && !tracked.is_dir()
            && let Some((ancestor_dir, logical_source_path)) =
                standalone_script_include_context(tracked, &target_project.sources[0])
        {
            let unit = &target_project.sources[0];
            let is_class_or_pkg = Self::is_standalone_class_or_package_unit(unit);
            let mut staged = target_project.clone();
            staged.sources[0].path = logical_source_path.clone();
            let (hydrated, sub_fp) = hydrate_project_includes_with_overrides(
                &staged,
                Some(ancestor_dir.as_path()),
                &open_tabs,
            );
            let mut hydrated_owned = hydrated.into_owned();
            let (_, expected_as2_path) = declared_package_segments(unit);
            let post_hydration_path = if let Some(exp_as2) = expected_as2_path {
                (logical_source_path == exp_as2
                    || logical_source_path.ends_with(&format!("/{exp_as2}")))
                .then_some(exp_as2)
            } else {
                Some(unit.path.clone())
            };
            if let Some(final_path) = post_hydration_path
                && final_path != logical_source_path
                && let Some(hydrated_unit) = hydrated_owned.sources.first_mut()
            {
                hydrated_unit.source = rewrite_includes_between_logical_paths(
                    hydrated_unit.language,
                    &hydrated_unit.source,
                    &logical_source_path,
                    &final_path,
                );
                hydrated_unit.path = final_path;
            }
            let mut h = std::collections::hash_map::DefaultHasher::new();
            sub_fp.hash(&mut h);
            logical_source_path.hash(&mut h);
            if is_class_or_pkg {
                Self::hydrate_standalone_class_dependencies(
                    ancestor_dir.as_path(),
                    &logical_source_path,
                    unit,
                    &mut hydrated_owned,
                    &open_tabs,
                    &mut h,
                );
            }
            return (Cow::Owned(hydrated_owned), h.finish());
        }
        let base_dir = self.active_project_include_base_dir();
        hydrate_project_includes_with_overrides(target_project, base_dir.as_deref(), &open_tabs)
    }

    /// Resolve a diagnostic/output file path (e.g. `"figures.as"` or `"sub/rotate.as"`) to either
    /// an already-open standalone `.as` tab or an existing non-symlink `.as`/`.asi` file on disk
    /// canonically contained within the owning session's root directory.
    pub(crate) fn resolve_external_as_path_for_navigation(
        &self,
        owner_document_id: Option<&str>,
        raw_path: &Path,
    ) -> Option<PathBuf> {
        let owner_base_dir: Option<PathBuf> = owner_document_id
            .and_then(|doc_id| {
                if self.has_document() && self.clipboard.session == doc_id {
                    self.active_project_include_base_dir()
                } else {
                    self.sessions
                        .iter()
                        .flatten()
                        .find(|s| s.clipboard.session == doc_id)
                        .and_then(|s| {
                            let p = s.tracked_disk_path()?;
                            if s.standalone_script
                                && !p.is_dir()
                                && let Some(unit) = s.project.sources.first()
                                && let Some((anc, _)) = standalone_script_include_context(p, unit)
                            {
                                return Some(anc);
                            }
                            let dir = if p.is_dir() { Some(p) } else { p.parent() }?;
                            (!dir.as_os_str().is_empty()).then(|| dir.to_path_buf())
                        })
                }
            })
            .or_else(|| self.active_project_include_base_dir());

        if raw_path.is_absolute() {
            if !has_actionscript_include_extension(raw_path) {
                return None;
            }
            let lmeta = std::fs::symlink_metadata(raw_path).ok()?;
            if lmeta.file_type().is_symlink() || !lmeta.is_file() {
                return None;
            }
            let canon_raw = std::fs::canonicalize(raw_path).ok()?;
            if !has_actionscript_include_extension(&canon_raw) {
                return None;
            }
            if let Some(dir) = owner_base_dir.as_deref() {
                let canon_dir = std::fs::canonicalize(dir).ok()?;
                if let Ok(rel) = raw_path.strip_prefix(dir) {
                    return resolve_relative_include_disk_path(dir, &rel.to_string_lossy());
                }
                if let Ok(rel) = raw_path.strip_prefix(&canon_dir) {
                    return resolve_relative_include_disk_path(&canon_dir, &rel.to_string_lossy());
                }
                if canon_raw.starts_with(&canon_dir) {
                    let rel = canon_raw.strip_prefix(&canon_dir).ok()?;
                    return resolve_relative_include_disk_path(&canon_dir, &rel.to_string_lossy());
                }
                let open_tabs = self.open_standalone_as_tab_buffers();
                if open_tabs
                    .iter()
                    .any(|(p, _)| crate::document_sessions::same_path(p, raw_path))
                {
                    return Some(raw_path.to_path_buf());
                }
                return None;
            }
            let parent = raw_path.parent()?;
            let file_name = raw_path.file_name()?.to_str()?;
            return resolve_relative_include_disk_path(parent, file_name);
        }

        let raw_str = raw_path.to_string_lossy().replace('\\', "/");
        let clean_rel = normalize_relative_include_logical_path(&raw_str)?;

        if let Some(dir) = owner_base_dir.as_deref() {
            if let Some(disk_path) = resolve_relative_include_disk_path(dir, &clean_rel) {
                return Some(disk_path);
            }
            if !path_has_symlink_segment_under(dir, &clean_rel) {
                let direct = dir.join(&clean_rel);
                if self.session_for_open_path(&direct).is_some() {
                    return Some(direct);
                }
            }
            return None;
        }

        // Fallback when the owning session has no disk path: match unique open standalone `.as` tab.
        if !clean_rel.contains('/') {
            let open_tabs = self.open_standalone_as_tab_buffers();
            let mut matches = open_tabs.into_iter().filter(|(p, _)| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case(&clean_rel))
            });
            if let Some((first_path, _)) = matches.next()
                && matches.next().is_none()
                && has_actionscript_include_extension(&first_path)
                && std::fs::symlink_metadata(&first_path)
                    .map_or(true, |m| !m.file_type().is_symlink())
            {
                return Some(first_path);
            }
        }
        None
    }

    /// Handle OS file drag-and-drop onto the empty editor window, standalone `.as` view, or tab bar,
    /// as well as `.as` file drops anywhere in the root window (`T-412`).
    pub(crate) fn handle_window_document_or_as_file_drop(&mut self, ctx: &egui::Context) -> bool {
        if self.beta_feedback.open || self.has_modal_dialog_in(ctx) {
            return false;
        }
        let (pointer, dropped) =
            ctx.input(|input| (input.pointer.latest_pos(), input.raw.dropped_files.clone()));
        if dropped.is_empty() {
            return false;
        }
        let paths: Vec<PathBuf> = dropped.iter().filter_map(|f| f.path.clone()).collect();
        if paths.is_empty() {
            return false;
        }

        let all_as = paths.iter().all(|p| Self::is_actionscript_source_path(p));
        let any_as = paths.iter().any(|p| Self::is_actionscript_source_path(p));
        let over_top_chrome = pointer.is_some_and(|pos| {
            let chrome_bottom = neoflash_ui::metrics::top_bar()
                + if self.has_document() {
                    neoflash_ui::metrics::panel_header()
                } else {
                    0.0
                };
            pos.y <= chrome_bottom
        });
        let intercept_all = all_as
            || !self.has_document()
            || self.is_standalone_script_session()
            || over_top_chrome;
        if intercept_all {
            ctx.input_mut(|input| input.raw.dropped_files.clear());
            return self.open_picked_paths(paths, ctx) > 0;
        }
        if any_as {
            let as_paths: Vec<PathBuf> = paths
                .into_iter()
                .filter(|p| Self::is_actionscript_source_path(p))
                .collect();
            ctx.input_mut(|input| {
                input.raw.dropped_files.retain(|file| {
                    file.path
                        .as_deref()
                        .is_none_or(|p| !Self::is_actionscript_source_path(p))
                });
            });
            return self.open_picked_paths(as_paths, ctx) > 0;
        }
        false
    }
}

#[cfg(test)]
mod t414_standalone_script_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn t414_symlink_file_and_dir_escape_rejected_in_hydration_and_navigation() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "neoflash-t414-symlink-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        let proj_dir = root.join("proj");
        let outside_dir = root.join("outside_dir");
        std::fs::create_dir_all(&proj_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();

        let outside_secret = root.join("outside_secret.txt");
        std::fs::write(&outside_secret, "var LEAKED = \"TOP_SECRET\";\n").unwrap();
        let outside_as = outside_dir.join("escape.as");
        std::fs::write(&outside_as, "var ESCAPED = 999;\n").unwrap();

        // Symlink file `leak.as -> ../outside_secret.txt` and symlink dir `sub -> ../outside_dir`.
        let leak_link = proj_dir.join("leak.as");
        symlink(&outside_secret, &leak_link).unwrap();
        let sub_link = proj_dir.join("sub");
        symlink(&outside_dir, &sub_link).unwrap();

        // Also place a real `.as` file inside `proj_dir` to confirm legitimate includes still work.
        let safe_as = proj_dir.join("safe.as");
        std::fs::write(&safe_as, "var SAFE_OK:Number = 1;\n").unwrap();

        assert!(resolve_relative_include_disk_path(&proj_dir, "leak.as").is_none());
        assert!(read_raw_include_bytes_from_disk(&proj_dir, "leak.as").is_none());
        assert!(resolve_relative_include_disk_path(&proj_dir, "sub/escape.as").is_none());
        assert!(read_raw_include_bytes_from_disk(&proj_dir, "sub/escape.as").is_none());
        assert!(resolve_relative_include_disk_path(&proj_dir, "safe.as").is_some());

        let mut project = Project::blank();
        project.name = "EvilProject".into();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        let root_id = project.root.clone();
        project
            .symbol_mut(&root_id)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source:
                        "#include \"leak.as\"\n#include \"sub/escape.as\"\n#include \"safe.as\"\n"
                            .into(),
                },
            });
        crate::timeline_commands::upgrade_all_format_versions(&mut project);
        let nflash_path = proj_dir.join("evil.nflash");
        std::fs::write(&nflash_path, project.to_json().unwrap()).unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        assert!(editor.try_open_path(nflash_path, &ctx));

        let hydrated = editor.project_with_hydrated_includes();
        assert!(
            hydrated.resources.iter().all(|r| r.path == "safe.as"),
            "hydration must reject both symlink file and symlink dir includes: {:?}",
            hydrated.resources
        );

        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, Path::new("leak.as"))
                .is_none(),
            "click-to-jump navigation must reject symlink file leak.as"
        );
        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, &leak_link)
                .is_none(),
            "click-to-jump navigation must reject absolute symlink file leak.as"
        );
        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, Path::new("sub/escape.as"))
                .is_none(),
            "click-to-jump navigation must reject symlink directory sub/escape.as"
        );
        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, &sub_link.join("escape.as"))
                .is_none(),
            "click-to-jump navigation must reject absolute path through symlink dir"
        );
        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, Path::new("safe.as"))
                .is_some()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn t414_standalone_as_parent_relative_include_and_package_root() {
        let root = std::env::temp_dir().join(format!(
            "neoflash-t414-parent-inc-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        let shared_dir = root.join("pkg/shared");
        let sub_dir = root.join("pkg/sub");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::create_dir_all(&sub_dir).unwrap();

        let consts_path = shared_dir.join("consts.as");
        std::fs::write(&consts_path, "public static const MAX_SPEED:int = 12;\n").unwrap();

        let player_path = sub_dir.join("Player.as");
        std::fs::write(
            &player_path,
            "package pkg.sub {\n    public class Player {\n        include \"../shared/consts.as\"\n        public function speed():int { return MAX_SPEED; }\n    }\n}\n",
        )
        .unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        assert!(editor.open_standalone_script_session(&ctx, player_path));
        assert!(editor.is_standalone_script_session());
        assert_eq!(
            editor.project.sources[0].path, "Player.as",
            "authored sources[0].path must remain stable"
        );
        assert!(!editor.dirty());

        editor.check_standalone_script(true);
        assert_eq!(
            editor.status, "Code accepted for current language support.",
            "parent-relative include on standalone .as tab must pass Check: {}",
            editor.status
        );
        editor.refresh_compiler_diagnostics();
        assert!(
            editor.compiler_errors.items.is_empty(),
            "expected 0 compiler errors on parent-relative include, got: {:?}",
            editor.compiler_errors.items
        );
        assert_eq!(editor.project.sources[0].path, "Player.as");
        assert!(!editor.dirty());

        // Also verify click-to-jump resolution for `"pkg/shared/consts.as"` under the inferred ancestor root!
        let nav = editor
            .resolve_external_as_path_for_navigation(None, Path::new("pkg/shared/consts.as"))
            .expect("navigation must resolve included sibling-folder .as file");
        assert_eq!(
            std::fs::canonicalize(nav).unwrap(),
            std::fs::canonicalize(&consts_path).unwrap()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn t414_standalone_as_save_as_syncs_history_filename_across_undo_redo() {
        let tmp = std::env::temp_dir().join(format!(
            "neoflash-t414-save-as-hist-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let foo_path = tmp.join("Foo.as");
        let bar_path = tmp.join("Bar.as");
        std::fs::write(&foo_path, "class Foo {\n}\n").unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        editor.workspace.code_editor.format_on_save = true;
        assert!(editor.open_standalone_script_session(&ctx, foo_path));
        assert!(!editor.dirty());

        // Edit `class Foo` -> unformatted `class   Bar` so `format_on_save` also records an Undo step.
        editor.transact_named("Rename class", |ed| {
            ed.project.sources[0].source = "class   Bar{\n}\n".into();
        });
        assert!(editor.dirty());

        assert!(editor.save_standalone_script_to_path(bar_path));
        assert!(!editor.dirty(), "must be clean right after Save As");
        assert_eq!(editor.project.name, "Bar.as");
        assert_eq!(editor.project.sources[0].path, "Bar.as");

        // Undo format-on-save and class edit: filename identity must NEVER revert to `Foo.as`!
        assert!(editor.history.can_undo());
        editor.undo();
        assert_eq!(editor.project.name, "Bar.as");
        assert_eq!(editor.project.sources[0].path, "Bar.as");

        assert!(editor.history.can_undo());
        editor.undo();
        assert_eq!(editor.project.name, "Bar.as");
        assert_eq!(editor.project.sources[0].path, "Bar.as");

        // Redo back to the saved state: `dirty()` must clear back to `false` and AS2 class-path check must pass!
        assert!(editor.history.can_redo());
        editor.redo();
        editor.redo();
        assert_eq!(editor.project.name, "Bar.as");
        assert_eq!(editor.project.sources[0].path, "Bar.as");
        assert!(
            !editor.dirty(),
            "dirty() must not stay stuck true after Undo + Redo back to Save As state"
        );
        editor.check_standalone_script(true);
        assert_eq!(editor.status, "Code accepted for current language support.");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn t414_symlinked_base_dir_allows_safe_includes_while_rejecting_inner_symlinks() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "neoflash-t414-symbase-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        let real_proj_dir = root.join("real_proj_dir");
        let outside_dir = root.join("outside_dir");
        std::fs::create_dir_all(&real_proj_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();

        let outside_secret = root.join("outside_secret.txt");
        std::fs::write(&outside_secret, "var LEAKED = \"TOP_SECRET\";\n").unwrap();
        let outside_as = outside_dir.join("escape.as");
        std::fs::write(&outside_as, "var ESCAPED = 999;\n").unwrap();

        let safe_as = real_proj_dir.join("safe.as");
        std::fs::write(&safe_as, "var SAFE_OK:Number = 7;\n").unwrap();
        symlink(&outside_secret, real_proj_dir.join("leak.as")).unwrap();
        symlink(&outside_dir, real_proj_dir.join("sub")).unwrap();

        // `symlink_proj -> real_proj_dir` models macOS `/tmp -> private/tmp`.
        let symlink_proj = root.join("symlink_proj");
        symlink(&real_proj_dir, &symlink_proj).unwrap();

        assert!(resolve_relative_include_disk_path(&symlink_proj, "safe.as").is_some());
        assert!(read_raw_include_bytes_from_disk(&symlink_proj, "safe.as").is_some());
        assert!(resolve_relative_include_disk_path(&symlink_proj, "leak.as").is_none());
        assert!(read_raw_include_bytes_from_disk(&symlink_proj, "leak.as").is_none());
        assert!(resolve_relative_include_disk_path(&symlink_proj, "sub/escape.as").is_none());

        let mut project = Project::blank();
        project.name = "SymlinkBaseProject".into();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        let root_id = project.root.clone();
        project
            .symbol_mut(&root_id)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source:
                        "#include \"safe.as\"\n#include \"leak.as\"\n#include \"sub/escape.as\"\n"
                            .into(),
                },
            });
        crate::timeline_commands::upgrade_all_format_versions(&mut project);
        let nflash_via_symlink = symlink_proj.join("movie.nflash");
        std::fs::write(&nflash_via_symlink, project.to_json().unwrap()).unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        assert!(editor.try_open_path(nflash_via_symlink, &ctx));

        let hydrated = editor.project_with_hydrated_includes();
        assert_eq!(hydrated.resources.len(), 1);
        assert_eq!(hydrated.resources[0].path, "safe.as");

        let nav_rel = editor
            .resolve_external_as_path_for_navigation(None, Path::new("safe.as"))
            .expect("relative safe.as must resolve when base_dir is a symlink");
        assert_eq!(
            std::fs::canonicalize(nav_rel).unwrap(),
            std::fs::canonicalize(&safe_as).unwrap()
        );
        let nav_abs = editor
            .resolve_external_as_path_for_navigation(None, &symlink_proj.join("safe.as"))
            .expect("absolute safe.as under symlinked base_dir must resolve");
        assert_eq!(
            std::fs::canonicalize(nav_abs).unwrap(),
            std::fs::canonicalize(&safe_as).unwrap()
        );
        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, Path::new("leak.as"))
                .is_none()
        );
        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, &symlink_proj.join("leak.as"))
                .is_none()
        );
        assert!(
            editor
                .resolve_external_as_path_for_navigation(None, Path::new("sub/escape.as"))
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn t414_standalone_as2_unqualified_and_packaged_classes_and_top_level_parent_includes() {
        let root = std::env::temp_dir().join(format!(
            "neoflash-t414-as2-parent-inc-{}-{}",
            std::process::id(),
            new_id("dir")
        ));
        let shared_dir = root.join("proj/shared");
        let foo_sub_dir = root.join("proj/sub");
        let common_dir = root.join("ws/common");
        let packaged_dir = root.join("ws/outer/sub");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::create_dir_all(&foo_sub_dir).unwrap();
        std::fs::create_dir_all(&common_dir).unwrap();
        std::fs::create_dir_all(&packaged_dir).unwrap();

        std::fs::write(shared_dir.join("consts.as"), "var MAX_SPEED:Number = 42;\n").unwrap();
        std::fs::write(common_dir.join("defs.as"), "var BOOST:Number = 9;\n").unwrap();

        // 1. Unqualified AS2 `class Foo` in `/proj/sub/Foo.as` with `#include "../shared/consts.as"`.
        let foo_path = foo_sub_dir.join("Foo.as");
        std::fs::write(
            &foo_path,
            "class Foo {\n    #include \"../shared/consts.as\"\n    public function speed():Number {\n        return MAX_SPEED;\n    }\n}\n",
        )
        .unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        assert!(editor.open_standalone_script_session(&ctx, foo_path));
        assert_eq!(editor.project.sources[0].path, "Foo.as");
        editor.check_standalone_script(true);
        assert_eq!(
            editor.status, "Code accepted for current language support.",
            "unqualified AS2 class Foo with ../ include failed Check: {}",
            editor.status
        );
        editor.refresh_compiler_diagnostics();
        assert!(
            editor.compiler_errors.items.is_empty(),
            "expected 0 compiler errors on unqualified AS2 class Foo, got: {:?}",
            editor.compiler_errors.items
        );
        assert_eq!(editor.project.sources[0].path, "Foo.as");
        assert!(!editor.dirty());

        // 2. Packaged AS2 `class sub.Player` in `/ws/outer/sub/Player.as` with `#include "../../common/defs.as"` (`inc_hops = 2 > pkg_hops = 1`).
        let player_path = packaged_dir.join("Player.as");
        std::fs::write(
            &player_path,
            "class sub.Player {\n    #include \"../../common/defs.as\"\n    public function boost():Number {\n        return BOOST;\n    }\n}\n",
        )
        .unwrap();
        assert!(editor.open_standalone_script_session(&ctx, player_path));
        assert_eq!(editor.project.sources[0].path, "Player.as");
        editor.check_standalone_script(true);
        assert_eq!(
            editor.status, "Code accepted for current language support.",
            "packaged AS2 class sub.Player with ../../ include failed Check: {}",
            editor.status
        );
        editor.refresh_compiler_diagnostics();
        assert!(
            editor.compiler_errors.items.is_empty(),
            "expected 0 compiler errors on packaged AS2 class sub.Player, got: {:?}",
            editor.compiler_errors.items
        );
        assert_eq!(editor.project.sources[0].path, "Player.as");
        assert!(!editor.dirty());

        // 3. Top-level non-class standalone `.as` in `/proj/sub/Script.as` with `#include "../shared/consts.as"`.
        let top_script_path = foo_sub_dir.join("Script.as");
        std::fs::write(
            &top_script_path,
            "#include \"../shared/consts.as\"\ntrace(MAX_SPEED);\n",
        )
        .unwrap();
        assert!(editor.open_standalone_script_session(&ctx, top_script_path));
        let (hydrated_top, _) = editor.hydrated_project_and_fingerprint();
        assert_eq!(hydrated_top.sources[0].path, "Script.as");
        assert_eq!(
            hydrated_top.sources[0].source, "#include \"shared/consts.as\"\ntrace(MAX_SPEED);\n",
            "hydrated.sources[0].source must carry rewritten include path for non-class standalone .as"
        );
        editor.check_standalone_script(true);
        assert_eq!(
            editor.status, "Code accepted for current language support.",
            "top-level standalone Script.as with ../ include failed Check: {}",
            editor.status
        );
        editor.refresh_compiler_diagnostics();
        assert!(
            editor.compiler_errors.items.is_empty(),
            "expected 0 compiler errors on top-level standalone .as, got: {:?}",
            editor.compiler_errors.items
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
