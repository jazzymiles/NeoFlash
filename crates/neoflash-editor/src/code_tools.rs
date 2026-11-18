//! Text commands of the Actions toolbar: comments and auto-format.
//!
//! Pure functions over `(source, selection)` so they are unit-tested without egui. Selections are
//! char ranges (`start <= end`), as in egui's `CCursorRange`. Every command returns the new text and
//! selection; a command that changes nothing returns `None` so no history entry is created.
//!
//! Auto-format is token-safe and idempotent across AS1/AS2/AS3/JS/TS: it normalizes indentation,
//! operator/delimiter/statement spacing, and accidental statement-ending colons while keeping
//! strings, comments, regexes, `#include` directives and E4X/XML literals verbatim.

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

/// "Auto-format": re-indent and format statement/operator/delimiter spacing with 4 spaces.
/// Returns `None` when it is already formatted.
#[cfg(test)]
pub(crate) fn auto_format(text: &str) -> Option<String> {
    auto_format_with_tab_size(text, 4)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FmtKind {
    Newline,
    LineComment,
    BlockComment,
    String,
    Regex,
    Xml,
    Directive,
    Number,
    Ident,
    Punct(&'static str),
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PunctRole {
    Normal,
    KeywordSpaceBeforeParen,
    Binary,
    UnaryPrefix,
    PrefixIncDec,
    PostfixIncDec,
    PostfixNonNull,
    WildcardAfterDot,
    GeneratorStar,
    TypeAny,
    GenericOpen,
    GenericClose,
    TernaryColon,
    CaseColon,
    ObjectColon,
    LabelColon,
    TypeColon,
    TrailingColonAsSemi,
}

#[derive(Clone, Debug)]
struct FmtTok {
    range: std::ops::Range<usize>,
    kind: FmtKind,
    role: PunctRole,
}

impl FmtTok {
    fn text<'a>(&self, src: &'a str) -> &'a str {
        if self.role == PunctRole::TrailingColonAsSemi {
            ";"
        } else {
            &src[self.range.clone()]
        }
    }

    fn is_code(&self) -> bool {
        !matches!(
            self.kind,
            FmtKind::Newline | FmtKind::LineComment | FmtKind::BlockComment
        )
    }
}

fn fmt_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn fmt_ident_char(c: u8) -> bool {
    fmt_ident_start(c) || c.is_ascii_digit()
}

const EXPR_PRECEDING_KEYWORDS: &[&str] = &[
    "return",
    "throw",
    "case",
    "in",
    "of",
    "instanceof",
    "typeof",
    "void",
    "delete",
    "new",
    "do",
    "else",
    "yield",
    "await",
];

const SPACE_BEFORE_PAREN_KEYWORDS: &[&str] = &[
    "if",
    "for",
    "while",
    "switch",
    "catch",
    "with",
    "each",
    "return",
    "throw",
    "case",
    "typeof",
    "delete",
    "void",
    "in",
    "of",
    "instanceof",
    "new",
    "await",
    "yield",
    "function",
];

const FMT_TYPE_KEYWORDS: &[&str] = &[
    "as",
    "class",
    "extends",
    "implements",
    "instanceof",
    "interface",
    "is",
    "new",
    "type",
];

const MULTI_PUNCTS: &[&str] = &[
    ">>>=", "===", "!==", ">>>", "<<=", ">>=", "&&=", "||=", "??=", "...", "==", "!=", "<>", "<=",
    ">=", "&&", "||", "??", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "++", "--",
    "=>", "?.", "::", "=", "+", "-", "*", "/", "%", "<", ">", "&", "|", "^", "!", "~", "?", ":",
    ",", ";", ".", "(", ")", "[", "]", "{", "}",
];

fn can_precede_regex_or_unary(prev: Option<(&FmtKind, &str)>) -> bool {
    let Some((kind, text)) = prev else {
        return true;
    };
    match kind {
        FmtKind::Ident => EXPR_PRECEDING_KEYWORDS.contains(&text),
        FmtKind::Punct(p) => !matches!(*p, ")" | "]" | "++" | "--"),
        FmtKind::Directive => true,
        _ => false,
    }
}

fn try_scan_regex(src: &str, start: usize) -> Option<usize> {
    let b = src.as_bytes();
    let n = b.len();
    if b.get(start) != Some(&b'/') || matches!(b.get(start + 1), Some(b'/' | b'*')) {
        return None;
    }
    let mut i = start + 1;
    let mut in_class = false;
    while i < n {
        match b[i] {
            b'\\' => {
                if i + 1 >= n || matches!(b[i + 1], b'\n' | b'\r') {
                    return None;
                }
                i += 2;
            }
            b'\n' | b'\r' => return None,
            b'[' => {
                in_class = true;
                i += 1;
            }
            b']' if in_class => {
                in_class = false;
                i += 1;
            }
            b'/' if !in_class => {
                if i == start + 1 {
                    return None;
                }
                i += 1;
                while i < n && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                return Some(i);
            }
            _ => i += 1,
        }
    }
    None
}

fn try_scan_xml_literal(src: &str, start: usize) -> Option<usize> {
    let b = src.as_bytes();
    let n = b.len();
    if b.get(start) != Some(&b'<') || !b.get(start + 1).is_some_and(|&c| fmt_ident_start(c)) {
        return None;
    }
    // Verify the initial opening tag `<Name ...>` closes cleanly without unquoted `;` or `{`.
    let mut probe = start + 1;
    while probe < n && (fmt_ident_char(b[probe]) || matches!(b[probe], b':' | b'-' | b'.')) {
        probe += 1;
    }
    let mut quote: Option<u8> = None;
    let mut first_self_closing = false;
    let mut first_end = None;
    while probe < n {
        let ch = b[probe];
        if let Some(q) = quote {
            if ch == b'\\' {
                probe = (probe + 2).min(n);
                continue;
            }
            if ch == q {
                quote = None;
            }
            probe += 1;
            continue;
        }
        match ch {
            b'"' | b'\'' => {
                quote = Some(ch);
                probe += 1;
            }
            b'/' if b.get(probe + 1) == Some(&b'>') => {
                first_self_closing = true;
                first_end = Some(probe + 2);
                break;
            }
            b'>' => {
                first_end = Some(probe + 1);
                break;
            }
            b';' | b'{' | b'}' | b'<' => return None,
            _ => probe += 1,
        }
    }
    let first_end = first_end?;
    if first_self_closing {
        return Some(first_end);
    }

    let mut depth = 1i32;
    let mut i = first_end;
    while i < n && depth > 0 {
        if b[i] == b'<' {
            if src[i..].starts_with("<!--") {
                if let Some(rel) = src[i + 4..].find("-->") {
                    i += 4 + rel + 3;
                    continue;
                }
                return None;
            }
            if src[i..].starts_with("<![CDATA[") {
                if let Some(rel) = src[i + 9..].find("]]>") {
                    i += 9 + rel + 3;
                    continue;
                }
                return None;
            }
            if b.get(i + 1) == Some(&b'/') {
                let mut j = i + 2;
                while j < n && b[j] != b'>' {
                    if matches!(b[j], b'<' | b';') {
                        return None;
                    }
                    j += 1;
                }
                if j >= n {
                    return None;
                }
                depth -= 1;
                i = j + 1;
                continue;
            }
            if b.get(i + 1).is_some_and(|&c| fmt_ident_start(c)) {
                let mut j = i + 1;
                let mut q: Option<u8> = None;
                let mut self_close = false;
                while j < n {
                    let ch = b[j];
                    if let Some(qc) = q {
                        if ch == b'\\' {
                            j = (j + 2).min(n);
                            continue;
                        }
                        if ch == qc {
                            q = None;
                        }
                        j += 1;
                        continue;
                    }
                    if ch == b'"' || ch == b'\'' {
                        q = Some(ch);
                        j += 1;
                    } else if ch == b'/' && b.get(j + 1) == Some(&b'>') {
                        self_close = true;
                        j += 2;
                        break;
                    } else if ch == b'>' {
                        j += 1;
                        break;
                    } else {
                        j += 1;
                    }
                }
                if j > n {
                    return None;
                }
                if !self_close {
                    depth += 1;
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    (depth == 0).then_some(i)
}

fn lex_fmt_tokens(src: &str) -> Vec<FmtTok> {
    let b = src.as_bytes();
    let n = b.len();
    let at = |idx: usize| b.get(idx).copied().unwrap_or(0);
    let mut out: Vec<FmtTok> = Vec::new();
    let mut i = 0usize;
    let mut at_line_start = true;
    let mut last_code_idx: Option<usize> = None;

    while i < n {
        let c = b[i];
        if c == b'\n' {
            out.push(FmtTok {
                range: i..i + 1,
                kind: FmtKind::Newline,
                role: PunctRole::Normal,
            });
            i += 1;
            at_line_start = true;
            continue;
        }
        if matches!(c, b' ' | b'\t' | b'\r') {
            i += 1;
            continue;
        }

        if at_line_start && c == b'#' {
            let rest = &src[i..];
            if rest.starts_with("#include")
                || rest.starts_with("#initclip")
                || rest.starts_with("#endinitclip")
            {
                let start = i;
                while i < n && !matches!(b[i], b'\n' | b'\r') {
                    if b[i] == b'/' && matches!(at(i + 1), b'/' | b'*') {
                        break;
                    }
                    i += 1;
                }
                let end = src[start..i].trim_end_matches([' ', '\t']).len() + start;
                let idx = out.len();
                out.push(FmtTok {
                    range: start..end,
                    kind: FmtKind::Directive,
                    role: PunctRole::Normal,
                });
                last_code_idx = Some(idx);
                at_line_start = false;
                continue;
            }
        }
        at_line_start = false;

        if c == b'/' && at(i + 1) == b'/' {
            let start = i;
            while i < n && !matches!(b[i], b'\n' | b'\r') {
                i += 1;
            }
            out.push(FmtTok {
                range: start..i,
                kind: FmtKind::LineComment,
                role: PunctRole::Normal,
            });
            continue;
        }
        if c == b'/' && at(i + 1) == b'*' {
            let start = i;
            i += 2;
            while i < n && !(b[i] == b'*' && at(i + 1) == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
            out.push(FmtTok {
                range: start..i,
                kind: FmtKind::BlockComment,
                role: PunctRole::Normal,
            });
            continue;
        }
        if matches!(c, b'"' | b'\'' | b'`') {
            let start = i;
            let quote = c;
            i += 1;
            while i < n && b[i] != quote {
                if b[i] == b'\\' {
                    i += 1;
                } else if b[i] == b'\n' && quote != b'`' {
                    break;
                }
                i += 1;
            }
            if i < n && b[i] == quote {
                i += 1;
            }
            let idx = out.len();
            out.push(FmtTok {
                range: start..i,
                kind: FmtKind::String,
                role: PunctRole::Normal,
            });
            last_code_idx = Some(idx);
            continue;
        }

        let prev_info = last_code_idx.map(|idx| (&out[idx].kind, &src[out[idx].range.clone()]));
        if c == b'/'
            && can_precede_regex_or_unary(prev_info)
            && let Some(end) = try_scan_regex(src, i)
        {
            let idx = out.len();
            out.push(FmtTok {
                range: i..end,
                kind: FmtKind::Regex,
                role: PunctRole::Normal,
            });
            last_code_idx = Some(idx);
            i = end;
            continue;
        }

        if c == b'<' {
            let can_start_xml = match prev_info {
                None => true,
                Some((FmtKind::Ident, w)) => EXPR_PRECEDING_KEYWORDS.contains(&w),
                Some((FmtKind::Punct(p), _)) => {
                    !matches!(*p, ")" | "]" | "." | "?." | "::" | "++" | "--" | ":")
                }
                _ => false,
            };
            if can_start_xml && let Some(end) = try_scan_xml_literal(src, i) {
                let idx = out.len();
                out.push(FmtTok {
                    range: i..end,
                    kind: FmtKind::Xml,
                    role: PunctRole::Normal,
                });
                last_code_idx = Some(idx);
                i = end;
                continue;
            }
        }

        if c.is_ascii_digit() || (c == b'.' && at(i + 1).is_ascii_digit()) {
            let start = i;
            if c == b'0' && matches!(at(i + 1), b'x' | b'X' | b'b' | b'B' | b'o' | b'O') {
                i += 2;
                while i < n && (b[i].is_ascii_hexdigit() || b[i] == b'_') {
                    i += 1;
                }
            } else {
                while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
                    i += 1;
                }
                if i < n && b[i] == b'.' {
                    i += 1;
                    while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
                        i += 1;
                    }
                }
                if i < n && matches!(b[i], b'e' | b'E') {
                    let mut exp = i + 1;
                    if matches!(at(exp), b'+' | b'-') {
                        exp += 1;
                    }
                    if at(exp).is_ascii_digit() {
                        i = exp + 1;
                        while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
                            i += 1;
                        }
                    }
                }
            }
            if i < n && b[i] == b'n' {
                i += 1;
            }
            let idx = out.len();
            out.push(FmtTok {
                range: start..i,
                kind: FmtKind::Number,
                role: PunctRole::Normal,
            });
            last_code_idx = Some(idx);
            continue;
        }

        if fmt_ident_start(c) || (c == b'@' && fmt_ident_start(at(i + 1))) {
            let start = i;
            i += 1;
            while i < n && fmt_ident_char(b[i]) {
                i += 1;
            }
            let idx = out.len();
            out.push(FmtTok {
                range: start..i,
                kind: FmtKind::Ident,
                role: PunctRole::Normal,
            });
            last_code_idx = Some(idx);
            continue;
        }

        if let Some(&punct) = MULTI_PUNCTS.iter().find(|&&p| src[i..].starts_with(p)) {
            let start = i;
            i += punct.len();
            let idx = out.len();
            out.push(FmtTok {
                range: start..i,
                kind: FmtKind::Punct(punct),
                role: PunctRole::Normal,
            });
            last_code_idx = Some(idx);
            continue;
        }

        let ch_len = src[i..].chars().next().map_or(1, char::len_utf8);
        let idx = out.len();
        out.push(FmtTok {
            range: i..i + ch_len,
            kind: FmtKind::Other,
            role: PunctRole::Normal,
        });
        last_code_idx = Some(idx);
        i += ch_len;
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FmtScope {
    BlockBrace,
    ObjectBrace,
    TypeBrace,
    Paren,
    Bracket,
}

fn find_generic_close_on_line(
    toks: &[FmtTok],
    src: &str,
    lt_idx: usize,
    already_matched_closes: &std::collections::HashSet<usize>,
) -> Option<usize> {
    let mut depth = 0i32;
    let mut j = lt_idx;
    while j < toks.len() {
        match &toks[j].kind {
            FmtKind::Newline => return None,
            FmtKind::LineComment | FmtKind::BlockComment => {}
            FmtKind::Punct("<") => depth += 1,
            FmtKind::Punct(p @ (">" | ">>" | ">>>")) => {
                let n_gt = p.len() as i32;
                if depth > n_gt {
                    depth -= n_gt;
                } else if depth == n_gt {
                    // Check token after closing `>`/`>>`/`>>>` is compatible with a generic type.
                    let mut k = j + 1;
                    while k < toks.len()
                        && matches!(toks[k].kind, FmtKind::LineComment | FmtKind::BlockComment)
                    {
                        k += 1;
                    }
                    let after_ok = match toks.get(k).map(|t| (&t.kind, &src[t.range.clone()])) {
                        None | Some((FmtKind::Newline, _)) => true,
                        Some((FmtKind::Punct(np), _)) => matches!(
                            *np,
                            "(" | ")"
                                | "["
                                | "]"
                                | "{"
                                | "}"
                                | ";"
                                | ","
                                | "="
                                | ":"
                                | "|"
                                | "&"
                                | ">"
                                | ">>"
                                | ">>>"
                                | "?"
                                | "."
                                | "=>"
                        ),
                        Some((FmtKind::Ident, w)) => matches!(w, "extends" | "implements"),
                        _ => false,
                    };
                    return after_ok.then_some(j);
                } else {
                    // `n_gt > depth`: an inner `<` shares a combined `>>`/`>>>` close token with
                    // an enclosing generic `<` that already validated `j`.
                    return already_matched_closes.contains(&j).then_some(j);
                }
            }
            FmtKind::Ident | FmtKind::Number | FmtKind::String => {}
            FmtKind::Punct(p)
                if matches!(
                    *p,
                    "," | "." | "[" | "]" | "|" | "&" | "?" | ":" | "=>" | "(" | ")" | "*" | "="
                ) => {}
            _ => return None,
        }
        j += 1;
    }
    None
}

fn paren_followed_by_method_sig(toks: &[FmtTok], open_paren_idx: usize) -> bool {
    let mut depth = 0i32;
    let mut j = open_paren_idx;
    while j < toks.len() {
        match toks[j].kind {
            FmtKind::Punct("(") => depth += 1,
            FmtKind::Punct(")") => {
                depth -= 1;
                if depth == 0 {
                    let mut k = j + 1;
                    while k < toks.len() && !toks[k].is_code() {
                        k += 1;
                    }
                    return match toks.get(k).map(|t| &t.kind) {
                        Some(FmtKind::Punct("{")) => true,
                        Some(FmtKind::Punct(":")) => {
                            // Method return-type `: RetType` has a type token on the same line
                            // (unlike an accidental statement-ending trailing `:`).
                            let mut m = k + 1;
                            while m < toks.len()
                                && matches!(
                                    toks[m].kind,
                                    FmtKind::LineComment | FmtKind::BlockComment
                                )
                            {
                                m += 1;
                            }
                            toks.get(m)
                                .is_some_and(|t| t.kind != FmtKind::Newline && t.is_code())
                        }
                        _ => false,
                    };
                }
            }
            _ => {}
        }
        j += 1;
    }
    false
}

fn annotate_fmt_roles(toks: &mut [FmtTok], src: &str) {
    let len = toks.len();
    let mut scopes: Vec<FmtScope> = Vec::new();
    let mut ternary_stack: Vec<usize> = Vec::new(); // scope depths where `?` opened
    let mut case_label_depths: Vec<usize> = Vec::new(); // scope depths where `case`/`default` opened
    let mut expect_type = false;
    let mut in_type_alias = false;
    let mut generic_closes: std::collections::HashSet<usize> = std::collections::HashSet::new();

    // Helper closures for previous/next code token indices.
    let prev_code = |toks: &[FmtTok], from: usize| -> Option<(usize, bool)> {
        let mut same_line = true;
        let mut idx = from;
        while idx > 0 {
            idx -= 1;
            if toks[idx].kind == FmtKind::Newline {
                same_line = false;
            } else if toks[idx].is_code() {
                return Some((idx, same_line));
            }
        }
        None
    };
    let next_code = |toks: &[FmtTok], from: usize| -> Option<(usize, bool)> {
        let mut same_line = true;
        let mut idx = from + 1;
        while idx < len {
            if toks[idx].kind == FmtKind::Newline {
                same_line = false;
            } else if toks[idx].is_code() {
                return Some((idx, same_line));
            }
            idx += 1;
        }
        None
    };

    for idx in 0..len {
        if !toks[idx].is_code() {
            continue;
        }
        if generic_closes.remove(&idx) {
            toks[idx].role = PunctRole::GenericClose;
            continue;
        }
        let prev = prev_code(toks, idx);
        let next = next_code(toks, idx);
        let prev_kind_text = prev.map(|(pi, _)| (&toks[pi].kind, &src[toks[pi].range.clone()]));

        match toks[idx].kind.clone() {
            FmtKind::Ident => {
                let word = &src[toks[idx].range.clone()];
                let after_dot =
                    matches!(prev_kind_text, Some((FmtKind::Punct("." | "?." | "::"), _)));
                let after_member_modifier = matches!(
                    prev_kind_text,
                    Some((
                        FmtKind::Ident,
                        "public"
                            | "private"
                            | "protected"
                            | "internal"
                            | "static"
                            | "override"
                            | "async"
                            | "abstract"
                            | "final"
                            | "readonly"
                            | "get"
                            | "set"
                    ))
                );
                let in_obj_key_pos =
                    matches!(
                        scopes.last(),
                        Some(FmtScope::ObjectBrace | FmtScope::TypeBrace)
                    ) && matches!(prev_kind_text, Some((FmtKind::Punct("{" | "," | ";"), _)));

                if SPACE_BEFORE_PAREN_KEYWORDS.contains(&word)
                    && !after_dot
                    && !after_member_modifier
                    && !in_obj_key_pos
                {
                    let is_method_decl = matches!(
                        word,
                        "delete"
                            | "typeof"
                            | "void"
                            | "in"
                            | "of"
                            | "instanceof"
                            | "new"
                            | "await"
                            | "yield"
                            | "return"
                            | "throw"
                    ) && matches!(
                        next,
                        Some((ni, true))
                            if toks[ni].kind == FmtKind::Punct("(")
                                && paren_followed_by_method_sig(toks, ni)
                    );
                    if !is_method_decl {
                        toks[idx].role = PunctRole::KeywordSpaceBeforeParen;
                    }
                }

                if !after_dot
                    && !in_obj_key_pos
                    && matches!(word, "case" | "default")
                    && case_label_depths.last() != Some(&scopes.len())
                {
                    case_label_depths.push(scopes.len());
                }
                if !after_dot && word == "type" {
                    in_type_alias = true;
                }
                if !after_dot && FMT_TYPE_KEYWORDS.contains(&word) {
                    expect_type = true;
                } else if expect_type {
                    let next_punct = next.and_then(|(ni, same)| {
                        same.then_some(&toks[ni].kind).and_then(|k| match k {
                            FmtKind::Punct(p) => Some(*p),
                            _ => None,
                        })
                    });
                    expect_type = matches!(next_punct, Some("." | "<" | "|" | "&"));
                }
            }
            FmtKind::Punct("{") => {
                let scope = if expect_type
                    || matches!(prev, Some((pi, _)) if toks[pi].role == PunctRole::TypeColon)
                {
                    FmtScope::TypeBrace
                } else {
                    let is_obj = match prev {
                        Some((pi, _)) => match &toks[pi].kind {
                            FmtKind::Punct("*") => toks[pi].role == PunctRole::Binary,
                            FmtKind::Punct(":") => matches!(
                                toks[pi].role,
                                PunctRole::ObjectColon | PunctRole::TernaryColon
                            ),
                            FmtKind::Punct(p) => matches!(
                                *p,
                                "=" | "("
                                    | "["
                                    | ","
                                    | "?"
                                    | "!"
                                    | "+"
                                    | "-"
                                    | "%"
                                    | "&"
                                    | "|"
                                    | "^"
                                    | "&&"
                                    | "||"
                                    | "??"
                            ),
                            FmtKind::Ident => {
                                let w = &src[toks[pi].range.clone()];
                                let after_type_colon = matches!(
                                    prev_code(toks, pi),
                                    Some((ppi, _)) if toks[ppi].role == PunctRole::TypeColon
                                );
                                !after_type_colon
                                    && EXPR_PRECEDING_KEYWORDS.contains(&w)
                                    && w != "else"
                                    && w != "do"
                            }
                            _ => false,
                        },
                        None => false,
                    };
                    if is_obj {
                        FmtScope::ObjectBrace
                    } else {
                        FmtScope::BlockBrace
                    }
                };
                scopes.push(scope);
                expect_type = false;
            }
            FmtKind::Punct("}") => {
                while matches!(scopes.last(), Some(FmtScope::Paren | FmtScope::Bracket)) {
                    scopes.pop();
                }
                scopes.pop();
                ternary_stack.retain(|&d| d <= scopes.len());
                case_label_depths.retain(|&d| d <= scopes.len());
                expect_type = false;
            }
            FmtKind::Punct("(") => {
                scopes.push(FmtScope::Paren);
                expect_type = false;
            }
            FmtKind::Punct(")") => {
                if scopes.last() == Some(&FmtScope::Paren) {
                    scopes.pop();
                }
                ternary_stack.retain(|&d| d <= scopes.len());
                case_label_depths.retain(|&d| d <= scopes.len());
                expect_type = false;
            }
            FmtKind::Punct("[") => {
                scopes.push(FmtScope::Bracket);
            }
            FmtKind::Punct("]") => {
                if scopes.last() == Some(&FmtScope::Bracket) {
                    scopes.pop();
                }
                ternary_stack.retain(|&d| d <= scopes.len());
                case_label_depths.retain(|&d| d <= scopes.len());
            }
            FmtKind::Punct("?") => {
                let next_is_colon = matches!(
                    next,
                    Some((ni, true)) if toks[ni].kind == FmtKind::Punct(":")
                );
                if !next_is_colon {
                    ternary_stack.push(scopes.len());
                }
            }
            FmtKind::Punct(":") => {
                let at_eol = next.is_none_or(|(_, same_line)| !same_line);
                let in_paren_or_bracket = scopes
                    .iter()
                    .any(|s| matches!(s, FmtScope::Paren | FmtScope::Bracket));
                let in_obj_brace = scopes.last() == Some(&FmtScope::ObjectBrace);
                let in_type_brace = scopes.last() == Some(&FmtScope::TypeBrace);

                if ternary_stack.last() == Some(&scopes.len()) {
                    ternary_stack.pop();
                    toks[idx].role = PunctRole::TernaryColon;
                    expect_type = false;
                } else if case_label_depths.last() == Some(&scopes.len()) {
                    case_label_depths.pop();
                    toks[idx].role = PunctRole::CaseColon;
                    expect_type = false;
                } else if in_obj_brace {
                    toks[idx].role = PunctRole::ObjectColon;
                    expect_type = false;
                } else if in_type_brace {
                    toks[idx].role = PunctRole::ObjectColon;
                    expect_type = true;
                } else {
                    // Collect code tokens of the current statement on this line before `:`.
                    let mut stmt_toks: Vec<usize> = Vec::new();
                    let mut scan = idx;
                    while scan > 0 {
                        scan -= 1;
                        if toks[scan].kind == FmtKind::Newline {
                            break;
                        }
                        if matches!(toks[scan].kind, FmtKind::Punct(";" | "{" | "}")) {
                            break;
                        }
                        if toks[scan].is_code() {
                            stmt_toks.push(scan);
                        }
                    }
                    stmt_toks.reverse();

                    let is_single_ident_label = stmt_toks.len() == 1
                        && toks[stmt_toks[0]].kind == FmtKind::Ident
                        && !matches!(
                            &src[toks[stmt_toks[0]].range.clone()],
                            "break" | "continue" | "return"
                        );
                    let has_assign = stmt_toks
                        .iter()
                        .any(|&ti| toks[ti].kind == FmtKind::Punct("="));
                    let first_word = stmt_toks
                        .first()
                        .filter(|&&ti| toks[ti].kind == FmtKind::Ident)
                        .map(|&ti| &src[toks[ti].range.clone()]);
                    let is_decl_or_case_header = matches!(first_word, Some("case" | "default"))
                        || (!has_assign
                            && matches!(
                                first_word,
                                Some(
                                    "var"
                                        | "let"
                                        | "const"
                                        | "function"
                                        | "public"
                                        | "private"
                                        | "protected"
                                        | "internal"
                                        | "static"
                                        | "override"
                                        | "export"
                                        | "async"
                                        | "readonly"
                                        | "abstract"
                                        | "final"
                                        | "dynamic"
                                        | "get"
                                        | "set"
                                )
                            ));

                    if is_single_ident_label {
                        toks[idx].role = PunctRole::LabelColon;
                        expect_type = false;
                    } else if at_eol
                        && !in_paren_or_bracket
                        && !stmt_toks.is_empty()
                        && !is_decl_or_case_header
                    {
                        toks[idx].role = PunctRole::TrailingColonAsSemi;
                        expect_type = false;
                        in_type_alias = false;
                    } else {
                        toks[idx].role = PunctRole::TypeColon;
                        expect_type = true;
                    }
                }
            }
            FmtKind::Punct("+" | "-") => {
                let is_unary = match prev {
                    None => true,
                    Some((pi, _)) => {
                        if matches!(
                            toks[pi].role,
                            PunctRole::GenericClose
                                | PunctRole::PostfixIncDec
                                | PunctRole::PostfixNonNull
                        ) {
                            false
                        } else {
                            can_precede_regex_or_unary(Some((
                                &toks[pi].kind,
                                &src[toks[pi].range.clone()],
                            )))
                        }
                    }
                };
                toks[idx].role = if is_unary {
                    PunctRole::UnaryPrefix
                } else {
                    PunctRole::Binary
                };
                expect_type = false;
            }
            FmtKind::Punct("++" | "--") => {
                let is_postfix = matches!(
                    prev,
                    Some((pi, true))
                        if matches!(
                            toks[pi].kind,
                            FmtKind::Ident | FmtKind::Punct(")" | "]")
                        ) && !EXPR_PRECEDING_KEYWORDS.contains(&&src[toks[pi].range.clone()])
                );
                toks[idx].role = if is_postfix {
                    PunctRole::PostfixIncDec
                } else {
                    PunctRole::PrefixIncDec
                };
                expect_type = false;
            }
            FmtKind::Punct("!") => {
                let prev_value = matches!(
                    prev,
                    Some((pi, true))
                        if matches!(toks[pi].kind, FmtKind::Ident | FmtKind::Punct(")" | "]"))
                            && !EXPR_PRECEDING_KEYWORDS.contains(&&src[toks[pi].range.clone()])
                );
                let next_chain = matches!(
                    next,
                    Some((ni, true))
                        if matches!(
                            toks[ni].kind,
                            FmtKind::Punct("." | "?." | "[" | "(" | "," | ";" | ")" | "]" | ":")
                        )
                );
                toks[idx].role = if prev_value && next_chain {
                    PunctRole::PostfixNonNull
                } else {
                    PunctRole::UnaryPrefix
                };
                expect_type = false;
            }
            FmtKind::Punct("~") => {
                toks[idx].role = PunctRole::UnaryPrefix;
                expect_type = false;
            }
            FmtKind::Punct("*") => {
                toks[idx].role = match prev {
                    Some((pi, true)) if toks[pi].kind == FmtKind::Punct(".") => {
                        PunctRole::WildcardAfterDot
                    }
                    Some((pi, true))
                        if toks[pi].kind == FmtKind::Ident
                            && &src[toks[pi].range.clone()] == "function" =>
                    {
                        PunctRole::GeneratorStar
                    }
                    Some((pi, _)) if toks[pi].role == PunctRole::TypeColon => PunctRole::TypeAny,
                    _ => PunctRole::Binary,
                };
                expect_type = false;
            }
            FmtKind::Punct("<") => {
                let after_dot = matches!(
                    prev,
                    Some((pi, true)) if toks[pi].kind == FmtKind::Punct(".")
                );
                let prev_ident = prev.and_then(|(pi, same)| {
                    (same && toks[pi].kind == FmtKind::Ident).then(|| &src[toks[pi].range.clone()])
                });
                let prev_prev_word =
                    prev.and_then(|(pi, _)| prev_code(toks, pi))
                        .and_then(|(ppi, _)| {
                            (toks[ppi].kind == FmtKind::Ident)
                                .then(|| &src[toks[ppi].range.clone()])
                        });
                let generic_candidate = after_dot
                    || expect_type
                    || !generic_closes.is_empty()
                    || matches!(
                        prev_ident,
                        Some(
                            "Array"
                                | "Vector"
                                | "Map"
                                | "Set"
                                | "Promise"
                                | "Dictionary"
                                | "ReadonlyArray"
                                | "Record"
                                | "Partial"
                        )
                    )
                    || (prev_ident.is_some()
                        && matches!(
                            prev_prev_word,
                            Some("function" | "class" | "interface" | "type")
                        ));
                if generic_candidate
                    && let Some(close_idx) =
                        find_generic_close_on_line(toks, src, idx, &generic_closes)
                {
                    toks[idx].role = PunctRole::GenericOpen;
                    generic_closes.insert(close_idx);
                    expect_type = false;
                } else {
                    toks[idx].role = PunctRole::Binary;
                    expect_type = false;
                }
            }
            FmtKind::Punct("=") => {
                toks[idx].role = PunctRole::Binary;
                expect_type = in_type_alias;
            }
            FmtKind::Punct(";") => {
                expect_type = false;
                in_type_alias = false;
                case_label_depths.retain(|&d| d < scopes.len());
                ternary_stack.retain(|&d| d < scopes.len());
            }
            FmtKind::Punct(p) => {
                if matches!(
                    p,
                    ">>>="
                        | "==="
                        | "!=="
                        | ">>>"
                        | "<<="
                        | ">>="
                        | "&&="
                        | "||="
                        | "??="
                        | "=="
                        | "!="
                        | "<>"
                        | "<="
                        | ">="
                        | "&&"
                        | "||"
                        | "??"
                        | "<<"
                        | ">>"
                        | "+="
                        | "-="
                        | "*="
                        | "/="
                        | "%="
                        | "&="
                        | "|="
                        | "^="
                        | "/"
                        | "%"
                        | ">"
                        | "&"
                        | "|"
                        | "^"
                        | "=>"
                ) {
                    toks[idx].role = PunctRole::Binary;
                }
                if !(matches!(p, "|" | "&") && in_type_alias) && p != "." {
                    expect_type = false;
                }
            }
            _ => {
                expect_type = false;
            }
        }
    }
}

fn space_between(prev: &FmtTok, next: &FmtTok, src: &str, had_space: bool) -> bool {
    let pt = prev.text(src);
    let nt = next.text(src);

    if next.kind == FmtKind::LineComment {
        return true;
    }
    if next.kind == FmtKind::BlockComment {
        return !matches!(pt, "(" | "[");
    }
    if prev.kind == FmtKind::BlockComment {
        return !matches!(nt, ")" | "]" | "," | ";");
    }

    // Never put space before `,` or `;` (including normalized trailing `:` -> `;`), except when
    // an incomplete statement ends right after a binary/assignment operator (`var x = ;`).
    if nt == ";" && prev.role == PunctRole::Binary {
        return true;
    }
    if matches!(nt, "," | ";") {
        return false;
    }
    if pt == "," {
        return !matches!(nt, "]" | ")");
    }
    if pt == ";" {
        return !matches!(nt, ";" | ")");
    }

    // Member access & namespace qualifiers.
    if matches!(pt, "." | "?." | "::") || matches!(nt, "." | "?." | "::") {
        return false;
    }

    // Generics `<...>`
    if next.role == PunctRole::GenericOpen || prev.role == PunctRole::GenericOpen {
        return false;
    }
    if next.role == PunctRole::GenericClose {
        return false;
    }
    if prev.role == PunctRole::GenericClose {
        return !matches!(nt, "(" | ")" | "[" | "]" | "," | ";" | "." | "?.")
            && next.role != PunctRole::GenericClose;
    }

    // Parentheses
    if pt == "(" || nt == ")" {
        return false;
    }
    if nt == "(" {
        if prev.kind == FmtKind::Ident {
            return prev.role == PunctRole::KeywordSpaceBeforeParen;
        }
        if prev.role == PunctRole::GeneratorStar {
            return true;
        }
        if prev.role == PunctRole::TypeColon {
            return had_space;
        }
        if matches!(pt, ")" | "]" | "(" | "[" | "...")
            || matches!(
                prev.role,
                PunctRole::UnaryPrefix
                    | PunctRole::PrefixIncDec
                    | PunctRole::PostfixNonNull
                    | PunctRole::GenericClose
            )
        {
            return false;
        }
        return true;
    }
    if pt == ")" {
        if matches!(nt, ")" | "]" | "(" | "[" | "," | ";")
            || matches!(
                next.role,
                PunctRole::PostfixIncDec
                    | PunctRole::PostfixNonNull
                    | PunctRole::TypeColon
                    | PunctRole::CaseColon
                    | PunctRole::ObjectColon
                    | PunctRole::GenericClose
            )
        {
            return false;
        }
        return true;
    }

    // Brackets
    if pt == "[" || nt == "]" {
        return false;
    }
    if nt == "[" {
        if prev.kind == FmtKind::Ident {
            return EXPR_PRECEDING_KEYWORDS.contains(&pt) && pt != "new";
        }
        if prev.role == PunctRole::TypeColon {
            return had_space;
        }
        if matches!(pt, ")" | "]" | "(" | "[")
            || matches!(prev.role, PunctRole::UnaryPrefix | PunctRole::GenericClose)
        {
            return false;
        }
        return true;
    }
    if pt == "]" {
        if matches!(nt, ")" | "]" | "(" | "[" | "," | ";")
            || matches!(
                next.role,
                PunctRole::PostfixIncDec
                    | PunctRole::PostfixNonNull
                    | PunctRole::TypeColon
                    | PunctRole::GenericClose
            )
        {
            return false;
        }
        return true;
    }

    // Braces
    if pt == "{" && nt == "}" {
        return false;
    }
    if nt == "{" {
        return !matches!(pt, "(" | "[");
    }
    if pt == "{" || nt == "}" {
        return true;
    }
    if pt == "}" {
        return !matches!(nt, ";" | "," | ")" | "]");
    }

    // Question mark (ternary vs optional `?:`)
    if pt == "?" && matches!(next.role, PunctRole::TypeColon | PunctRole::ObjectColon) {
        return false;
    }

    // Colons
    match next.role {
        PunctRole::TernaryColon => return true,
        PunctRole::ObjectColon
        | PunctRole::CaseColon
        | PunctRole::LabelColon
        | PunctRole::TypeColon => return false,
        _ => {}
    }
    match prev.role {
        PunctRole::TernaryColon
        | PunctRole::ObjectColon
        | PunctRole::CaseColon
        | PunctRole::LabelColon => return true,
        PunctRole::TypeColon => return had_space,
        _ => {}
    }

    // Optional `ident?: Type` — `?` right before `TypeColon` has no space before `?` either.
    // (Checked by looking at `nt == "?"` when `had_space == false`? Better: handled via `pt`/`nt` below.)

    // Unary / inc-dec / spread / wildcard / type `*`
    if next.role == PunctRole::PostfixIncDec || next.role == PunctRole::PostfixNonNull {
        return false;
    }
    if prev.role == PunctRole::PrefixIncDec {
        return false;
    }
    if prev.role == PunctRole::PostfixIncDec {
        return !matches!(nt, ")" | "]" | "," | ";");
    }
    if prev.role == PunctRole::PostfixNonNull {
        return !matches!(nt, "[" | "(" | "," | ";" | ")" | "]")
            && next.role != PunctRole::TypeColon;
    }
    if prev.role == PunctRole::UnaryPrefix {
        if (pt == "+" && nt.starts_with('+')) || (pt == "-" && nt.starts_with('-')) {
            return true;
        }
        return false;
    }
    if next.role == PunctRole::UnaryPrefix || next.role == PunctRole::PrefixIncDec {
        if prev.role == PunctRole::UnaryPrefix {
            return (pt == "+" && nt.starts_with('+')) || (pt == "-" && nt.starts_with('-'));
        }
        return !matches!(pt, "(" | "[");
    }
    if nt == "..." {
        return !matches!(pt, "(" | "[");
    }
    if pt == "..." {
        return false;
    }
    if next.role == PunctRole::GeneratorStar || next.role == PunctRole::WildcardAfterDot {
        return false;
    }
    if prev.role == PunctRole::GeneratorStar {
        return true;
    }
    if prev.role == PunctRole::WildcardAfterDot {
        return nt != ";";
    }
    if prev.role == PunctRole::TypeAny {
        return !matches!(nt, ")" | "]" | "," | ";");
    }

    // Binary & assignment operators and ternary `?`
    if prev.role == PunctRole::Binary || next.role == PunctRole::Binary || pt == "?" || nt == "?" {
        return true;
    }

    // Adjacent identifiers, keywords, and literals (`var x`, `new Array`, `return 0`, `else if`).
    true
}

/// "Auto-format": re-indent and format statement/operator/delimiter spacing using `tab_size`
/// spaces per level (`1..=8`). Preserves strings, comments, regexes, `#include` directives and
/// E4X/XML literals verbatim, normalizes accidental trailing statement `:` into `;`, and returns
/// `None` when `text` is already formatted.
pub(crate) fn auto_format_with_tab_size(text: &str, tab_size: u8) -> Option<String> {
    let indent = " ".repeat(usize::from(tab_size.clamp(1, 8)));
    let mut toks = lex_fmt_tokens(text);
    annotate_fmt_roles(&mut toks, text);

    let mut line_starts = vec![0usize];
    line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));

    // Multi-line verbatim spans (`/* ... */`, multi-line strings/templates, multi-line E4X XML).
    let inside_multiline_verbatim = |line_start: usize| -> bool {
        toks.iter().any(|tok| {
            matches!(
                tok.kind,
                FmtKind::BlockComment | FmtKind::String | FmtKind::Xml
            ) && tok.range.start < line_start
                && line_start < tok.range.end
        })
    };
    let ends_inside_multiline_xml_or_str = |line_end: usize| -> bool {
        toks.iter().any(|tok| {
            matches!(
                tok.kind,
                FmtKind::BlockComment | FmtKind::String | FmtKind::Xml
            ) && tok.range.start < line_end
                && line_end < tok.range.end
        })
    };

    let mut tok_cursor = 0usize;
    let mut out_lines: Vec<String> = Vec::with_capacity(line_starts.len());
    let mut depth: i32 = 0;
    let mut case_depths: Vec<i32> = Vec::new();

    for (line_idx, &start) in line_starts.iter().enumerate() {
        let end = line_starts
            .get(line_idx + 1)
            .map_or(text.len(), |next| next - 1);
        let raw_line = text[start..end]
            .strip_suffix('\r')
            .unwrap_or(&text[start..end]);

        // Advance `tok_cursor` past any tokens that ended before `start`.
        while tok_cursor < toks.len() && toks[tok_cursor].range.end <= start {
            tok_cursor += 1;
        }

        // Collect indices of tokens that start on `[start..end)` (or overlap `end` if multiline).
        let mut line_toks: Vec<usize> = Vec::new();
        let mut scan = tok_cursor;
        while scan < toks.len() {
            if toks[scan].kind == FmtKind::Newline && toks[scan].range.start >= end {
                break;
            }
            if toks[scan].range.start >= end && end > start {
                break;
            }
            if toks[scan].range.start >= start && toks[scan].range.start < end {
                line_toks.push(scan);
            }
            scan += 1;
        }

        let starts_verbatim = inside_multiline_verbatim(start);
        // Compute structural depth delta contributed by `{ } ( ) [ ]` on this line.
        let mut line_delta = 0i32;
        let mut leading_close = 0i32;
        let mut seen_non_close = false;
        for &ti in &line_toks {
            if let FmtKind::Punct(p) = toks[ti].kind {
                let d = match p {
                    "{" | "(" | "[" => 1,
                    "}" | ")" | "]" => -1,
                    _ => 0,
                };
                if !seen_non_close {
                    if d == -1 {
                        leading_close += 1;
                    } else {
                        seen_non_close = true;
                    }
                }
                line_delta += d;
            } else if toks[ti].is_code() {
                seen_non_close = true;
            }
        }

        if starts_verbatim {
            // If this line closes a multi-line XML tag followed by `;` (e.g. `</a>;`), keep raw line
            // trimmed on the right so multi-line comment/string/XML interiors are never altered.
            out_lines.push(raw_line.trim_end_matches([' ', '\t']).to_owned());
            depth = (depth + line_delta).max(0);
            continue;
        }

        if line_toks.is_empty() || raw_line.trim().is_empty() {
            out_lines.push(String::new());
            continue;
        }

        // Format the tokens on this line into `formatted_body`.
        let spans_into_next = ends_inside_multiline_xml_or_str(end);
        let mut formatted_body = String::with_capacity(raw_line.len() + 8);
        for (pos, &ti) in line_toks.iter().enumerate() {
            let tok = &toks[ti];
            if pos > 0 {
                let prev_tok = &toks[line_toks[pos - 1]];
                let had_space = prev_tok.range.end < tok.range.start;
                // Optional parameter/property `ident?: Type`: no space before `?` when followed immediately by `TypeColon`/`ObjectColon`.
                let is_optional_qmark = tok.kind == FmtKind::Punct("?")
                    && line_toks.get(pos + 1).is_some_and(|&nti| {
                        matches!(
                            toks[nti].role,
                            PunctRole::TypeColon | PunctRole::ObjectColon
                        )
                    });
                let want_space = if is_optional_qmark {
                    false
                } else {
                    space_between(prev_tok, tok, text, had_space)
                };
                if want_space {
                    formatted_body.push(' ');
                }
            }
            if spans_into_next && pos + 1 == line_toks.len() && tok.range.end > end {
                // Multi-line token (`/* ...`, `` `... ``, `<a>...`) starts on this line and continues onto next line(s):
                // emit only the first-line slice up to `end`!
                let slice = text[tok.range.start..end]
                    .strip_suffix('\r')
                    .unwrap_or(&text[tok.range.start..end]);
                formatted_body.push_str(slice.trim_end_matches([' ', '\t']));
            } else {
                formatted_body.push_str(tok.text(text));
            }
        }

        let first_code_tok = line_toks.iter().copied().find(|&ti| toks[ti].is_code());
        let case_header_line = first_code_tok.is_some_and(|ti| {
            toks[ti].kind == FmtKind::Ident
                && matches!(&text[toks[ti].range.clone()], "case" | "default")
        }) && line_toks
            .iter()
            .any(|&ti| toks[ti].role == PunctRole::CaseColon);
        while case_depths
            .last()
            .is_some_and(|&d| d > depth - leading_close || (d == depth && case_header_line))
        {
            case_depths.pop();
        }
        let level = (depth - leading_close).max(0) + case_depths.len() as i32;
        out_lines.push(format!(
            "{}{}",
            indent.repeat(level as usize),
            formatted_body
        ));
        if case_header_line && line_delta == 0 {
            case_depths.push(depth);
        }
        depth = (depth + line_delta).max(0);
    }

    let formatted = out_lines.join("\n");
    (formatted != text).then_some(formatted)
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

/// Timeout in seconds after pressing `<ESC>` before a pending two-key chord expires automatically.
pub(crate) const ESC_CHORD_TIMEOUT_SECS: f64 = 5.0;

pub(crate) const ESC_CHORD_ARMED_PROMPT: &str =
    "ESC shortcut: type 2 keys (fn = function, tr = trace, st = stop, if, fr…)";

pub(crate) const ESC_CHORD_CANCELLED_STATUS: &str = "ESC shortcut cancelled.";

/// Initial caret or placeholder selection within an `<ESC>` chord's raw template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EscChordSelection {
    /// Place caret at the end of the inserted snippet.
    End,
    /// Place caret at an exact char offset inside the raw template.
    CaretAt(usize),
    /// Highlight the first placeholder token `(start_char, end_char)` in the raw template.
    Range(usize, usize),
}

/// One entry of Classic Flash's 50 `<ESC>` + two-key code-insertion shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EscChordSpec {
    pub keys: [char; 2],
    pub summary: &'static str,
    pub template: &'static str,
    pub selection: EscChordSelection,
}

/// Complete 50-chord Classic Flash `<ESC>` + two-key table in canonical reference order.
pub(crate) const ESC_CHORDS: [EscChordSpec; 50] = [
    // 1. br -> break;
    EscChordSpec {
        keys: ['b', 'r'],
        summary: "break",
        template: "break;",
        selection: EscChordSelection::End,
    },
    // 2. ca -> call ( );
    EscChordSpec {
        keys: ['c', 'a'],
        summary: "call",
        template: "call ( );",
        selection: EscChordSelection::CaretAt(6),
    },
    // 3. // -> //
    EscChordSpec {
        keys: ['/', '/'],
        summary: "comment",
        template: "// ",
        selection: EscChordSelection::End,
    },
    // 4. co -> continue;
    EscChordSpec {
        keys: ['c', 'o'],
        summary: "continue",
        template: "continue;",
        selection: EscChordSelection::End,
    },
    // 5. da -> delete ( );
    EscChordSpec {
        keys: ['d', 'a'],
        summary: "delete",
        template: "delete ( );",
        selection: EscChordSelection::CaretAt(8),
    },
    // 6. do -> do\n{\n} while ( condition );
    EscChordSpec {
        keys: ['d', 'o'],
        summary: "do..while",
        template: "do\n{\n} while ( condition );",
        selection: EscChordSelection::Range(15, 24),
    },
    // 7. dm -> duplicateMovieClip ( target, newname, depth );
    EscChordSpec {
        keys: ['d', 'm'],
        summary: "duplicateMovieClip",
        template: "duplicateMovieClip ( target, newname, depth );",
        selection: EscChordSelection::Range(21, 27),
    },
    // 8. el -> } else {
    EscChordSpec {
        keys: ['e', 'l'],
        summary: "else",
        template: "} else {",
        selection: EscChordSelection::End,
    },
    // 9. ei -> } else if ( condition ) {
    EscChordSpec {
        keys: ['e', 'i'],
        summary: "else if",
        template: "} else if ( condition ) {",
        selection: EscChordSelection::Range(12, 21),
    },
    // 10. fr -> for ( init; condition; next ) {\n}
    EscChordSpec {
        keys: ['f', 'r'],
        summary: "for",
        template: "for ( init; condition; next ) {\n}",
        selection: EscChordSelection::Range(6, 10),
    },
    // 11. fi -> for ( iterator in object ) {\n}
    EscChordSpec {
        keys: ['f', 'i'],
        summary: "for..in",
        template: "for ( iterator in object ) {\n}",
        selection: EscChordSelection::Range(6, 14),
    },
    // 12. fs -> fscommand ( command, arguments );
    EscChordSpec {
        keys: ['f', 's'],
        summary: "fscommand",
        template: "fscommand ( command, arguments );",
        selection: EscChordSelection::Range(12, 19),
    },
    // 13. fn -> function name ( ) {\n\t\n}
    EscChordSpec {
        keys: ['f', 'n'],
        summary: "function",
        template: "function name ( ) {\n\t\n}",
        selection: EscChordSelection::Range(9, 13),
    },
    // 14. gu -> getURL ( url, window, method );
    EscChordSpec {
        keys: ['g', 'u'],
        summary: "getURL",
        template: "getURL ( url, window, method );",
        selection: EscChordSelection::Range(9, 12),
    },
    // 15. gp -> gotoAndPlay ( frame );
    EscChordSpec {
        keys: ['g', 'p'],
        summary: "gotoAndPlay",
        template: "gotoAndPlay ( frame );",
        selection: EscChordSelection::Range(14, 19),
    },
    // 16. gs -> gotoAndStop ( frame );
    EscChordSpec {
        keys: ['g', 's'],
        summary: "gotoAndStop",
        template: "gotoAndStop ( frame );",
        selection: EscChordSelection::Range(14, 19),
    },
    // 17. if -> if ( condition ) {\n}
    EscChordSpec {
        keys: ['i', 'f'],
        summary: "if",
        template: "if ( condition ) {\n}",
        selection: EscChordSelection::Range(5, 14),
    },
    // 18. il -> ifFrameLoaded ( frame ) {\n\t\n}
    EscChordSpec {
        keys: ['i', 'l'],
        summary: "ifFrameLoaded",
        template: "ifFrameLoaded ( frame ) {\n\t\n}",
        selection: EscChordSelection::Range(16, 21),
    },
    // 19. in -> #include "path"
    EscChordSpec {
        keys: ['i', 'n'],
        summary: "#include",
        template: "#include \"path\"",
        selection: EscChordSelection::Range(10, 14),
    },
    // 20. lm -> loadMovie ( url, target, method );
    EscChordSpec {
        keys: ['l', 'm'],
        summary: "loadMovie",
        template: "loadMovie ( url, target, method );",
        selection: EscChordSelection::Range(12, 15),
    },
    // 21. ln -> loadMovieNum ( url, level, method );
    EscChordSpec {
        keys: ['l', 'n'],
        summary: "loadMovieNum",
        template: "loadMovieNum ( url, level, method );",
        selection: EscChordSelection::Range(15, 18),
    },
    // 22. lv -> loadVariables ( url, target, method );
    EscChordSpec {
        keys: ['l', 'v'],
        summary: "loadVariables",
        template: "loadVariables ( url, target, method );",
        selection: EscChordSelection::Range(16, 19),
    },
    // 23. vn -> loadVariablesNum ( url, level, method );
    EscChordSpec {
        keys: ['v', 'n'],
        summary: "loadVariablesNum",
        template: "loadVariablesNum ( url, level, method );",
        selection: EscChordSelection::Range(19, 22),
    },
    // 24. nf -> nextFrame ();
    EscChordSpec {
        keys: ['n', 'f'],
        summary: "nextFrame",
        template: "nextFrame ();",
        selection: EscChordSelection::End,
    },
    // 25. ns -> nextScene ();
    EscChordSpec {
        keys: ['n', 's'],
        summary: "nextScene",
        template: "nextScene ();",
        selection: EscChordSelection::End,
    },
    // 26. on -> on ( event ) {\n\t\n}
    EscChordSpec {
        keys: ['o', 'n'],
        summary: "on",
        template: "on ( event ) {\n\t\n}",
        selection: EscChordSelection::Range(5, 10),
    },
    // 27. oc -> onClipEvent ( event ) {\n}
    EscChordSpec {
        keys: ['o', 'c'],
        summary: "onClipEvent",
        template: "onClipEvent ( event ) {\n}",
        selection: EscChordSelection::Range(14, 19),
    },
    // 28. pl -> play ( );
    EscChordSpec {
        keys: ['p', 'l'],
        summary: "play",
        template: "play ( );",
        selection: EscChordSelection::End,
    },
    // 29. pf -> prevFrame ( );
    EscChordSpec {
        keys: ['p', 'f'],
        summary: "prevFrame",
        template: "prevFrame ( );",
        selection: EscChordSelection::End,
    },
    // 30. ps -> prevScene ( );
    EscChordSpec {
        keys: ['p', 's'],
        summary: "prevScene",
        template: "prevScene ( );",
        selection: EscChordSelection::End,
    },
    // 31. pr -> print ( target, type );
    EscChordSpec {
        keys: ['p', 'r'],
        summary: "print",
        template: "print ( target, type );",
        selection: EscChordSelection::Range(8, 14),
    },
    // 32. pb -> printAsBitmap ( target, type );
    EscChordSpec {
        keys: ['p', 'b'],
        summary: "printAsBitmap",
        template: "printAsBitmap ( target, type );",
        selection: EscChordSelection::Range(16, 22),
    },
    // 33. bn -> printAsBitmapNum ( level, type );
    EscChordSpec {
        keys: ['b', 'n'],
        summary: "printAsBitmapNum",
        template: "printAsBitmapNum ( level, type );",
        selection: EscChordSelection::Range(19, 24),
    },
    // 34. pn -> printNum ( level, type );
    EscChordSpec {
        keys: ['p', 'n'],
        summary: "printNum",
        template: "printNum ( level, type );",
        selection: EscChordSelection::Range(11, 16),
    },
    // 35. rm -> removeMovieClip ( target );
    EscChordSpec {
        keys: ['r', 'm'],
        summary: "removeMovieClip",
        template: "removeMovieClip ( target );",
        selection: EscChordSelection::Range(18, 24),
    },
    // 36. rt -> return ( );
    EscChordSpec {
        keys: ['r', 't'],
        summary: "return",
        template: "return ( );",
        selection: EscChordSelection::CaretAt(8),
    },
    // 37. sv -> set ( name, value );
    EscChordSpec {
        keys: ['s', 'v'],
        summary: "set",
        template: "set ( name, value );",
        selection: EscChordSelection::Range(6, 10),
    },
    // 38. sp -> setProperty ( target, property, value );
    EscChordSpec {
        keys: ['s', 'p'],
        summary: "setProperty",
        template: "setProperty ( target, property, value );",
        selection: EscChordSelection::Range(14, 20),
    },
    // 39. dr -> startDrag ( target, lockcenter, l, t, r, b );
    EscChordSpec {
        keys: ['d', 'r'],
        summary: "startDrag",
        template: "startDrag ( target, lockcenter, l, t, r, b );",
        selection: EscChordSelection::Range(12, 18),
    },
    // 40. st -> stop ( );
    EscChordSpec {
        keys: ['s', 't'],
        summary: "stop",
        template: "stop ( );",
        selection: EscChordSelection::End,
    },
    // 41. ss -> stopAllSounds ( );
    EscChordSpec {
        keys: ['s', 's'],
        summary: "stopAllSounds",
        template: "stopAllSounds ( );",
        selection: EscChordSelection::End,
    },
    // 42. sd -> stopDrag ( );
    EscChordSpec {
        keys: ['s', 'd'],
        summary: "stopDrag",
        template: "stopDrag ( );",
        selection: EscChordSelection::End,
    },
    // 43. tt -> tellTarget ( target ) {\n}
    EscChordSpec {
        keys: ['t', 't'],
        summary: "tellTarget",
        template: "tellTarget ( target ) {\n}",
        selection: EscChordSelection::Range(13, 19),
    },
    // 44. tq -> toggleHighQuality ( );
    EscChordSpec {
        keys: ['t', 'q'],
        summary: "toggleHighQuality",
        template: "toggleHighQuality ( );",
        selection: EscChordSelection::End,
    },
    // 45. tr -> trace ( message );
    EscChordSpec {
        keys: ['t', 'r'],
        summary: "trace",
        template: "trace ( message );",
        selection: EscChordSelection::Range(8, 15),
    },
    // 46. um -> unloadMovie ( target );
    EscChordSpec {
        keys: ['u', 'm'],
        summary: "unloadMovie",
        template: "unloadMovie ( target );",
        selection: EscChordSelection::Range(14, 20),
    },
    // 47. un -> unloadMovieNum ( level );
    EscChordSpec {
        keys: ['u', 'n'],
        summary: "unloadMovieNum",
        template: "unloadMovieNum ( level );",
        selection: EscChordSelection::Range(17, 22),
    },
    // 48. vr -> var ;
    EscChordSpec {
        keys: ['v', 'r'],
        summary: "var",
        template: "var ;",
        selection: EscChordSelection::CaretAt(4),
    },
    // 49. wh -> while ( condition ) {\n}
    EscChordSpec {
        keys: ['w', 'h'],
        summary: "while",
        template: "while ( condition ) {\n}",
        selection: EscChordSelection::Range(8, 17),
    },
    // 50. wt -> with ( object ) {\n}
    EscChordSpec {
        keys: ['w', 't'],
        summary: "with",
        template: "with ( object ) {\n}",
        selection: EscChordSelection::Range(7, 13),
    },
];

pub(crate) fn normalize_esc_chord_char(ch: char) -> char {
    ch.to_ascii_lowercase()
}

pub(crate) fn is_esc_chord_first_char(c1: char) -> bool {
    let norm = normalize_esc_chord_char(c1);
    ESC_CHORDS.iter().any(|chord| chord.keys[0] == norm)
}

pub(crate) fn find_esc_chord(c1: char, c2: char) -> Option<&'static EscChordSpec> {
    let pair = [normalize_esc_chord_char(c1), normalize_esc_chord_char(c2)];
    ESC_CHORDS.iter().find(|chord| chord.keys == pair)
}

pub(crate) fn esc_chord_first_char_prompt(c1: char) -> Option<String> {
    let norm = normalize_esc_chord_char(c1);
    let mut matches: Vec<&EscChordSpec> = ESC_CHORDS
        .iter()
        .filter(|chord| chord.keys[0] == norm)
        .collect();
    if matches.is_empty() {
        return None;
    }
    // Put `fn` first under `f_` so the status prompt matches Classic Flash muscle memory (`ESC f_: fn (function), fr (for), ...`).
    if norm == 'f'
        && let Some(pos) = matches.iter().position(|c| c.keys == ['f', 'n'])
    {
        let fn_spec = matches.remove(pos);
        matches.insert(0, fn_spec);
    }
    let items: Vec<String> = matches
        .into_iter()
        .map(|c| format!("{}{} ({})", c.keys[0], c.keys[1], c.summary))
        .collect();
    Some(format!("ESC {norm}_: {}", items.join(", ")))
}

/// Expand the Classic Flash `<ESC>` chord `(c1, c2)` at char `selection` inside `text`,
/// indenting subsequent lines of multi-line templates with the current line's leading whitespace
/// and returning the updated text plus the highlighted placeholder/caret char range.
#[cfg(test)]
pub(crate) fn expand_esc_chord(
    text: &str,
    selection: (usize, usize),
    c1: char,
    c2: char,
) -> Option<Edit> {
    expand_esc_chord_with_spec(text, selection, c1, c2).map(|(_, edit)| edit)
}

pub(crate) fn expand_esc_chord_with_spec(
    text: &str,
    selection: (usize, usize),
    c1: char,
    c2: char,
) -> Option<(&'static EscChordSpec, Edit)> {
    let chord = find_esc_chord(c1, c2)?;
    let total_chars = text.chars().count();
    let lo = selection.0.min(selection.1).min(total_chars);
    let hi = selection.0.max(selection.1).min(total_chars);
    let from = char_to_byte(text, lo);
    let to = char_to_byte(text, hi);

    let line_start = text[..from].rfind('\n').map_or(0, |idx| idx + 1);
    let line_indent: String = text[line_start..from]
        .chars()
        .take_while(|&c| c == ' ' || c == '\t')
        .collect();
    let indent_chars = line_indent.chars().count();

    let indented = if line_indent.is_empty() || !chord.template.contains('\n') {
        chord.template.to_owned()
    } else {
        let mut built = String::with_capacity(chord.template.len() + line_indent.len() * 2);
        for (idx, part) in chord.template.split('\n').enumerate() {
            if idx > 0 {
                built.push('\n');
                built.push_str(&line_indent);
            }
            built.push_str(part);
        }
        built
    };

    let (rel_start, rel_end) = match chord.selection {
        EscChordSelection::End => {
            let len = indented.chars().count();
            (len, len)
        }
        EscChordSelection::CaretAt(raw_pos) => {
            let nl_before = chord
                .template
                .chars()
                .take(raw_pos)
                .filter(|&c| c == '\n')
                .count();
            let pos = raw_pos + nl_before * indent_chars;
            (pos, pos)
        }
        EscChordSelection::Range(raw_start, raw_end) => {
            let nl_before = chord
                .template
                .chars()
                .take(raw_start)
                .filter(|&c| c == '\n')
                .count();
            let shift = nl_before * indent_chars;
            (raw_start + shift, raw_end + shift)
        }
    };

    let out = format!("{}{}{}", &text[..from], indented, &text[to..]);
    Some((chord, (out, (lo + rel_start, lo + rel_end))))
}

/// Interactive state machine for `<ESC>` + two-key quick-typing shortcuts in a code editor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum EscChordPhase {
    #[default]
    Idle,
    Armed {
        armed_at: f64,
    },
    FirstChar {
        first: char,
        armed_at: f64,
    },
}

impl EscChordPhase {
    pub(crate) fn armed_at(self) -> Option<f64> {
        match self {
            Self::Idle => None,
            Self::Armed { armed_at } | Self::FirstChar { armed_at, .. } => Some(armed_at),
        }
    }
}

pub(crate) struct EscChordStepOutcome {
    pub edit: Option<Edit>,
    pub status: Option<String>,
}

fn is_esc_chord_disarm_key(key: eframe::egui::Key) -> bool {
    use eframe::egui::Key;
    matches!(
        key,
        Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Backspace
            | Key::Delete
            | Key::Enter
            | Key::Tab
            | Key::Insert
    )
}

fn key_to_fallback_chord_char(
    key: eframe::egui::Key,
    modifiers: eframe::egui::Modifiers,
) -> Option<char> {
    use eframe::egui::Key;
    if modifiers.command || modifiers.ctrl || modifiers.alt {
        return None;
    }
    match key {
        Key::A => Some('a'),
        Key::B => Some('b'),
        Key::C => Some('c'),
        Key::D => Some('d'),
        Key::E => Some('e'),
        Key::F => Some('f'),
        Key::G => Some('g'),
        Key::H => Some('h'),
        Key::I => Some('i'),
        Key::J => Some('j'),
        Key::K => Some('k'),
        Key::L => Some('l'),
        Key::M => Some('m'),
        Key::N => Some('n'),
        Key::O => Some('o'),
        Key::P => Some('p'),
        Key::Q => Some('q'),
        Key::R => Some('r'),
        Key::S => Some('s'),
        Key::T => Some('t'),
        Key::U => Some('u'),
        Key::V => Some('v'),
        Key::W => Some('w'),
        Key::X => Some('x'),
        Key::Y => Some('y'),
        Key::Z => Some('z'),
        Key::Slash if !modifiers.shift => Some('/'),
        _ => None,
    }
}

/// Intercept `<ESC>` and subsequent chord characters in `ctx.input_mut` before `egui::TextEdit` runs.
pub(crate) fn step_esc_chord_input(
    phase: &mut EscChordPhase,
    ctx: &eframe::egui::Context,
    source: &str,
    selection: (usize, usize),
) -> EscChordStepOutcome {
    use eframe::egui;

    let now = ctx.input(|i| i.time);
    if let Some(armed_at) = phase.armed_at()
        && now - armed_at > ESC_CHORD_TIMEOUT_SECS
    {
        *phase = EscChordPhase::Idle;
    }
    if *phase != EscChordPhase::Idle
        && ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_click())
    {
        *phase = EscChordPhase::Idle;
    }

    let mut cur_text = std::borrow::Cow::Borrowed(source);
    let mut cur_sel = (selection.0.min(selection.1), selection.0.max(selection.1));
    let mut did_edit = false;
    let mut status: Option<String> = None;

    let mut feed_char = |phase: &mut EscChordPhase, status: &mut Option<String>, ch: char| {
        let norm = normalize_esc_chord_char(ch);
        match *phase {
            EscChordPhase::Idle => {}
            EscChordPhase::Armed { armed_at } => {
                if is_esc_chord_first_char(norm) {
                    *phase = EscChordPhase::FirstChar {
                        first: norm,
                        armed_at,
                    };
                    *status = esc_chord_first_char_prompt(norm);
                } else {
                    *phase = EscChordPhase::Idle;
                    *status = Some(ESC_CHORD_CANCELLED_STATUS.to_owned());
                }
            }
            EscChordPhase::FirstChar { first, .. } => {
                *phase = EscChordPhase::Idle;
                if let Some((chord, (new_text, new_sel))) =
                    expand_esc_chord_with_spec(&cur_text, cur_sel, first, norm)
                {
                    cur_text = std::borrow::Cow::Owned(new_text);
                    cur_sel = new_sel;
                    did_edit = true;
                    *status = Some(format!(
                        "Inserted ESC+{}{}: {}",
                        chord.keys[0], chord.keys[1], chord.summary
                    ));
                } else {
                    *status = Some(ESC_CHORD_CANCELLED_STATUS.to_owned());
                }
            }
        }
    };

    ctx.input_mut(|input| {
        let old_events = std::mem::take(&mut input.events);
        let len = old_events.len();
        let mut has_text_after = vec![false; len];
        let mut seen_text = false;
        for idx in (0..len).rev() {
            has_text_after[idx] = seen_text;
            if matches!(&old_events[idx], egui::Event::Text(t) if !t.is_empty()) {
                seen_text = true;
            }
        }
        let mut kept = Vec::with_capacity(len);

        for (idx, event) in old_events.into_iter().enumerate() {
            match event {
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if *phase == EscChordPhase::Idle {
                        if modifiers.is_none() {
                            *phase = EscChordPhase::Armed { armed_at: now };
                            status = Some(ESC_CHORD_ARMED_PROMPT.to_owned());
                        } else {
                            kept.push(event);
                        }
                    } else {
                        *phase = EscChordPhase::Idle;
                        status = Some(ESC_CHORD_CANCELLED_STATUS.to_owned());
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if *phase != EscChordPhase::Idle => {
                    if modifiers.command
                        || modifiers.ctrl
                        || modifiers.alt
                        || is_esc_chord_disarm_key(key)
                    {
                        *phase = EscChordPhase::Idle;
                        status = Some(ESC_CHORD_CANCELLED_STATUS.to_owned());
                        kept.push(event);
                    } else if !has_text_after[idx] {
                        if let Some(ch) = key_to_fallback_chord_char(key, modifiers) {
                            feed_char(phase, &mut status, ch);
                        } else {
                            *phase = EscChordPhase::Idle;
                            status = Some(ESC_CHORD_CANCELLED_STATUS.to_owned());
                        }
                    }
                }
                egui::Event::Text(text) if *phase != EscChordPhase::Idle && !text.is_empty() => {
                    let mut chars = text.chars();
                    let mut forwarded = String::new();
                    while let Some(ch) = chars.next() {
                        if *phase == EscChordPhase::Idle {
                            forwarded.push(ch);
                            continue;
                        }
                        if matches!(ch, '\n' | '\r' | '\t') {
                            *phase = EscChordPhase::Idle;
                            status = Some(ESC_CHORD_CANCELLED_STATUS.to_owned());
                            forwarded.push(ch);
                            forwarded.extend(chars);
                            break;
                        }
                        feed_char(phase, &mut status, ch);
                    }
                    if !forwarded.is_empty() {
                        kept.push(egui::Event::Text(forwarded));
                    }
                }
                egui::Event::Paste(_) | egui::Event::Cut | egui::Event::Copy
                    if *phase != EscChordPhase::Idle =>
                {
                    *phase = EscChordPhase::Idle;
                    kept.push(event);
                }
                _ => kept.push(event),
            }
        }
        input.events = kept;
    });

    if let Some(armed_at) = phase.armed_at() {
        let remaining = (ESC_CHORD_TIMEOUT_SECS - (now - armed_at)).max(0.05);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
    }

    EscChordStepOutcome {
        edit: did_edit.then(|| (cur_text.into_owned(), cur_sel)),
        status,
    }
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

    #[test]
    fn all_50_classic_flash_esc_chords_match_reference_table_indentation_and_placeholders() {
        let expected_table: [(&str, &str, Option<&str>); 50] = [
            ("br", "break;", None),
            ("ca", "call ( );", None),
            ("//", "// ", None),
            ("co", "continue;", None),
            ("da", "delete ( );", None),
            ("do", "do\n{\n} while ( condition );", Some("condition")),
            (
                "dm",
                "duplicateMovieClip ( target, newname, depth );",
                Some("target"),
            ),
            ("el", "} else {", None),
            ("ei", "} else if ( condition ) {", Some("condition")),
            ("fr", "for ( init; condition; next ) {\n}", Some("init")),
            ("fi", "for ( iterator in object ) {\n}", Some("iterator")),
            ("fs", "fscommand ( command, arguments );", Some("command")),
            ("fn", "function name ( ) {\n\t\n}", Some("name")),
            ("gu", "getURL ( url, window, method );", Some("url")),
            ("gp", "gotoAndPlay ( frame );", Some("frame")),
            ("gs", "gotoAndStop ( frame );", Some("frame")),
            ("if", "if ( condition ) {\n}", Some("condition")),
            ("il", "ifFrameLoaded ( frame ) {\n\t\n}", Some("frame")),
            ("in", "#include \"path\"", Some("path")),
            ("lm", "loadMovie ( url, target, method );", Some("url")),
            ("ln", "loadMovieNum ( url, level, method );", Some("url")),
            ("lv", "loadVariables ( url, target, method );", Some("url")),
            (
                "vn",
                "loadVariablesNum ( url, level, method );",
                Some("url"),
            ),
            ("nf", "nextFrame ();", None),
            ("ns", "nextScene ();", None),
            ("on", "on ( event ) {\n\t\n}", Some("event")),
            ("oc", "onClipEvent ( event ) {\n}", Some("event")),
            ("pl", "play ( );", None),
            ("pf", "prevFrame ( );", None),
            ("ps", "prevScene ( );", None),
            ("pr", "print ( target, type );", Some("target")),
            ("pb", "printAsBitmap ( target, type );", Some("target")),
            ("bn", "printAsBitmapNum ( level, type );", Some("level")),
            ("pn", "printNum ( level, type );", Some("level")),
            ("rm", "removeMovieClip ( target );", Some("target")),
            ("rt", "return ( );", None),
            ("sv", "set ( name, value );", Some("name")),
            (
                "sp",
                "setProperty ( target, property, value );",
                Some("target"),
            ),
            (
                "dr",
                "startDrag ( target, lockcenter, l, t, r, b );",
                Some("target"),
            ),
            ("st", "stop ( );", None),
            ("ss", "stopAllSounds ( );", None),
            ("sd", "stopDrag ( );", None),
            ("tt", "tellTarget ( target ) {\n}", Some("target")),
            ("tq", "toggleHighQuality ( );", None),
            ("tr", "trace ( message );", Some("message")),
            ("um", "unloadMovie ( target );", Some("target")),
            ("un", "unloadMovieNum ( level );", Some("level")),
            ("vr", "var ;", None),
            ("wh", "while ( condition ) {\n}", Some("condition")),
            ("wt", "with ( object ) {\n}", Some("object")),
        ];

        assert_eq!(ESC_CHORDS.len(), 50);
        let mut seen_keys = std::collections::HashSet::new();
        for (idx, &(keys_str, expected_tpl, expected_placeholder)) in
            expected_table.iter().enumerate()
        {
            let mut kc = keys_str.chars();
            let c1 = kc.next().unwrap();
            let c2 = kc.next().unwrap();
            assert!(kc.next().is_none());
            assert!(seen_keys.insert((c1, c2)), "duplicate chord key {keys_str}");
            assert!(is_esc_chord_first_char(c1));
            assert!(is_esc_chord_first_char(c1.to_ascii_uppercase()));

            let spec = &ESC_CHORDS[idx];
            assert_eq!(spec.keys, [c1, c2], "chord index {idx}");
            assert_eq!(
                spec.template, expected_tpl,
                "template mismatch for {keys_str}"
            );

            let (expanded, (s, e)) =
                expand_esc_chord("", (0, 0), c1, c2).expect("chord must expand");
            assert_eq!(
                expanded, expected_tpl,
                "unindented expansion for {keys_str}"
            );

            if let Some(placeholder) = expected_placeholder {
                let s_byte = char_to_byte(&expanded, s);
                let e_byte = char_to_byte(&expanded, e);
                assert_eq!(
                    &expanded[s_byte..e_byte],
                    placeholder,
                    "placeholder selection mismatch for {keys_str}"
                );
                // Ensure `fs` selects the parameter `command`, not `fs[command]`.
                if keys_str == "fs" {
                    assert!(s > "fscommand".len());
                }
            } else {
                assert_eq!(s, e, "expected collapsed caret for {keys_str}");
                match keys_str {
                    "vr" => {
                        assert_eq!(s, 4);
                        assert_eq!(&expanded[..s], "var ");
                        assert_eq!(&expanded[s..], ";");
                    }
                    "ca" | "da" | "rt" => {
                        let open_p = expanded.find('(').unwrap();
                        let close_p = expanded.find(')').unwrap();
                        assert!(
                            open_p < s && s < close_p,
                            "caret {s} should sit inside `( )` for {keys_str}"
                        );
                    }
                    _ => {
                        assert_eq!(s, expanded.chars().count(), "caret at end for {keys_str}");
                    }
                }
            }

            // Verify multi-line indentation and shifted placeholder selection on an indented line!
            let prefix = "    ";
            let (ind_out, (ind_s, ind_e)) = expand_esc_chord(
                prefix,
                (4, 4),
                c1.to_ascii_uppercase(),
                c2.to_ascii_uppercase(),
            )
            .expect("case-insensitive chord expansion");
            if expected_tpl.contains('\n') {
                for (line_idx, line) in ind_out.split('\n').enumerate() {
                    assert!(
                        line.starts_with(prefix),
                        "line {line_idx} of indented `{keys_str}` missing indent: {ind_out:?}"
                    );
                }
            }
            if let Some(placeholder) = expected_placeholder {
                let sb = char_to_byte(&ind_out, ind_s);
                let eb = char_to_byte(&ind_out, ind_e);
                assert_eq!(
                    &ind_out[sb..eb],
                    placeholder,
                    "indented placeholder mismatch for {keys_str}"
                );
            }
        }

        // Verify valid 1st chord character set matches exact 18 Classic Flash prefixes.
        for valid in [
            'b', 'c', '/', 'd', 'e', 'f', 'g', 'i', 'l', 'v', 'n', 'o', 'p', 'r', 's', 't', 'u',
            'w',
        ] {
            assert!(is_esc_chord_first_char(valid));
            assert!(esc_chord_first_char_prompt(valid).is_some());
        }
        for invalid in ['a', 'h', 'j', 'k', 'm', 'q', 'x', 'y', 'z', '1', ';'] {
            assert!(!is_esc_chord_first_char(invalid));
            assert!(esc_chord_first_char_prompt(invalid).is_none());
        }
        assert_eq!(
            esc_chord_first_char_prompt('f').as_deref(),
            Some("ESC f_: fn (function), fr (for), fi (for..in), fs (fscommand)")
        );
        assert!(expand_esc_chord("keep", (4, 4), 'f', 'z').is_none());
    }

    #[test]
    fn t373_auto_format_formats_screenshot_tetris_operators_delimiters_and_stays_idempotent() {
        let unformatted = "\
TFigure=new Array([0,1,0],[1,1,1]);
lFigure = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);
QFigure = new Array ([1,1],[1,1]);
LlFigure = new Array ([0,1],[0,1],[1,1]);
LrFigure = new Array ([1,0],[1,0],[1,1]);
ZlFigure = new Array ([0,1],[1,1],[1,0]);
ZrFigure = new Array ([1,0],[1,1],[0,1]);

AllFigures = new Array(TFigure, lFigure, QFigure, LlFigure, LrFigure,ZlFigure, ZrFigure);

function rotateFigure (figure, direction) {
    var n=figure.length;
    var m=figure[0].length;
    //trace (n,m);
    var tempFigure:Array=new Array();
    for(i=0;i<m;i++){
        tempFigure[i]=new Array();
    }
}";
        let expected = "\
TFigure = new Array([0, 1, 0], [1, 1, 1]);
lFigure = new Array([0, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0]);
QFigure = new Array([1, 1], [1, 1]);
LlFigure = new Array([0, 1], [0, 1], [1, 1]);
LrFigure = new Array([1, 0], [1, 0], [1, 1]);
ZlFigure = new Array([0, 1], [1, 1], [1, 0]);
ZrFigure = new Array([1, 0], [1, 1], [0, 1]);

AllFigures = new Array(TFigure, lFigure, QFigure, LlFigure, LrFigure, ZlFigure, ZrFigure);

function rotateFigure(figure, direction) {
    var n = figure.length;
    var m = figure[0].length;
    //trace (n,m);
    var tempFigure:Array = new Array();
    for (i = 0; i < m; i++) {
        tempFigure[i] = new Array();
    }
}";
        let formatted = auto_format(unformatted).expect("should format Tetris script");
        assert_eq!(formatted, expected);
        assert_eq!(
            auto_format(&formatted),
            None,
            "auto_format must be strictly idempotent"
        );
    }

    #[test]
    fn t373_auto_format_fixes_trailing_colon_typos_and_preserves_valid_colons_strings_regexes() {
        let src = "\
#include \"defs:init.as\"
TFigure = new Array([0, 1, 0]):
var x:Number = 1e-5 + 1e+5:
var y :  int = -x + ~0 - -2:
trace (x,y):
foo ():
var obj={a:1,b : 2,nested:{k:TFigure}};
var pick=ready?TFigure:lFigure;
var re=/a+b*c:1/gi;
var str=\"a=1,b:2\";
var anon = function (a: number, b?: string) {
    return (a + 1);
};
outerLoop:
for(i=0;i<10;i++){
    if(i<>5&&i<=8||i>>>1==2){
        switch(i){
        case 1 :
            x+=2:
            break:
        default :
            x>>>=1:
        }
    }else{
        continue:
    }
}";
        let formatted = auto_format(src).expect("should format complex script");
        let expected = "\
#include \"defs:init.as\"
TFigure = new Array([0, 1, 0]);
var x:Number = 1e-5 + 1e+5;
var y: int = -x + ~0 - -2;
trace(x, y);
foo();
var obj = { a: 1, b: 2, nested: { k: TFigure } };
var pick = ready ? TFigure : lFigure;
var re = /a+b*c:1/gi;
var str = \"a=1,b:2\";
var anon = function (a: number, b?: string) {
    return (a + 1);
};
outerLoop:
for (i = 0; i < 10; i++) {
    if (i <> 5 && i <= 8 || i >>> 1 == 2) {
        switch (i) {
            case 1:
                x += 2;
                break;
            default:
                x >>>= 1;
        }
    } else {
        continue;
    }
}";
        assert_eq!(formatted, expected);
        assert_eq!(
            auto_format(&formatted),
            None,
            "auto_format must be strictly idempotent on complex constructs"
        );
        // Incomplete/broken expressions with missing RHS must remain stable under format_on_save.
        assert_eq!(auto_format("var brokenCharacter = ;"), None);
    }

    #[test]
    fn t373_auto_format_handles_nested_case_expressions_block_scopes_member_calls_and_2d_generics()
    {
        let src = "\
function onFrame(e:Event):* {
    var a:Array = []:
    var g:Vector.<Vector.<int>> = new Vector.<Vector.<int>> ():
    let tsGrid: Array<Array<number> > = [];
    let o: { a: MyType, b?: Array<Array<number>> } = { a: item };
    promise.catch (errHandler);
    promise?.catch (errHandler);
    map.delete (key);
    try {
        delete (obj.prop):
    } catch(err) {
        trace (err):
    }
    outer: {
        var b:Array = [1,2]:
    }
    switch(mode){
    case (ready?1:2):
        a.push (1):
        break:
    case ready?3:4:
        a.push (2):
        break:
    case makeKey({ a:1, nested:{ k:2 } }): {
        var c:Array = [3,4]:
        break:
    }
    default:
        break:
    }
}
class Store {
    delete(key: string): boolean {
        return map.delete (key);
    }
}";
        let expected = "\
function onFrame(e:Event):* {
    var a:Array = [];
    var g:Vector.<Vector.<int>> = new Vector.<Vector.<int>>();
    let tsGrid: Array<Array<number>> = [];
    let o: { a: MyType, b?: Array<Array<number>> } = { a: item };
    promise.catch(errHandler);
    promise?.catch(errHandler);
    map.delete(key);
    try {
        delete (obj.prop);
    } catch (err) {
        trace(err);
    }
    outer: {
        var b:Array = [1, 2];
    }
    switch (mode) {
        case (ready ? 1 : 2):
            a.push(1);
            break;
        case ready ? 3 : 4:
            a.push(2);
            break;
        case makeKey({ a: 1, nested: { k: 2 } }): {
            var c:Array = [3, 4];
            break;
        }
        default:
            break;
    }
}
class Store {
    delete(key: string): boolean {
        return map.delete(key);
    }
}";
        let formatted = auto_format(src).expect("should format edge-case constructs");
        assert_eq!(formatted, expected);
        assert_eq!(
            auto_format(&formatted),
            None,
            "auto_format must remain strictly idempotent on nested cases, :* blocks, member calls, and 2D generics"
        );
    }
}
