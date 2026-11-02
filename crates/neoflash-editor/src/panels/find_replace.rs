//! Find and replace modal dialog (`Dialog / Find and replace`, `DL-13`; reference
//! `document-dialogs.png`), opened by Edit ▸ Find and replace (`⌘F`).
//!
//! Supports all seven NF2 modes (`Text`, `Font`, `Colour`, `Symbol`, `Sound`, `Video`, `Bitmap`),
//! document/scene scoping, `Whole word`, `Match case`, `Regular expression` (built-in engine,
//! zero external dependencies), `Live edit`, target filters (`Text field contents`,
//! `Frames / layers / scenes`, `ActionScript`), result-table click navigation, `Replace`, and
//! `Replace all` in a single `"Find and replace"` Undo transaction.
use super::planned::caption;
use crate::app::Editor;
use crate::panels::actions_sidebar::ScriptLocation;
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::{Color, ElementKind, Id, ObjectEvent, Project};
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::space;
use std::collections::HashSet;

const DIALOG_WIDTH: f32 = 640.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FindKind {
    #[default]
    Text,
    Font,
    Colour,
    Symbol,
    Sound,
    Video,
    Bitmap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MatchTarget {
    TextElement {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
        range: (usize, usize),
    },
    SymbolName {
        symbol: Id,
        range: (usize, usize),
    },
    LayerName {
        symbol: Id,
        layer_id: Id,
        layer_index: usize,
        range: (usize, usize),
    },
    FrameLabel {
        symbol: Id,
        layer_id: Id,
        layer_index: usize,
        frame: u32,
        range: (usize, usize),
    },
    InstanceName {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
        range: (usize, usize),
    },
    FrameScript {
        symbol: Id,
        frame: u32,
        range: (usize, usize),
        line: usize,
        column: usize,
    },
    ObjectScript {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
        event: ObjectEvent,
        range: (usize, usize),
        line: usize,
        column: usize,
    },
    SourceScript {
        source_id: Id,
        range: (usize, usize),
        line: usize,
        column: usize,
    },
    FontElement {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
    },
    ColourSlot {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
        slot: ColourSlotKind,
    },
    SymbolInstance {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
    },
    FrameSound {
        symbol: Id,
        layer_id: Id,
        layer_index: usize,
        frame: u32,
    },
    VideoPlacement {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
    },
    BitmapPlacement {
        symbol: Id,
        layer_index: usize,
        frame: u32,
        element_id: Id,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColourSlotKind {
    Fill,
    Stroke,
    FillGradientStop(usize),
    StrokeGradientStop(usize),
    Text,
    TextBox,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FindHit {
    pub place_key: String,
    pub location_label: String,
    pub found_preview: String,
    pub target: MatchTarget,
}

#[derive(Clone, Debug)]
pub(crate) struct FindReplaceState {
    pub kind: FindKind,
    pub in_current_scene: bool,
    pub scene_filter: Option<Id>,
    pub find: String,
    pub replace: String,
    pub whole_word: bool,
    pub match_case: bool,
    pub regex: bool,
    pub live_edit: bool,
    pub in_text_fields: bool,
    pub in_names_labels: bool,
    pub in_actionscript: bool,
    pub results: Vec<FindHit>,
    pub selected: usize,
    pub focus_find: bool,
}

impl Default for FindReplaceState {
    fn default() -> Self {
        Self {
            kind: FindKind::Text,
            in_current_scene: false,
            scene_filter: None,
            find: String::new(),
            replace: String::new(),
            whole_word: false,
            match_case: false,
            regex: false,
            live_edit: false,
            in_text_fields: true,
            in_names_labels: true,
            in_actionscript: true,
            results: Vec::new(),
            selected: 0,
            focus_find: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Self-contained bounded regular-expression matcher (no external crate needed)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum RegexNode {
    Seq(Vec<RegexPiece>),
    Alt(Vec<RegexNode>),
}

#[derive(Clone, Debug)]
struct RegexPiece {
    atom: RegexAtom,
    min: usize,
    max: Option<usize>,
}

#[derive(Clone, Debug)]
enum RegexAtom {
    Any,
    Literal(char),
    AnchorStart,
    AnchorEnd,
    WordBoundary(bool),
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
    Group(Box<RegexNode>),
}

#[derive(Clone, Debug)]
enum ClassItem {
    Single(char),
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn chars_eq(a: char, b: char, match_case: bool) -> bool {
    if match_case {
        a == b
    } else if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(&b)
    } else {
        a.to_lowercase().eq(b.to_lowercase())
    }
}

impl ClassItem {
    fn matches(&self, ch: char, match_case: bool) -> bool {
        match *self {
            Self::Single(c) => chars_eq(ch, c, match_case),
            Self::Range(lo, hi) => {
                if match_case {
                    ch >= lo && ch <= hi
                } else {
                    let cl = ch.to_ascii_lowercase();
                    cl >= lo.to_ascii_lowercase() && cl <= hi.to_ascii_lowercase()
                }
            }
            Self::Digit(pos) => ch.is_ascii_digit() == pos,
            Self::Word(pos) => is_word_char(ch) == pos,
            Self::Space(pos) => ch.is_whitespace() == pos,
        }
    }
}

struct RegexParser<'a> {
    chars: Vec<char>,
    pos: usize,
    _src: &'a str,
}

impl<'a> RegexParser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
            _src: src,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    fn parse_alt(&mut self, in_group: bool) -> Result<RegexNode, ()> {
        let mut branches = vec![self.parse_seq(in_group)?];
        while self.peek() == Some('|') {
            self.bump();
            branches.push(self.parse_seq(in_group)?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(RegexNode::Alt(branches))
        }
    }

    fn parse_seq(&mut self, in_group: bool) -> Result<RegexNode, ()> {
        let mut pieces = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == '|' || (in_group && ch == ')') {
                break;
            }
            if !in_group && ch == ')' {
                return Err(());
            }
            let atom = self.parse_atom()?;
            let (min, max) = self.parse_quantifier()?;
            pieces.push(RegexPiece { atom, min, max });
        }
        Ok(RegexNode::Seq(pieces))
    }

    fn parse_atom(&mut self) -> Result<RegexAtom, ()> {
        let ch = self.bump().ok_or(())?;
        match ch {
            '.' => Ok(RegexAtom::Any),
            '^' => Ok(RegexAtom::AnchorStart),
            '$' => Ok(RegexAtom::AnchorEnd),
            '(' => {
                if self.peek() == Some('?') && self.chars.get(self.pos + 1) == Some(&':') {
                    self.pos += 2;
                }
                let inner = self.parse_alt(true)?;
                if self.bump() != Some(')') {
                    return Err(());
                }
                Ok(RegexAtom::Group(Box::new(inner)))
            }
            '[' => self.parse_class(),
            '\\' => {
                let esc = self.bump().ok_or(())?;
                match esc {
                    'b' => Ok(RegexAtom::WordBoundary(true)),
                    'B' => Ok(RegexAtom::WordBoundary(false)),
                    'd' => Ok(RegexAtom::Class {
                        negated: false,
                        items: vec![ClassItem::Digit(true)],
                    }),
                    'D' => Ok(RegexAtom::Class {
                        negated: false,
                        items: vec![ClassItem::Digit(false)],
                    }),
                    'w' => Ok(RegexAtom::Class {
                        negated: false,
                        items: vec![ClassItem::Word(true)],
                    }),
                    'W' => Ok(RegexAtom::Class {
                        negated: false,
                        items: vec![ClassItem::Word(false)],
                    }),
                    's' => Ok(RegexAtom::Class {
                        negated: false,
                        items: vec![ClassItem::Space(true)],
                    }),
                    'S' => Ok(RegexAtom::Class {
                        negated: false,
                        items: vec![ClassItem::Space(false)],
                    }),
                    'n' => Ok(RegexAtom::Literal('\n')),
                    'r' => Ok(RegexAtom::Literal('\r')),
                    't' => Ok(RegexAtom::Literal('\t')),
                    other => Ok(RegexAtom::Literal(other)),
                }
            }
            '*' | '+' | '?' => Err(()),
            other => Ok(RegexAtom::Literal(other)),
        }
    }

    fn parse_class(&mut self) -> Result<RegexAtom, ()> {
        let negated = if self.peek() == Some('^') {
            self.bump();
            true
        } else {
            false
        };
        let mut items = Vec::new();
        let mut first = true;
        while let Some(ch) = self.bump() {
            if ch == ']' && !first {
                return Ok(RegexAtom::Class { negated, items });
            }
            first = false;
            let item = if ch == '\\' {
                let esc = self.bump().ok_or(())?;
                match esc {
                    'd' => ClassItem::Digit(true),
                    'D' => ClassItem::Digit(false),
                    'w' => ClassItem::Word(true),
                    'W' => ClassItem::Word(false),
                    's' => ClassItem::Space(true),
                    'S' => ClassItem::Space(false),
                    'n' => ClassItem::Single('\n'),
                    'r' => ClassItem::Single('\r'),
                    't' => ClassItem::Single('\t'),
                    other => ClassItem::Single(other),
                }
            } else {
                ClassItem::Single(ch)
            };
            if let ClassItem::Single(lo) = item
                && self.peek() == Some('-')
                && self.chars.get(self.pos + 1).is_some_and(|&c| c != ']')
            {
                self.bump(); // '-'
                let hi_raw = self.bump().ok_or(())?;
                let hi = if hi_raw == '\\' {
                    self.bump().ok_or(())?
                } else {
                    hi_raw
                };
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(item);
            }
        }
        Err(())
    }

    fn parse_quantifier(&mut self) -> Result<(usize, Option<usize>), ()> {
        match self.peek() {
            Some('*') => {
                self.bump();
                Ok((0, None))
            }
            Some('+') => {
                self.bump();
                Ok((1, None))
            }
            Some('?') => {
                self.bump();
                Ok((0, Some(1)))
            }
            Some('{') => {
                let saved = self.pos;
                self.bump();
                let Some(min) = self.parse_number() else {
                    self.pos = saved;
                    return Ok((1, Some(1)));
                };
                let max = if self.peek() == Some(',') {
                    self.bump();
                    if self.peek() == Some('}') {
                        None
                    } else {
                        let Some(m) = self.parse_number() else {
                            self.pos = saved;
                            return Ok((1, Some(1)));
                        };
                        if m < min {
                            return Err(());
                        }
                        Some(m)
                    }
                } else {
                    Some(min)
                };
                if self.peek() == Some('}') {
                    self.bump();
                    Ok((min, max))
                } else {
                    self.pos = saved;
                    Ok((1, Some(1)))
                }
            }
            _ => Ok((1, Some(1))),
        }
    }

    fn parse_number(&mut self) -> Option<usize> {
        let mut n: usize = 0;
        let mut any = false;
        while let Some(ch) = self.peek().filter(|c| c.is_ascii_digit()) {
            self.bump();
            any = true;
            n = n
                .saturating_mul(10)
                .saturating_add((ch as usize) - ('0' as usize));
            if n > 1024 {
                return None;
            }
        }
        any.then_some(n)
    }
}

struct CompiledRegex {
    root: RegexNode,
    match_case: bool,
}

impl CompiledRegex {
    fn compile(pattern: &str, match_case: bool) -> Option<Self> {
        if pattern.is_empty() {
            return None;
        }
        let mut p = RegexParser::new(pattern);
        let root = p.parse_alt(false).ok()?;
        if p.pos != p.chars.len() {
            return None;
        }
        Some(Self { root, match_case })
    }

    fn find_all(&self, text: &str) -> Vec<(usize, usize)> {
        let indexed: Vec<(usize, char)> = text.char_indices().collect();
        let len = indexed.len();
        let byte_at = |ci: usize| if ci < len { indexed[ci].0 } else { text.len() };
        let mut matches = Vec::new();
        let mut ci = 0;
        while ci <= len {
            let mut budget = 20_000usize;
            if let Some(end_ci) = self.match_node(&self.root, &indexed, ci, &mut budget) {
                let b_start = byte_at(ci);
                let b_end = byte_at(end_ci);
                if b_end > b_start {
                    matches.push((b_start, b_end));
                    ci = end_ci;
                    continue;
                }
            }
            ci += 1;
        }
        matches
    }

    fn match_node(
        &self,
        node: &RegexNode,
        chars: &[(usize, char)],
        pos: usize,
        budget: &mut usize,
    ) -> Option<usize> {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        match node {
            RegexNode::Alt(branches) => {
                for b in branches {
                    if let Some(end) = self.match_node(b, chars, pos, budget) {
                        return Some(end);
                    }
                }
                None
            }
            RegexNode::Seq(pieces) => self.match_seq(pieces, chars, pos, budget),
        }
    }

    fn match_seq(
        &self,
        pieces: &[RegexPiece],
        chars: &[(usize, char)],
        pos: usize,
        budget: &mut usize,
    ) -> Option<usize> {
        let Some((first, rest)) = pieces.split_first() else {
            return Some(pos);
        };
        let mut positions = vec![pos];
        let mut count = 0usize;
        let max = first.max.unwrap_or(chars.len().saturating_sub(pos) + 1);
        while count < max {
            let cur = *positions.last().unwrap();
            let Some(next) = self.match_atom(&first.atom, chars, cur, budget) else {
                break;
            };
            count += 1;
            if next == cur && count > first.min {
                break;
            }
            positions.push(next);
        }
        if count < first.min {
            return None;
        }
        for idx in (first.min..=positions.len() - 1).rev() {
            if let Some(done) = self.match_seq(rest, chars, positions[idx], budget) {
                return Some(done);
            }
            if *budget == 0 {
                return None;
            }
        }
        None
    }

    fn match_atom(
        &self,
        atom: &RegexAtom,
        chars: &[(usize, char)],
        pos: usize,
        budget: &mut usize,
    ) -> Option<usize> {
        match atom {
            RegexAtom::Any => {
                let (_, ch) = *chars.get(pos)?;
                (ch != '\n').then_some(pos + 1)
            }
            RegexAtom::Literal(expected) => {
                let (_, ch) = *chars.get(pos)?;
                chars_eq(ch, *expected, self.match_case).then_some(pos + 1)
            }
            RegexAtom::AnchorStart => {
                let at_start = pos == 0 || chars.get(pos - 1).is_some_and(|(_, c)| *c == '\n');
                at_start.then_some(pos)
            }
            RegexAtom::AnchorEnd => {
                let at_end = pos == chars.len() || chars.get(pos).is_some_and(|(_, c)| *c == '\n');
                at_end.then_some(pos)
            }
            RegexAtom::WordBoundary(positive) => {
                let prev_w = pos > 0 && chars.get(pos - 1).is_some_and(|(_, c)| is_word_char(*c));
                let next_w = chars.get(pos).is_some_and(|(_, c)| is_word_char(*c));
                let is_boundary = prev_w != next_w;
                (is_boundary == *positive).then_some(pos)
            }
            RegexAtom::Class { negated, items } => {
                let (_, ch) = *chars.get(pos)?;
                let hit = items.iter().any(|it| it.matches(ch, self.match_case));
                (hit != *negated).then_some(pos + 1)
            }
            RegexAtom::Group(inner) => self.match_node(inner, chars, pos, budget),
        }
    }
}

fn find_text_ranges(
    haystack: &str,
    needle: &str,
    whole_word: bool,
    match_case: bool,
    use_regex: bool,
) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let raw = if use_regex {
        let Some(re) = CompiledRegex::compile(needle, match_case) else {
            return Vec::new();
        };
        re.find_all(haystack)
    } else if match_case {
        haystack
            .match_indices(needle)
            .map(|(i, s)| (i, i + s.len()))
            .collect()
    } else {
        // Case-insensitive substring scan preserving exact byte offsets in `haystack`.
        let needle_chars: Vec<char> = needle.chars().collect();
        let hay_chars: Vec<(usize, char)> = haystack.char_indices().collect();
        let nlen = needle_chars.len();
        let mut out = Vec::new();
        let mut i = 0usize;
        while i + nlen <= hay_chars.len() {
            if (0..nlen).all(|k| chars_eq(hay_chars[i + k].1, needle_chars[k], false)) {
                let b_start = hay_chars[i].0;
                let b_end = if i + nlen < hay_chars.len() {
                    hay_chars[i + nlen].0
                } else {
                    haystack.len()
                };
                out.push((b_start, b_end));
                i += nlen;
            } else {
                i += 1;
            }
        }
        out
    };

    if !whole_word {
        return raw;
    }
    raw.into_iter()
        .filter(|&(start, end)| {
            let before_ok = haystack[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !is_word_char(c));
            let after_ok = haystack[end..]
                .chars()
                .next()
                .is_none_or(|c| !is_word_char(c));
            before_ok && after_ok
        })
        .collect()
}

fn line_col_and_preview(source: &str, byte_start: usize) -> (usize, usize, String) {
    let prefix = &source[..byte_start.min(source.len())];
    let line = prefix.chars().filter(|&c| c == '\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    let col = source[line_start..byte_start.min(source.len())]
        .chars()
        .count()
        + 1;
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |i| line_start + i);
    let preview = source[line_start..line_end].trim().to_owned();
    (line, col, preview)
}

pub(crate) fn parse_hex_color(input: &str) -> Option<(Color, bool)> {
    let hex = input.trim().strip_prefix('#').unwrap_or(input.trim());
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(([r, g, b, 255], false))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(([r, g, b, 255], false))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(([r, g, b, a], true))
        }
        _ => None,
    }
}

fn format_hex_color(c: Color) -> String {
    if c[3] == 255 {
        format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
    }
}

fn color_matches(candidate: Color, query: Option<(Color, bool)>) -> bool {
    if candidate[3] == 0 && query.is_none_or(|(q, has_a)| !has_a || q[3] != 0) {
        return false;
    }
    match query {
        None => true,
        Some((q, true)) => candidate == q,
        Some((q, false)) => candidate[0] == q[0] && candidate[1] == q[1] && candidate[2] == q[2],
    }
}

fn string_filter_matches(value: &str, filter: &str, match_case: bool) -> bool {
    let f = filter.trim();
    if f.is_empty() {
        return true;
    }
    if match_case {
        value.contains(f)
    } else {
        value.to_ascii_lowercase().contains(&f.to_ascii_lowercase())
    }
}

fn symbol_creates_cycle(project: &Project, container_id: &str, target_id: &str) -> bool {
    if container_id == target_id || project.is_scene(target_id) {
        return true;
    }
    let mut stack = vec![target_id.to_owned()];
    let mut visited = HashSet::new();
    while let Some(cur) = stack.pop() {
        if cur == container_id {
            return true;
        }
        if !visited.insert(cur.clone()) {
            continue;
        }
        if let Some(sym) = project.symbol(&cur) {
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for el in &key.elements {
                        if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                            stack.push(symbol_id.clone());
                        }
                    }
                }
            }
        }
    }
    false
}

pub(crate) fn collect_find_hits(
    project: &Project,
    current_symbol_id: &str,
    state: &FindReplaceState,
) -> Vec<FindHit> {
    let mut hits = Vec::new();
    let scoped_symbols: Vec<&neoflash_core::Symbol> = project
        .symbols
        .iter()
        .filter(|sym| {
            if state.in_current_scene {
                sym.id == current_symbol_id
            } else if let Some(scene_id) = &state.scene_filter {
                &sym.id == scene_id
            } else {
                true
            }
        })
        .collect();
    let include_sources = !state.in_current_scene && state.scene_filter.is_none();

    match state.kind {
        FindKind::Text => {
            if state.find.is_empty() {
                return hits;
            }
            for sym in &scoped_symbols {
                let sym_prefix = if project.is_scene(&sym.id) {
                    sym.name.clone()
                } else {
                    format!("Library › {}", sym.name)
                };
                if state.in_names_labels {
                    for range in find_text_ranges(
                        &sym.name,
                        &state.find,
                        state.whole_word,
                        state.match_case,
                        state.regex,
                    ) {
                        hits.push(FindHit {
                            place_key: format!("sym-name:{}", sym.id),
                            location_label: format!("{sym_prefix} · name"),
                            found_preview: sym.name.clone(),
                            target: MatchTarget::SymbolName {
                                symbol: sym.id.clone(),
                                range,
                            },
                        });
                    }
                }
                if state.in_actionscript {
                    for fs in &sym.timeline.scripts {
                        for range in find_text_ranges(
                            &fs.script.source,
                            &state.find,
                            state.whole_word,
                            state.match_case,
                            state.regex,
                        ) {
                            let (line, column, preview) =
                                line_col_and_preview(&fs.script.source, range.0);
                            hits.push(FindHit {
                                place_key: format!("fs:{}:{}", sym.id, fs.frame),
                                location_label: format!(
                                    "{sym_prefix} › Frame {} · line {line}",
                                    fs.frame + 1
                                ),
                                found_preview: preview,
                                target: MatchTarget::FrameScript {
                                    symbol: sym.id.clone(),
                                    frame: fs.frame,
                                    range,
                                    line,
                                    column,
                                },
                            });
                        }
                    }
                }
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    if state.in_names_labels {
                        for range in find_text_ranges(
                            &layer.name,
                            &state.find,
                            state.whole_word,
                            state.match_case,
                            state.regex,
                        ) {
                            hits.push(FindHit {
                                place_key: format!("layer:{}:{}", sym.id, layer.id),
                                location_label: format!(
                                    "{sym_prefix} › {} · layer name",
                                    layer.name
                                ),
                                found_preview: layer.name.clone(),
                                target: MatchTarget::LayerName {
                                    symbol: sym.id.clone(),
                                    layer_id: layer.id.clone(),
                                    layer_index: layer_idx,
                                    range,
                                },
                            });
                        }
                    }
                    for key in &layer.keyframes {
                        if state.in_names_labels && !key.label.is_empty() {
                            for range in find_text_ranges(
                                &key.label,
                                &state.find,
                                state.whole_word,
                                state.match_case,
                                state.regex,
                            ) {
                                hits.push(FindHit {
                                    place_key: format!(
                                        "flabel:{}:{}:{}",
                                        sym.id, layer.id, key.frame
                                    ),
                                    location_label: format!(
                                        "{sym_prefix} › {} › Frame {} · label",
                                        layer.name,
                                        key.frame + 1
                                    ),
                                    found_preview: key.label.clone(),
                                    target: MatchTarget::FrameLabel {
                                        symbol: sym.id.clone(),
                                        layer_id: layer.id.clone(),
                                        layer_index: layer_idx,
                                        frame: key.frame,
                                        range,
                                    },
                                });
                            }
                        }
                        for el in &key.elements {
                            if state.in_names_labels && !el.name.is_empty() {
                                for range in find_text_ranges(
                                    &el.name,
                                    &state.find,
                                    state.whole_word,
                                    state.match_case,
                                    state.regex,
                                ) {
                                    hits.push(FindHit {
                                        place_key: format!("ename:{}:{}", sym.id, el.id),
                                        location_label: format!(
                                            "{sym_prefix} › {} › Frame {} · instance name",
                                            layer.name,
                                            key.frame + 1
                                        ),
                                        found_preview: el.name.clone(),
                                        target: MatchTarget::InstanceName {
                                            symbol: sym.id.clone(),
                                            layer_index: layer_idx,
                                            frame: key.frame,
                                            element_id: el.id.clone(),
                                            range,
                                        },
                                    });
                                }
                            }
                            if state.in_text_fields {
                                let text_opt = match &el.kind {
                                    ElementKind::Text { text, .. }
                                    | ElementKind::TextBox { text, .. } => Some(text.as_str()),
                                    _ => None,
                                };
                                if let Some(txt) = text_opt {
                                    for range in find_text_ranges(
                                        txt,
                                        &state.find,
                                        state.whole_word,
                                        state.match_case,
                                        state.regex,
                                    ) {
                                        let (_, _, preview) = line_col_and_preview(txt, range.0);
                                        hits.push(FindHit {
                                            place_key: format!("txt:{}:{}", sym.id, el.id),
                                            location_label: format!(
                                                "{sym_prefix} › {} › Frame {} · text",
                                                layer.name,
                                                key.frame + 1
                                            ),
                                            found_preview: preview,
                                            target: MatchTarget::TextElement {
                                                symbol: sym.id.clone(),
                                                layer_index: layer_idx,
                                                frame: key.frame,
                                                element_id: el.id.clone(),
                                                range,
                                            },
                                        });
                                    }
                                }
                            }
                            if state.in_actionscript {
                                for os in &el.scripts {
                                    for range in find_text_ranges(
                                        &os.script.source,
                                        &state.find,
                                        state.whole_word,
                                        state.match_case,
                                        state.regex,
                                    ) {
                                        let (line, column, preview) =
                                            line_col_and_preview(&os.script.source, range.0);
                                        hits.push(FindHit {
                                            place_key: format!(
                                                "os:{}:{}:{:?}",
                                                sym.id, el.id, os.event
                                            ),
                                            location_label: format!(
                                                "{sym_prefix} › {} › Frame {} · line {line}",
                                                layer.name,
                                                key.frame + 1
                                            ),
                                            found_preview: preview,
                                            target: MatchTarget::ObjectScript {
                                                symbol: sym.id.clone(),
                                                layer_index: layer_idx,
                                                frame: key.frame,
                                                element_id: el.id.clone(),
                                                event: os.event,
                                                range,
                                                line,
                                                column,
                                            },
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if include_sources && state.in_actionscript {
                for unit in &project.sources {
                    for range in find_text_ranges(
                        &unit.source,
                        &state.find,
                        state.whole_word,
                        state.match_case,
                        state.regex,
                    ) {
                        let (line, column, preview) = line_col_and_preview(&unit.source, range.0);
                        hits.push(FindHit {
                            place_key: format!("src:{}", unit.id),
                            location_label: format!("{} · line {line}", unit.path),
                            found_preview: preview,
                            target: MatchTarget::SourceScript {
                                source_id: unit.id.clone(),
                                range,
                                line,
                                column,
                            },
                        });
                    }
                }
            }
        }
        FindKind::Font => {
            for sym in &scoped_symbols {
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    for key in &layer.keyframes {
                        for el in &key.elements {
                            let font = match &el.kind {
                                ElementKind::Text { font_family, .. } => Some(font_family.as_str()),
                                ElementKind::TextBox { format, .. } => Some(format.font.as_str()),
                                _ => None,
                            };
                            if let Some(font_name) = font
                                && string_filter_matches(font_name, &state.find, state.match_case)
                            {
                                hits.push(FindHit {
                                    place_key: format!("font:{}:{}", sym.id, el.id),
                                    location_label: format!(
                                        "{} › {} › Frame {}",
                                        sym.name,
                                        layer.name,
                                        key.frame + 1
                                    ),
                                    found_preview: font_name.to_owned(),
                                    target: MatchTarget::FontElement {
                                        symbol: sym.id.clone(),
                                        layer_index: layer_idx,
                                        frame: key.frame,
                                        element_id: el.id.clone(),
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
        FindKind::Colour => {
            let query = if state.find.trim().is_empty() {
                None
            } else {
                let Some(parsed) = parse_hex_color(&state.find) else {
                    return hits;
                };
                Some(parsed)
            };
            for sym in &scoped_symbols {
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    for key in &layer.keyframes {
                        for el in &key.elements {
                            let mut push_slot = |slot: ColourSlotKind, c: Color, role: &str| {
                                if color_matches(c, query) {
                                    hits.push(FindHit {
                                        place_key: format!("col:{}:{}", sym.id, el.id),
                                        location_label: format!(
                                            "{} › {} › Frame {} · {role}",
                                            sym.name,
                                            layer.name,
                                            key.frame + 1
                                        ),
                                        found_preview: format_hex_color(c),
                                        target: MatchTarget::ColourSlot {
                                            symbol: sym.id.clone(),
                                            layer_index: layer_idx,
                                            frame: key.frame,
                                            element_id: el.id.clone(),
                                            slot,
                                        },
                                    });
                                }
                            };
                            match &el.kind {
                                ElementKind::Rectangle { fill, stroke, .. }
                                | ElementKind::Ellipse { fill, stroke, .. }
                                | ElementKind::Path { fill, stroke, .. } => {
                                    push_slot(ColourSlotKind::Fill, *fill, "fill");
                                    push_slot(ColourSlotKind::Stroke, *stroke, "stroke");
                                }
                                ElementKind::PaintedPath {
                                    fill,
                                    stroke,
                                    fill_gradient,
                                    stroke_gradient,
                                    ..
                                } => {
                                    if let Some(grad) = fill_gradient {
                                        for (si, stop) in grad.stops.iter().enumerate() {
                                            push_slot(
                                                ColourSlotKind::FillGradientStop(si),
                                                stop.color,
                                                "gradient fill",
                                            );
                                        }
                                    } else {
                                        push_slot(ColourSlotKind::Fill, *fill, "fill");
                                    }
                                    if let Some(grad) = stroke_gradient {
                                        for (si, stop) in grad.stops.iter().enumerate() {
                                            push_slot(
                                                ColourSlotKind::StrokeGradientStop(si),
                                                stop.color,
                                                "gradient stroke",
                                            );
                                        }
                                    } else {
                                        push_slot(ColourSlotKind::Stroke, *stroke, "stroke");
                                    }
                                }
                                ElementKind::Text { color, .. } => {
                                    push_slot(ColourSlotKind::Text, *color, "text colour");
                                }
                                ElementKind::TextBox { format, .. } => {
                                    let c = [
                                        ((format.color >> 16) & 0xFF) as u8,
                                        ((format.color >> 8) & 0xFF) as u8,
                                        (format.color & 0xFF) as u8,
                                        255,
                                    ];
                                    push_slot(ColourSlotKind::TextBox, c, "text colour");
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        FindKind::Symbol => {
            for sym in &scoped_symbols {
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    for key in &layer.keyframes {
                        for el in &key.elements {
                            if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                                let target_name = project
                                    .symbol(symbol_id)
                                    .map_or(symbol_id.as_str(), |s| s.name.as_str());
                                if string_filter_matches(target_name, &state.find, state.match_case)
                                {
                                    hits.push(FindHit {
                                        place_key: format!("inst:{}:{}", sym.id, el.id),
                                        location_label: format!(
                                            "{} › {} › Frame {}",
                                            sym.name,
                                            layer.name,
                                            key.frame + 1
                                        ),
                                        found_preview: if el.name.is_empty() {
                                            target_name.to_owned()
                                        } else {
                                            format!("{} ({target_name})", el.name)
                                        },
                                        target: MatchTarget::SymbolInstance {
                                            symbol: sym.id.clone(),
                                            layer_index: layer_idx,
                                            frame: key.frame,
                                            element_id: el.id.clone(),
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        FindKind::Sound => {
            for sym in &scoped_symbols {
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    for key in &layer.keyframes {
                        if let Some(snd) = &key.sound {
                            let res_path = project
                                .resources
                                .iter()
                                .find(|r| r.id == snd.resource)
                                .map_or(snd.resource.as_str(), |r| r.path.as_str());
                            if string_filter_matches(res_path, &state.find, state.match_case) {
                                hits.push(FindHit {
                                    place_key: format!("snd:{}:{}:{}", sym.id, layer.id, key.frame),
                                    location_label: format!(
                                        "{} › {} › Frame {}",
                                        sym.name,
                                        layer.name,
                                        key.frame + 1
                                    ),
                                    found_preview: res_path.to_owned(),
                                    target: MatchTarget::FrameSound {
                                        symbol: sym.id.clone(),
                                        layer_id: layer.id.clone(),
                                        layer_index: layer_idx,
                                        frame: key.frame,
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
        FindKind::Video => {
            for sym in &scoped_symbols {
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    for key in &layer.keyframes {
                        for el in &key.elements {
                            if let ElementKind::Video { width, height } = &el.kind {
                                let desc = if el.name.is_empty() {
                                    format!("{}×{}", width.round() as i32, height.round() as i32)
                                } else {
                                    format!(
                                        "{} ({}×{})",
                                        el.name,
                                        width.round() as i32,
                                        height.round() as i32
                                    )
                                };
                                if string_filter_matches(&desc, &state.find, state.match_case) {
                                    hits.push(FindHit {
                                        place_key: format!("vid:{}:{}", sym.id, el.id),
                                        location_label: format!(
                                            "{} › {} › Frame {}",
                                            sym.name,
                                            layer.name,
                                            key.frame + 1
                                        ),
                                        found_preview: desc,
                                        target: MatchTarget::VideoPlacement {
                                            symbol: sym.id.clone(),
                                            layer_index: layer_idx,
                                            frame: key.frame,
                                            element_id: el.id.clone(),
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        FindKind::Bitmap => {
            for sym in &scoped_symbols {
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    for key in &layer.keyframes {
                        for el in &key.elements {
                            if let ElementKind::Bitmap { resource_id, .. } = &el.kind {
                                let res_path = project
                                    .resources
                                    .iter()
                                    .find(|r| &r.id == resource_id)
                                    .map_or(resource_id.as_str(), |r| r.path.as_str());
                                if string_filter_matches(res_path, &state.find, state.match_case) {
                                    hits.push(FindHit {
                                        place_key: format!("bmp:{}:{}", sym.id, el.id),
                                        location_label: format!(
                                            "{} › {} › Frame {}",
                                            sym.name,
                                            layer.name,
                                            key.frame + 1
                                        ),
                                        found_preview: res_path.to_owned(),
                                        target: MatchTarget::BitmapPlacement {
                                            symbol: sym.id.clone(),
                                            layer_index: layer_idx,
                                            frame: key.frame,
                                            element_id: el.id.clone(),
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    hits
}

fn replace_range(
    s: &mut String,
    range: (usize, usize),
    replacement: &str,
    must_stay_nonempty: bool,
) -> bool {
    if range.0 > range.1
        || range.1 > s.len()
        || !s.is_char_boundary(range.0)
        || !s.is_char_boundary(range.1)
    {
        return false;
    }
    let mut next = String::with_capacity(s.len() - (range.1 - range.0) + replacement.len());
    next.push_str(&s[..range.0]);
    next.push_str(replacement);
    next.push_str(&s[range.1..]);
    if must_stay_nonempty && next.trim().is_empty() {
        return false;
    }
    if *s == next {
        return false;
    }
    *s = next;
    true
}

fn apply_single_replacement(
    project: &mut Project,
    target: &MatchTarget,
    replacement: &str,
) -> bool {
    match target {
        MatchTarget::TextElement {
            symbol,
            frame,
            element_id,
            range,
            ..
        } => {
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id) {
                        return match &mut el.kind {
                            ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                                replace_range(text, *range, replacement, false)
                            }
                            _ => false,
                        };
                    }
                }
            }
            false
        }
        MatchTarget::SymbolName { symbol, range } => {
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            replace_range(&mut sym.name, *range, replacement, true)
        }
        MatchTarget::LayerName {
            symbol,
            layer_id,
            range,
            ..
        } => {
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            let Some(layer) = sym.timeline.layers.iter_mut().find(|l| &l.id == layer_id) else {
                return false;
            };
            replace_range(&mut layer.name, *range, replacement, true)
        }
        MatchTarget::FrameLabel {
            symbol,
            layer_id,
            frame,
            range,
            ..
        } => {
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            let Some(layer) = sym.timeline.layers.iter_mut().find(|l| &l.id == layer_id) else {
                return false;
            };
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == *frame) else {
                return false;
            };
            replace_range(&mut key.label, *range, replacement, false)
        }
        MatchTarget::InstanceName {
            symbol,
            frame,
            element_id,
            range,
            ..
        } => {
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id) {
                        return replace_range(&mut el.name, *range, replacement, false);
                    }
                }
            }
            false
        }
        MatchTarget::FrameScript {
            symbol,
            frame,
            range,
            ..
        } => {
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            let Some(fs) = sym
                .timeline
                .scripts
                .iter_mut()
                .find(|fs| fs.frame == *frame)
            else {
                return false;
            };
            replace_range(&mut fs.script.source, *range, replacement, false)
        }
        MatchTarget::ObjectScript {
            symbol,
            frame,
            element_id,
            event,
            range,
            ..
        } => {
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id)
                        && let Some(os) = el.scripts.iter_mut().find(|os| os.event == *event)
                    {
                        return replace_range(&mut os.script.source, *range, replacement, false);
                    }
                }
            }
            false
        }
        MatchTarget::SourceScript {
            source_id, range, ..
        } => {
            let Some(unit) = project.sources.iter_mut().find(|u| &u.id == source_id) else {
                return false;
            };
            replace_range(&mut unit.source, *range, replacement, false)
        }
        MatchTarget::FontElement {
            symbol,
            frame,
            element_id,
            ..
        } => {
            let new_font = replacement.trim();
            if new_font.is_empty() || new_font.chars().any(char::is_control) {
                return false;
            }
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id) {
                        return match &mut el.kind {
                            ElementKind::Text { font_family, .. } if font_family != new_font => {
                                *font_family = new_font.to_owned();
                                true
                            }
                            ElementKind::TextBox { format, .. } if format.font != new_font => {
                                format.font = new_font.to_owned();
                                true
                            }
                            _ => false,
                        };
                    }
                }
            }
            false
        }
        MatchTarget::ColourSlot {
            symbol,
            frame,
            element_id,
            slot,
            ..
        } => {
            let Some((new_color, has_alpha)) = parse_hex_color(replacement) else {
                return false;
            };
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            let apply_color = |dst: &mut Color| -> bool {
                let mut target = new_color;
                if !has_alpha {
                    target[3] = dst[3];
                }
                if *dst != target {
                    *dst = target;
                    true
                } else {
                    false
                }
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id) {
                        return match (&mut el.kind, slot) {
                            (
                                ElementKind::Rectangle { fill, .. }
                                | ElementKind::Ellipse { fill, .. }
                                | ElementKind::Path { fill, .. }
                                | ElementKind::PaintedPath { fill, .. },
                                ColourSlotKind::Fill,
                            ) => apply_color(fill),
                            (
                                ElementKind::Rectangle { stroke, .. }
                                | ElementKind::Ellipse { stroke, .. }
                                | ElementKind::Path { stroke, .. }
                                | ElementKind::PaintedPath { stroke, .. },
                                ColourSlotKind::Stroke,
                            ) => apply_color(stroke),
                            (
                                ElementKind::PaintedPath {
                                    fill_gradient: Some(grad),
                                    ..
                                },
                                ColourSlotKind::FillGradientStop(idx),
                            ) => grad
                                .stops
                                .get_mut(*idx)
                                .is_some_and(|s| apply_color(&mut s.color)),
                            (
                                ElementKind::PaintedPath {
                                    stroke_gradient: Some(grad),
                                    ..
                                },
                                ColourSlotKind::StrokeGradientStop(idx),
                            ) => grad
                                .stops
                                .get_mut(*idx)
                                .is_some_and(|s| apply_color(&mut s.color)),
                            (ElementKind::Text { color, .. }, ColourSlotKind::Text) => {
                                apply_color(color)
                            }
                            (ElementKind::TextBox { format, .. }, ColourSlotKind::TextBox) => {
                                let rgb = (u32::from(new_color[0]) << 16)
                                    | (u32::from(new_color[1]) << 8)
                                    | u32::from(new_color[2]);
                                if format.color != rgb {
                                    format.color = rgb;
                                    true
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        };
                    }
                }
            }
            false
        }
        MatchTarget::SymbolInstance {
            symbol,
            frame,
            element_id,
            ..
        } => {
            let wanted = replacement.trim();
            if wanted.is_empty() {
                return false;
            }
            let Some(new_sym_id) = project
                .symbols
                .iter()
                .find(|s| {
                    !project.is_scene(&s.id)
                        && (s.name.eq_ignore_ascii_case(wanted) || s.id == wanted)
                })
                .map(|s| s.id.clone())
            else {
                return false;
            };
            if symbol_creates_cycle(project, symbol, &new_sym_id) {
                return false;
            }
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id)
                        && let ElementKind::Instance { symbol_id, .. } = &mut el.kind
                        && *symbol_id != new_sym_id
                    {
                        *symbol_id = new_sym_id;
                        return true;
                    }
                }
            }
            false
        }
        MatchTarget::FrameSound {
            symbol,
            layer_id,
            frame,
            ..
        } => {
            let wanted = replacement.trim();
            if wanted.is_empty() {
                return false;
            }
            let Some(new_res_id) = project
                .resources
                .iter()
                .find(|r| r.is_audio() && (r.path.eq_ignore_ascii_case(wanted) || r.id == wanted))
                .map(|r| r.id.clone())
            else {
                return false;
            };
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            let Some(layer) = sym.timeline.layers.iter_mut().find(|l| &l.id == layer_id) else {
                return false;
            };
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == *frame) else {
                return false;
            };
            if let Some(snd) = &mut key.sound
                && snd.resource != new_res_id
            {
                snd.resource = new_res_id;
                return true;
            }
            false
        }
        MatchTarget::VideoPlacement {
            symbol,
            frame,
            element_id,
            ..
        } => {
            let new_name = replacement.trim();
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id)
                        && matches!(el.kind, ElementKind::Video { .. })
                        && el.name != new_name
                    {
                        el.name = new_name.to_owned();
                        return true;
                    }
                }
            }
            false
        }
        MatchTarget::BitmapPlacement {
            symbol,
            frame,
            element_id,
            ..
        } => {
            let wanted = replacement.trim();
            if wanted.is_empty() {
                return false;
            }
            let Some(new_res_id) = project
                .resources
                .iter()
                .find(|r| {
                    r.media_type.starts_with("image/")
                        && (r.path.eq_ignore_ascii_case(wanted) || r.id == wanted)
                })
                .map(|r| r.id.clone())
            else {
                return false;
            };
            let Some(sym) = project.symbol_mut(symbol) else {
                return false;
            };
            for layer in &mut sym.timeline.layers {
                for key in layer.keyframes.iter_mut().filter(|k| k.frame == *frame) {
                    if let Some(el) = key.elements.iter_mut().find(|e| &e.id == element_id)
                        && let ElementKind::Bitmap { resource_id, .. } = &mut el.kind
                        && *resource_id != new_res_id
                    {
                        *resource_id = new_res_id;
                        return true;
                    }
                }
            }
            false
        }
    }
}

fn count_distinct_places(hits: &[FindHit]) -> usize {
    hits.iter()
        .map(|h| h.place_key.as_str())
        .collect::<HashSet<_>>()
        .len()
}

impl Editor {
    pub fn open_find_replace(&mut self) {
        self.set_playing(false);
        let mut state = self.find_replace.take().unwrap_or_default();
        state.focus_find = true;
        if state.find.is_empty()
            && let Some(el) = self.selected_element()
            && !el.name.trim().is_empty()
        {
            state.find = el.name.trim().to_owned();
        }
        if !state.find.is_empty() {
            state.results = collect_find_hits(&self.project, self.symbol_id(), &state);
            state.selected = 0;
        }
        self.find_replace = Some(state);
    }

    pub fn find_all_matches(&mut self) -> usize {
        let sym_id = self.symbol_id().to_owned();
        let Some(state) = self.find_replace.as_mut() else {
            return 0;
        };
        state.results = collect_find_hits(&self.project, &sym_id, state);
        state.selected = 0;
        let len = state.results.len();
        let live_first = (state.live_edit && len > 0).then(|| state.results[0].target.clone());
        if let Some(target) = live_first {
            self.jump_to_find_target(&target, None);
        }
        len
    }

    pub(crate) fn jump_to_find_target(
        &mut self,
        target: &MatchTarget,
        ctx: Option<&egui::Context>,
    ) {
        let enter_sym = |editor: &mut Editor, symbol: &str| {
            editor.finish_edits();
            if editor.symbol_id() != symbol {
                if editor.project.is_scene(symbol) {
                    editor.switch_scene(symbol);
                } else {
                    editor.leave_to_level(0);
                    if symbol != editor.project.root {
                        editor.enter_symbol(symbol.to_owned());
                    }
                }
            }
        };
        match target {
            MatchTarget::SymbolName { symbol, .. } => {
                enter_sym(self, symbol);
            }
            MatchTarget::LayerName {
                symbol,
                layer_index,
                ..
            } => {
                enter_sym(self, symbol);
                if *layer_index < self.symbol().timeline.layers.len() {
                    self.layer = *layer_index;
                }
                self.normalize_selection();
            }
            MatchTarget::FrameLabel {
                symbol,
                layer_index,
                frame,
                ..
            }
            | MatchTarget::FrameSound {
                symbol,
                layer_index,
                frame,
                ..
            } => {
                enter_sym(self, symbol);
                if *layer_index < self.symbol().timeline.layers.len() {
                    self.layer = *layer_index;
                }
                self.set_playing(false);
                self.frame = *frame;
                self.normalize_selection();
            }
            MatchTarget::TextElement {
                symbol,
                layer_index,
                frame,
                element_id,
                ..
            }
            | MatchTarget::InstanceName {
                symbol,
                layer_index,
                frame,
                element_id,
                ..
            }
            | MatchTarget::FontElement {
                symbol,
                layer_index,
                frame,
                element_id,
            }
            | MatchTarget::ColourSlot {
                symbol,
                layer_index,
                frame,
                element_id,
                ..
            }
            | MatchTarget::SymbolInstance {
                symbol,
                layer_index,
                frame,
                element_id,
            }
            | MatchTarget::VideoPlacement {
                symbol,
                layer_index,
                frame,
                element_id,
            }
            | MatchTarget::BitmapPlacement {
                symbol,
                layer_index,
                frame,
                element_id,
            } => {
                enter_sym(self, symbol);
                if *layer_index < self.symbol().timeline.layers.len() {
                    self.layer = *layer_index;
                }
                self.set_playing(false);
                self.frame = *frame;
                self.select_single_id(element_id.clone());
            }
            MatchTarget::FrameScript {
                symbol,
                frame,
                line,
                column,
                ..
            } => {
                let loc = ScriptLocation::Frame {
                    symbol: symbol.clone(),
                    frame: *frame,
                };
                let src = self.script_source_for(&loc).unwrap_or_default();
                self.open_script_location(loc);
                self.open_actions();
                if let Some(ctx) = ctx {
                    crate::sources::jump_to(ctx, self.actions_editor_id(), &src, *line, *column);
                }
            }
            MatchTarget::ObjectScript {
                symbol,
                frame,
                element_id,
                event,
                line,
                column,
                ..
            } => {
                let loc = ScriptLocation::Object {
                    symbol: symbol.clone(),
                    frame: *frame,
                    element: element_id.clone(),
                    event: *event,
                };
                let src = self.script_source_for(&loc).unwrap_or_default();
                self.open_script_location(loc);
                self.open_actions();
                if let Some(ctx) = ctx {
                    crate::sources::jump_to(ctx, self.actions_editor_id(), &src, *line, *column);
                }
            }
            MatchTarget::SourceScript {
                source_id,
                line,
                column,
                ..
            } => {
                let loc = ScriptLocation::Source(source_id.clone());
                let src = self.script_source_for(&loc).unwrap_or_default();
                self.open_script_location(loc);
                self.open_actions();
                if let Some(ctx) = ctx {
                    crate::sources::jump_to(ctx, self.actions_editor_id(), &src, *line, *column);
                }
            }
        }
    }

    pub fn replace_selected_match(&mut self) -> bool {
        let Some(state) = self.find_replace.as_ref() else {
            return false;
        };
        let Some(hit) = state.results.get(state.selected).cloned() else {
            return false;
        };
        let replacement = state.replace.clone();
        let selected_idx = state.selected;
        let mut changed = false;
        self.finish_edits();
        self.transact_named("Find and replace", |s| {
            let mut candidate = s.project.clone();
            if apply_single_replacement(&mut candidate, &hit.target, &replacement)
                && candidate.validate().is_ok()
            {
                s.project = candidate;
                changed = true;
            }
        });
        if changed {
            let sym_id = self.symbol_id().to_owned();
            if let Some(state) = self.find_replace.as_mut() {
                state.results = collect_find_hits(&self.project, &sym_id, state);
                state.selected = selected_idx.min(state.results.len().saturating_sub(1));
                let live_target = state
                    .live_edit
                    .then(|| state.results.get(state.selected).map(|h| h.target.clone()))
                    .flatten();
                if let Some(t) = live_target {
                    self.jump_to_find_target(&t, None);
                }
            }
        }
        changed
    }

    pub fn replace_all_matches(&mut self) -> usize {
        let sym_id = self.symbol_id().to_owned();
        let Some(state) = self.find_replace.as_ref().cloned() else {
            return 0;
        };
        let hits = collect_find_hits(&self.project, &sym_id, &state);
        if hits.is_empty() {
            return 0;
        }
        let replacement = state.replace.clone();
        let mut count = 0usize;
        self.finish_edits();
        self.transact_named("Find and replace", |s| {
            let mut candidate = s.project.clone();
            // Apply in reverse order so byte offsets inside the same string remain valid!
            for hit in hits.iter().rev() {
                if apply_single_replacement(&mut candidate, &hit.target, &replacement) {
                    count += 1;
                }
            }
            if count > 0 && candidate.validate().is_ok() {
                s.project = candidate;
            } else {
                count = 0;
            }
        });
        let sym_id = self.symbol_id().to_owned();
        if let Some(st) = self.find_replace.as_mut() {
            st.results = collect_find_hits(&self.project, &sym_id, st);
            st.selected = 0;
        }
        count
    }

    pub(crate) fn find_replace_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.find_replace.clone() else {
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let mut close = false;
        let mut do_find_all = false;
        let mut do_replace_one = false;
        let mut do_replace_all = false;
        let mut clicked_row: Option<usize> = None;

        let modal = egui::Modal::new(egui::Id::new("find-replace"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(DIALOG_WIDTH);
                let top = ui.cursor().min;
                close |= c::DialogHeader {
                    title: "Find and replace",
                }
                .show(ui);
                ui.painter().hline(
                    top.x..=top.x + DIALOG_WIDTH,
                    top.y + metrics::DIALOG_HEADER - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(DIALOG_WIDTH - 2.0 * space::S5);
                        find_replace_body(
                            ui,
                            &self.project,
                            &mut state,
                            &mut do_find_all,
                            &mut do_replace_one,
                            &mut do_replace_all,
                            &mut clicked_row,
                        );
                    });
            });

        if !close {
            let popup_open = egui::Popup::is_any_open(ctx);
            let esc = !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if esc || modal.should_close() {
                close = true;
            }
        }

        if close {
            self.find_replace = None;
            return;
        }

        self.find_replace = Some(state);

        if do_find_all {
            self.find_all_matches();
        } else if do_replace_one {
            self.replace_selected_match();
        } else if do_replace_all {
            self.replace_all_matches();
        } else if let Some(idx) = clicked_row {
            let target = self.find_replace.as_mut().and_then(|st| {
                st.selected = idx;
                st.results.get(idx).map(|h| h.target.clone())
            });
            if let Some(target) = target {
                self.jump_to_find_target(&target, Some(ctx));
            }
        }
    }
}

fn find_replace_body(
    ui: &mut Ui,
    project: &Project,
    state: &mut FindReplaceState,
    do_find_all: &mut bool,
    do_replace_one: &mut bool,
    do_replace_all: &mut bool,
    clicked_row: &mut Option<usize>,
) {
    ui.spacing_mut().item_spacing.y = space::S3;
    let prev_kind = state.kind;
    c::Segmented::show(
        ui,
        &mut state.kind,
        &[
            (FindKind::Text, "Text"),
            (FindKind::Font, "Font"),
            (FindKind::Colour, "Colour"),
            (FindKind::Symbol, "Symbol"),
            (FindKind::Sound, "Sound"),
            (FindKind::Video, "Video"),
            (FindKind::Bitmap, "Bitmap"),
        ],
        false,
    );
    if state.kind != prev_kind {
        state.results.clear();
        state.selected = 0;
    }

    ui.horizontal(|ui| {
        c::Select::options(
            ui,
            "find-replace-scope-in",
            &mut state.in_current_scene,
            &[(false, "In: Current document"), (true, "In: Current scene")],
            220.0,
        );
        let scene_label = match &state.scene_filter {
            None => "Scene: All scenes".to_owned(),
            Some(id) => format!(
                "Scene: {}",
                project.symbol(id).map_or(id.as_str(), |s| s.name.as_str())
            ),
        };
        ui.add_enabled_ui(!state.in_current_scene, |ui| {
            c::Select::new("find-replace-scene", &scene_label)
                .width(200.0)
                .show(ui, |ui| {
                    if ui
                        .add(
                            c::MenuItem::new("Scene: All scenes")
                                .checked(state.scene_filter.is_none()),
                        )
                        .clicked()
                    {
                        state.scene_filter = None;
                    }
                    for sid in project.scene_ids() {
                        let sname = project
                            .symbol(&sid)
                            .map_or_else(|| sid.clone(), |s| s.name.clone());
                        if ui
                            .add(
                                c::MenuItem::new(&format!("Scene: {sname}"))
                                    .checked(state.scene_filter.as_deref() == Some(sid.as_str())),
                            )
                            .clicked()
                        {
                            state.scene_filter = Some(sid);
                        }
                    }
                });
        });
    });

    let (find_hint, replace_hint) = match state.kind {
        FindKind::Text => ("Text or identifier", "Replacement text"),
        FindKind::Font => ("Font family (empty = all)", "Replacement font family"),
        FindKind::Colour => ("#RRGGBB (empty = all)", "#RRGGBB replacement"),
        FindKind::Symbol => ("Symbol name (empty = all)", "Replacement symbol name"),
        FindKind::Sound => ("Sound path (empty = all)", "Replacement audio path"),
        FindKind::Video => ("Video name (empty = all)", "Replacement instance name"),
        FindKind::Bitmap => ("Bitmap path (empty = all)", "Replacement image path"),
    };
    let w = ui.available_width();
    let find_resp = ui.add(
        c::TextField::new(&mut state.find, "Find")
            .prefix("Find")
            .hint(find_hint)
            .width(w),
    );
    if state.focus_find {
        ui.memory_mut(|m| m.request_focus(c::TextField::edit_id(find_resp.id)));
        state.focus_find = false;
    }
    let replace_resp = ui.add(
        c::TextField::new(&mut state.replace, "Replace")
            .prefix("Replace")
            .hint(replace_hint)
            .width(w),
    );
    if (find_resp.has_focus() || replace_resp.has_focus())
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        *do_find_all = true;
    }

    let is_text = state.kind == FindKind::Text;
    let case_applicable = !matches!(state.kind, FindKind::Colour);
    ui.horizontal(|ui| {
        ui.add_enabled(
            is_text,
            c::Checkbox::new(&mut state.whole_word, "Whole word"),
        );
        ui.add_enabled(
            case_applicable,
            c::Checkbox::new(&mut state.match_case, "Match case"),
        );
        ui.add_enabled(
            is_text,
            c::Checkbox::new(&mut state.regex, "Regular expression"),
        );
        ui.add(c::Checkbox::new(&mut state.live_edit, "Live edit"));
    });
    ui.horizontal(|ui| {
        ui.add_enabled(
            is_text,
            c::Checkbox::new(&mut state.in_text_fields, "Text field contents"),
        );
        ui.add_enabled(
            is_text,
            c::Checkbox::new(&mut state.in_names_labels, "Frames / layers / scenes"),
        );
        ui.add_enabled(
            is_text,
            c::Checkbox::new(&mut state.in_actionscript, "ActionScript"),
        );
    });

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let cols = &[0.0, 0.42];
        ui.add(c::TableRow {
            cells: &["Location", "Found"],
            kind: c::TableRowKind::Header,
            columns: cols,
        });
        egui::ScrollArea::vertical()
            .id_salt("find-replace-results")
            .max_height(168.0)
            .auto_shrink([false, true])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, hit) in state.results.iter().enumerate() {
                    let resp = ui.add(c::TableRow {
                        cells: &[&hit.location_label, &hit.found_preview],
                        kind: if state.selected == i {
                            c::TableRowKind::Selected
                        } else {
                            c::TableRowKind::Row
                        },
                        columns: cols,
                    });
                    if resp.clicked() {
                        *clicked_row = Some(i);
                    }
                }
            });
    });

    let places = count_distinct_places(&state.results);
    let summary = format!(
        "{} result{} in {} place{}",
        state.results.len(),
        if state.results.len() == 1 { "" } else { "s" },
        places,
        if places == 1 { "" } else { "s" }
    );
    let can_find = !is_text || !state.find.is_empty();
    let can_replace = match state.kind {
        FindKind::Text => !state.find.is_empty(),
        FindKind::Colour => parse_hex_color(&state.replace).is_some(),
        FindKind::Font | FindKind::Symbol | FindKind::Sound | FindKind::Bitmap => {
            !state.replace.trim().is_empty()
        }
        FindKind::Video => true,
    };

    ui.horizontal(|ui| {
        caption(ui, &summary);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    can_replace && (!state.results.is_empty() || can_find),
                    c::Button::primary("Replace all"),
                )
                .clicked()
            {
                *do_replace_all = true;
            }
            if ui
                .add_enabled(
                    can_replace && state.results.get(state.selected).is_some(),
                    c::Button::new("Replace"),
                )
                .clicked()
            {
                *do_replace_one = true;
            }
            if ui
                .add_enabled(can_find, c::Button::ghost("Find all"))
                .clicked()
            {
                *do_find_all = true;
            }
        });
    });
}
