use super::*;

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
                        | "**="
                        | "/="
                        | "%="
                        | "&="
                        | "|="
                        | "^="
                        | "**"
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

        // A token crossing the line start can only be a multiline comment, string or XML.
        let spanning_token = toks.get(tok_cursor).filter(|tok| tok.range.start < start);
        let first_token = tok_cursor + usize::from(spanning_token.is_some());
        let line_end = first_token
            + toks[first_token..]
                .iter()
                .take_while(|tok| tok.range.start < end)
                .count();
        let line_toks = &toks[first_token..line_end];
        // Compute structural depth delta contributed by `{ } ( ) [ ]` on this line.
        let mut line_delta = 0i32;
        let mut leading_close = 0i32;
        let mut seen_non_close = false;
        for tok in line_toks {
            if let FmtKind::Punct(p) = tok.kind {
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
            } else if tok.is_code() {
                seen_non_close = true;
            }
        }

        if let Some(tok) = spanning_token {
            let line = if line_toks.last().unwrap_or(tok).range.end >= end {
                // Preserve spaces and carriage returns inside the token.
                &text[start..end]
            } else {
                raw_line.trim_end_matches([' ', '\t'])
            };
            out_lines.push(line.to_owned());
            depth = (depth + line_delta).max(0);
            continue;
        }

        if line_toks.is_empty() || raw_line.trim().is_empty() {
            out_lines.push(String::new());
            continue;
        }

        // Format the tokens on this line into `formatted_body`.
        let mut formatted_body = String::with_capacity(raw_line.len() + 8);
        for (pos, tok) in line_toks.iter().enumerate() {
            if pos > 0 {
                let prev_tok = &line_toks[pos - 1];
                let had_space = prev_tok.range.end < tok.range.start;
                // Optional parameter/property `ident?: Type`: no space before `?` when followed immediately by `TypeColon`/`ObjectColon`.
                let is_optional_qmark = tok.kind == FmtKind::Punct("?")
                    && line_toks.get(pos + 1).is_some_and(|next| {
                        matches!(next.role, PunctRole::TypeColon | PunctRole::ObjectColon)
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
            if tok.range.end > end {
                // Multi-line token (`/* ...`, `` `... ``, `<a>...`) starts on this line and continues onto next line(s):
                // emit only the first-line slice up to `end`!
                formatted_body.push_str(&text[tok.range.start..end]);
            } else {
                formatted_body.push_str(tok.text(text));
            }
        }

        let first_code_tok = line_toks.iter().find(|tok| tok.is_code());
        let case_header_line = first_code_tok.is_some_and(|tok| {
            tok.kind == FmtKind::Ident && matches!(tok.text(text), "case" | "default")
        }) && line_toks.iter().any(|tok| tok.role == PunctRole::CaseColon);
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
