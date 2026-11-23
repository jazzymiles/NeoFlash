#[cfg(test)]
use super::auto_format_with_tab_size;

/// "Auto-format": re-indent and format statement/operator/delimiter spacing with 4 spaces.
/// Returns `None` when it is already formatted.
#[cfg(test)]
pub(crate) fn auto_format(text: &str) -> Option<String> {
    auto_format_with_tab_size(text, 4)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FmtKind {
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
pub(super) enum PunctRole {
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
pub(super) struct FmtTok {
    pub(super) range: std::ops::Range<usize>,
    pub(super) kind: FmtKind,
    pub(super) role: PunctRole,
}

impl FmtTok {
    pub(super) fn text<'a>(&self, src: &'a str) -> &'a str {
        if self.role == PunctRole::TrailingColonAsSemi {
            ";"
        } else {
            &src[self.range.clone()]
        }
    }

    pub(super) fn is_code(&self) -> bool {
        !matches!(
            self.kind,
            FmtKind::Newline | FmtKind::LineComment | FmtKind::BlockComment
        )
    }
}

pub(super) fn fmt_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

pub(super) fn fmt_ident_char(c: u8) -> bool {
    fmt_ident_start(c) || c.is_ascii_digit()
}

pub(super) const EXPR_PRECEDING_KEYWORDS: &[&str] = &[
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

pub(super) const SPACE_BEFORE_PAREN_KEYWORDS: &[&str] = &[
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

pub(super) const FMT_TYPE_KEYWORDS: &[&str] = &[
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

pub(super) const MULTI_PUNCTS: &[&str] = &[
    ">>>=", "===", "!==", ">>>", "<<=", ">>=", "&&=", "||=", "??=", "**=", "...", "==", "!=", "<>",
    "<=", ">=", "&&", "||", "??", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**",
    "++", "--", "=>", "?.", "::", "=", "+", "-", "*", "/", "%", "<", ">", "&", "|", "^", "!", "~",
    "?", ":", ",", ";", ".", "(", ")", "[", "]", "{", "}",
];

pub(super) fn can_precede_regex_or_unary(prev: Option<(&FmtKind, &str)>) -> bool {
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

pub(super) fn try_scan_regex(src: &str, start: usize) -> Option<usize> {
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

pub(super) fn try_scan_xml_literal(src: &str, start: usize) -> Option<usize> {
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

pub(super) fn lex_fmt_tokens(src: &str) -> Vec<FmtTok> {
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
                if b[i] == b'\\' && b.get(i + 1..i + 3) == Some(b"\r\n") {
                    i += 3;
                } else if b[i] == b'\\' {
                    i = (i + 2).min(n);
                } else if b[i] == b'\n' && quote != b'`' {
                    break;
                } else {
                    i += 1;
                }
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
pub(super) enum FmtScope {
    BlockBrace,
    ObjectBrace,
    TypeBrace,
    Paren,
    Bracket,
}

pub(super) fn find_generic_close_on_line(
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

pub(super) fn paren_followed_by_method_sig(toks: &[FmtTok], open_paren_idx: usize) -> bool {
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
