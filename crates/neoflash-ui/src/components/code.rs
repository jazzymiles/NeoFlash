//! `NF2/Code Editor` (§7 of the reference) and the message rows around it:
//!
//! * [`highlight`] — syntax colouring with the NF2 Night tokens (`color/syntax/*`) as a `LayoutJob`,
//!   for `egui::TextEdit::layouter`. ECMAScript family: ActionScript 1–3, JavaScript, TypeScript and
//!   E4X/XML literals. It is a presentation lexer only (no parsing, no diagnostics).
//! * [`Gutter`] — line numbers and markers (error, warning) painted beside a laid-out code galley,
//!   plus [`current_line`] for the current-line band.
//! * [`script_status`] — "Line 19, Col 10 · 20 lines" and the problems counter of the script bar.
//! * [`CompilerMessage`] — `Compiler errors` entry (message, location, chevron; click jumps to code).
//! * [`LogLine`] — one line of the Output log (icon, optional time, mono text).
//!
//! Red (`syntax/error`) is only used for errors, amber (`syntax/warning`) for warnings.
use super::{State, a11y, galley};
use crate::icons::{self, Icon, i16};
use crate::theme::{Theme, cr, palette};
use crate::tokens::{Palette, TextStyle, radius, space};
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, Ui, Widget, text::LayoutJob};
use std::ops::Range;

/// Severity of a gutter marker, compiler message or log line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Neutral information (Output `trace`, status messages).
    #[default]
    Info,
    /// A check passed ("Code accepted").
    Success,
    Warning,
    Error,
}

impl Severity {
    pub fn icon(self) -> Icon {
        match self {
            Self::Info => i16::INFO,
            Self::Success => i16::CHECK,
            Self::Warning => i16::WARNING,
            Self::Error => i16::ERROR,
        }
    }
    /// Icon / text colour: errors `syntax/error`, warnings `syntax/warning`.
    pub fn color(self, p: &Palette) -> Color32 {
        match self {
            Self::Info => p.text_secondary,
            Self::Success => p.success_default,
            Self::Warning => p.syntax_warning,
            Self::Error => p.syntax_error,
        }
    }
    /// Row fill of a compiler message.
    pub fn fill(self, p: &Palette) -> Color32 {
        match self {
            Self::Info | Self::Success => p.bg_hover,
            Self::Warning => p.warning_subtle,
            Self::Error => p.danger_subtle,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Success => "Passed",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Syntax colouring

/// Token classes of the NF2 Night scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    /// Whitespace (`text/primary`).
    Plain,
    /// User variables, parameters, properties and identifiers (`syntax/identifier`).
    Identifier,
    /// Operators and punctuation (`text/secondary`).
    Punct,
    Keyword,
    /// Classes and type annotations.
    Type,
    Function,
    String,
    Number,
    Comment,
    /// XML tag names and brackets (never red).
    Tag,
    /// XML attribute names.
    Attr,
}

impl Token {
    pub fn color(self, p: &Palette) -> Color32 {
        match self {
            Self::Plain => p.text_primary,
            Self::Identifier => p.syntax_identifier,
            Self::Punct => p.text_secondary,
            Self::Keyword => p.syntax_keyword,
            Self::Type => p.syntax_type,
            Self::Function => p.syntax_function,
            Self::String => p.syntax_string,
            Self::Number => p.syntax_number,
            Self::Comment => p.syntax_comment,
            Self::Tag => p.syntax_tag,
            Self::Attr => p.syntax_attr,
        }
    }
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "declare",
    "default",
    "delete",
    "do",
    "dynamic",
    "each",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "for",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "include",
    "instanceof",
    "interface",
    "internal",
    "is",
    "keyof",
    "let",
    "namespace",
    "native",
    "new",
    "null",
    "of",
    "override",
    "package",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "use",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Lower-case built-in types coloured as types in type contexts (`var n:int`, `x: number`).
const ANNOTATION_TYPES: &[&str] = &[
    "int", "uint", "any", "number", "string", "boolean", "never", "unknown", "object", "symbol",
    "bigint",
];

/// Built-in AS1/AS2/AS3/JS/TS classes and global constructors coloured as [`Token::Type`].
const BUILTIN_TYPES: &[&str] = &[
    "Array",
    "BitmapData",
    "Boolean",
    "ByteArray",
    "Color",
    "ColorTransform",
    "Date",
    "Dictionary",
    "Error",
    "Event",
    "Function",
    "JSON",
    "Key",
    "KeyboardEvent",
    "LoadVars",
    "Loader",
    "Map",
    "Math",
    "Matrix",
    "Mouse",
    "MouseEvent",
    "MovieClip",
    "MovieClipLoader",
    "NetConnection",
    "NetStream",
    "Number",
    "Object",
    "Point",
    "Promise",
    "Rectangle",
    "RegExp",
    "Set",
    "Shape",
    "SharedObject",
    "SimpleButton",
    "Sound",
    "Sprite",
    "Stage",
    "String",
    "Symbol",
    "System",
    "TextField",
    "TextFormat",
    "Timer",
    "URLLoader",
    "URLRequest",
    "Vector",
    "Video",
    "Void",
    "XML",
    "XMLNode",
];

const TYPE_CONTEXT_KEYWORDS: &[&str] = &[
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum LexScope {
    Block,
    ObjectLiteral,
    TypeObjectLiteral,
    Paren,
    Bracket,
}

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}
fn ident_char(c: u8) -> bool {
    ident_start(c) || c.is_ascii_digit()
}

/// Split source into coloured runs. Ranges are byte ranges on char boundaries and cover the whole text.
pub fn tokens(src: &str) -> Vec<(Range<usize>, Token)> {
    let b = src.as_bytes();
    let n = b.len();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut out: Vec<(Range<usize>, Token)> = Vec::new();
    let mut push = |range: Range<usize>, token: Token| match out.last_mut() {
        Some((last, t)) if *t == token && last.end == range.start => last.end = range.end,
        _ => out.push((range, token)),
    };
    let mut i = 0;
    // Last significant byte (outside whitespace and comments): decides `<` = tag vs. less-than.
    let mut prev: u8 = 0;
    let mut prev2: u8 = 0;
    let mut last_word: &str = "";
    let mut last_word_was_type = false;
    let mut prev_was_stmt_start_ident = false;
    let mut last_colon_was_case_or_label = false;
    let mut last_star_was_type_any = false;
    let mut in_tag = false;
    let mut tag_name = false;
    let mut after_function = false;
    let mut expect_type = false;
    let mut in_type_alias = false;
    let mut type_angle_depth: u32 = 0;
    let mut case_label_depths: Vec<usize> = Vec::new();
    let mut ternary_depths: Vec<usize> = Vec::new();
    let mut scopes: Vec<LexScope> = Vec::new();
    while i < n {
        let c = b[i];
        let start = i;
        if c.is_ascii_whitespace() {
            while i < n && b[i].is_ascii_whitespace() {
                i += 1;
            }
            push(start..i, Token::Plain);
            continue;
        }
        if c == b'/' && at(i + 1) == b'/' && !in_tag {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            push(start..i, Token::Comment);
            continue;
        }
        if c == b'/' && at(i + 1) == b'*' && !in_tag {
            i += 2;
            while i < n && !(b[i] == b'*' && at(i + 1) == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
            push(start..i, Token::Comment);
            continue;
        }
        let token = if c == b'"' || c == b'\'' || c == b'`' {
            i += 1;
            while i < n && b[i] != c {
                if b[i] == b'\\' {
                    i += 1;
                } else if b[i] == b'\n' && c != b'`' && !in_tag {
                    break;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            expect_type = false;
            prev_was_stmt_start_ident = false;
            last_colon_was_case_or_label = false;
            last_star_was_type_any = false;
            last_word = "";
            Token::String
        } else if in_tag {
            if c == b'>' {
                i += 1;
                in_tag = false;
                Token::Tag
            } else if c == b'/' && at(i + 1) == b'>' {
                i += 2;
                in_tag = false;
                Token::Tag
            } else if ident_start(c) {
                while i < n && (ident_char(b[i]) || matches!(b[i], b':' | b'.' | b'-')) {
                    i += 1;
                }
                let t = if tag_name { Token::Tag } else { Token::Attr };
                tag_name = false;
                t
            } else {
                i += 1;
                Token::Punct
            }
        } else if c == b'<'
            && !expect_type
            && type_angle_depth == 0
            && prev != b'.'
            && (ident_start(at(i + 1)) || at(i + 1) == b'/')
            && !(ident_char(prev) || prev == b')' || prev == b']')
        {
            i += 1;
            if at(i) == b'/' {
                i += 1;
            }
            in_tag = true;
            tag_name = true;
            prev_was_stmt_start_ident = false;
            last_colon_was_case_or_label = false;
            last_star_was_type_any = false;
            last_word = "";
            Token::Tag
        } else if c.is_ascii_digit() || (c == b'.' && at(i + 1).is_ascii_digit()) {
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                if matches!(b[i], b'e' | b'E') && matches!(at(i + 1), b'+' | b'-') {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            expect_type = false;
            prev_was_stmt_start_ident = false;
            last_colon_was_case_or_label = false;
            last_star_was_type_any = false;
            last_word = "";
            Token::Number
        } else if ident_start(c) {
            while i < n && ident_char(b[i]) {
                i += 1;
            }
            let word = &src[start..i];
            let mut j = i;
            while j < n && (b[j] == b' ' || b[j] == b'\t') {
                j += 1;
            }
            let is_kw = KEYWORDS.contains(&word);
            let in_type_ctx = expect_type || type_angle_depth > 0;
            let t = if after_function && !is_kw {
                Token::Function
            } else if is_kw {
                Token::Keyword
            } else if in_type_ctx
                || (prev != b'.'
                    && (BUILTIN_TYPES.contains(&word)
                        || (in_type_ctx && ANNOTATION_TYPES.contains(&word))))
            {
                Token::Type
            } else if at(j) == b'(' {
                Token::Function
            } else {
                Token::Identifier
            };
            after_function = matches!(word, "function" | "get" | "set")
                && (word == "function" || after_function);
            let in_obj_key = matches!(
                scopes.last(),
                Some(LexScope::ObjectLiteral | LexScope::TypeObjectLiteral)
            ) && matches!(prev, b'{' | b',' | b';');
            if prev != b'.'
                && !in_obj_key
                && matches!(word, "case" | "default")
                && case_label_depths.last() != Some(&scopes.len())
            {
                case_label_depths.push(scopes.len());
            }
            if prev != b'.' && word == "type" {
                in_type_alias = true;
            }
            if prev != b'.' && TYPE_CONTEXT_KEYWORDS.contains(&word) {
                expect_type = true;
            } else if expect_type {
                expect_type = matches!(at(j), b'.' | b'<' | b'|' | b'&');
            }
            prev_was_stmt_start_ident = !is_kw && matches!(prev, 0 | b';' | b'{' | b'}');
            last_colon_was_case_or_label = false;
            last_star_was_type_any = false;
            last_word_was_type = in_type_ctx;
            last_word = word;
            push(start..i, t);
            prev2 = prev;
            prev = b[i - 1];
            continue;
        } else {
            i += 1;
            let mut next = i;
            while next < n && (b[next] == b' ' || b[next] == b'\t') {
                next += 1;
            }
            match c {
                b'{' => {
                    let is_arrow = prev2 == b'=' && prev == b'>';
                    let scope = if !is_arrow && expect_type {
                        LexScope::TypeObjectLiteral
                    } else {
                        let prev_punct_opens_obj = match prev {
                            b'=' | b'(' | b'[' | b',' | b'?' | b'!' | b'+' | b'-' | b'%' | b'&'
                            | b'|' | b'^' => true,
                            b'*' => !last_star_was_type_any,
                            b':' => !last_colon_was_case_or_label,
                            _ => false,
                        };
                        let prev_kw_opens_obj = ident_char(prev)
                            && !last_word_was_type
                            && matches!(
                                last_word,
                                "return"
                                    | "throw"
                                    | "case"
                                    | "in"
                                    | "of"
                                    | "yield"
                                    | "await"
                                    | "typeof"
                                    | "void"
                                    | "delete"
                            );
                        if !is_arrow && (prev_punct_opens_obj || prev_kw_opens_obj) {
                            LexScope::ObjectLiteral
                        } else {
                            LexScope::Block
                        }
                    };
                    scopes.push(scope);
                    expect_type = false;
                    type_angle_depth = 0;
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b'}' => {
                    while matches!(scopes.last(), Some(LexScope::Paren | LexScope::Bracket)) {
                        scopes.pop();
                    }
                    scopes.pop();
                    ternary_depths.retain(|&d| d <= scopes.len());
                    case_label_depths.retain(|&d| d <= scopes.len());
                    expect_type = false;
                    type_angle_depth = 0;
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b'(' => {
                    scopes.push(LexScope::Paren);
                    expect_type = false;
                    type_angle_depth = 0;
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b')' => {
                    if scopes.last() == Some(&LexScope::Paren) {
                        scopes.pop();
                    }
                    ternary_depths.retain(|&d| d <= scopes.len());
                    case_label_depths.retain(|&d| d <= scopes.len());
                    expect_type = false;
                    type_angle_depth = 0;
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b'[' => {
                    scopes.push(LexScope::Bracket);
                    if type_angle_depth == 0 && !expect_type {
                        expect_type = false;
                    }
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b']' => {
                    if scopes.last() == Some(&LexScope::Bracket) {
                        scopes.pop();
                    }
                    ternary_depths.retain(|&d| d <= scopes.len());
                    case_label_depths.retain(|&d| d <= scopes.len());
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b'?' => {
                    let next_c = at(next);
                    if !matches!(next_c, b'.' | b'?' | b':') && prev != b'?' {
                        ternary_depths.push(scopes.len());
                    }
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b':' => {
                    last_star_was_type_any = false;
                    if at(i) == b':' || prev == b':' {
                        expect_type = false;
                        last_colon_was_case_or_label = false;
                    } else if ternary_depths.last() == Some(&scopes.len()) {
                        ternary_depths.pop();
                        expect_type = false;
                        last_colon_was_case_or_label = false;
                    } else if case_label_depths.last() == Some(&scopes.len()) {
                        case_label_depths.pop();
                        expect_type = false;
                        last_colon_was_case_or_label = true;
                    } else if scopes.last() == Some(&LexScope::ObjectLiteral) {
                        expect_type = false;
                        last_colon_was_case_or_label = false;
                    } else if prev_was_stmt_start_ident
                        && scopes.last() != Some(&LexScope::TypeObjectLiteral)
                    {
                        expect_type = false;
                        last_colon_was_case_or_label = true;
                    } else {
                        expect_type = true;
                        last_colon_was_case_or_label = false;
                    }
                }
                b'*' => {
                    last_star_was_type_any = expect_type && prev != b'.';
                    last_colon_was_case_or_label = false;
                    if type_angle_depth == 0 {
                        expect_type = false;
                    }
                }
                b'.' => {
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                    if !(expect_type && (ident_start(at(next)) || at(next) == b'<')) {
                        expect_type = false;
                    }
                }
                b'<' => {
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                    let builtin_generic =
                        matches!(
                            last_word,
                            "Array" | "Map" | "Set" | "Promise" | "Vector" | "Dictionary"
                        ) && src[i..].split('\n').next().is_some_and(|l| l.contains('>'));
                    if expect_type
                        || type_angle_depth > 0
                        || (prev == b'.' && last_word == "Vector")
                        || builtin_generic
                    {
                        type_angle_depth = type_angle_depth.saturating_add(1);
                        expect_type = false;
                    }
                }
                b'>' => {
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                    if type_angle_depth > 0 && prev != b'=' {
                        type_angle_depth -= 1;
                    }
                }
                b'|' | b'&' => {
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                    if at(i) == c {
                        expect_type = false;
                        type_angle_depth = 0;
                    } else if in_type_alias {
                        expect_type = true;
                    }
                }
                b';' => {
                    expect_type = false;
                    in_type_alias = false;
                    type_angle_depth = 0;
                    ternary_depths.retain(|&d| d < scopes.len());
                    case_label_depths.retain(|&d| d < scopes.len());
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b'=' => {
                    type_angle_depth = 0;
                    expect_type = in_type_alias && at(i) != b'=' && at(i) != b'>';
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                b',' => {
                    if type_angle_depth == 0 {
                        expect_type = false;
                    }
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
                _ => {
                    if type_angle_depth == 0 {
                        expect_type = false;
                    }
                    last_colon_was_case_or_label = false;
                    last_star_was_type_any = false;
                }
            }
            prev_was_stmt_start_ident = false;
            if c != b'.' {
                last_word = "";
            }
            Token::Punct
        };
        after_function = false;
        push(start..i, token);
        prev2 = prev;
        prev = b[i - 1];
    }
    out
}

fn syntax_palette_id() -> egui::Id {
    egui::Id::new("nf2-syntax-palette-override")
}

/// Install an optional custom syntax palette override (`Preferences ▸ Code ▸ Colour scheme`, `T-376`)
/// scoped to the current [`Theme`]. Passing `None` restores the theme's default [`Palette`].
pub fn set_syntax_palette(ctx: &egui::Context, custom: Option<Palette>) {
    let theme = Theme::current(ctx);
    ctx.data_mut(|d| match custom {
        Some(p) => d.insert_temp(syntax_palette_id(), (theme, p)),
        None => d.remove::<(Theme, Palette)>(syntax_palette_id()),
    });
}

/// Effective [`Palette`] for code syntax highlighting, gutter markers, current-line band, and
/// diagnostic chrome in `ctx`.
pub fn syntax_palette(ctx: &egui::Context) -> Palette {
    let theme = Theme::current(ctx);
    ctx.data(|d| d.get_temp::<(Theme, Palette)>(syntax_palette_id()))
        .and_then(|(saved_theme, p)| (saved_theme == theme).then_some(p))
        .unwrap_or_else(|| *theme.palette())
}

/// Colour `text` with the NF2 Night tokens in `NF2/Mono` (uncached; prefer [`highlight`]).
pub fn layout_job(text: &str, p: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (range, token) in tokens(text) {
        job.append(
            &text[range],
            0.0,
            crate::typography::format(TextStyle::Mono, token.color(p)),
        );
    }
    if job.sections.is_empty() {
        job.append(
            "",
            0.0,
            crate::typography::format(TextStyle::Mono, p.text_primary),
        );
    }
    job
}

#[derive(Default)]
struct Highlighter;

impl egui::cache::ComputerMut<(Palette, &str), LayoutJob> for Highlighter {
    fn compute(&mut self, (p, text): (Palette, &str)) -> LayoutJob {
        layout_job(text, &p)
    }
}

type HighlightCache = egui::cache::FrameCache<LayoutJob, Highlighter>;

/// Syntax-coloured `LayoutJob` for a code `TextEdit` layouter, cached per text and effective syntax palette.
/// The caller sets `wrap.max_width` and lays it out:
///
/// ```ignore
/// let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
///     let mut job = neoflash_ui::components::code::highlight(ui.ctx(), text.as_str());
///     job.wrap.max_width = wrap;
///     ui.fonts_mut(|f| f.layout_job(job))
/// };
/// ```
pub fn highlight(ctx: &egui::Context, text: &str) -> LayoutJob {
    let p = syntax_palette(ctx);
    ctx.memory_mut(|m| m.caches.cache::<HighlightCache>().get((p, text)))
}

// ---------------------------------------------------------------------------------------------
// Gutter, current line, status

/// Width of the gutter for `lines` lines: marker column, line numbers, 8 px padding (≥ 48, Figma 49).
pub fn gutter_width(ui: &Ui, lines: usize) -> f32 {
    let digits = lines.max(1).ilog10() as usize + 1;
    let p = palette(ui.ctx());
    let probe = galley(
        ui,
        &"0".repeat(digits.max(2)),
        TextStyle::Mono,
        p.text_tertiary,
    )
    .size()
    .x;
    (space::S1 + 16.0 + space::S1 + probe + space::S3).max(48.0)
}

/// `Gutter` of `NF2/Code Editor`: `bg/panel` column, line numbers (`text/tertiary`, current line
/// `text/primary`) and markers. Paint it after the `TextEdit` so it can use the laid-out galley.
pub struct Gutter<'a> {
    /// 1-based line numbers with a marker; the most severe marker of a line wins.
    pub markers: &'a [(usize, Severity)],
    /// 1-based line of the caret.
    pub current_line: Option<usize>,
}

impl Gutter<'_> {
    /// `rect` is the gutter column (full editor height); `galley_pos` is where the code galley was painted.
    pub fn paint(&self, ui: &Ui, rect: Rect, code: &egui::Galley, galley_pos: egui::Pos2) {
        let p = syntax_palette(ui.ctx());
        self.paint_with_breakpoints(ui, rect, code, galley_pos, &[], &p);
    }

    /// Paint the gutter with optional 1-based `breakpoints` (`i16::BREAKPOINT` in `p.syntax_breakpoint`)
    /// and a resolved syntax [`Palette`].
    pub fn paint_with_breakpoints(
        &self,
        ui: &Ui,
        rect: Rect,
        code: &egui::Galley,
        galley_pos: egui::Pos2,
        breakpoints: &[usize],
        p: &Palette,
    ) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, cr(0.0), p.bg_panel);
        painter.vline(
            rect.right() - 0.5,
            rect.y_range(),
            Stroke::new(1.0_f32, p.border_subtle),
        );
        let mut line = 1;
        let mut first_row = true;
        for row in &code.rows {
            let r = row.rect().translate(galley_pos.to_vec2());
            if first_row && r.bottom() >= rect.top() && r.top() <= rect.bottom() {
                let current = self.current_line == Some(line);
                let color = if current {
                    p.text_primary
                } else {
                    p.text_tertiary
                };
                let g = galley(ui, &line.to_string(), TextStyle::Mono, color);
                let pos = egui::pos2(
                    rect.right() - space::S3 - g.size().x,
                    r.center().y - g.size().y / 2.0,
                );
                painter.galley(pos, g, color);
                let marker = self
                    .markers
                    .iter()
                    .filter(|(l, _)| *l == line)
                    .map(|(_, s)| *s)
                    .max_by_key(|s| *s as u8);
                let has_bp = breakpoints.contains(&line);
                let icon_rect = Rect::from_center_size(
                    egui::pos2(rect.left() + space::S1 + 8.0, r.center().y),
                    egui::vec2(16.0, 16.0),
                );
                match (marker, has_bp) {
                    (Some(sev @ (Severity::Error | Severity::Warning)), _) => {
                        icons::paint(&painter, ui.ctx(), icon_rect, sev.icon(), sev.color(p));
                    }
                    (_, true) => {
                        icons::paint(
                            &painter,
                            ui.ctx(),
                            icon_rect,
                            i16::BREAKPOINT,
                            p.syntax_breakpoint,
                        );
                    }
                    (Some(sev), false) => {
                        icons::paint(&painter, ui.ctx(), icon_rect, sev.icon(), sev.color(p));
                    }
                    (None, false) => {}
                }
            }
            first_row = row.ends_with_newline;
            if row.ends_with_newline {
                line += 1;
            }
        }
    }
}

/// Current-line band (`syntax/current-line`) for the row holding `cursor`, spanning `x_range`.
/// Paint it **below** the text (reserve a shape slot with `painter.add(Shape::Noop)` before the TextEdit).
pub fn current_line(
    ctx: &egui::Context,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    cursor: egui::text::CCursor,
    x_range: egui::Rangef,
) -> egui::Shape {
    let p = syntax_palette(ctx);
    let r = galley
        .pos_from_cursor(cursor)
        .translate(galley_pos.to_vec2());
    let row = Rect::from_x_y_ranges(x_range, r.y_range());
    egui::Shape::rect_filled(row, cr(0.0), p.syntax_current_line)
}

/// 1-based (line, column) of a char index in `text`.
pub fn line_col(text: &str, char_index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for c in text.chars().take(char_index) {
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Char index of a 1-based (line, column) in `text`, clamped to the text.
pub fn char_index(text: &str, line: usize, column: usize) -> usize {
    let mut index = 0;
    let mut current = 1;
    let mut chars = text.chars().peekable();
    while current < line {
        match chars.next() {
            Some('\n') => {
                current += 1;
                index += 1;
            }
            Some(_) => index += 1,
            None => return index,
        }
    }
    let mut col = 1;
    while col < column {
        match chars.next() {
            Some('\n') | None => break,
            Some(_) => {
                index += 1;
                col += 1;
            }
        }
    }
    index
}

/// Script bar status: "Line 19, Col 10 · 20 lines" in `NF2/Mono`, `text/tertiary`, then the problems
/// counter (error / warning icons with counts, or "No errors") when `problems` is known. Right to left.
/// Interactive (`Sense::click()`, hugging its own width) so callers can attach tooltips and
/// click-to-jump diagnostic navigation without overlapping the left-hand script tabs (`T-363`).
pub fn script_status(
    ui: &mut Ui,
    line: usize,
    column: usize,
    lines: usize,
    problems: Option<(usize, usize)>,
) -> Response {
    let p = &syntax_palette(ui.ctx());
    let dim = p.text_tertiary;
    let lines_str = if lines == 1 {
        "1 line".to_owned()
    } else {
        format!("{lines} lines")
    };
    let full_pos_label = format!("Line {line}, Col {column}  ·  {lines_str}");
    let sep_galley = problems.map(|_| galley(ui, "·", TextStyle::Mono, dim));
    let no_errors_galley = problems.and_then(|(e, w)| {
        (e == 0 && w == 0).then(|| galley(ui, "No errors", TextStyle::Mono, dim))
    });
    let err_galley = problems.and_then(|(e, _)| {
        (e > 0).then(|| {
            galley(
                ui,
                &e.to_string(),
                TextStyle::Mono,
                Severity::Error.color(p),
            )
        })
    });
    let warn_galley = problems.and_then(|(_, w)| {
        (w > 0).then(|| {
            galley(
                ui,
                &w.to_string(),
                TextStyle::Mono,
                Severity::Warning.color(p),
            )
        })
    });

    let gap = space::S1;
    let icon_sz = 16.0;
    let mut problems_only_w = 0.0;
    if let Some(ne) = &no_errors_galley {
        problems_only_w += ne.size().x;
    } else {
        if let Some(eg) = &err_galley {
            if problems_only_w > 0.0 {
                problems_only_w += gap;
            }
            problems_only_w += icon_sz + gap + eg.size().x;
        }
        if let Some(wg) = &warn_galley {
            if problems_only_w > 0.0 {
                problems_only_w += gap;
            }
            problems_only_w += icon_sz + gap + wg.size().x;
        }
    }
    let sep_and_problems_w = sep_galley
        .as_ref()
        .map_or(0.0, |sep| gap + sep.size().x + gap + problems_only_w);

    let avail_w = ui.available_width();
    let max_w = if avail_w.is_finite() && avail_w > 0.0 {
        avail_w
    } else {
        f32::INFINITY
    };

    let candidates = [
        full_pos_label.clone(),
        format!("Ln {line}, Col {column}"),
        format!("{line}:{column}"),
    ];
    let mut pos_galley: Option<std::sync::Arc<egui::Galley>> = None;
    for candidate in &candidates {
        let g = galley(ui, candidate, TextStyle::Mono, dim);
        if g.size().x + sep_and_problems_w <= max_w + 0.5 {
            pos_galley = Some(g);
            break;
        }
    }
    if pos_galley.is_none() {
        let pos_budget = max_w - sep_and_problems_w;
        if pos_budget >= 28.0 {
            pos_galley = Some(super::galley_truncated(
                ui,
                &candidates[2],
                TextStyle::Mono,
                dim,
                pos_budget,
            ));
        } else if problems.is_none() {
            pos_galley = Some(super::galley_truncated(
                ui,
                &candidates[2],
                TextStyle::Mono,
                dim,
                max_w.max(1.0),
            ));
        }
    }

    let show_sep = pos_galley.is_some() && sep_galley.is_some();
    let mut total_w = pos_galley.as_ref().map_or(0.0, |g| g.size().x);
    if show_sep {
        total_w += sep_and_problems_w;
    } else if problems.is_some() {
        total_w += problems_only_w;
    }
    let alloc_w = total_w.min(max_w).max(1.0);
    let h = ui
        .available_height()
        .max(TextStyle::Mono.spec().line_height + 4.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(alloc_w, h), Sense::click());
    let has_problems = problems.is_some_and(|(e, w)| e > 0 || w > 0);
    let s = State::of(ui, &response);
    if has_problems && s.hovered {
        ui.painter().rect_filled(rect, cr(radius::SM), p.bg_hover);
    }

    let mut x = rect.left();
    if let Some(pg) = pos_galley {
        let pw = pg.size().x;
        super::text_at(ui, x, rect, pg, dim);
        x += pw;
    }
    if let Some(sep) = sep_galley {
        if show_sep {
            x += gap;
            let sw = sep.size().x;
            super::text_at(ui, x, rect, sep, dim);
            x += sw + gap;
        }
        if let Some(ne) = no_errors_galley {
            super::text_at(ui, x, rect, ne, dim);
        } else {
            let mut drew_any = false;
            if let Some(eg) = err_galley {
                let c = Severity::Error.color(p);
                icons::paint(
                    ui.painter(),
                    ui.ctx(),
                    Rect::from_center_size(
                        egui::pos2(x + icon_sz * 0.5, rect.center().y),
                        egui::vec2(icon_sz, icon_sz),
                    ),
                    Severity::Error.icon(),
                    c,
                );
                x += icon_sz + gap;
                let ew = eg.size().x;
                super::text_at(ui, x, rect, eg, c);
                x += ew;
                drew_any = true;
            }
            if let Some(wg) = warn_galley {
                let c = Severity::Warning.color(p);
                if drew_any {
                    x += gap;
                }
                icons::paint(
                    ui.painter(),
                    ui.ctx(),
                    Rect::from_center_size(
                        egui::pos2(x + icon_sz * 0.5, rect.center().y),
                        egui::vec2(icon_sz, icon_sz),
                    ),
                    Severity::Warning.icon(),
                    c,
                );
                x += icon_sz + gap;
                super::text_at(ui, x, rect, wg, c);
            }
        }
    }
    if has_problems {
        super::focus(ui, s, rect, radius::SM);
    }
    let pos_label = full_pos_label;
    let a11y_label = match problems {
        None => pos_label,
        Some((0, 0)) => format!("{pos_label}  ·  No errors"),
        Some((errors, warnings)) => {
            let mut parts = Vec::new();
            if errors > 0 {
                parts.push(if errors == 1 {
                    "1 error".to_owned()
                } else {
                    format!("{errors} errors")
                });
            }
            if warnings > 0 {
                parts.push(if warnings == 1 {
                    "1 warning".to_owned()
                } else {
                    format!("{warnings} warnings")
                });
            }
            format!("{pos_label}  ·  {}", parts.join(", "))
        }
    };
    let widget_type = if has_problems {
        egui::WidgetType::Button
    } else {
        egui::WidgetType::Label
    };
    a11y(&response, widget_type, &a11y_label, None);
    if has_problems {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

// ---------------------------------------------------------------------------------------------
// Messages

/// `Compiler errors` entry: severity fill, icon, message (`NF2/Body`), location (`NF2/Small`), chevron.
/// Clicking the row is the jump-to-code gesture.
pub struct CompilerMessage<'a> {
    pub severity: Severity,
    pub message: &'a str,
    /// "Scene 1 · layer "orbit" · frame 1 · Line 19"; empty hides the second line.
    pub location: &'a str,
}

impl Widget for CompilerMessage<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let width = ui.available_width();
        let text_w = (width - space::S3 - 16.0 - space::S3 - 16.0 - space::S3 * 2.0).max(40.0);
        let fg = if self.severity == Severity::Error || self.severity == Severity::Warning {
            p.text_primary
        } else {
            self.severity.color(p)
        };
        let mut job = LayoutJob::single_section(
            self.message.to_owned(),
            crate::typography::format(TextStyle::Body, fg),
        );
        job.wrap.max_width = text_w;
        let message = ui.fonts_mut(|f| f.layout_job(job));
        let location = (!self.location.is_empty())
            .then(|| galley(ui, self.location, TextStyle::Small, p.text_secondary));
        let h = space::S2 * 2.0
            + message.size().y
            + location.as_ref().map_or(0.0, |g| g.size().y + 2.0);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, h.max(crate::metrics::row())),
            Sense::click(),
        );
        let s = State::of(ui, &response);
        ui.painter()
            .rect_filled(rect, cr(radius::MD), self.severity.fill(p));
        if s.hovered {
            ui.painter().rect_stroke(
                rect,
                cr(radius::MD),
                Stroke::new(1.0_f32, self.severity.color(p)),
                egui::StrokeKind::Inside,
            );
        }
        let icon = Rect::from_min_size(
            rect.min + egui::vec2(space::S3, space::S2 + 2.0),
            egui::vec2(16.0, 16.0),
        );
        icons::paint(
            ui.painter(),
            ui.ctx(),
            icon,
            self.severity.icon(),
            self.severity.color(p),
        );
        let x = icon.right() + space::S3;
        let message_h = message.size().y;
        ui.painter()
            .galley(egui::pos2(x, rect.top() + space::S2), message, fg);
        if let Some(g) = location {
            ui.painter().galley(
                egui::pos2(x, rect.top() + space::S2 + message_h + 2.0),
                g,
                p.text_secondary,
            );
        }
        let chev = Rect::from_center_size(
            egui::pos2(rect.right() - space::S3 - 8.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        );
        icons::paint(
            ui.painter(),
            ui.ctx(),
            chev,
            i16::CHEVRIGHT,
            if s.hovered {
                p.text_primary
            } else {
                p.text_secondary
            },
        );
        super::focus(ui, s, rect, radius::MD);
        let label = format!(
            "{}: {} {}",
            self.severity.label(),
            self.message,
            self.location
        );
        a11y(&response, egui::WidgetType::Button, label.trim_end(), None);
        response
    }
}

/// One Output log line: severity icon, optional time (`text/tertiary`), wrapped `NF2/Mono` text.
/// Info lines use `text/primary`, warnings amber, errors red.
pub struct LogLine<'a> {
    pub severity: Severity,
    pub time: &'a str,
    pub text: &'a str,
}

impl Widget for LogLine<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let width = ui.available_width();
        let color = match self.severity {
            Severity::Info => p.text_primary,
            other => other.color(p),
        };
        let time = (!self.time.is_empty())
            .then(|| galley(ui, self.time, TextStyle::Mono, p.text_tertiary));
        let x0 =
            space::S2 + 16.0 + space::S2 + time.as_ref().map_or(0.0, |g| g.size().x + space::S3);
        let mut job = LayoutJob::single_section(
            self.text.to_owned(),
            crate::typography::format(TextStyle::Mono, color),
        );
        job.wrap.max_width = (width - x0 - space::S2).max(40.0);
        let text = ui.fonts_mut(|f| f.layout_job(job));
        let line_h = TextStyle::Mono.spec().line_height;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, text.size().y.max(line_h) + space::S1),
            Sense::hover(),
        );
        let top = rect.top() + space::S1 / 2.0;
        let icon = Rect::from_center_size(
            egui::pos2(rect.left() + space::S2 + 8.0, top + line_h / 2.0),
            egui::vec2(16.0, 16.0),
        );
        icons::paint(
            ui.painter(),
            ui.ctx(),
            icon,
            self.severity.icon(),
            self.severity.color(p),
        );
        if let Some(g) = time {
            ui.painter().galley(
                egui::pos2(icon.right() + space::S2, top),
                g,
                p.text_tertiary,
            );
        }
        ui.painter()
            .galley(egui::pos2(rect.left() + x0, top), text, color);
        let label = if self.time.is_empty() {
            format!("{}: {}", self.severity.label(), self.text)
        } else {
            format!("{} {}: {}", self.time, self.severity.label(), self.text)
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
        response
    }
}

// ---------------------------------------------------------------------------------------------
// Diagnostic range underline and Error peek (R-033)

/// Half period of the range underline zigzag (Figma: 47 pt range = 32 half periods).
pub const UNDERLINE_HALF_PERIOD: f32 = 1.46875;
/// Peak-to-peak height of the range underline.
pub const UNDERLINE_AMPLITUDE: f32 = 2.0;
/// Stroke width of the range underline.
pub const UNDERLINE_STROKE: f32 = 1.2;

/// Points of the wavy range underline under `x` (screen range of the exact source range). `y` is the
/// bottom edge of the text line: the wave runs between `y` and `y + 2`, starting low, so it sits in
/// the 4 pt band below the glyphs like `Range underline · error` (Figma `129:18649`).
pub fn range_underline_points(x: egui::Rangef, y: f32) -> Vec<egui::Pos2> {
    let width = x.span().max(0.0);
    let steps = ((width / UNDERLINE_HALF_PERIOD).round() as usize).max(2);
    let dx = width / steps as f32;
    (0..=steps)
        .map(|i| {
            let low = i % 2 == 0;
            egui::pos2(
                x.min + dx * i as f32,
                if low { y + UNDERLINE_AMPLITUDE } else { y },
            )
        })
        .collect()
}

/// Wavy range underline (R-033): 1.2 pt zigzag, 2 pt high, `syntax/error` for errors and
/// `syntax/warning` for warnings (Info / Success use their [`Severity::color`]). The gutter and
/// overview marks stay as they are.
pub fn paint_range_underline(painter: &egui::Painter, x: egui::Rangef, y: f32, severity: Severity) {
    let p = &syntax_palette(painter.ctx());
    painter.add(egui::Shape::line(
        range_underline_points(x, y),
        Stroke::new(UNDERLINE_STROKE, severity.color(p)),
    ));
}

/// Error peek under a diagnostic line (`Error peek` in the Actions panel, warning variant
/// `Warning peek`, Figma `129:18651` / `129:18670`): `danger/subtle` or `warning/subtle` fill, 1 pt
/// `syntax/error` or `syntax/warning` border, radius md, 10 × 6 padding; 16 pt severity icon,
/// `NF2/Small` message and an optional `NF2/Small Strong` accent action ("Quick fix ⌘.", "2 fixes ⌘.").
#[must_use = "call .show(ui)"]
pub struct ErrorPeek<'a> {
    severity: Severity,
    message: &'a str,
    action: Option<&'a str>,
    max_width: Option<f32>,
}

/// Result of [`ErrorPeek::show`].
pub struct ErrorPeekResponse {
    pub response: Response,
    /// The action text (quick fix) when shown; click = run it.
    pub action: Option<Response>,
}

impl ErrorPeekResponse {
    pub fn action_clicked(&self) -> bool {
        self.action.as_ref().is_some_and(Response::clicked)
    }
}

impl<'a> ErrorPeek<'a> {
    /// `Error` or `Warning` (other severities use the neutral `bg/hover` fill).
    pub fn new(severity: Severity, message: &'a str) -> Self {
        Self {
            severity,
            message,
            action: None,
            max_width: None,
        }
    }
    pub fn action(mut self, action: &'a str) -> Self {
        self.action = Some(action);
        self
    }
    /// Cap the hugging width (the message ends with "…").
    pub fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    pub fn show(self, ui: &mut Ui) -> ErrorPeekResponse {
        let p = &syntax_palette(ui.ctx());
        let pad = egui::vec2(10.0, 6.0);
        let action = self
            .action
            .map(|a| galley(ui, a, TextStyle::SmallStrong, p.accent_default));
        let action_w = action.as_ref().map_or(0.0, |g| space::S2 + g.size().x);
        let fixed = pad.x * 2.0 + 16.0 + space::S2 + action_w;
        let max = self.max_width.unwrap_or(f32::INFINITY);
        let message = super::galley_truncated(
            ui,
            self.message,
            TextStyle::Small,
            p.text_primary,
            (max - fixed).max(24.0),
        );
        let width = fixed + message.size().x;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 16.0 + pad.y * 2.0 + 2.0), Sense::hover());
        let border = match self.severity {
            Severity::Error | Severity::Warning => self.severity.color(p),
            _ => p.border_default,
        };
        let mut out_action = None;
        if ui.is_rect_visible(rect) {
            ui.painter()
                .rect_filled(rect, cr(radius::MD), self.severity.fill(p));
            ui.painter().rect_stroke(
                rect,
                cr(radius::MD),
                Stroke::new(1.0_f32, border),
                egui::StrokeKind::Inside,
            );
            let icon = Rect::from_min_size(
                egui::pos2(rect.left() + pad.x, rect.center().y - 8.0),
                egui::vec2(16.0, 16.0),
            );
            icons::paint(
                ui.painter(),
                ui.ctx(),
                icon,
                self.severity.icon(),
                self.severity.color(p),
            );
            let x = icon.right() + space::S2;
            let mw = message.size().x;
            super::text_at(ui, x, rect, message, p.text_primary);
            if let Some(g) = action {
                let ax = x + mw + space::S2;
                let hit = Rect::from_min_size(
                    egui::pos2(ax, rect.top()),
                    egui::vec2(g.size().x, rect.height()),
                );
                let r = ui.interact(hit, response.id.with("action"), Sense::click());
                let c = if r.hovered() {
                    p.accent_hover
                } else {
                    p.accent_default
                };
                super::text_at(ui, ax, rect, g, c);
                if let Some(label) = self.action {
                    a11y(&r, egui::WidgetType::Button, label, None);
                }
                out_action = Some(r.on_hover_cursor(egui::CursorIcon::PointingHand));
            }
        }
        let label = format!("{}: {}", self.severity.label(), self.message);
        a11y(&response, egui::WidgetType::Label, &label, None);
        ErrorPeekResponse {
            response,
            action: out_action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_underline_matches_the_reference_wave() {
        // Figma `Range underline · error`: 47 pt wide, 32 half periods, y 1…3 in a 4 pt band.
        let pts = range_underline_points(egui::Rangef::new(10.0, 57.0), 100.0);
        assert_eq!(pts.len(), 33);
        assert_eq!(pts[0], egui::pos2(10.0, 102.0));
        assert_eq!(pts[1].y, 100.0);
        assert!((pts[32].x - 57.0).abs() < 1e-4);
        // A one-character range still gets a visible wave.
        assert!(range_underline_points(egui::Rangef::new(0.0, 1.0), 0.0).len() >= 3);
    }

    fn kinds(src: &str) -> Vec<(&str, Token)> {
        tokens(src)
            .into_iter()
            .map(|(r, t)| (&src[r], t))
            .filter(|(s, _)| !s.trim().is_empty())
            .collect()
    }

    #[test]
    fn tokens_cover_the_text_and_classify_script_and_xml() {
        let src = "import fl.motion.Animator;\nvar xml:XML = <Motion duration=\"20\"><a/></Motion>;\n// note\nvar n:int = 0x1F; trace(\"é\");\nif (a < b) { f(); }";
        let runs = tokens(src);
        assert_eq!(runs.first().unwrap().0.start, 0);
        assert_eq!(runs.last().unwrap().0.end, src.len());
        assert!(runs.windows(2).all(|w| w[0].0.end == w[1].0.start));
        let k = kinds(src);
        assert!(k.contains(&("import", Token::Keyword)));
        assert!(k.contains(&("XML", Token::Type)));
        assert!(k.contains(&("<Motion", Token::Tag)));
        assert!(k.contains(&("duration", Token::Attr)));
        assert!(k.contains(&("\"20\"", Token::String)));
        assert!(k.contains(&("// note", Token::Comment)));
        assert!(k.contains(&("int", Token::Type)));
        assert!(k.contains(&("0x1F", Token::Number)));
        assert!(k.contains(&("trace", Token::Function)));
        assert!(k.contains(&("\"é\"", Token::String)));
        assert!(
            k.iter().any(|(s, t)| s.contains('<') && *t == Token::Punct),
            "a < b is an operator: {k:?}"
        );
    }

    #[test]
    fn t375_identifiers_highlight_consistently_regardless_of_case_while_types_stay_distinct() {
        let tetris = "\
TFigure = new Array([0, 1, 0], [1, 1, 1]);
lFigure = new Array([0, 1, 0], [0, 1, 0]);
QFigure = new Array([1, 1], [1, 1]);
AllFigures = new Array(TFigure, lFigure, QFigure);
function rotateFigure(figure, direction) {
    var n = figure.length;
    var m = figure[0].length;
    var tempFigure:Array = new Array();
    for (i = 0; i < m; i++) {
        tempFigure[i] = new Array();
    }
}";
        let k = kinds(tetris);
        for ident in [
            "TFigure",
            "lFigure",
            "QFigure",
            "AllFigures",
            "figure",
            "direction",
            "n",
            "m",
            "length",
            "tempFigure",
            "i",
        ] {
            assert!(
                k.iter()
                    .filter(|(s, _)| *s == ident)
                    .all(|(_, t)| *t == Token::Identifier),
                "expected `{ident}` to be Token::Identifier everywhere in {k:?}"
            );
            assert!(
                k.iter()
                    .any(|(s, t)| *s == ident && *t == Token::Identifier),
                "missing `{ident}` in {k:?}"
            );
        }
        assert!(
            k.iter()
                .filter(|(s, _)| *s == "Array")
                .all(|(_, t)| *t == Token::Type),
            "expected `Array` in `:Array` and `new Array(...)` to be Token::Type: {k:?}"
        );
        assert!(k.contains(&("rotateFigure", Token::Function)));
        for kw in ["new", "function", "var", "for"] {
            assert!(k.contains(&(kw, Token::Keyword)));
        }

        // Object literals, ternaries, and switch cases must NOT misclassify values after `:` as types,
        // while `class`, `extends`, `implements`, `new`, `is`, `as`, `type`, and `: CustomType` DO.
        let contexts = "\
class Board extends MovieClip implements IGrid {
    var cell:CustomTile = new CustomTile();
    var vec:Vector.<int> = new Vector.<int>();
    var map = { piece: TFigure, alt: lFigure };
    var pick = ready ? TFigure : lFigure;
    function check(x:CustomTile):Boolean {
        switch (x) {
            case TFigure:
                return x is CustomTile && (x as CustomTile) instanceof Sprite;
            default:
                return false;
        }
    }
}
type GridRow = Array<number>;";
        let ck = kinds(contexts);
        for ty in [
            "Board",
            "MovieClip",
            "IGrid",
            "CustomTile",
            "Vector",
            "int",
            "Boolean",
            "Sprite",
            "GridRow",
            "Array",
            "number",
        ] {
            assert!(
                ck.iter()
                    .filter(|(s, _)| *s == ty)
                    .all(|(_, t)| *t == Token::Type),
                "expected `{ty}` to be Token::Type: {ck:?}"
            );
        }
        for id in [
            "cell", "vec", "map", "piece", "TFigure", "alt", "lFigure", "pick", "ready", "x",
        ] {
            assert!(
                ck.iter()
                    .filter(|(s, _)| *s == id)
                    .all(|(_, t)| *t == Token::Identifier),
                "expected `{id}` to be Token::Identifier: {ck:?}"
            );
        }

        // Regression coverage: `:* {`, `:void {`, `case ...: {`, `label: {`, TS `{ a: MyType }`, and 2D generics.
        let edge_blocks = "\
function handle(e:Event):* {
    var inUntyped:StageItem = rawVal;
    var grid:Vector.<Vector.<CellNode>> = new Vector.<Vector.<CellNode>>();
    var typedObj: { prop: PropType; second: NextType } = { prop: firstVal, second: secondVal };
    outerLabel: {
        var inLabeled:LabelType = labelVal;
    }
    switch (mode) {
        case (flag ? 1 : 2): {
            var inCaseBlock:CaseType = caseVal;
            break;
        }
        case build({ k: innerVal }):
            var afterObjCase:AfterCaseType = afterVal;
            break;
    }
}
function resetAll():void {
    var inVoid:VoidBodyType = voidVal;
}";
        let ek = kinds(edge_blocks);
        for ty in [
            "Event",
            "StageItem",
            "Vector",
            "CellNode",
            "PropType",
            "NextType",
            "LabelType",
            "CaseType",
            "AfterCaseType",
            "VoidBodyType",
        ] {
            assert!(
                ek.iter()
                    .filter(|(s, _)| *s == ty)
                    .all(|(_, t)| *t == Token::Type),
                "expected `{ty}` to be Token::Type in edge_blocks: {ek:?}"
            );
        }
        for id in [
            "inUntyped",
            "rawVal",
            "grid",
            "typedObj",
            "prop",
            "second",
            "firstVal",
            "secondVal",
            "outerLabel",
            "inLabeled",
            "labelVal",
            "mode",
            "flag",
            "inCaseBlock",
            "caseVal",
            "k",
            "innerVal",
            "afterObjCase",
            "afterVal",
        ] {
            assert!(
                ek.iter()
                    .filter(|(s, _)| *s == id)
                    .all(|(_, t)| *t == Token::Identifier),
                "expected `{id}` to be Token::Identifier in edge_blocks: {ek:?}"
            );
        }
    }

    #[test]
    fn line_col_round_trips() {
        let text = "ab\ncde\n\nf";
        for i in 0..=text.chars().count() {
            let (l, c) = line_col(text, i);
            assert_eq!(char_index(text, l, c), i);
        }
        assert_eq!(char_index(text, 2, 99), 6);
        assert_eq!(char_index(text, 99, 1), text.chars().count());
    }
}
