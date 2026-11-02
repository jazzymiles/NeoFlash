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
                .and_then(|q| crate::script_api::missing_import(text, q))
            {
                // After the last leading import, otherwise at the top.
                let at = out
                    .match_indices("import ")
                    .filter(|(i, _)| *i == 0 || out.as_bytes()[i - 1] == b'\n')
                    .last()
                    .map_or(0, |(i, _)| {
                        out[i..].find('\n').map_or(out.len(), |n| i + n + 1)
                    });
                let line = if at == out.len() && !out.ends_with('\n') && !out.is_empty() {
                    format!("\n{line}")
                } else {
                    format!("{line}\n")
                };
                if out[..at].chars().count() <= position {
                    position += line.chars().count();
                }
                out.insert_str(at, &line);
            }
            Some((out, (position, position)))
        }
    }
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
}
