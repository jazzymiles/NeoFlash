use super::*;

pub(super) fn is_valid_identifier(s: &str) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && !bytes[0].is_ascii_digit()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'$')
}

pub(super) fn is_supported_builtin_pure_call_head(head: &str) -> bool {
    matches!(
        head,
        "eval"
            | "chr"
            | "ord"
            | "mbchr"
            | "mbord"
            | "length"
            | "mblength"
            | "substring"
            | "mbsubstring"
            | "String"
            | "Number"
            | "Boolean"
            | "int"
            | "uint"
            | "parseInt"
            | "parseFloat"
            | "Math.abs"
            | "Math.floor"
            | "Math.ceil"
            | "Math.round"
            | "Math.sqrt"
            | "Math.pow"
            | "Math.min"
            | "Math.max"
    )
}

pub(super) fn is_supported_builtin_pure_call_head_for(head: &str, lang: ScriptLanguage) -> bool {
    if head == "eval" {
        return lang != ScriptLanguage::ActionScript3;
    }
    if matches!(
        head,
        "chr" | "ord" | "mbchr" | "mbord" | "length" | "mblength" | "substring" | "mbsubstring"
    ) {
        return matches!(
            lang,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        );
    }
    is_supported_builtin_pure_call_head(head)
}

pub(super) fn is_supported_postfix_value_method(method: &str) -> bool {
    matches!(
        method,
        "join"
            | "toString"
            | "valueOf"
            | "slice"
            | "concat"
            | "indexOf"
            | "lastIndexOf"
            | "split"
            | "charAt"
            | "charCodeAt"
            | "substring"
            | "substr"
            | "toUpperCase"
            | "toLowerCase"
            | "trim"
    )
}

pub(super) fn is_supported_preview_rhs_for(rhs: &str, lang: ScriptLanguage) -> bool {
    let mut trimmed = rhs.trim();
    while trimmed.starts_with('(') && trimmed.ends_with(')') && is_single_paren_group(trimmed) {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }
    if trimmed.is_empty() || extract_function_literal_body(trimmed).is_some() {
        return false;
    }
    if let Some(rest) = trimmed.strip_prefix("new ") {
        let ctor = rest.trim_start();
        return ctor.starts_with("Array(")
            || ctor == "Array"
            || ctor.starts_with("Object(")
            || ctor == "Object";
    }
    if let Some((head, _)) = parse_qualified_call(trimmed) {
        if head == "Array" || is_supported_builtin_pure_call_head_for(head, lang) {
            return true;
        }
        if let Some((_, method)) = split_receiver_member(head) {
            return is_supported_postfix_value_method(method);
        }
        return false;
    }
    if let Some((base, tokens)) = parse_postfix_steps(trimmed) {
        if let Some((head, _)) = parse_qualified_call(base)
            && head != "Array"
            && !is_supported_builtin_pure_call_head_for(head, lang)
        {
            return false;
        }
        if tokens
            .iter()
            .any(|t| matches!(t, PostfixToken::Call(m, _) if !is_supported_postfix_value_method(m)))
        {
            return false;
        }
    }
    true
}

pub(super) fn apply_compound_var_op(
    op: &str,
    cur_val: &PreviewValue,
    rhs_val: &PreviewValue,
    lang: ScriptLanguage,
) -> Option<PreviewValue> {
    let is_as1 = lang == ScriptLanguage::ActionScript1;
    match op {
        "=" => Some(rhs_val.clone()),
        "+=" => {
            if matches!(cur_val, PreviewValue::String(_) | PreviewValue::Array(_))
                || matches!(rhs_val, PreviewValue::String(_) | PreviewValue::Array(_))
            {
                let mut s = cur_val.to_es_string_for(lang);
                s.push_str(&rhs_val.to_es_string_for(lang));
                Some(PreviewValue::String(s))
            } else {
                let a = cur_val.to_compound_lhs_number(is_as1);
                let b = rhs_val.to_number(is_as1);
                Some(PreviewValue::Number(a + b))
            }
        }
        "-=" => Some(PreviewValue::Number(
            cur_val.to_compound_lhs_number(is_as1) - rhs_val.to_number(is_as1),
        )),
        "*=" => Some(PreviewValue::Number(
            cur_val.to_compound_lhs_number(is_as1) * rhs_val.to_number(is_as1),
        )),
        "/=" => {
            let a = cur_val.to_compound_lhs_number(is_as1);
            let b = rhs_val.to_number(is_as1);
            if b.is_nan() {
                Some(PreviewValue::Number(f64::NAN))
            } else if b.abs() < 1e-12 {
                Some(PreviewValue::Number(0.0))
            } else {
                Some(PreviewValue::Number(a / b))
            }
        }
        "%=" => {
            let a = cur_val.to_compound_lhs_number(is_as1);
            let b = rhs_val.to_number(is_as1);
            if b.is_nan() {
                Some(PreviewValue::Number(f64::NAN))
            } else if b.abs() < 1e-12 {
                Some(PreviewValue::Number(0.0))
            } else {
                Some(PreviewValue::Number(a % b))
            }
        }
        "&=" => Some(PreviewValue::Number(f64::from(
            cur_val.to_i32(is_as1) & rhs_val.to_i32(is_as1),
        ))),
        "|=" => Some(PreviewValue::Number(f64::from(
            cur_val.to_i32(is_as1) | rhs_val.to_i32(is_as1),
        ))),
        "^=" => Some(PreviewValue::Number(f64::from(
            cur_val.to_i32(is_as1) ^ rhs_val.to_i32(is_as1),
        ))),
        "<<=" => Some(PreviewValue::Number(f64::from(
            cur_val
                .to_i32(is_as1)
                .wrapping_shl(rhs_val.to_u32(is_as1) & 0x1f),
        ))),
        ">>=" => Some(PreviewValue::Number(f64::from(
            cur_val
                .to_i32(is_as1)
                .wrapping_shr(rhs_val.to_u32(is_as1) & 0x1f),
        ))),
        ">>>=" => Some(PreviewValue::Number(f64::from(
            cur_val
                .to_u32(is_as1)
                .wrapping_shr(rhs_val.to_u32(is_as1) & 0x1f),
        ))),
        _ => None,
    }
}

pub(super) fn truncate_snippet(s: &str, max_chars: usize) -> String {
    let single_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        single_line
    } else {
        let head: String = single_line.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

pub(super) fn format_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        }
    } else if (n - n.round()).abs() < 1e-9 && n.abs() < (i64::MAX as f64) {
        format!("{}", n.round() as i64)
    } else {
        format!("{n}")
    }
}

pub(super) fn unescape_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Strip `//` and `/* ... */` comments outside string literals (`'...'`, `"..."`, `` `...` ``)
/// while preserving arbitrary UTF-8 codepoints verbatim (`T-259`).
pub(super) fn strip_comments_and_trim(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    if bytes[i] == b'\n' {
                        out.push(b'\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'`' => in_backtick = true,
            _ => {}
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out)
        .unwrap_or_else(|_| s.to_owned())
        .trim()
        .to_owned()
}

/// Brace-, paren-, bracket-, string- and comment-aware top-level statement splitter.
/// Returns `(1_based_line, statement_slice)`.
pub(super) fn split_script_statements(source: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut start_line = 1usize;
    let mut cur_line = 1usize;
    let mut saw_token_in_seg = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && saw_token_in_seg {
                    let seg = source[start..i].trim();
                    if !seg.is_empty() {
                        out.push((start_line, seg));
                    }
                    start = i + 1;
                    saw_token_in_seg = false;
                }
                cur_line += 1;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_single {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'\\' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'\n' {
                    cur_line += 1;
                }
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'\\' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'\n' {
                    cur_line += 1;
                }
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'\\' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'\n' {
                    cur_line += 1;
                }
                i += 2;
                continue;
            }
            if b == b'`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }
        if !b.is_ascii_whitespace() && !saw_token_in_seg {
            saw_token_in_seg = true;
            start_line = cur_line;
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'`' => in_backtick = true,
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
                    // Check if followed by `else` (after optional whitespace/comments) or `;`
                    let after = &source[i + 1..];
                    let after_trimmed = strip_comments_and_trim(after);
                    if !after_trimmed.starts_with("else")
                        && !after_trimmed.starts_with(';')
                        && !after_trimmed.starts_with(')')
                        && !after_trimmed.starts_with(',')
                    {
                        let seg = source[start..=i].trim();
                        if !seg.is_empty() {
                            out.push((start_line, seg));
                        }
                        start = i + 1;
                        saw_token_in_seg = false;
                    }
                }
            }
            b';' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                let seg = source[start..i].trim();
                if !seg.is_empty() {
                    out.push((start_line, seg));
                }
                start = i + 1;
                saw_token_in_seg = false;
            }
            b'\n' => {
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && saw_token_in_seg {
                    let after = strip_comments_and_trim(&source[i + 1..]);
                    if !after.starts_with("else") && !after.starts_with('{') {
                        let seg = source[start..i].trim();
                        if !seg.is_empty() {
                            out.push((start_line, seg));
                        }
                        start = i + 1;
                        saw_token_in_seg = false;
                    }
                }
                cur_line += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < source.len() {
        let seg = source[start..].trim();
        if !seg.is_empty() {
            out.push((start_line, seg));
        }
    }
    out
}

pub(super) fn collect_local_functions(source: &str) -> HashMap<String, (usize, String)> {
    let mut map = HashMap::new();
    for (line, raw_stmt) in split_script_statements(source) {
        let stmt = strip_comments_and_trim(raw_stmt);
        if let Some((name, body)) = parse_named_function_decl(&stmt) {
            map.insert(name.to_owned(), (line, body.to_owned()));
        }
    }
    map
}

pub(super) fn parse_named_function_decl(stmt: &str) -> Option<(&str, &str)> {
    let rest = stmt.strip_prefix("function")?.trim_start();
    if rest.starts_with('(') {
        return None;
    }
    let open_paren = rest.find('(')?;
    let name = rest[..open_paren].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let after_name = &rest[open_paren..];
    let close_paren = find_matching_delim(after_name, b'(', b')')?;
    let after_params = after_name[close_paren + 1..].trim_start();
    let brace_idx = after_params.find('{')?;
    // Between `)` and `{` only optional `: ReturnType` annotation is allowed
    let between = after_params[..brace_idx].trim();
    if !between.is_empty() && !between.starts_with(':') {
        return None;
    }
    let body_block = after_params[brace_idx..].trim();
    let close_brace = find_matching_delim(body_block, b'{', b'}')?;
    if close_brace + 1 != body_block.len() {
        return None;
    }
    Some((name, &body_block[1..close_brace]))
}

pub(super) fn parse_wrapper_block<'a>(stmt: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let rest = stmt.strip_prefix(keyword)?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close_paren = find_matching_delim(rest, b'(', b')')?;
    let events = &rest[1..close_paren];
    let body_part = rest[close_paren + 1..].trim();
    if !body_part.starts_with('{') {
        return None;
    }
    let close_brace = find_matching_delim(body_part, b'{', b'}')?;
    if close_brace + 1 != body_part.len() {
        return None;
    }
    Some((events, &body_part[1..close_brace]))
}

pub(super) fn parse_if_statement(stmt: &str) -> Option<(&str, &str, Option<&str>)> {
    let rest = stmt.strip_prefix("if")?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close_paren = find_matching_delim(rest, b'(', b')')?;
    let cond = &rest[1..close_paren];
    let after_cond = rest[close_paren + 1..].trim();
    if let Some(close_brace) = after_cond
        .starts_with('{')
        .then(|| find_matching_delim(after_cond, b'{', b'}'))
        .flatten()
    {
        let then_body = &after_cond[1..close_brace];
        let tail = after_cond[close_brace + 1..].trim();
        if tail.is_empty() {
            return Some((cond, then_body, None));
        }
        if let Some(else_rest) = tail.strip_prefix("else")
            && !else_rest
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            let else_trimmed = else_rest.trim();
            if else_trimmed.starts_with('{')
                && let Some(end_b) = find_matching_delim(else_trimmed, b'{', b'}')
                && end_b + 1 == else_trimmed.len()
            {
                return Some((cond, then_body, Some(&else_trimmed[1..end_b])));
            }
            return Some((cond, then_body, Some(else_trimmed)));
        }
        None
    } else {
        // Single-statement `if (cond) stmt`
        Some((cond, after_cond, None))
    }
}

pub(super) fn extract_function_literal_body(expr: &str) -> Option<&str> {
    let s = expr.trim();
    if let Some(rest) = s.strip_prefix("function") {
        let open_paren = rest.find('(')?;
        let after_open = &rest[open_paren..];
        let close_paren = find_matching_delim(after_open, b'(', b')')?;
        let after_params = after_open[close_paren + 1..].trim_start();
        let brace_idx = after_params.find('{')?;
        let block = after_params[brace_idx..].trim();
        let close_brace = find_matching_delim(block, b'{', b'}')?;
        if close_brace + 1 == block.len() {
            return Some(&block[1..close_brace]);
        }
        return None;
    }
    // Arrow function: `(e) => { ... }` or `() => stmt` or `e => { ... }`
    if let Some(arrow_pos) = find_top_level_arrow(s) {
        let rhs = s[arrow_pos + 2..].trim();
        if rhs.starts_with('{')
            && let Some(close_brace) = find_matching_delim(rhs, b'{', b'}')
            && close_brace + 1 == rhs.len()
        {
            return Some(&rhs[1..close_brace]);
        }
        return Some(rhs);
    }
    None
}

pub(super) fn find_top_level_arrow(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut in_s = false;
    let mut in_d = false;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'=' if bytes[i + 1] == b'>' && paren == 0 && brace == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn find_matching_delim(s: &str, open: u8, close: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first().copied() != Some(open) {
        return None;
    }
    let mut depth = 0usize;
    let mut i = 0;
    let mut in_s = false;
    let mut in_d = false;
    let mut in_b = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        if in_b {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'`' {
                in_b = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'`' => in_b = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn is_single_braced_group(s: &str) -> bool {
    find_matching_delim(s, b'{', b'}') == Some(s.len().saturating_sub(1))
}

pub(super) fn is_single_paren_group(s: &str) -> bool {
    find_matching_delim(s, b'(', b')') == Some(s.len().saturating_sub(1))
}

pub(super) fn extract_call_arg<'a>(stmt: &'a str, fn_name: &str) -> Option<&'a str> {
    let rest = stmt.strip_prefix(fn_name)?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close = find_matching_delim(rest, b'(', b')')?;
    if close + 1 != rest.len() {
        return None;
    }
    Some(rest[1..close].trim())
}

pub(super) fn parse_qualified_call(stmt: &str) -> Option<(&str, &str)> {
    let open = stmt.find('(')?;
    let head = stmt[..open].trim();
    if head.is_empty()
        || !head
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
    {
        return None;
    }
    let args_part = &stmt[open..];
    let close = find_matching_delim(args_part, b'(', b')')?;
    if close + 1 != args_part.len() {
        return None;
    }
    Some((head, args_part[1..close].trim()))
}

pub(super) fn parse_simple_call(stmt: &str) -> Option<(&str, &str)> {
    let (head, args) = parse_qualified_call(stmt)?;
    if head.contains('.') {
        return None;
    }
    Some((head, args))
}

pub(super) fn split_receiver_member(expr: &str) -> Option<(&str, &str)> {
    let trimmed = expr.trim();
    let dot = trimmed.rfind('.')?;
    let recv = trimmed[..dot].trim();
    let member = trimmed[dot + 1..].trim();
    if recv.is_empty() || member.is_empty() {
        return None;
    }
    if !recv
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
    {
        return None;
    }
    Some((recv, member))
}

pub(super) fn split_top_level_assign(stmt: &str) -> Option<(&str, &str)> {
    let bytes = stmt.as_bytes();
    let mut i = 0;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut in_s = false;
    let mut in_d = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            b'=' if paren == 0 && brace == 0 && bracket == 0 => {
                let prev = if i > 0 { bytes[i - 1] } else { 0 };
                let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
                if !matches!(
                    prev,
                    b'!' | b'='
                        | b'<'
                        | b'>'
                        | b'+'
                        | b'-'
                        | b'*'
                        | b'/'
                        | b'%'
                        | b'&'
                        | b'|'
                        | b'^'
                ) && next != b'='
                    && next != b'>'
                {
                    return Some((stmt[..i].trim(), stmt[i + 1..].trim()));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn split_compound_op<'a>(stmt: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let parts = split_top_level_op(stmt, op);
    if parts.len() == 2 {
        Some((parts[0].trim(), parts[1].trim()))
    } else {
        None
    }
}

pub(super) fn split_top_level_comma(s: &str) -> Vec<&str> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let parts = split_top_level_op(trimmed, ",");
    if parts.is_empty() {
        vec![trimmed]
    } else {
        parts
    }
}

pub(super) fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

pub(super) fn strip_unary_keyword<'a>(expr: &'a str, kw: &str) -> Option<&'a str> {
    let rest = expr.strip_prefix(kw)?;
    let first = rest.as_bytes().first().copied()?;
    if is_ident_byte(first) {
        return None;
    }
    let trimmed = rest.trim_start();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(super) fn op_has_boundary_conflict(expr: &str, bytes: &[u8], i: usize, op: &str) -> bool {
    let op_bytes = op.as_bytes();
    let prev = if i > 0 { bytes[i - 1] } else { 0 };
    let next = bytes.get(i + op_bytes.len()).copied().unwrap_or(0);

    if op_bytes[0].is_ascii_alphabetic() {
        return prev == b'.' || is_ident_byte(prev) || is_ident_byte(next);
    }

    match op {
        "===" | "!==" | ">>>=" | "<<=" => false,
        ">=" => prev == b'>',
        "<=" => prev == b'<',
        ">>=" => prev == b'>',
        "==" => prev == b'=' || prev == b'!' || next == b'=',
        "!=" => next == b'=',
        "<>" => false,
        ">>>" => next == b'=',
        "<<" => next == b'=',
        ">>" => prev == b'>' || next == b'>' || next == b'=',
        "<" => prev == b'<' || next == b'<' || next == b'=' || next == b'>',
        ">" => prev == b'>' || prev == b'<' || prev == b'=' || next == b'>' || next == b'=',
        "&&" => next == b'=',
        "||" => next == b'=',
        "&" => prev == b'&' || next == b'&' || next == b'=',
        "|" => prev == b'|' || next == b'|' || next == b'=',
        "^" => next == b'=',
        "*" | "/" | "%" => next == b'=',
        "+" => {
            if prev == b'+' || next == b'+' || next == b'=' {
                return true;
            }
            if matches!(prev, b'e' | b'E')
                && i >= 2
                && (bytes[i - 2].is_ascii_digit() || bytes[i - 2] == b'.')
            {
                return true;
            }
            let prev_non_ws = expr[..i].trim_end().as_bytes().last().copied();
            !prev_non_ws.is_some_and(|p| {
                p.is_ascii_alphanumeric() || matches!(p, b')' | b']' | b'"' | b'\'' | b'_' | b'$')
            })
        }
        "-" => {
            if prev == b'-' || next == b'-' || next == b'=' || next == b'>' {
                return true;
            }
            if matches!(prev, b'e' | b'E')
                && i >= 2
                && (bytes[i - 2].is_ascii_digit() || bytes[i - 2] == b'.')
            {
                return true;
            }
            let prev_non_ws = expr[..i].trim_end().as_bytes().last().copied();
            !prev_non_ws.is_some_and(|p| {
                p.is_ascii_alphanumeric() || matches!(p, b')' | b']' | b'"' | b'\'' | b'_' | b'$')
            })
        }
        _ => false,
    }
}

pub(super) fn split_top_level_op<'a>(expr: &'a str, op: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let bytes = expr.as_bytes();
    let op_bytes = op.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            _ if paren == 0
                && brace == 0
                && bracket == 0
                && i + op_bytes.len() <= bytes.len()
                && &bytes[i..i + op_bytes.len()] == op_bytes
                && !op_has_boundary_conflict(expr, bytes, i, op) =>
            {
                parts.push(expr[start..i].trim());
                start = i + op_bytes.len();
                i = start;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if !parts.is_empty() {
        parts.push(expr[start..].trim());
    }
    parts
}

pub(super) fn rfind_top_level_ops<'a, 'b>(
    expr: &'a str,
    ops: &[&'b str],
) -> Option<(&'a str, &'b str, &'a str)> {
    let bytes = expr.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut last_match: Option<(usize, &'b str)> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => {
                in_single = true;
                i += 1;
                continue;
            }
            b'"' => {
                in_double = true;
                i += 1;
                continue;
            }
            b'(' => {
                paren += 1;
                i += 1;
                continue;
            }
            b')' => {
                paren = paren.saturating_sub(1);
                i += 1;
                continue;
            }
            b'{' => {
                brace += 1;
                i += 1;
                continue;
            }
            b'}' => {
                brace = brace.saturating_sub(1);
                i += 1;
                continue;
            }
            b'[' => {
                bracket += 1;
                i += 1;
                continue;
            }
            b']' => {
                bracket = bracket.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }
        if paren == 0 && brace == 0 && bracket == 0 {
            let mut matched = None;
            for &op in ops {
                let op_bytes = op.as_bytes();
                if i + op_bytes.len() <= bytes.len()
                    && &bytes[i..i + op_bytes.len()] == op_bytes
                    && !op_has_boundary_conflict(expr, bytes, i, op)
                {
                    matched = Some(op);
                    break;
                }
            }
            if let Some(op) = matched {
                let lhs = expr[..i].trim();
                let rhs = expr[i + op.len()..].trim();
                if !lhs.is_empty() && !rhs.is_empty() {
                    last_match = Some((i, op));
                }
                i += op.len();
                continue;
            }
        }
        i += 1;
    }
    let (pos, op) = last_match?;
    Some((expr[..pos].trim(), op, expr[pos + op.len()..].trim()))
}

pub(super) fn split_top_level_ternary(expr: &str) -> Option<(&str, &str, &str)> {
    let bytes = expr.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut q_pos = None;
    let mut nested_q = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            b'?' if paren == 0 && brace == 0 && bracket == 0 => {
                let next = bytes.get(i + 1).copied().unwrap_or(0);
                if next != b'?' && next != b'.' {
                    if q_pos.is_none() {
                        q_pos = Some(i);
                    } else {
                        nested_q += 1;
                    }
                }
            }
            b':' if paren == 0 && brace == 0 && bracket == 0 && q_pos.is_some() => {
                let prev = if i > 0 { bytes[i - 1] } else { 0 };
                let next = bytes.get(i + 1).copied().unwrap_or(0);
                if prev != b':' && next != b':' {
                    if nested_q == 0 {
                        let q = q_pos?;
                        let cond = expr[..q].trim();
                        let yes = expr[q + 1..i].trim();
                        let no = expr[i + 1..].trim();
                        if !cond.is_empty() && !yes.is_empty() && !no.is_empty() {
                            return Some((cond, yes, no));
                        }
                        return None;
                    }
                    nested_q -= 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
