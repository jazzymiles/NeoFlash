//! ActionScript lexical scanner and on-demand token buffer.

use crate::Diagnostic;
use neoflash_core::Script;
use neoflash_core::model::ScriptLanguage;
use std::cell::{Cell, RefCell};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    Ident(String),
    Number(String),
    String(String),
    Punct(&'static str),
    End,
    // Rich diagnostics must not enlarge every successful token on recursive parser frames.
    Invalid(Box<Diagnostic>),
}

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub(super) kind: Kind,
    pub(super) offset: usize,
    pub(super) end: usize,
}

pub(super) struct Lexer<'a> {
    pub(super) script: &'a Script,
    pub(super) offset: usize,
}

impl<'a> Lexer<'a> {
    pub(super) fn rest(&self) -> &'a str {
        &self.script.source[self.offset..]
    }
    pub(super) fn bump(&mut self) -> Option<char> {
        let c = self.rest().chars().next()?;
        self.offset += c.len_utf8();
        Some(c)
    }
    pub(super) fn error(&self, offset: usize, message: impl Into<String>) -> Diagnostic {
        Diagnostic::at(self.script, offset, message)
    }
    pub(super) fn next_token(&mut self) -> Result<Token, Diagnostic> {
        while !self.rest().is_empty() {
            if self.rest().starts_with(char::is_whitespace) {
                self.bump();
                continue;
            }
            if self.rest().starts_with("//") {
                while self
                    .bump()
                    .is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
                {
                }
                continue;
            }
            if self.rest().starts_with("/*") {
                let start = self.offset;
                self.offset += 2;
                let Some(end) = self.rest().find("*/") else {
                    return Err(self.error(start, "Unterminated block comment."));
                };
                self.offset += end + 2;
                continue;
            }
            let start = self.offset;
            let c = self.bump().unwrap();
            let kind = if c.is_alphabetic() || c == '_' || c == '$' {
                while self
                    .rest()
                    .starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$')
                {
                    self.bump();
                }
                Kind::Ident(self.script.source[start..self.offset].into())
            } else if c.is_ascii_digit()
                || (c == '.' && self.rest().starts_with(|c: char| c.is_ascii_digit()))
            {
                if c == '0' && (self.rest().starts_with('x') || self.rest().starts_with('X')) {
                    self.bump();
                    let digits = self.offset;
                    while self.rest().starts_with(|c: char| c.is_ascii_hexdigit()) {
                        self.bump();
                    }
                    if self.offset == digits {
                        return Err(self.error(start, "Expected hexadecimal digits after 0x."));
                    }
                } else {
                    while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                        self.bump();
                    }
                    if c != '.' && self.rest().starts_with('.') {
                        self.bump();
                        while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                            self.bump();
                        }
                    }
                    if self.rest().starts_with(['e', 'E']) {
                        self.bump();
                        if self.rest().starts_with(['+', '-']) {
                            self.bump();
                        }
                        let digits = self.offset;
                        while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                            self.bump();
                        }
                        if self.offset == digits {
                            return Err(self.error(start, "Expected digits in numeric exponent."));
                        }
                    }
                }
                if self
                    .rest()
                    .starts_with(|c: char| c.is_alphabetic() || c == '_')
                {
                    return Err(self.error(start, "Invalid number literal."));
                }
                let literal = &self.script.source[start..self.offset];
                if literal.len() > 1
                    && literal.starts_with('0')
                    && literal.as_bytes()[1].is_ascii_digit()
                {
                    return Err(self.error(start, "Legacy leading-zero numeric literals are not supported; use decimal or 0x hexadecimal."));
                }
                Kind::Number(literal.into())
            } else if c == '\'' || c == '"' {
                let mut closed = false;
                while let Some(next) = self.bump() {
                    if next == c {
                        closed = true;
                        break;
                    }
                    if matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                        return Err(self.error(start, "Unterminated string literal."));
                    }
                    if next == '\\' && self.bump().is_none() {
                        break;
                    }
                }
                if !closed {
                    return Err(self.error(start, "Unterminated string literal."));
                }
                Kind::String(self.script.source[start..self.offset].into())
            } else if c == '#' && self.rest().starts_with(|ch: char| ch.is_ascii_alphabetic()) {
                while self.rest().starts_with(|ch: char| ch.is_ascii_alphabetic()) {
                    self.bump();
                }
                match &self.script.source[start..self.offset] {
                    "#initclip" => Kind::Punct("#initclip"),
                    "#endinitclip" => Kind::Punct("#endinitclip"),
                    "#include" => {
                        return Err(self.error(
                            start,
                            "#include requires compile_with_program and bundled .as project resources.",
                        ));
                    }
                    directive => {
                        return Err(self.error(
                            start,
                            format!(
                                "Unsupported compiler directive '{directive}' in ActionScript."
                            ),
                        ));
                    }
                }
            } else {
                self.offset = start;
                const OPERATORS: &[&str] = &[
                    ">>>=", "===", "!==", ">>>", "...", "<<=", ">>=", "&&=", "||=", "++", "--",
                    "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>",
                    "&=", "|=", "^=", "<>", "::", "..", "{", "}", "(", ")", "[", "]", ";", ",",
                    ".", ":", "?", "+", "-", "*", "/", "%", "=", "!", "~", "<", ">", "&", "|", "^",
                    "@",
                ];
                let Some(operator) = OPERATORS.iter().find(|operator| {
                    self.rest().starts_with(**operator)
                        && (**operator != "<>"
                            || self.script.language == ScriptLanguage::ActionScript2)
                }) else {
                    return Err(self.error(
                        start,
                        format!("Unsupported character {c:?} in ActionScript."),
                    ));
                };
                self.offset += operator.len();
                Kind::Punct(operator)
            };
            return Ok(Token {
                kind,
                offset: start,
                end: self.offset,
            });
        }
        Ok(Token {
            kind: Kind::End,
            offset: self.offset,
            end: self.offset,
        })
    }
}

// Buffered on demand: the parser decides when '<' starts an XML literal.
// XML content is never lexed as ActionScript ahead of that decision.
#[derive(Clone, Default)]
pub(super) struct TokenCursor {
    pub(super) buffer: RefCell<Vec<Token>>,
    pub(super) offset: Cell<usize>,
    pub(super) produced: Cell<usize>,
}
impl TokenCursor {
    pub(super) fn get(&self, script: &Script, index: usize) -> Option<Token> {
        let mut buffer = self.buffer.borrow_mut();
        while buffer.len() <= index {
            if buffer
                .last()
                .is_some_and(|t| matches!(t.kind, Kind::End | Kind::Invalid(_)))
            {
                return None;
            }
            let mut lexer = Lexer {
                script,
                offset: self.offset.get(),
            };
            let result = if self.produced.get() >= 100_000 {
                Err(Diagnostic::at(
                    script,
                    lexer.offset,
                    "Script exceeds the 100,000-token limit.",
                ))
            } else {
                lexer.next_token()
            };
            self.produced.set(self.produced.get() + 1);
            let token = result.unwrap_or_else(|error| Token {
                kind: Kind::Invalid(Box::new(error)),
                offset: lexer.offset,
                end: lexer.offset,
            });
            self.offset.set(lexer.offset);
            buffer.push(token);
        }
        buffer.get(index).cloned()
    }
    pub(super) fn split(&self, index: usize, kind: Kind) {
        let mut buffer = self.buffer.borrow_mut();
        buffer[index].kind = kind;
        buffer[index].offset += 1;
    }
    pub(super) fn jump(&self, index: usize, offset: usize) {
        self.buffer.borrow_mut().truncate(index);
        self.offset.set(offset);
    }
    pub(super) fn literal(&self, index: usize, start: usize, end: usize) {
        self.jump(index, end);
        self.buffer.borrow_mut().push(Token {
            kind: Kind::String(String::new()),
            offset: start,
            end,
        });
    }
}
