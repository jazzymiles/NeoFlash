//! Diagnostic-only parsing. Recovery is never used to emit or execute a partial script.
use super::{CompileContext, Frontend, Kind, Lexer, Stmt, TokenCursor, completion};
use crate::{CompilerWarning, Diagnostic, DiagnosticSeverity};
use neoflash_core::{Script, ScriptLanguage};
use std::collections::BTreeSet;

fn frontend(
    script: &Script,
    tokens: TokenCursor,
    pos: usize,
    context: CompileContext,
) -> Frontend<'_> {
    Frontend {
        script,
        tokens,
        pos,
        context,
        depth: 0,
        controls: Vec::new(),
        function_depth: 0,
        next_function: 1,
        no_in: false,
        with_depth: 0,
        in_initclip: false,
    }
}

pub(crate) fn import_insertion(source: &str, language: ScriptLanguage) -> usize {
    let script = Script {
        source: source.to_owned(),
        language,
    };
    let mut lexer = Lexer {
        script: &script,
        offset: 0,
    };
    if matches!(lexer.next_token().map(|t| t.kind), Ok(Kind::Ident(word)) if word == "package") {
        while let Ok(token) = lexer.next_token() {
            match token.kind {
                Kind::Punct("{") => return token.end,
                Kind::End => break,
                _ => {}
            }
        }
    }
    0
}

/// Identifiers after the parser has consumed opaque regexp/XML literals. Source-unit classes
/// use the lexer fallback only when no context-sensitive literal could hide identifier text.
pub(crate) fn word_tokens(script: &Script) -> Vec<(String, usize, usize)> {
    let mut parser = frontend(script, TokenCursor::default(), 0, CompileContext::default());
    if parser.statements(false).is_ok() {
        return parser
            .tokens
            .buffer
            .borrow()
            .iter()
            .filter_map(|token| {
                if let Kind::Ident(word) = &token.kind {
                    Some((word.clone(), token.offset, token.end))
                } else {
                    None
                }
            })
            .collect();
    }
    let mut lexer = Lexer { script, offset: 0 };
    let mut words = Vec::new();
    for _ in 0..100_000 {
        let Ok(token) = lexer.next_token() else {
            return Vec::new();
        };
        match token.kind {
            Kind::Ident(word) => words.push((word, token.offset, token.end)),
            Kind::Punct("/" | "/=" | "<") => return Vec::new(),
            Kind::End => return words,
            _ => {}
        }
    }
    Vec::new()
}

pub(crate) fn local_variable_names(script: &Script) -> BTreeSet<String> {
    fn visit(statement: &Stmt, local: bool, names: &mut BTreeSet<String>) {
        let mut body = |body: &[Stmt], local| {
            for statement in body {
                visit(statement, local, names);
            }
        };
        match statement.unlocated() {
            Stmt::Var(vars) if local => names.extend(vars.iter().map(|(name, _, _)| name.clone())),
            Stmt::Function(f) | Stmt::LegacyEvent(_, f) => body(&f.body, true),
            Stmt::Block(items) | Stmt::InitClip(_, items, _) => body(items, local),
            Stmt::If(_, yes, no) => {
                visit(yes, local, names);
                if let Some(no) = no {
                    visit(no, local, names);
                }
            }
            Stmt::For(init, _, _, body) | Stmt::ForIn(init, _, _, body) => {
                visit(init, local, names);
                visit(body, local, names);
            }
            Stmt::While(_, body)
            | Stmt::DoWhile(body, _)
            | Stmt::With(_, body, _)
            | Stmt::TellTarget(_, body, _)
            | Stmt::IfFrameLoaded(_, body)
            | Stmt::Label(_, body) => visit(body, local, names),
            Stmt::Switch(_, cases) => {
                for case in cases {
                    body(&case.body, local);
                }
            }
            Stmt::Try(items, catches, finally) => {
                body(items, local);
                for catch in catches {
                    body(&catch.body, local);
                }
                if let Some(finally) = finally {
                    body(finally, local);
                }
            }
            _ => {}
        }
    }
    let mut parser = frontend(script, TokenCursor::default(), 0, CompileContext::default());
    let mut names = BTreeSet::new();
    if let Ok(body) = parser.statements(false) {
        for statement in body {
            visit(&statement, false, &mut names);
        }
    }
    names
}

/// Collect independent top-level syntax failures at balanced statement boundaries.
/// Semantic errors and errors inside incomplete compound statements retain the original failure.
pub(crate) fn collect_errors(script: &Script, first: Diagnostic) -> Vec<Diagnostic> {
    if first.code.as_deref() != Some("syntax") {
        return vec![first];
    }
    let mut parser = frontend(script, TokenCursor::default(), 0, CompileContext::default());
    let mut diagnostics = Vec::new();
    while parser.token().kind != Kind::End && diagnostics.len() < 32 {
        let start = parser.pos;
        let context = parser.context.clone();
        match parser.statement() {
            Ok(_) => {}
            Err(error) => {
                if diagnostics.is_empty()
                    && (error.line, error.column) != (first.line, first.column)
                {
                    return vec![first];
                }
                diagnostics.push(error);
                // Re-scan this statement using the compiler token cursor, not text scanning.
                // Nested syntax is discarded as one whole statement; no synthetic braces/expressions.
                let mut nesting = Vec::new();
                let mut pos = start;
                let next = loop {
                    let Some(token) = parser.peek(pos) else {
                        break None;
                    };
                    match token.kind {
                        Kind::End | Kind::Invalid(_) => break None,
                        Kind::Punct("(") => nesting.push(")"),
                        Kind::Punct("[") => nesting.push("]"),
                        Kind::Punct("{") => nesting.push("}"),
                        Kind::Punct(close @ (")" | "]" | "}")) => {
                            if nesting.pop() != Some(close) {
                                break None;
                            }
                        }
                        Kind::Punct(";") if nesting.is_empty() => break Some(pos + 1),
                        _ => {}
                    }
                    pos += 1;
                };
                let Some(next) = next else { break };
                parser = frontend(script, parser.tokens, next, context);
            }
        }
    }
    if diagnostics.is_empty() {
        vec![first]
    } else {
        diagnostics
    }
}

/// Reuse the compiler's completion analysis, so conditional returns and comments do not
/// create false unreachable warnings. Unsupported/incomplete parse trees produce no lint.
pub(crate) fn unreachable_warnings(script: &Script, path: Option<&str>) -> Vec<CompilerWarning> {
    let mut normalized = script.clone();
    if normalized.language == ScriptLanguage::ActionScript1 {
        normalized.language = ScriptLanguage::ActionScript2;
    }
    let mut parser = frontend(
        &normalized,
        TokenCursor::default(),
        0,
        CompileContext::default(),
    );
    let Ok(body) = parser.statements(false) else {
        return Vec::new();
    };
    let mut offsets = Vec::new();
    visit_body(&body, &mut offsets);
    offsets.sort_unstable();
    offsets.dedup();
    offsets
        .into_iter()
        .map(|offset| {
            let diagnostic = Diagnostic::at(
                script,
                offset,
                "Unreachable code after an unconditional exit.",
            );
            CompilerWarning {
                code: "unreachable-code".into(),
                severity: DiagnosticSeverity::Warning,
                language: script.language,
                path: path.map(str::to_owned),
                line: diagnostic.line,
                column: diagnostic.column,
                span: diagnostic.span,
                message: diagnostic.message,
                // Removing declarations can alter hoisting before the exit. Do not offer a deletion.
                quick_fixes: Vec::new(),
            }
        })
        .collect()
}

fn visit_body(body: &[Stmt], offsets: &mut Vec<usize>) {
    let mut terminated = false;
    for statement in body {
        if terminated && !matches!(statement.unlocated(), Stmt::Empty | Stmt::Function(_)) {
            if let Stmt::Located(offset, _) = statement {
                offsets.push(*offset);
            }
            // One warning per unreachable run; declarations/functions are still inspected below.
            break;
        }
        visit_statement(statement, offsets);
        terminated |= completion::never_falls_through(std::slice::from_ref(statement));
    }
}

fn visit_statement(statement: &Stmt, offsets: &mut Vec<usize>) {
    match statement.unlocated() {
        Stmt::Block(body) | Stmt::InitClip(_, body, _) => visit_body(body, offsets),
        Stmt::Function(function) | Stmt::LegacyEvent(_, function) => {
            visit_body(&function.body, offsets)
        }
        Stmt::If(_, yes, no) => {
            visit_statement(yes, offsets);
            if let Some(no) = no {
                visit_statement(no, offsets);
            }
        }
        Stmt::While(_, body)
        | Stmt::With(_, body, _)
        | Stmt::TellTarget(_, body, _)
        | Stmt::IfFrameLoaded(_, body)
        | Stmt::For(_, _, _, body)
        | Stmt::ForIn(_, _, _, body)
        | Stmt::DoWhile(body, _)
        | Stmt::Label(_, body) => visit_statement(body, offsets),
        Stmt::Switch(_, cases) => {
            for case in cases {
                visit_body(&case.body, offsets);
            }
        }
        Stmt::Try(body, catches, finally) => {
            visit_body(body, offsets);
            for catch in catches {
                visit_body(&catch.body, offsets);
            }
            if let Some(finally) = finally {
                visit_body(finally, offsets);
            }
        }
        _ => {}
    }
}
