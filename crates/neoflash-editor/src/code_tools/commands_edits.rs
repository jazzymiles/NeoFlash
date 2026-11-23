use super::*;

pub(crate) type Edit = (String, (usize, usize));

/// A toolbar/toolbox command for the visible code editor. It is queued in `ActionsUi::pending`
/// and applied by the code editor on its next frame, where the text and selection are known.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CodeCommand {
    LineComment,
    BlockComment,
    Uncomment,
    AutoFormat,
    /// Open code hints at the caret (toolbar "Show code hint", ⌃Space).
    ShowHints,
    /// Replace the selection with `text`, caret at char `caret` inside it; optionally add an AS3
    /// `import` for the qualified class when the script does not import it yet.
    Insert {
        text: String,
        caret: usize,
        import: Option<String>,
    },
    /// Apply a compiler `QuickFix` replacement at its byte span.
    #[allow(dead_code)]
    ApplyQuickFix(neoflash_script::QuickFix),
}

/// Apply a text command using the default 4-space tab size. `ShowHints` changes no text and returns `None`.
#[cfg(test)]
pub(crate) fn apply(command: &CodeCommand, text: &str, selection: (usize, usize)) -> Option<Edit> {
    apply_with_tab_size(command, text, selection, 4)
}

/// Apply a text command using `tab_size` spaces (`1..=8`). `ShowHints` changes no text and returns `None`.
pub(crate) fn apply_with_tab_size(
    command: &CodeCommand,
    text: &str,
    selection: (usize, usize),
    tab_size: u8,
) -> Option<Edit> {
    let selection = (selection.0.min(selection.1), selection.0.max(selection.1));
    match command {
        CodeCommand::LineComment => line_comment_with_tab_size(text, selection, tab_size),
        CodeCommand::BlockComment => block_comment(text, selection),
        CodeCommand::Uncomment => uncomment(text, selection),
        CodeCommand::AutoFormat => {
            // Keep the caret on the same line, at the same distance from the line end.
            let formatted = auto_format_with_tab_size(text, tab_size)?;
            let line = text
                .chars()
                .take(selection.1)
                .filter(|c| *c == '\n')
                .count();
            let line_bounds = |s: &str| {
                let start: usize = s
                    .split('\n')
                    .take(line)
                    .map(|l| l.chars().count() + 1)
                    .sum();
                let end = start + s.split('\n').nth(line).map_or(0, |l| l.chars().count());
                (start, end)
            };
            let (_, old_end) = line_bounds(text);
            let (new_start, new_end) = line_bounds(&formatted);
            let from_end = old_end.saturating_sub(selection.1);
            let caret = new_end
                .saturating_sub(from_end)
                .max(new_start.min(formatted.chars().count()))
                .min(formatted.chars().count());
            Some((formatted, (caret, caret)))
        }
        CodeCommand::ShowHints => None,
        CodeCommand::ApplyQuickFix(fix) => apply_quick_fix_edit(text, fix),
        CodeCommand::Insert {
            text: insert,
            caret,
            import,
        } => {
            let from = char_to_byte(text, selection.0);
            let to = char_to_byte(text, selection.1);
            let mut out = format!("{}{}{}", &text[..from], insert, &text[to..]);
            let mut position = selection.0 + caret;
            if let Some(line) = import
                .as_deref()
                .and_then(|q| crate::script_api::missing_import(&out, q))
            {
                let (at, inserted_chars) = insert_as3_import_line(&mut out, &line);
                if out[..at].chars().count() <= position {
                    position += inserted_chars;
                }
            }
            Some((out, (position, position)))
        }
    }
}

pub(crate) fn insert_as3_import_line(out: &mut String, line: &str) -> (usize, usize) {
    if let Some(open_brace) = find_package_open_brace(out) {
        let after_brace = &out[open_brace + 1..];
        let rel_nl = after_brace.find('\n');
        let rest_of_brace_line = rel_nl.map_or(after_brace, |n| &after_brace[..n]);
        if let Some(nl_offset) = rel_nl.filter(|_| {
            rest_of_brace_line
                .bytes()
                .all(|b| matches!(b, b' ' | b'\t' | b'\r'))
        }) {
            let body_start = open_brace + 1 + nl_offset + 1;
            let mut cursor = body_start;
            let mut last_import_end: Option<usize> = None;
            let mut last_import_indent: Option<String> = None;
            let mut first_stmt_indent: Option<String> = None;
            let mut in_block_comment = false;

            while cursor < out.len() {
                let rest = &out[cursor..];
                let (line_str, line_end_with_nl) = match rest.find('\n') {
                    Some(n) => (&rest[..n], cursor + n + 1),
                    None => (rest, out.len()),
                };
                let line_clean = line_str.strip_suffix('\r').unwrap_or(line_str);
                let trimmed = line_clean.trim_start_matches([' ', '\t']);
                let indent = &line_clean[..line_clean.len() - trimmed.len()];

                if in_block_comment {
                    if let Some(end_idx) = trimmed.find("*/") {
                        in_block_comment = false;
                        let after_comment = trimmed[end_idx + 2..].trim_start_matches([' ', '\t']);
                        if after_comment.is_empty() {
                            cursor = line_end_with_nl;
                            continue;
                        }
                    } else {
                        cursor = line_end_with_nl;
                        continue;
                    }
                }

                if trimmed.is_empty() || trimmed.starts_with("//") {
                    cursor = line_end_with_nl;
                    continue;
                }
                if trimmed.starts_with("/*") {
                    if !trimmed.contains("*/") {
                        in_block_comment = true;
                    }
                    cursor = line_end_with_nl;
                    continue;
                }

                if first_stmt_indent.is_none() && !trimmed.starts_with('}') {
                    first_stmt_indent = Some(indent.to_owned());
                }

                if trimmed.starts_with("import ") || trimmed.starts_with("import\t") {
                    last_import_end = Some(line_end_with_nl);
                    last_import_indent = Some(indent.to_owned());
                    cursor = line_end_with_nl;
                    continue;
                }

                break;
            }

            if let Some(at) = last_import_end {
                let indent = last_import_indent.unwrap_or_else(|| "    ".to_owned());
                let prefix = if at > 0 && out.as_bytes()[at - 1] != b'\n' {
                    "\n"
                } else {
                    ""
                };
                let formatted = format!("{prefix}{indent}{line}\n");
                let chars = formatted.chars().count();
                out.insert_str(at, &formatted);
                return (at, chars);
            }

            let at = body_start;
            let indent = first_stmt_indent
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "    ".to_owned());
            let formatted = format!("{indent}{line}\n");
            let chars = formatted.chars().count();
            out.insert_str(at, &formatted);
            return (at, chars);
        }

        // Single-line `package { ... }` block: advance past any inline `import ...;` statements.
        let mut scan = open_brace + 1;
        let mut inline_import_end: Option<usize> = None;
        let bytes = out.as_bytes();
        loop {
            while scan < bytes.len() && matches!(bytes[scan], b' ' | b'\t' | b'\r') {
                scan += 1;
            }
            let remaining = &out[scan..];
            if (remaining.starts_with("import ") || remaining.starts_with("import\t"))
                && let Some(semi) = remaining.find(';')
            {
                scan += semi + 1;
                inline_import_end = Some(scan);
                continue;
            }
            break;
        }
        let at = inline_import_end.unwrap_or(open_brace + 1);
        let formatted = if out
            .as_bytes()
            .get(at)
            .is_some_and(|b| b.is_ascii_whitespace())
        {
            format!(" {line}")
        } else {
            format!(" {line} ")
        };
        let chars = formatted.chars().count();
        out.insert_str(at, &formatted);
        return (at, chars);
    }

    // No `package` block: insert after the last leading/top-level import, or at the top.
    let mut cursor = 0;
    let mut last_import_end: Option<usize> = None;
    let mut last_import_indent = String::new();
    while cursor < out.len() {
        let rest = &out[cursor..];
        let (line_str, line_end_with_nl) = match rest.find('\n') {
            Some(n) => (&rest[..n], cursor + n + 1),
            None => (rest, out.len()),
        };
        let line_clean = line_str.strip_suffix('\r').unwrap_or(line_str);
        let trimmed = line_clean.trim_start_matches([' ', '\t']);
        if trimmed.starts_with("import ") || trimmed.starts_with("import\t") {
            let indent = &line_clean[..line_clean.len() - trimmed.len()];
            last_import_end = Some(line_end_with_nl);
            last_import_indent = indent.to_owned();
        }
        cursor = line_end_with_nl;
    }

    let at = last_import_end.unwrap_or(0);
    let formatted = if at == out.len() && !out.ends_with('\n') && !out.is_empty() {
        format!("\n{last_import_indent}{line}\n")
    } else {
        format!("{last_import_indent}{line}\n")
    };
    let chars = formatted.chars().count();
    out.insert_str(at, &formatted);
    (at, chars)
}

pub(super) fn find_package_open_brace(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut depth = 0usize;

    while i < len {
        let b = bytes[i];
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            continue;
        }
        if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(len);
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b == b'{' {
            depth += 1;
            i += 1;
            continue;
        }
        if b == b'}' {
            depth = depth.saturating_sub(1);
            i += 1;
            continue;
        }
        if depth == 0 && source[i..].starts_with("package") {
            let prev_ok = i == 0
                || (!bytes[i - 1].is_ascii_alphanumeric()
                    && bytes[i - 1] != b'_'
                    && bytes[i - 1] != b'$'
                    && bytes[i - 1] != b'.');
            let after_pkg = i + "package".len();
            let next_ok = after_pkg >= len
                || (!bytes[after_pkg].is_ascii_alphanumeric()
                    && bytes[after_pkg] != b'_'
                    && bytes[after_pkg] != b'$');
            if prev_ok && next_ok {
                let mut j = after_pkg;
                while j < len {
                    let jb = bytes[j];
                    if jb.is_ascii_whitespace()
                        || jb.is_ascii_alphanumeric()
                        || jb == b'_'
                        || jb == b'$'
                        || jb == b'.'
                    {
                        j += 1;
                        continue;
                    }
                    if jb == b'/' && j + 1 < len && bytes[j + 1] == b'/' {
                        j += 2;
                        while j < len && bytes[j] != b'\n' {
                            j += 1;
                        }
                        continue;
                    }
                    if jb == b'/' && j + 1 < len && bytes[j + 1] == b'*' {
                        j += 2;
                        while j + 1 < len && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                            j += 1;
                        }
                        j = (j + 2).min(len);
                        continue;
                    }
                    if jb == b'{' {
                        return Some(j);
                    }
                    break;
                }
            }
        }
        i += 1;
    }
    None
}

pub(super) fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(i, _)| i)
}

pub(super) fn visual_indent(line: &str, step: usize) -> usize {
    let mut col = 0;
    for ch in line.chars() {
        match ch {
            ' ' => col += 1,
            '\t' => col = (col / step + 1) * step,
            _ => break,
        }
    }
    col
}

/// Byte ranges of the whole lines touched by a char selection.
pub(super) fn line_span(text: &str, (start, end): (usize, usize)) -> (usize, usize) {
    let start = char_to_byte(text, start);
    let mut end = char_to_byte(text, end);
    // A selection ending at column 1 does not include that line.
    if end > start && text[..end].ends_with('\n') {
        end -= 1;
    }
    let first = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let last = text[end..].find('\n').map_or(text.len(), |i| end + i);
    (first, last)
}

pub(super) fn replace_lines(text: &str, span: (usize, usize), lines: &[String]) -> Option<Edit> {
    let joined = lines.join("\n");
    if joined == text[span.0..span.1] {
        return None;
    }
    let out = format!("{}{}{}", &text[..span.0], joined, &text[span.1..]);
    let start = text[..span.0].chars().count();
    Some((out, (start, start + joined.chars().count())))
}

/// "Apply line comment": prefix every selected line with `// ` at the block's indentation.
#[cfg(test)]
pub(crate) fn line_comment(text: &str, selection: (usize, usize)) -> Option<Edit> {
    line_comment_with_tab_size(text, selection, 4)
}

/// "Apply line comment" accounting for `tab_size` visual columns when leading tabs and spaces mix.
pub(crate) fn line_comment_with_tab_size(
    text: &str,
    selection: (usize, usize),
    tab_size: u8,
) -> Option<Edit> {
    let step = usize::from(tab_size.clamp(1, 8));
    let span = line_span(text, selection);
    let block = &text[span.0..span.1];
    let min_col = block
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| visual_indent(l, step))
        .min()
        .unwrap_or(0);
    let lines: Vec<String> = block
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return line.to_owned();
            }
            let mut col = 0;
            let mut split_byte = 0;
            let mut split_tab_excess = None;
            for (i, ch) in line.char_indices() {
                if col == min_col {
                    split_byte = i;
                    break;
                }
                match ch {
                    ' ' => {
                        col += 1;
                        split_byte = i + 1;
                    }
                    '\t' => {
                        let next_col = (col / step + 1) * step;
                        if next_col <= min_col {
                            col = next_col;
                            split_byte = i + 1;
                        } else {
                            split_tab_excess = Some((i, min_col - col, next_col - min_col));
                            break;
                        }
                    }
                    _ => {
                        split_byte = i;
                        break;
                    }
                }
            }
            if let Some((tab_byte, before, after)) = split_tab_excess {
                format!(
                    "{}{}// {}{}",
                    &line[..tab_byte],
                    " ".repeat(before),
                    " ".repeat(after),
                    &line[tab_byte + 1..]
                )
            } else {
                format!("{}// {}", &line[..split_byte], &line[split_byte..])
            }
        })
        .collect();
    replace_lines(text, span, &lines)
}

/// "Apply block comment": wrap the selection in `/* */`; without a selection, the caret line.
pub(crate) fn block_comment(text: &str, (start, end): (usize, usize)) -> Option<Edit> {
    let (from, to) = if start == end {
        let (first, last) = line_span(text, (start, end));
        let line = &text[first..last];
        let lead = line.len() - line.trim_start().len();
        (first + lead, last)
    } else {
        (char_to_byte(text, start), char_to_byte(text, end))
    };
    if text[from..to].trim().is_empty() {
        return None;
    }
    let out = format!("{}/* {} */{}", &text[..from], &text[from..to], &text[to..]);
    let begin = text[..from].chars().count();
    Some((out, (begin, begin + text[from..to].chars().count() + 6)))
}

/// "Remove comment": unwrap a selected `/* … */` block, otherwise strip a leading `//` from every
/// selected line.
pub(crate) fn uncomment(text: &str, selection: (usize, usize)) -> Option<Edit> {
    let (start, end) = (
        char_to_byte(text, selection.0),
        char_to_byte(text, selection.1),
    );
    let selected = text[start..end].trim();
    if selected.starts_with("/*") && selected.ends_with("*/") && selected.len() >= 4 {
        let lead = start + (text[start..end].len() - text[start..end].trim_start().len());
        let tail = lead + selected.len();
        let inner = &text[lead + 2..tail - 2];
        let inner = inner.strip_prefix(' ').unwrap_or(inner);
        let inner = inner.strip_suffix(' ').unwrap_or(inner);
        let out = format!("{}{}{}", &text[..lead], inner, &text[tail..]);
        let begin = text[..lead].chars().count();
        return Some((out, (begin, begin + inner.chars().count())));
    }
    let span = line_span(text, selection);
    let lines: Vec<String> = text[span.0..span.1]
        .split('\n')
        .map(|line| {
            let body = line.trim_start();
            match body.strip_prefix("//") {
                Some(rest) => {
                    let rest = rest.strip_prefix(' ').unwrap_or(rest);
                    format!("{}{}", &line[..line.len() - body.len()], rest)
                }
                None => line.to_owned(),
            }
        })
        .collect();
    replace_lines(text, span, &lines)
}

/// Insert `tab_size` spaces at the caret, replace a single-line selection with `tab_size` spaces,
/// or indent every non-empty line of a multi-line selection by `tab_size` spaces.
pub(crate) fn indent_tab(text: &str, selection: (usize, usize), tab_size: u8) -> Edit {
    let (start, end) = (selection.0.min(selection.1), selection.0.max(selection.1));
    let step = usize::from(tab_size.clamp(1, 8));
    let pad = " ".repeat(step);
    let from = char_to_byte(text, start);
    let to = char_to_byte(text, end);
    if start < end && text[from..to].contains('\n') {
        let span = line_span(text, (start, end));
        let lines: Vec<String> = text[span.0..span.1]
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{pad}{line}")
                }
            })
            .collect();
        let joined = lines.join("\n");
        let out = format!("{}{}{}", &text[..span.0], joined, &text[span.1..]);
        let span_start = text[..span.0].chars().count();
        return (out, (span_start, span_start + joined.chars().count()));
    }
    let out = format!("{}{}{}", &text[..from], pad, &text[to..]);
    let caret = start + step;
    (out, (caret, caret))
}

/// Remove one leading tab or up to `tab_size` leading spaces from each selected line (`⇧Tab`).
pub(crate) fn outdent_tab(text: &str, selection: (usize, usize), tab_size: u8) -> Option<Edit> {
    let (start, end) = (selection.0.min(selection.1), selection.0.max(selection.1));
    let step = usize::from(tab_size.clamp(1, 8));
    let span = line_span(text, (start, end));
    let block = &text[span.0..span.1];
    let mut stripped_counts = Vec::new();
    let lines: Vec<String> = block
        .split('\n')
        .map(|line| {
            if let Some(rest) = line.strip_prefix('\t') {
                stripped_counts.push(1usize);
                rest.to_owned()
            } else {
                let spaces = line.chars().take_while(|&c| c == ' ').count().min(step);
                stripped_counts.push(spaces);
                line[spaces..].to_owned()
            }
        })
        .collect();
    if stripped_counts.iter().all(|&n| n == 0) {
        return None;
    }
    let joined = lines.join("\n");
    let out = format!("{}{}{}", &text[..span.0], joined, &text[span.1..]);
    let span_start = text[..span.0].chars().count();
    if stripped_counts.len() == 1 {
        let removed = stripped_counts[0];
        let new_start = span_start + (start - span_start).saturating_sub(removed);
        let new_end = span_start + (end - span_start).saturating_sub(removed);
        Some((out, (new_start, new_end)))
    } else {
        Some((out, (span_start, span_start + joined.chars().count())))
    }
}

pub(crate) fn is_auto_close_trigger(ch: char) -> bool {
    matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`')
}

pub(super) fn is_escaped_at(text: &str, byte_pos: usize) -> bool {
    text[..byte_pos]
        .chars()
        .rev()
        .take_while(|&c| c == '\\')
        .count()
        % 2
        == 1
}

/// Auto-close `()`, `[]`, `{}`, `"`, `'`, `` ` `` or skip over an existing closing delimiter.
/// Returns `None` when `ch` should be inserted literally.
pub(crate) fn auto_close_char(text: &str, caret: usize, ch: char) -> Option<Edit> {
    if !is_auto_close_trigger(ch) || caret > text.chars().count() {
        return None;
    }
    let byte_pos = char_to_byte(text, caret);
    if is_escaped_at(text, byte_pos) {
        return None;
    }
    let prev = text[..byte_pos].chars().next_back();
    let next = text[byte_pos..].chars().next();
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';

    match ch {
        ')' | ']' | '}' => {
            if next == Some(ch) {
                return Some((text.to_owned(), (caret + 1, caret + 1)));
            }
            None
        }
        '"' | '\'' | '`' => {
            if next == Some(ch) {
                return Some((text.to_owned(), (caret + 1, caret + 1)));
            }
            if prev.is_some_and(is_word) || next.is_some_and(is_word) {
                return None;
            }
            // If an odd number of unescaped `ch` already appear on this line before the caret,
            // the user is closing an open quote rather than starting a new pair.
            let line_start = text[..byte_pos].rfind('\n').map_or(0, |i| i + 1);
            let mut unescaped = 0usize;
            for (offset, c) in text[line_start..byte_pos].char_indices() {
                if c == ch && !is_escaped_at(text, line_start + offset) {
                    unescaped += 1;
                }
            }
            if unescaped % 2 == 1 {
                return None;
            }
            let out = format!("{}{ch}{ch}{}", &text[..byte_pos], &text[byte_pos..]);
            Some((out, (caret + 1, caret + 1)))
        }
        '(' | '[' | '{' => {
            if next.is_some_and(is_word) {
                return None;
            }
            let close = match ch {
                '(' => ')',
                '[' => ']',
                '{' => '}',
                _ => unreachable!(),
            };
            let out = format!("{}{ch}{close}{}", &text[..byte_pos], &text[byte_pos..]);
            Some((out, (caret + 1, caret + 1)))
        }
        _ => None,
    }
}

/// Delete both characters of an empty `()`, `[]`, `{}`, `""`, `''`, or `` `` `` pair on Backspace.
pub(crate) fn auto_delete_pair(text: &str, caret: usize) -> Option<Edit> {
    if caret == 0 || caret > text.chars().count() {
        return None;
    }
    let byte_pos = char_to_byte(text, caret);
    let prev = text[..byte_pos].chars().next_back()?;
    let next = text[byte_pos..].chars().next()?;
    let is_pair = matches!(
        (prev, next),
        ('(', ')') | ('[', ']') | ('{', '}') | ('"', '"') | ('\'', '\'') | ('`', '`')
    );
    if !is_pair {
        return None;
    }
    let prev_byte = byte_pos - prev.len_utf8();
    if is_escaped_at(text, prev_byte) {
        return None;
    }
    let next_end = byte_pos + next.len_utf8();
    let out = format!("{}{}", &text[..prev_byte], &text[next_end..]);
    Some((out, (caret - 1, caret - 1)))
}

/// Suggest deterministic, char-boundary-safe `QuickFix` actions for `diagnostic` against `source`.
pub(crate) fn suggest_quick_fixes_for_diagnostic(
    diagnostic: &neoflash_script::Diagnostic,
    source: &str,
) -> Vec<neoflash_script::QuickFix> {
    diagnostic
        .quick_fixes(source)
        .into_iter()
        .filter(|fix| apply_quick_fix(source, fix).is_some())
        .collect()
}

/// Apply a `neoflash_script::QuickFix` to `source` with Unicode char-boundary safety,
/// returning `Some(new_source)` only when the span is valid and changes `source`.
pub(crate) fn apply_quick_fix(source: &str, fix: &neoflash_script::QuickFix) -> Option<String> {
    let (start, end) = fix.span;
    if start > end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    let out = fix.try_apply(source)?;
    (out != source).then_some(out)
}

/// Apply a `neoflash_script::QuickFix` to `text`, returning the new text and updated caret position.
pub(crate) fn apply_quick_fix_edit(text: &str, fix: &neoflash_script::QuickFix) -> Option<Edit> {
    let out = apply_quick_fix(text, fix)?;
    let caret = text[..fix.span.0].chars().count() + fix.replacement.chars().count();
    Some((out, (caret, caret)))
}
