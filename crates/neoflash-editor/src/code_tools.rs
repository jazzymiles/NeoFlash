//! Text commands of the Actions toolbar: comments and auto-format.
//!
//! Pure functions over `(source, selection)` so they are unit-tested without egui. Selections are
//! char ranges (`start <= end`), as in egui's `CCursorRange`. Every command returns the new text and
//! selection; a command that changes nothing returns `None` so no history entry is created.
//!
//! Auto-format is deliberately conservative: it only rewrites leading indentation (brace, paren and
//! `case` depth, 4 spaces) and trailing spaces. It never moves tokens, so it cannot change program
//! meaning. Lines inside multi-line comments/strings and XML literal lines keep their text.
use neoflash_ui::components::code::{Token, tokens};

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
            let line_end = |s: &str| {
                let start: usize = s
                    .split('\n')
                    .take(line)
                    .map(|l| l.chars().count() + 1)
                    .sum();
                start + s.split('\n').nth(line).map_or(0, |l| l.chars().count())
            };
            let from_end = line_end(text).saturating_sub(selection.1);
            let caret = line_end(&formatted).saturating_sub(from_end);
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

fn find_package_open_brace(source: &str) -> Option<usize> {
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

fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(i, _)| i)
}

fn visual_indent(line: &str, step: usize) -> usize {
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
fn line_span(text: &str, (start, end): (usize, usize)) -> (usize, usize) {
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

fn replace_lines(text: &str, span: (usize, usize), lines: &[String]) -> Option<Edit> {
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

/// "Auto-format": re-indent the whole script with 4 spaces. Returns `None` when it is already formatted.
#[cfg(test)]
pub(crate) fn auto_format(text: &str) -> Option<String> {
    auto_format_with_tab_size(text, 4)
}

/// "Auto-format": re-indent the whole script using `tab_size` spaces per level (`1..=8`).
/// Returns `None` when it is already formatted.
pub(crate) fn auto_format_with_tab_size(text: &str, tab_size: u8) -> Option<String> {
    let indent = " ".repeat(usize::from(tab_size.clamp(1, 8)));
    // Classify each line start: inside a multi-line comment/string (keep verbatim) and the
    // structural depth contributed by braces/parens outside strings, comments and XML.
    let toks = tokens(text);
    let mut line_starts = vec![0];
    line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let verbatim = |offset: usize| {
        toks.iter().any(|(range, token)| {
            matches!(token, Token::Comment | Token::String)
                && range.start < offset
                && offset < range.end
        })
    };
    let first_token = |offset: usize, end: usize| {
        toks.iter()
            .find(|(range, token)| {
                range.end > offset
                    && range.start < end
                    && !(*token == Token::Plain && text[range.clone()].trim().is_empty())
            })
            .map(|(_, token)| *token)
    };
    // Depth events: (byte offset, delta) for `{ } ( ) [ ]` in punctuation tokens.
    let mut events: Vec<(usize, i32)> = Vec::new();
    for (range, token) in &toks {
        if *token != Token::Punct {
            continue;
        }
        for (i, c) in text[range.clone()].char_indices() {
            let delta = match c {
                '{' | '(' | '[' => 1,
                '}' | ')' | ']' => -1,
                _ => continue,
            };
            events.push((range.start + i, delta));
        }
    }
    let mut out_lines = Vec::new();
    let mut depth: i32 = 0;
    let mut event = 0;
    // Brace depths at which a `case`/`default` label is open (its body is one level deeper).
    let mut case_depths: Vec<i32> = Vec::new();
    for (n, &start) in line_starts.iter().enumerate() {
        let end = line_starts.get(n + 1).map_or(text.len(), |next| next - 1);
        let line = &text[start..end];
        let body = line.trim();
        // Depth changes on this line.
        let mut line_events = Vec::new();
        while event < events.len() && events[event].0 < end.max(start) {
            line_events.push(events[event].1);
            event += 1;
        }
        let keep =
            verbatim(start) || matches!(first_token(start, end), Some(Token::Tag | Token::Attr));
        let leading_close = body
            .chars()
            .take_while(|c| matches!(c, '}' | ')' | ']'))
            .count() as i32;
        while case_depths
            .last()
            .is_some_and(|&d| d > depth - leading_close || (d == depth && is_case(body)))
        {
            case_depths.pop();
        }
        let level = (depth - leading_close).max(0) + case_depths.len() as i32;
        if keep {
            out_lines.push(line.trim_end_matches([' ', '\t']).to_owned());
        } else if body.is_empty() {
            out_lines.push(String::new());
        } else {
            out_lines.push(format!("{}{}", indent.repeat(level as usize), body));
        }
        if !keep && is_case(body) {
            case_depths.push(depth);
        }
        depth = (depth + line_events.iter().sum::<i32>()).max(0);
    }
    let formatted = out_lines.join("\n");
    (formatted != text).then_some(formatted)
}

fn is_case(body: &str) -> bool {
    (body.starts_with("case ") || body.starts_with("default")) && body.ends_with(':')
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

fn is_escaped_at(text: &str, byte_pos: usize) -> bool {
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

/// Apply a `neoflash_script::QuickFix` to `text`, returning the new text and updated caret position.
pub(crate) fn apply_quick_fix_edit(text: &str, fix: &neoflash_script::QuickFix) -> Option<Edit> {
    let out = fix.try_apply(text)?;
    if out == text {
        return None;
    }
    let caret = text[..fix.span.0].chars().count() + fix.replacement.chars().count();
    Some((out, (caret, caret)))
}

/// Category of a resolved code completion item.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompletionKind {
    LocalVariable,
    Parameter,
    Field,
    Method,
    Class,
    Interface,
    Function,
    ModuleExport,
    Builtin,
}

/// A resolved code hint / completion candidate from local scope, current class, imported project
/// classes/modules, or language built-ins.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompletionItem {
    pub label: String,
    pub insert: String,
    pub caret: usize,
    pub signature: String,
    pub kind: CompletionKind,
    pub import: Option<String>,
}

/// Infer local variable and parameter types (`var name: Type`, `let name: Type`,
/// `const name = new ClassName()`, literals, AS1/AS2 suffixes, comment hints, and `this`/`super`).
#[allow(dead_code)]
pub(crate) fn infer_local_variable_types(
    source: &str,
    language: neoflash_core::ScriptLanguage,
) -> std::collections::BTreeMap<String, String> {
    let mut types = std::collections::BTreeMap::new();

    // 1. Check AS1/AS2 comment type hints (`// MovieClip hero;` or `// TextField label;`)
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(comment) = trimmed.strip_prefix("//") {
            let parts: Vec<&str> = comment
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .collect();
            if parts.len() == 2 && is_ident(parts[0]) && is_ident(parts[1]) {
                types.insert(parts[1].to_string(), short_type_name(parts[0]).to_string());
            }
        }
    }

    // 2. Detect enclosing `class ClassName [extends SuperName]` for `this` and `super`
    let tokens = neoflash_script::warnings::scan_word_tokens(source);
    for (idx, (word, _, _)) in tokens.iter().enumerate() {
        if word == "class"
            && let Some((cls_name, _, _)) = tokens.get(idx + 1)
            && is_ident(cls_name)
        {
            types
                .entry("this".to_string())
                .or_insert_with(|| cls_name.clone());
            if let Some((ext_kw, _, _)) = tokens.get(idx + 2)
                && ext_kw == "extends"
                && let Some((base_name, _, _)) = tokens.get(idx + 3)
            {
                types
                    .entry("super".to_string())
                    .or_insert_with(|| base_name.clone());
            }
        }
    }

    // 3. Scan variable declarations (`var`, `let`, `const`) and function parameters
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Function parameter annotations: `function foo(a: Type, b: OtherType)`
        if let Some(fn_idx) = trimmed.find("function") {
            let after_fn = &trimmed[fn_idx + "function".len()..];
            if let Some(open_p) = after_fn.find('(')
                && let Some(close_p) = after_fn[open_p + 1..].find(')')
            {
                let params_str = &after_fn[open_p + 1..open_p + 1 + close_p];
                for raw_param in params_str.split(',') {
                    let p = raw_param.trim().trim_start_matches("...");
                    if let Some((p_name, p_rest)) = p.split_once(':') {
                        let name = p_name.trim().trim_end_matches('?');
                        let ty_raw = p_rest.split('=').next().unwrap_or(p_rest).trim();
                        if is_ident(name) && !ty_raw.is_empty() {
                            types.insert(name.to_string(), normalize_type_label(ty_raw));
                        }
                    }
                }
            }
        }

        // Variable declarations (`var`, `let`, `const`, including `public var`, `private var`, `export const`)
        for kw in ["var ", "let ", "const "] {
            let mut search_from = 0usize;
            while let Some(rel_idx) = trimmed[search_from..].find(kw) {
                let abs_idx = search_from + rel_idx;
                let valid_boundary = abs_idx == 0
                    || !trimmed[..abs_idx]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_');
                search_from = abs_idx + kw.len();
                if !valid_boundary {
                    continue;
                }
                let rest = trimmed[search_from..].trim_start();
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                let after_name = rest[name.len()..].trim_start();
                let after_name = after_name
                    .strip_prefix('?')
                    .unwrap_or(after_name)
                    .trim_start();

                if let Some(after_colon) = after_name.strip_prefix(':') {
                    let ty_part = after_colon
                        .split(['=', ';', ','])
                        .next()
                        .unwrap_or("")
                        .trim();
                    if !ty_part.is_empty() {
                        types.insert(name.clone(), normalize_type_label(ty_part));
                        continue;
                    }
                }

                if let Some(after_eq) = after_name.strip_prefix('=') {
                    let rhs = after_eq.split(';').next().unwrap_or(after_eq).trim();
                    if let Some(inferred) = infer_rhs_type(rhs, language) {
                        types.insert(name.clone(), inferred);
                        continue;
                    }
                }

                if let Some(suffix_ty) = as2_suffix_type(&name) {
                    types.insert(name, suffix_ty.to_string());
                }
            }
        }
    }

    // 4. Also check any identifier in the script for AS1/AS2 suffix conventions (`hero_mc`, `title_txt`, etc.)
    for (word, _, _) in &tokens {
        if !types.contains_key(word)
            && let Some(suffix_ty) = as2_suffix_type(word)
        {
            types.insert(word.clone(), suffix_ty.to_string());
        }
    }

    types
}

fn infer_rhs_type(rhs: &str, language: neoflash_core::ScriptLanguage) -> Option<String> {
    let trimmed = rhs.trim();
    if let Some(after_new) = trimmed.strip_prefix("new ") {
        let ctor: String = after_new
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
            .collect();
        if !ctor.is_empty() {
            return Some(short_type_name(&ctor).to_string());
        }
    }
    if trimmed.starts_with('"') || trimmed.starts_with('\'') || trimmed.starts_with('`') {
        return Some(
            if language == neoflash_core::ScriptLanguage::TypeScript {
                "string"
            } else {
                "String"
            }
            .to_string(),
        );
    }
    if trimmed.starts_with("true") || trimmed.starts_with("false") {
        return Some(
            if language == neoflash_core::ScriptLanguage::TypeScript {
                "boolean"
            } else {
                "Boolean"
            }
            .to_string(),
        );
    }
    if trimmed.starts_with('[') {
        return Some("Array".to_string());
    }
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || (c == '-' && trimmed.len() > 1))
    {
        return Some(
            if language == neoflash_core::ScriptLanguage::TypeScript {
                "number"
            } else {
                "Number"
            }
            .to_string(),
        );
    }
    None
}

fn as2_suffix_type(ident: &str) -> Option<&'static str> {
    const SUFFIXES: &[(&str, &str)] = &[
        ("_mc", "MovieClip"),
        ("_btn", "Button"),
        ("_txt", "TextField"),
        ("_fmt", "TextFormat"),
        ("_sound", "Sound"),
        ("_color", "Color"),
        ("_array", "Array"),
        ("_str", "String"),
        ("_date", "Date"),
        ("_xml", "XML"),
        ("_xmlsocket", "XMLSocket"),
        ("_lv", "LoadVars"),
        ("_lc", "LocalConnection"),
        ("_nc", "NetConnection"),
        ("_ns", "NetStream"),
        ("_so", "SharedObject"),
        ("_cam", "Camera"),
        ("_mic", "Microphone"),
        ("_video", "Video"),
        ("_pj", "PrintJob"),
        ("_cm", "ContextMenu"),
        ("_cmi", "ContextMenuItem"),
        ("_style", "StyleSheet"),
    ];
    SUFFIXES
        .iter()
        .find_map(|&(sfx, ty)| ident.ends_with(sfx).then_some(ty))
}

fn normalize_type_label(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.ends_with("[]") {
        return "Array".to_string();
    }
    if let Some(before_generic) = trimmed.split('<').next() {
        return short_type_name(before_generic.trim()).to_string();
    }
    short_type_name(trimmed).to_string()
}

fn short_type_name(qualified: &str) -> &str {
    qualified.rsplit('.').next().unwrap_or(qualified).trim()
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Metadata extracted from a class, interface, or module in the current file or a project `SourceUnit`.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub(crate) struct ExtractedTypeInfo {
    pub name: String,
    pub qualified_name: String,
    pub super_class: Option<String>,
    pub is_interface: bool,
    pub members: Vec<CompletionItem>,
}

/// Extract classes, interfaces, and module exports (fields, methods, getters/setters, exported functions/consts)
/// from a script or project `SourceUnit`.
#[allow(dead_code)]
pub(crate) fn extract_types_and_members(
    source: &str,
    path: Option<&str>,
) -> std::collections::BTreeMap<String, ExtractedTypeInfo> {
    let mut map: std::collections::BTreeMap<String, ExtractedTypeInfo> =
        std::collections::BTreeMap::new();

    // Detect package name if AS3 `package foo.bar {`
    let mut package_prefix = String::new();
    for line in source.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("package ") {
            let pkg: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !pkg.is_empty() {
                package_prefix = pkg;
            }
            break;
        }
    }

    let mut current_type: Option<ExtractedTypeInfo> = None;
    let mut module_exports: Vec<CompletionItem> = Vec::new();

    for line in source.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }

        // Check for `class Name` or `interface Name`
        if let Some((kind_is_iface, decl_after)) = find_class_or_interface_header(t) {
            if let Some(prev) = current_type.take() {
                map.insert(prev.name.clone(), prev);
            }
            let raw_name: String = decl_after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !raw_name.is_empty() {
                let short = short_type_name(&raw_name).to_string();
                let qualified = if raw_name.contains('.') {
                    raw_name.clone()
                } else if !package_prefix.is_empty() {
                    format!("{package_prefix}.{short}")
                } else {
                    short.clone()
                };
                let after_name = decl_after[raw_name.len()..].trim_start();
                let super_class = after_name
                    .strip_prefix("extends ")
                    .map(|ext| {
                        ext.trim_start()
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                            .collect::<String>()
                    })
                    .filter(|s| !s.is_empty())
                    .map(|s| short_type_name(&s).to_string());

                current_type = Some(ExtractedTypeInfo {
                    name: short,
                    qualified_name: qualified,
                    super_class,
                    is_interface: kind_is_iface,
                    members: Vec::new(),
                });
                continue;
            }
        }

        // Inside a class/interface, or at module top-level:
        if let Some(member) = parse_member_line(t, current_type.as_ref().map(|c| c.name.as_str())) {
            if let Some(cur) = &mut current_type {
                if !cur.members.iter().any(|m| m.label == member.label) {
                    cur.members.push(member);
                }
            } else if t.starts_with("export ") {
                let mut exp_item = member;
                exp_item.kind = CompletionKind::ModuleExport;
                module_exports.push(exp_item);
            }
        }
    }

    if let Some(prev) = current_type.take() {
        map.insert(prev.name.clone(), prev);
    }

    if !module_exports.is_empty()
        && let Some(p) = path
    {
        let mod_stem = std::path::Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(p)
            .to_string();
        map.entry(mod_stem.clone())
            .or_insert_with(|| ExtractedTypeInfo {
                name: mod_stem,
                qualified_name: p.to_string(),
                super_class: None,
                is_interface: false,
                members: module_exports,
            });
    }

    map
}

fn find_class_or_interface_header(trimmed: &str) -> Option<(bool, &str)> {
    let mut s = trimmed;
    for kw in [
        "export ",
        "default ",
        "public ",
        "internal ",
        "dynamic ",
        "final ",
        "abstract ",
    ] {
        while let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim_start();
        }
    }
    if let Some(rest) = s.strip_prefix("class ") {
        return Some((false, rest.trim_start()));
    }
    if let Some(rest) = s.strip_prefix("interface ") {
        return Some((true, rest.trim_start()));
    }
    None
}

fn parse_member_line(trimmed: &str, enclosing_class: Option<&str>) -> Option<CompletionItem> {
    let mut s = trimmed;
    let is_exported = s.starts_with("export ");
    for kw in [
        "export ",
        "public ",
        "private ",
        "protected ",
        "internal ",
        "static ",
        "override ",
        "final ",
        "readonly ",
        "async ",
    ] {
        while let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim_start();
        }
    }

    // Getter / setter: `get foo()` or `function get foo()`
    if let Some(rest) = s
        .strip_prefix("function get ")
        .or_else(|| s.strip_prefix("function set "))
        .or_else(|| s.strip_prefix("get "))
        .or_else(|| s.strip_prefix("set "))
    {
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if is_ident(&name) && rest[name.len()..].trim_start().starts_with('(') {
            return Some(CompletionItem {
                caret: name.chars().count(),
                label: name.clone(),
                insert: name.clone(),
                signature: format!("{}.{name}", enclosing_class.unwrap_or("property")),
                kind: CompletionKind::Field,
                import: None,
            });
        }
    }

    // Function or AS method: `function move(dx: Number): Void`
    if let Some(rest) = s.strip_prefix("function ") {
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if is_ident(&name) {
            if enclosing_class == Some(name.as_str()) || name == "constructor" {
                return None;
            }
            let after_name = rest[name.len()..].trim_start();
            let sig_params = after_name
                .find(')')
                .map(|idx| &after_name[..=idx])
                .unwrap_or("()");
            let has_args = sig_params != "()";
            let insert = format!("{name}()");
            let caret = if has_args {
                name.chars().count() + 1
            } else {
                insert.chars().count()
            };
            return Some(CompletionItem {
                label: name.clone(),
                insert,
                caret,
                signature: format!("{name}{sig_params}"),
                kind: if enclosing_class.is_some() {
                    CompletionKind::Method
                } else {
                    CompletionKind::Function
                },
                import: None,
            });
        }
    }

    // Variable / field declaration: `var speed: Number`, `let count = 0`, `const MAX = 10`
    for kw in ["var ", "let ", "const "] {
        if let Some(rest) = s.strip_prefix(kw) {
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if is_ident(&name) {
                let after = rest[name.len()..].trim_start();
                let ty_str = after
                    .strip_prefix(':')
                    .and_then(|r| r.split(['=', ';']).next())
                    .map(str::trim)
                    .unwrap_or("any");
                return Some(CompletionItem {
                    caret: name.chars().count(),
                    label: name.clone(),
                    insert: name.clone(),
                    signature: format!("{name}: {ty_str}"),
                    kind: if enclosing_class.is_some() {
                        CompletionKind::Field
                    } else if is_exported {
                        CompletionKind::ModuleExport
                    } else {
                        CompletionKind::LocalVariable
                    },
                    import: None,
                });
            }
        }
    }

    // TypeScript / ES6 class method or property without `function`/`var` keyword inside a class/interface
    if enclosing_class.is_some() {
        let name: String = s
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if is_ident(&name)
            && !matches!(
                name.as_str(),
                "if" | "for"
                    | "while"
                    | "switch"
                    | "return"
                    | "throw"
                    | "try"
                    | "catch"
                    | "new"
                    | "constructor"
                    | "super"
            )
        {
            let after = s[name.len()..].trim_start();
            let after = after.strip_prefix('?').unwrap_or(after).trim_start();
            if after.starts_with('(') {
                let sig_params = after.find(')').map(|idx| &after[..=idx]).unwrap_or("()");
                let has_args = sig_params != "()";
                let insert = format!("{name}()");
                let caret = if has_args {
                    name.chars().count() + 1
                } else {
                    insert.chars().count()
                };
                return Some(CompletionItem {
                    label: name.clone(),
                    insert,
                    caret,
                    signature: format!("{name}{sig_params}"),
                    kind: CompletionKind::Method,
                    import: None,
                });
            } else if let Some(after_colon) = after.strip_prefix(':') {
                let ty_str = after_colon
                    .split(['=', ';'])
                    .next()
                    .map(str::trim)
                    .unwrap_or("any");
                return Some(CompletionItem {
                    caret: name.chars().count(),
                    label: name.clone(),
                    insert: name.clone(),
                    signature: format!("{name}: {ty_str}"),
                    kind: CompletionKind::Field,
                    import: None,
                });
            }
        }
    }

    None
}

/// Resolve code completion candidates at `caret` (char index) in `source`, combining:
/// - Local variable and parameter declarations (`var name: Type`, `let name: Type`, `const name = new ClassName()`)
/// - Class fields and methods of the current file and imported/project `SourceUnit` classes (including inheritance)
/// - Exported members of imported JS/TS modules (`import * as ns from "./mod"`, `import { fn } from "./mod"`)
/// - AS1/AS2 variable suffixes (`_mc`, `_txt`, `_btn`, `_sound`) and `// ClassName varName;` comment hints
/// - Built-in language APIs from `script_api`.
#[allow(dead_code)]
pub(crate) fn complete_at_caret(
    language: neoflash_core::ScriptLanguage,
    source: &str,
    caret: usize,
    project_sources: &[neoflash_core::SourceUnit],
) -> Vec<CompletionItem> {
    let byte_pos = char_to_byte(source, caret.min(source.chars().count()));
    let before = &source[..byte_pos];

    // Collect all class/interface/module definitions from the current file and project_sources
    let mut type_catalog = extract_types_and_members(source, None);
    for unit in project_sources {
        for (k, v) in extract_types_and_members(&unit.source, Some(&unit.path)) {
            type_catalog.entry(k).or_insert(v);
        }
    }

    let mut local_types = infer_local_variable_types(source, language);

    // Also track JS/TS namespace imports (`import * as math from "./math"`)
    for line in source.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("import * as ")
            && let Some((ns_name, from_part)) = rest.split_once(" from ")
        {
            let ns = ns_name.trim();
            let spec = from_part
                .trim()
                .trim_end_matches(';')
                .trim_matches(['"', '\'']);
            let mod_stem = std::path::Path::new(spec)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(spec);
            if is_ident(ns) {
                local_types.insert(ns.to_string(), mod_stem.to_string());
            }
        }
    }

    // Check if we are completing a member access `receiver.prefix`
    let ident_len: usize = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .map(char::len_utf8)
        .sum();
    let prefix = &before[before.len() - ident_len..];
    let before_prefix = &before[..before.len() - ident_len];

    if let Some(before_dot) = before_prefix.strip_suffix('.') {
        let receiver: String = before_dot
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        if !receiver.is_empty() {
            let mut results: Vec<CompletionItem> = Vec::new();
            let resolved_type = local_types.get(&receiver).cloned().or_else(|| {
                type_catalog
                    .contains_key(&receiver)
                    .then(|| receiver.clone())
            });

            if let Some(ty_name) = resolved_type {
                // Walk class hierarchy in `type_catalog`
                let mut cur_ty = Some(ty_name.clone());
                let mut visited = std::collections::BTreeSet::new();
                while let Some(ct) = cur_ty {
                    if !visited.insert(ct.clone()) {
                        break;
                    }
                    if let Some(info) = type_catalog.get(&ct) {
                        for m in &info.members {
                            if m.label
                                .to_ascii_lowercase()
                                .starts_with(&prefix.to_ascii_lowercase())
                                && !results.iter().any(|r| r.label == m.label)
                            {
                                results.push(m.clone());
                            }
                        }
                        cur_ty = info.super_class.clone();
                    } else {
                        break;
                    }
                }

                // Also include built-in members if `ty_name` is a known built-in class (e.g. MovieClip, TextField, Array, String)
                if let Some(api) = crate::script_api::api(language) {
                    let lower_prefix = prefix.to_ascii_lowercase();
                    for m in &api.members {
                        let owner_short = crate::script_api::short_name(&m.owner);
                        if (owner_short == ty_name || m.owner == ty_name)
                            && m.name.to_ascii_lowercase().starts_with(&lower_prefix)
                        {
                            let label = m.label();
                            if !results.iter().any(|r| r.label == label) {
                                let (insert, caret) = m.insertion();
                                results.push(CompletionItem {
                                    label,
                                    insert,
                                    caret,
                                    signature: format!("{}.{}", owner_short, m.name),
                                    kind: CompletionKind::Builtin,
                                    import: None,
                                });
                            }
                        }
                    }
                }
            }
            return results;
        }
    }

    // Unqualified completion at caret: local variables + current class members + project classes + language built-ins
    let mut items: Vec<CompletionItem> = Vec::new();
    let lower_prefix = prefix.to_ascii_lowercase();

    for (var_name, var_ty) in &local_types {
        if var_name == "this" || var_name == "super" {
            continue;
        }
        if var_name.to_ascii_lowercase().starts_with(&lower_prefix)
            && !items.iter().any(|i| i.label == *var_name)
        {
            items.push(CompletionItem {
                caret: var_name.chars().count(),
                label: var_name.clone(),
                insert: var_name.clone(),
                signature: format!("{var_name}: {var_ty}"),
                kind: CompletionKind::LocalVariable,
                import: None,
            });
        }
    }

    // Current class members (accessible directly in AS2/AS3)
    if let Some(this_ty) = local_types.get("this")
        && let Some(info) = type_catalog.get(this_ty)
    {
        for m in &info.members {
            if m.label.to_ascii_lowercase().starts_with(&lower_prefix)
                && !items.iter().any(|i| i.label == m.label)
            {
                items.push(m.clone());
            }
        }
    }

    // Project classes & interfaces
    for (cls_name, info) in &type_catalog {
        if cls_name.to_ascii_lowercase().starts_with(&lower_prefix)
            && !items.iter().any(|i| i.label == *cls_name)
        {
            let needs_import = language == neoflash_core::ScriptLanguage::ActionScript3
                && info.qualified_name.contains('.')
                && crate::script_api::missing_import(source, &info.qualified_name).is_some();
            items.push(CompletionItem {
                caret: cls_name.chars().count(),
                label: cls_name.clone(),
                insert: cls_name.clone(),
                signature: info.qualified_name.clone(),
                kind: if info.is_interface {
                    CompletionKind::Interface
                } else {
                    CompletionKind::Class
                },
                import: needs_import.then(|| info.qualified_name.clone()),
            });
        }
    }

    // Language built-ins
    if let Some(api) = crate::script_api::api(language) {
        for m in &api.members {
            if m.owner.is_empty() && m.name.to_ascii_lowercase().starts_with(&lower_prefix) {
                let label = m.label();
                if !items.iter().any(|i| i.label == label) {
                    let (insert, caret) = m.insertion();
                    items.push(CompletionItem {
                        label,
                        insert,
                        caret,
                        signature: m.ty.clone(),
                        kind: CompletionKind::Builtin,
                        import: None,
                    });
                }
            }
        }
    }

    items
}

/// Collect compiler diagnostics and lint warnings along with their actionable `QuickFix` items for `source`.
#[allow(dead_code)]
pub(crate) fn diagnostics_and_quick_fixes(
    language: neoflash_core::ScriptLanguage,
    source: &str,
) -> Vec<(String, Vec<neoflash_script::QuickFix>)> {
    let script = neoflash_core::Script {
        language,
        source: source.to_string(),
    };
    let mut out = Vec::new();
    if let Err(diags) = neoflash_script::compile(&script) {
        for d in diags {
            let fixes = d.quick_fixes(source);
            out.push((d.message, fixes));
        }
    }
    for w in neoflash_script::check_warnings(&script) {
        out.push((w.message, w.quick_fixes));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_adds_missing_as3_import_once() {
        let command = CodeCommand::Insert {
            text: "Point".into(),
            caret: 5,
            import: Some("flash.geom.Point".into()),
        };
        let (out, sel) = apply(&command, "var p:", (6, 6)).unwrap();
        assert_eq!(out, "import flash.geom.Point;\nvar p:Point");
        assert_eq!(sel, (36, 36));
        let (again, _) = apply(&command, &out, (36, 36)).unwrap();
        assert_eq!(again.matches("import").count(), 1);
    }

    #[test]
    fn line_comments_round_trip() {
        let text = "a();\n  b();\n  c();\nd();";
        let (commented, sel) = line_comment(text, (6, 14)).unwrap();
        assert_eq!(commented, "a();\n  // b();\n  // c();\nd();");
        let (plain, _) = uncomment(&commented, sel).unwrap();
        assert_eq!(plain, text);
        assert!(uncomment(text, (0, 0)).is_none());
    }

    #[test]
    fn block_comment_wraps_selection_or_caret_line() {
        let (out, sel) = block_comment("x = 1;\ny = 2;", (0, 6)).unwrap();
        assert_eq!(out, "/* x = 1; */\ny = 2;");
        assert_eq!(sel, (0, 12));
        let (back, _) = uncomment(&out, sel).unwrap();
        assert_eq!(back, "x = 1;\ny = 2;");
        let (line, _) = block_comment("  stop();", (3, 3)).unwrap();
        assert_eq!(line, "  /* stop(); */");
    }

    #[test]
    fn auto_format_reindents_without_moving_tokens() {
        let source = "function f(n:int):void {\nif (n > 0) {\ntrace(\"{ not a brace\");\n}   \nswitch (n) {\ncase 1:\nstop();\nbreak;\ndefault:\nplay();\n}\n}";
        let formatted = auto_format(source).unwrap();
        assert_eq!(
            formatted,
            "function f(n:int):void {\n    if (n > 0) {\n        trace(\"{ not a brace\");\n    }\n    switch (n) {\n        case 1:\n            stop();\n            break;\n        default:\n            play();\n    }\n}"
        );
        assert!(auto_format(&formatted).is_none(), "idempotent");
        let strip = |s: &str| s.split_whitespace().collect::<String>();
        assert_eq!(strip(&formatted), strip(source));
    }

    #[test]
    fn auto_format_keeps_comments_and_xml_lines() {
        let source =
            "/* keep\n      this */\nvar x:XML = <a>\n      <b/>\n</a>;\nif (x) {\nstop();\n}";
        let formatted = auto_format(source).unwrap();
        assert!(formatted.contains("      this */"));
        assert!(formatted.contains("\n      <b/>\n"));
        assert!(formatted.ends_with("if (x) {\n    stop();\n}"));
    }

    #[test]
    fn tab_size_controls_auto_format_line_comment_and_tab_indent_outdent() {
        let src = "function f() {\nif (ok) {\nstop();\n}\n}";
        assert_eq!(
            auto_format_with_tab_size(src, 2).unwrap(),
            "function f() {\n  if (ok) {\n    stop();\n  }\n}"
        );
        assert_eq!(
            auto_format_with_tab_size(src, 8).unwrap(),
            "function f() {\n        if (ok) {\n                stop();\n        }\n}"
        );

        // Single caret Tab / Shift+Tab with 2 and 4 spaces.
        let (indented, sel) = indent_tab("stop();", (0, 0), 2);
        assert_eq!(indented, "  stop();");
        assert_eq!(sel, (2, 2));
        let (outdented, sel) = outdent_tab(&indented, (9, 9), 2).unwrap();
        assert_eq!(outdented, "stop();");
        assert_eq!(sel, (7, 7));
        assert!(outdent_tab("stop();", (0, 0), 4).is_none());

        // Multi-line block indent & outdent preserves empty lines and selection span.
        let block = "a();\n\nb();";
        let (ind2, sel2) = indent_tab(block, (0, block.chars().count()), 4);
        assert_eq!(ind2, "    a();\n\n    b();");
        let (back, _) = outdent_tab(&ind2, sel2, 4).unwrap();
        assert_eq!(back, block);

        // Mixed leading tab and spaces in line comment uses visual tab_size columns.
        let mixed = "\ta();\n    b();";
        let (commented, _) =
            line_comment_with_tab_size(mixed, (0, mixed.chars().count()), 4).unwrap();
        assert_eq!(commented, "\t// a();\n    // b();");
    }

    #[test]
    fn auto_close_and_backspace_pair_handle_brackets_quotes_escapes_and_skip_over() {
        // Opening brackets pair when not before identifier char, then closing bracket skips over.
        let (s, sel) = auto_close_char("trace", 5, '(').unwrap();
        assert_eq!(s, "trace()");
        assert_eq!(sel, (6, 6));
        let (s, sel) = auto_close_char(&s, sel.0, ')').unwrap();
        assert_eq!(s, "trace()");
        assert_eq!(sel, (7, 7));

        // Do not auto-close directly before an identifier character.
        assert!(auto_close_char("foo", 0, '(').is_none());
        assert!(auto_close_char("foo", 0, '"').is_none());

        // Quotes pair, skip over closing quote, ignore escaped quotes, and close unclosed strings.
        let (q, qsel) = auto_close_char("x = ", 4, '"').unwrap();
        assert_eq!(q, "x = \"\"");
        assert_eq!(qsel, (5, 5));
        let (q2, qsel2) = auto_close_char(&q, qsel.0, '"').unwrap();
        assert_eq!(q2, "x = \"\"");
        assert_eq!(qsel2, (6, 6));
        assert!(auto_close_char("x = \"hello", 10, '"').is_none());
        assert!(auto_close_char("x = \"hello\\", 11, '"').is_none());

        // Backspace between an empty pair deletes both delimiters; non-empty or escaped pairs do not.
        for (pair, caret) in [
            ("()", 1),
            ("[]", 1),
            ("{}", 1),
            ("\"\"", 1),
            ("''", 1),
            ("``", 1),
        ] {
            let (deleted, dsel) = auto_delete_pair(pair, caret).unwrap();
            assert_eq!(deleted, "");
            assert_eq!(dsel, (0, 0));
        }
        assert!(auto_delete_pair("(a)", 1).is_none());
        assert!(auto_delete_pair("\\\"\"", 2).is_none());
    }

    #[test]
    fn infer_local_variable_types_resolves_annotations_constructors_suffixes_and_comment_hints() {
        let ts_src = r#"
// MovieClip legacyClip;
class Player extends Actor {
    speed: number = 10;
    move(dx: number, dy: string): void {
        let count: number = 0;
        const hero = new HeroSprite();
        const items: string[] = [];
        var label_txt;
    }
}
"#;
        let types = infer_local_variable_types(ts_src, neoflash_core::ScriptLanguage::TypeScript);
        assert_eq!(
            types.get("legacyClip").map(String::as_str),
            Some("MovieClip")
        );
        assert_eq!(types.get("this").map(String::as_str), Some("Player"));
        assert_eq!(types.get("super").map(String::as_str), Some("Actor"));
        assert_eq!(types.get("count").map(String::as_str), Some("number"));
        assert_eq!(types.get("hero").map(String::as_str), Some("HeroSprite"));
        assert_eq!(types.get("items").map(String::as_str), Some("Array"));
        assert_eq!(
            types.get("label_txt").map(String::as_str),
            Some("TextField")
        );
    }

    #[test]
    fn complete_at_caret_resolves_current_and_imported_project_class_members() {
        let project_sources = vec![
            neoflash_core::SourceUnit {
                id: "actor".into(),
                path: "game/Actor.as".into(),
                language: neoflash_core::ScriptLanguage::ActionScript3,
                source: r#"package game {
    public class Actor {
        public var health: Number = 100;
        public function takeDamage(amount: Number): void {}
    }
}"#
                .into(),
            },
            neoflash_core::SourceUnit {
                id: "hero".into(),
                path: "game/Hero.ts".into(),
                language: neoflash_core::ScriptLanguage::TypeScript,
                source: r#"export class Hero extends Actor {
    stamina: number = 50;
    jump(force: number): void {}
}"#
                .into(),
            },
            neoflash_core::SourceUnit {
                id: "math".into(),
                path: "utils/math.ts".into(),
                language: neoflash_core::ScriptLanguage::TypeScript,
                source: r#"export const GRAVITY: number = 9.8;
export function clamp(val: number, min: number, max: number): number { return val; }"#
                    .into(),
            },
        ];

        // Member completion on `player = new Hero()` resolves both `Hero` own members and inherited `Actor` members!
        let handler_src =
            "import * as math from \"./utils/math\";\nconst player = new Hero();\nplayer.";
        let completions = complete_at_caret(
            neoflash_core::ScriptLanguage::TypeScript,
            handler_src,
            handler_src.chars().count(),
            &project_sources,
        );
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"stamina"),
            "expected Hero.stamina in {labels:?}"
        );
        assert!(labels.contains(&"jump"), "expected Hero.jump in {labels:?}");
        assert!(
            labels.contains(&"health"),
            "expected inherited Actor.health in {labels:?}"
        );
        assert!(
            labels.contains(&"takeDamage"),
            "expected inherited Actor.takeDamage in {labels:?}"
        );

        // Namespace module completion (`math.`) resolves exported members of `utils/math.ts`
        let mod_src = "import * as math from \"./utils/math\";\nmath.";
        let mod_completions = complete_at_caret(
            neoflash_core::ScriptLanguage::TypeScript,
            mod_src,
            mod_src.chars().count(),
            &project_sources,
        );
        let mod_labels: Vec<&str> = mod_completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            mod_labels.contains(&"GRAVITY"),
            "expected GRAVITY in {mod_labels:?}"
        );
        assert!(
            mod_labels.contains(&"clamp"),
            "expected clamp in {mod_labels:?}"
        );

        // Unqualified completion in AS3 offers unimported project class `Actor` with `import: Some("game.Actor")`
        let as3_src = "var a: Act";
        let as3_completions = complete_at_caret(
            neoflash_core::ScriptLanguage::ActionScript3,
            as3_src,
            as3_src.chars().count(),
            &project_sources,
        );
        let actor_item = as3_completions
            .iter()
            .find(|c| c.label == "Actor")
            .expect("Actor class completion");
        assert_eq!(actor_item.import.as_deref(), Some("game.Actor"));
    }

    #[test]
    fn quick_fixes_apply_through_code_command_for_warnings_and_diagnostics() {
        // 1. Misspelled identifier quick fix on TypeScript diagnostic
        let bad_ts = "let score: number = 10;\nscr = 20;";
        let fixes = diagnostics_and_quick_fixes(neoflash_core::ScriptLanguage::TypeScript, bad_ts);
        let (_, typo_fixes) = fixes
            .iter()
            .find(|(msg, f)| msg.contains("Did you mean 'score'") && !f.is_empty())
            .expect("misspelled identifier quick fix");
        let (fixed_ts, _) = apply(
            &CodeCommand::ApplyQuickFix(typo_fixes[0].clone()),
            bad_ts,
            (0, 0),
        )
        .unwrap();
        assert_eq!(fixed_ts, "let score: number = 10;\nscore = 20;");

        // 2. Unused variable & deprecated AS2 operator quick fixes
        let as2_src = "function example():Void { var unusedCount:Number = 1; }\nif (a and b) {\n    trace(a);\n}";
        let as2_fixes =
            diagnostics_and_quick_fixes(neoflash_core::ScriptLanguage::ActionScript2, as2_src);
        let (_, unused_f) = as2_fixes
            .iter()
            .find(|(msg, _)| msg.contains("Variable 'unusedCount'"))
            .expect("unused variable warning");
        let (prefixed, _) = apply(
            &CodeCommand::ApplyQuickFix(unused_f[0].clone()),
            as2_src,
            (0, 0),
        )
        .unwrap();
        assert!(prefixed.contains("var _unusedCount:Number = 1;"));

        let (_, and_f) = as2_fixes
            .iter()
            .find(|(msg, _)| msg.contains("'and'"))
            .expect("deprecated and operator warning");
        let (modern_and, _) = apply(
            &CodeCommand::ApplyQuickFix(and_f[0].clone()),
            as2_src,
            (0, 0),
        )
        .unwrap();
        assert!(modern_and.contains("if (a && b)"));
    }

    #[test]
    fn insert_places_as3_imports_inside_package_block_and_deduplicates() {
        let pkg_src = "package com.demo {\n    public class Main {\n        public function Main() {\n            \n        }\n    }\n}\n";
        let caret = pkg_src.find("            \n").unwrap() + 12;
        let (after_first, (new_pos, _)) = apply(
            &CodeCommand::Insert {
                text: "var mc:MovieClip = new MovieClip();".into(),
                caret: "var mc:MovieClip = new MovieClip();".chars().count(),
                import: Some("flash.display.MovieClip".into()),
            },
            pkg_src,
            (caret, caret),
        )
        .unwrap();
        assert!(
            after_first.starts_with(
                "package com.demo {\n    import flash.display.MovieClip;\n    public class Main {"
            ),
            "unexpected output:\n{after_first}"
        );
        assert!(!after_first.starts_with("import "));

        let (after_second, _) = apply(
            &CodeCommand::Insert {
                text: "\n            var ev:Event;".into(),
                caret: "\n            var ev:Event;".chars().count(),
                import: Some("flash.events.Event".into()),
            },
            &after_first,
            (new_pos, new_pos),
        )
        .unwrap();
        assert!(
            after_second.starts_with(
                "package com.demo {\n    import flash.display.MovieClip;\n    import flash.events.Event;\n    public class Main {"
            ),
            "unexpected output:\n{after_second}"
        );

        // Re-inserting an already-imported type does not duplicate the import line.
        let (after_dup, _) = apply(
            &CodeCommand::Insert {
                text: "".into(),
                caret: 0,
                import: Some("flash.display.MovieClip".into()),
            },
            &after_second,
            (new_pos, new_pos),
        )
        .unwrap();
        assert_eq!(
            after_dup.matches("import flash.display.MovieClip;").count(),
            1
        );
    }
}
