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
    /// Identifiers and whitespace (`text/primary`).
    Plain,
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

/// Lower-case built-in types coloured as types after a `:` annotation (`var n:int`, `x: number`).
const ANNOTATION_TYPES: &[&str] = &[
    "int", "uint", "any", "number", "string", "boolean", "never", "unknown", "object", "symbol",
    "bigint",
];

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
    let mut in_tag = false;
    let mut tag_name = false;
    let mut after_function = false;
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
            && (ident_start(at(i + 1)) || at(i + 1) == b'/')
            && !(ident_char(prev) || prev == b')' || prev == b']')
        {
            i += 1;
            if at(i) == b'/' {
                i += 1;
            }
            in_tag = true;
            tag_name = true;
            Token::Tag
        } else if c.is_ascii_digit() || (c == b'.' && at(i + 1).is_ascii_digit()) {
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
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
            let t = if after_function && !KEYWORDS.contains(&word) {
                Token::Function
            } else if KEYWORDS.contains(&word) {
                Token::Keyword
            } else if at(j) == b'(' {
                Token::Function
            } else if word.as_bytes()[0].is_ascii_uppercase()
                || (prev == b':' && ANNOTATION_TYPES.contains(&word))
            {
                Token::Type
            } else {
                Token::Plain
            };
            after_function = matches!(word, "function" | "get" | "set")
                && (word == "function" || after_function);
            push(start..i, t);
            prev = b[i - 1];
            continue;
        } else {
            i += 1;
            Token::Punct
        };
        after_function = false;
        push(start..i, token);
        prev = b[i - 1];
    }
    out
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

impl egui::cache::ComputerMut<(Theme, &str), LayoutJob> for Highlighter {
    fn compute(&mut self, (theme, text): (Theme, &str)) -> LayoutJob {
        layout_job(text, theme.palette())
    }
}

type HighlightCache = egui::cache::FrameCache<LayoutJob, Highlighter>;

/// Syntax-coloured `LayoutJob` for a code `TextEdit` layouter, cached per text and theme.
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
    let theme = Theme::current(ctx);
    ctx.memory_mut(|m| m.caches.cache::<HighlightCache>().get((theme, text)))
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
        let p = palette(ui.ctx());
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
                if let Some(severity) = marker {
                    let icon = Rect::from_center_size(
                        egui::pos2(rect.left() + space::S1 + 8.0, r.center().y),
                        egui::vec2(16.0, 16.0),
                    );
                    icons::paint(&painter, ui.ctx(), icon, severity.icon(), severity.color(p));
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
    let p = palette(ctx);
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
pub fn script_status(
    ui: &mut Ui,
    line: usize,
    column: usize,
    lines: usize,
    problems: Option<(usize, usize)>,
) -> Response {
    let p = palette(ui.ctx());
    let dim = p.text_tertiary;
    let response = ui
        .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::S1;
            if let Some((errors, warnings)) = problems {
                if errors == 0 && warnings == 0 {
                    ui.label(crate::typography::rich("No errors", TextStyle::Mono).color(dim));
                } else {
                    for (count, severity) in
                        [(warnings, Severity::Warning), (errors, Severity::Error)]
                    {
                        if count > 0 {
                            let c = severity.color(p);
                            ui.label(
                                crate::typography::rich(count.to_string(), TextStyle::Mono)
                                    .color(c),
                            );
                            icons::show(ui, severity.icon(), c);
                        }
                    }
                }
                ui.label(crate::typography::rich("·", TextStyle::Mono).color(dim));
            }
            let lines = if lines == 1 {
                "1 line".to_owned()
            } else {
                format!("{lines} lines")
            };
            ui.label(
                crate::typography::rich(
                    format!("Line {line}, Col {column}  ·  {lines}"),
                    TextStyle::Mono,
                )
                .color(dim),
            );
        })
        .response;
    let text = format!("Line {line}, column {column}, {lines} lines");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
    response
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
            egui::vec2(width, h.max(crate::metrics::ROW)),
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

#[cfg(test)]
mod tests {
    use super::*;

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
