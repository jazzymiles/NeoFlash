//! Read-only declaration analysis using the ActionScript compiler's lexer.
//!
//! This deliberately does not promise full type checking: inferred types are limited to explicit
//! annotations, constructors and literals. Invalid/incomplete tails keep their preceding declarations.
use super::{Kind, Lexer};
use neoflash_core::{Script, ScriptLanguage};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Variable,
    Parameter,
    Function,
    Property,
    Class,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSymbol {
    pub name: String,
    pub ty: String,
    /// Byte range of the declared identifier, exclusive at the end.
    pub range: Range<usize>,
    pub kind: SymbolKind,
    pub params: Option<String>,
    pub owner: Option<String>,
    pub private: bool,
    pub is_static: bool,
    scope: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct SourceClass {
    pub qualified: String,
    pub is_interface: bool,
    pub parent: Option<String>,
    body: Range<usize>,
}

#[derive(Clone, Debug)]
struct Lexeme {
    text: String,
    range: Range<usize>,
    literal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionKind {
    Comment,
    String,
}

/// Literal/comment regions from the same lexer as compilation. Trivia is read only between tokens.
pub(crate) fn regions(source: &str) -> Vec<(Range<usize>, RegionKind)> {
    lex(&Script {
        language: ScriptLanguage::ActionScript3,
        source: source.into(),
    })
    .1
}

fn trivia(source: &str, range: Range<usize>, out: &mut Vec<(Range<usize>, RegionKind)>) {
    let mut at = range.start;
    while at < range.end {
        if source[at..].starts_with("//") {
            let end = source[at..range.end]
                .find(['\n', '\r'])
                .map_or(range.end, |n| at + n);
            out.push((at..end, RegionKind::Comment));
            at = end;
        } else if source[at..].starts_with("/*") {
            let end = source[at + 2..range.end]
                .find("*/")
                .map_or(range.end, |n| at + 2 + n + 2);
            out.push((at..end, RegionKind::Comment));
            at = end;
        } else {
            at += source[at..].chars().next().unwrap().len_utf8();
        }
    }
}

fn lex(script: &Script) -> (Vec<Lexeme>, Vec<(Range<usize>, RegionKind)>) {
    let mut lexer = Lexer { script, offset: 0 };
    let mut tokens = Vec::new();
    let mut regions = Vec::new();
    // Share the compiler's input/token bounds rather than allocating unbounded editor work.
    if script.source.len() > 1_048_576 {
        return (tokens, regions);
    }
    for _ in 0..100_000 {
        let before = lexer.offset;
        match lexer.next_token() {
            Ok(token) => {
                trivia(&script.source, before..token.offset, &mut regions);
                if matches!(token.kind, Kind::End) {
                    break;
                }
                let literal = matches!(token.kind, Kind::String(_) | Kind::Number(_));
                if matches!(token.kind, Kind::String(_)) {
                    regions.push((token.offset..token.end, RegionKind::String));
                }
                tokens.push(Lexeme {
                    text: script.source[token.offset..token.end].into(),
                    range: token.offset..token.end,
                    literal,
                });
            }
            Err(error) => {
                let start =
                    crate::line_col_to_byte_offset(&script.source, error.line, error.column);
                trivia(&script.source, before..start.max(before), &mut regions);
                if script.source[start..].starts_with("/*")
                    || script.source[start..].starts_with("//")
                {
                    regions.push((start..script.source.len(), RegionKind::Comment));
                } else if script.source[start..].starts_with(['\'', '"']) {
                    regions.push((start..script.source.len(), RegionKind::String));
                }
                break;
            }
        }
    }
    (tokens, regions)
}

/// A snapshot of a script. Positions accepted by lookup methods are Unicode character offsets;
/// returned declaration ranges are byte ranges suitable for edits and precise navigation.
#[derive(Clone, Debug)]
pub struct Analysis {
    source: String,
    symbols: Vec<SourceSymbol>,
    classes: Vec<SourceClass>,
    tokens: Vec<Lexeme>,
}

impl Analysis {
    pub fn new(script: &Script) -> Self {
        let (tokens, _) = lex(script);
        let mut this = Self {
            source: script.source.clone(),
            symbols: Vec::new(),
            classes: Vec::new(),
            tokens,
        };
        this.collect();
        this
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn symbols(&self) -> &[SourceSymbol] {
        &self.symbols
    }
    pub fn classes(&self) -> &[SourceClass] {
        &self.classes
    }
    fn byte(&self, offset_chars: usize) -> usize {
        self.source
            .char_indices()
            .nth(offset_chars)
            .map_or(self.source.len(), |(i, _)| i)
    }
    pub fn visible_symbols(&self, offset_chars: usize) -> Vec<&SourceSymbol> {
        let at = self.byte(offset_chars);
        let mut found: Vec<_> = self
            .symbols
            .iter()
            .filter(|s| {
                s.owner.is_none()
                    && s.scope.start <= at
                    && at <= s.scope.end
                    && (s.range.start <= at || s.kind == SymbolKind::Function)
            })
            .collect();
        found.sort_by_key(|s| {
            (
                s.scope.end - s.scope.start,
                std::cmp::Reverse(s.range.start),
            )
        });
        let mut names = std::collections::HashSet::new();
        found.retain(|s| names.insert(s.name.as_str()));
        found
    }
    pub fn resolve(&self, name: &str, offset_chars: usize) -> Option<&SourceSymbol> {
        self.visible_symbols(offset_chars)
            .into_iter()
            .find(|s| s.name == name)
    }
    pub fn definition(&self, offset_chars: usize) -> Option<Range<usize>> {
        let at = self.byte(offset_chars);
        let token = self
            .tokens
            .iter()
            .find(|t| t.range.start <= at && at < t.range.end)?;
        if token.literal {
            return None;
        }
        self.resolve(&token.text, offset_chars)
            .map(|s| s.range.clone())
    }
    pub fn hover(&self, offset_chars: usize) -> Option<String> {
        let at = self.byte(offset_chars);
        let token = self
            .tokens
            .iter()
            .find(|t| t.range.start <= at && at < t.range.end)?;
        let symbol = self.resolve(&token.text, offset_chars)?;
        Some(match &symbol.params {
            Some(params) => format!("{}({params}): {}", symbol.name, symbol.ty),
            None => format!("{}: {}", symbol.name, symbol.ty),
        })
    }
    pub(crate) fn imports(&self) -> Vec<String> {
        self.import_statements()
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }
    pub(crate) fn import_statements(&self) -> Vec<(String, Range<usize>)> {
        let mut imports = Vec::new();
        for i in 0..self.tokens.len() {
            if self.text(i) != "import" || (i > 0 && self.text(i - 1) == ".") {
                continue;
            }
            let mut j = i + 1;
            let mut name = String::new();
            while j < self.tokens.len()
                && (is_ident(self.text(j)) || matches!(self.text(j), "." | "*"))
            {
                if j > i + 1
                    && self.source[self.tokens[j - 1].range.end..self.tokens[j].range.start]
                        .contains('\n')
                {
                    break;
                }
                name.push_str(self.text(j));
                j += 1;
            }
            if !name.is_empty() {
                let end = if self.text(j) == ";" {
                    self.tokens[j].range.end
                } else {
                    self.tokens[j - 1].range.end
                };
                imports.push((name, self.tokens[i].range.start..end));
            }
        }
        imports
    }
    pub(crate) fn package_name(&self) -> String {
        self.tokens
            .iter()
            .position(|token| token.text == "package")
            .map(|i| self.qualified(i + 1).0)
            .unwrap_or_default()
    }
    /// Conservative direct identifier.member uses only. Literal-heavy expression syntax is left
    /// to the full parser, so lints never mistake regexp/XML payload for an API reference.
    pub(crate) fn direct_member_uses(&self) -> Vec<(&str, &str, Range<usize>)> {
        if self
            .tokens
            .iter()
            .any(|t| matches!(t.text.as_str(), "/" | "<"))
        {
            return Vec::new();
        }
        (1..self.tokens.len().saturating_sub(1))
            .filter(|&i| {
                self.text(i) == "."
                    && is_ident(self.text(i - 1))
                    && is_ident(self.text(i + 1))
                    && (i < 2 || !matches!(self.text(i - 2), "." | "::"))
            })
            .map(|i| {
                (
                    self.text(i - 1),
                    self.text(i + 1),
                    self.tokens[i + 1].range.clone(),
                )
            })
            .collect()
    }

    fn text(&self, i: usize) -> &str {
        self.tokens.get(i).map_or("", |t| t.text.as_str())
    }
    fn qualified(&self, mut i: usize) -> (String, usize) {
        let mut value = String::new();
        while i < self.tokens.len() {
            let t = self.text(i);
            if t == "*" && value.is_empty() {
                return ("*".into(), i + 1);
            }
            if !is_ident(t) {
                break;
            }
            value.push_str(t);
            i += 1;
            if self.text(i) == "." && is_ident(self.text(i + 1)) {
                value.push('.');
                i += 1;
            } else {
                break;
            }
        }
        // Preserve Vector.<T> annotations for typed element completion.
        if self.text(i) == "." && self.text(i + 1) == "<" {
            let (element, next) = self.qualified(i + 2);
            if self.text(next) == ">" {
                value.push_str(&format!(".<{element}>"));
                i = next + 1;
            }
        }
        (value, i)
    }
    fn collect(&mut self) {
        let len = self.source.len();
        let mut pairs = vec![None; self.tokens.len()];
        let mut stack = Vec::new();
        for i in 0..self.tokens.len() {
            match self.text(i) {
                "(" | "[" | "{" => stack.push(i),
                ")" | "]" | "}" => {
                    if let Some(&open) = stack.last()
                        && matches!(
                            (self.text(open), self.text(i)),
                            ("(", ")") | ("[", "]") | ("{", "}")
                        )
                    {
                        stack.pop();
                        pairs[open] = Some(i);
                    }
                }
                _ => {}
            }
        }
        let package = (0..self.tokens.len())
            .find(|&i| self.text(i) == "package")
            .map(|i| self.qualified(i + 1).0)
            .unwrap_or_default();
        for i in 0..self.tokens.len() {
            if !matches!(self.text(i), "class" | "interface") {
                continue;
            }
            let (name, next) = self.qualified(i + 1);
            if name.is_empty() {
                continue;
            }
            let qualified = if package.is_empty() || name.contains('.') {
                name
            } else {
                format!("{package}.{name}")
            };
            let parent = (self.text(next) == "extends").then(|| self.qualified(next + 1).0);
            let Some(open) = (next..self.tokens.len()).find(|&j| matches!(self.text(j), "{" | ";"))
            else {
                continue;
            };
            if self.text(open) != "{" {
                continue;
            }
            let end = pairs[open].map_or(len, |j| self.tokens[j].range.end);
            self.classes.push(SourceClass {
                is_interface: self.text(i) == "interface",
                qualified,
                parent,
                body: self.tokens[open].range.start..end,
            });
        }
        let mut functions = Vec::<(usize, Range<usize>)>::new();
        for i in 0..self.tokens.len() {
            if self.text(i) != "function" {
                continue;
            }
            let mut name_index = i + 1;
            let accessor = matches!(self.text(name_index), "get" | "set");
            if accessor {
                name_index += 1;
            }
            let named = is_ident(self.text(name_index));
            let open = if named { name_index + 1 } else { name_index };
            if self.text(open) != "(" {
                continue;
            }
            let Some(close) = pairs[open] else {
                continue;
            };
            let (ty, after) = if self.text(close + 1) == ":" {
                self.qualified(close + 2)
            } else {
                (String::new(), close + 1)
            };
            if self.text(after) != "{" && self.text(after) != ";" {
                continue;
            }
            let body_end = if self.text(after) == "{" {
                pairs[after].map_or(len, |j| self.tokens[j].range.end)
            } else {
                self.tokens[after].range.end
            };
            let scope = self.tokens[open].range.start..body_end;
            functions.push((i, scope.clone()));
            // Function parameters are declarations in this function only, across line breaks.
            let mut p = open + 1;
            while p < close {
                if self.text(p) == "..." {
                    p += 1;
                }
                if !is_ident(self.text(p)) {
                    p += 1;
                    continue;
                }
                let index = p;
                p += 1;
                let param_ty = if self.text(p) == ":" {
                    let (t, next) = self.qualified(p + 1);
                    p = next;
                    t
                } else {
                    String::new()
                };
                self.symbols.push(SourceSymbol {
                    name: self.text(index).into(),
                    ty: param_ty,
                    range: self.tokens[index].range.clone(),
                    kind: SymbolKind::Parameter,
                    params: None,
                    owner: None,
                    private: false,
                    is_static: false,
                    scope: scope.clone(),
                });
                while p < close && self.text(p) != "," {
                    if let Some(end) = pairs[p] {
                        p = end + 1;
                    } else {
                        p += 1;
                    }
                }
                p += 1;
            }
            if named {
                let ty = if accessor && self.text(name_index - 1) == "set" {
                    self.symbols
                        .iter()
                        .find(|symbol| {
                            symbol.kind == SymbolKind::Parameter && symbol.scope == scope
                        })
                        .map_or(String::new(), |symbol| symbol.ty.clone())
                } else {
                    ty
                };
                let owner = self
                    .classes
                    .iter()
                    .filter(|c| c.body.contains(&self.tokens[i].range.start))
                    .min_by_key(|c| c.body.len())
                    .map(|c| c.qualified.clone());
                let modifiers = self.modifiers(i);
                let params = self.source
                    [self.tokens[open].range.end..self.tokens[close].range.start]
                    .trim()
                    .to_owned();
                self.symbols.push(SourceSymbol {
                    name: self.text(name_index).into(),
                    ty,
                    range: self.tokens[name_index].range.clone(),
                    kind: if accessor {
                        SymbolKind::Property
                    } else {
                        SymbolKind::Function
                    },
                    params: (!accessor).then_some(params),
                    owner,
                    private: modifiers.0,
                    is_static: modifiers.1,
                    scope: 0..len,
                });
            }
        }
        // Resolve function declaration owners after all nesting scopes are known.
        for s in &mut self.symbols {
            if matches!(s.kind, SymbolKind::Function | SymbolKind::Property)
                && let Some((_, scope)) = functions
                    .iter()
                    .filter(|(_, range)| range.contains(&s.range.start))
                    .min_by_key(|(_, r)| r.len())
            {
                s.scope = scope.clone();
                s.owner = None;
            }
        }
        for i in 0..self.tokens.len() {
            if !matches!(self.text(i), "var" | "let" | "const") {
                continue;
            }
            let at = self.tokens[i].range.start;
            let function = functions
                .iter()
                .filter(|(_, r)| r.contains(&at))
                .min_by_key(|(_, r)| r.len());
            let owner = if function.is_none() {
                self.classes
                    .iter()
                    .filter(|c| c.body.contains(&at))
                    .min_by_key(|c| c.body.len())
                    .map(|c| c.qualified.clone())
            } else {
                None
            };
            let scope = function.map_or(0..len, |(_, r)| r.clone());
            let modifiers = self.modifiers(i);
            let mut p = i + 1;
            loop {
                if !is_ident(self.text(p)) {
                    break;
                }
                let index = p;
                p += 1;
                let mut ty = if self.text(p) == ":" {
                    let (t, next) = self.qualified(p + 1);
                    p = next;
                    t
                } else {
                    String::new()
                };
                if self.text(p) == "=" && ty.is_empty() {
                    ty = self.initializer_type(p + 1);
                }
                self.symbols.push(SourceSymbol {
                    name: self.text(index).into(),
                    ty,
                    range: self.tokens[index].range.clone(),
                    kind: if owner.is_some() {
                        SymbolKind::Property
                    } else {
                        SymbolKind::Variable
                    },
                    params: None,
                    owner: owner.clone(),
                    private: modifiers.0,
                    is_static: modifiers.1,
                    scope: scope.clone(),
                });
                while p < self.tokens.len() && !matches!(self.text(p), "," | ";" | "}" | "in" | ")")
                {
                    if let Some(end) = pairs[p] {
                        p = end + 1;
                    } else {
                        p += 1;
                    }
                }
                if self.text(p) != "," {
                    break;
                }
                p += 1;
            }
        }
    }
    fn modifiers(&self, at: usize) -> (bool, bool) {
        let start = (0..at)
            .rev()
            .find(|&j| {
                !matches!(
                    self.text(j),
                    "public"
                        | "private"
                        | "protected"
                        | "internal"
                        | "static"
                        | "final"
                        | "override"
                        | "dynamic"
                        | "native"
                )
            })
            .map_or(0, |j| j + 1);
        (
            (start..at).any(|j| self.text(j) == "private"),
            (start..at).any(|j| self.text(j) == "static"),
        )
    }
    fn initializer_type(&self, i: usize) -> String {
        match self.text(i) {
            "new" => self.qualified(i + 1).0,
            "true" | "false" => "Boolean".into(),
            "[" => "Array".into(),
            t if t.starts_with(['\'', '"']) => "String".into(),
            t if t.chars().next().is_some_and(|c| c.is_ascii_digit()) => "Number".into(),
            _ => String::new(),
        }
    }
}
fn is_ident(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}
