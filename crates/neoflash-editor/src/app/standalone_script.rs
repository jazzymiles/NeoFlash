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
    let text = String::from_utf8(bytes).map_err(|_| {
        format!(
            "{} must contain valid UTF-8 ActionScript text",
            path.display()
        )
    })?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(stripped) => stripped.to_owned(),
        None => text,
    })
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

/// Returns `true` if any existing path segment of `clean_rel` under `base_dir` is a symlink
/// (or if `base_dir` itself is a symlink).
fn path_has_symlink_segment_under(base_dir: &Path, clean_rel: &str) -> bool {
    let Ok(base_meta) = std::fs::symlink_metadata(base_dir) else {
        return false;
    };
    if base_meta.file_type().is_symlink() || !base_meta.is_dir() {
        return true;
    }
    let mut current = base_dir.to_path_buf();
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
    Some((resolved, bytes))
}

/// Resolve a relative `.as` / `.asi` path (`"figures.as"` or `"sub/rotate.as"`) to an existing
/// non-symlink disk path canonically contained within `base_dir`, if present.
pub(crate) fn resolve_relative_include_disk_path(
    base_dir: &Path,
    logical_path: &str,
) -> Option<PathBuf> {
    let clean = normalize_relative_include_logical_path(logical_path)?;
    let base_meta = std::fs::symlink_metadata(base_dir).ok()?;
    if base_meta.file_type().is_symlink() || !base_meta.is_dir() {
        return None;
    }
    let canon_base = std::fs::canonicalize(base_dir).ok()?;
    let segments: Vec<&str> = clean.split('/').collect();
    let mut current = base_dir.to_path_buf();
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

fn declared_package_segments(unit: &SourceUnit) -> (Vec<String>, bool) {
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
                    return (Vec::new(), false);
                }
                let segs: Vec<String> = pkg
                    .split('.')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                return (segs, false);
            }
            if unit.language == ScriptLanguage::ActionScript2 {
                let mut decl = trimmed;
                loop {
                    let mut stripped = false;
                    for kw in ["public ", "dynamic ", "intrinsic "] {
                        if let Some(next) = decl.strip_prefix(kw) {
                            decl = next.trim_start();
                            stripped = true;
                        }
                    }
                    if !stripped {
                        break;
                    }
                }
                if let Some(after_kw) = decl
                    .strip_prefix("class ")
                    .or_else(|| decl.strip_prefix("interface "))
                {
                    let qname = after_kw
                        .split(|c: char| c.is_whitespace() || c == '{')
                        .next()
                        .unwrap_or("")
                        .trim();
                    let parts: Vec<&str> = qname.split('.').filter(|s| !s.is_empty()).collect();
                    if parts.len() > 1 {
                        return (
                            parts[..parts.len() - 1]
                                .iter()
                                .map(|s| (*s).to_owned())
                                .collect(),
                            true,
                        );
                    }
                    return (Vec::new(), true);
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
    (Vec::new(), false)
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
        let Ok(parent_meta) = std::fs::symlink_metadata(parent) else {
            break;
        };
        if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
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
    let (pkg_segments, is_as2_type) = declared_package_segments(unit);
    let pkg_hops = matched_trailing_dir_hops(file_dir, &pkg_segments);
    let inc_hops = required_parent_hops_for_script(unit.language, &unit.source);
    let desired_hops = if is_as2_type && !pkg_segments.is_empty() && pkg_hops == pkg_segments.len()
    {
        pkg_hops
    } else if is_as2_type && pkg_segments.is_empty() && inc_hops == 0 {
        0
    } else {
        pkg_hops.max(inc_hops)
    };
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

impl Editor {
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
            && logical_source_path != target_project.sources[0].path
        {
            let mut staged = target_project.clone();
            staged.sources[0].path = logical_source_path.clone();
            let (hydrated, sub_fp) = hydrate_project_includes_with_overrides(
                &staged,
                Some(ancestor_dir.as_path()),
                &open_tabs,
            );
            let mut h = std::collections::hash_map::DefaultHasher::new();
            sub_fp.hash(&mut h);
            logical_source_path.hash(&mut h);
            return (Cow::Owned(hydrated.into_owned()), h.finish());
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
        let over_top_chrome = pointer.is_some_and(|pos| {
            let chrome_bottom = neoflash_ui::metrics::top_bar()
                + if self.has_document() {
                    neoflash_ui::metrics::panel_header()
                } else {
                    0.0
                };
            pos.y <= chrome_bottom
        });
        let intercept = all_as
            || !self.has_document()
            || self.is_standalone_script_session()
            || over_top_chrome;
        if !intercept {
            return false;
        }

        ctx.input_mut(|input| input.raw.dropped_files.clear());
        let mut opened_any = false;
        for path in paths {
            if self.session_for_open_path(&path).is_none()
                && crate::panels::figma_import::is_figma_file_path(&path)
            {
                self.open_figma_import_dialog_for_open(ctx, path);
                opened_any = true;
            } else if self.try_open_path(path, ctx) {
                opened_any = true;
            }
        }
        opened_any
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
}
