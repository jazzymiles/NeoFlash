//! ActionScript 1 text for decoded authoring commands.
//!
//! Each command maps to the statement the Flash Actions panel displays for it.
//! Two constructs the NeoFlash AVM1 frontend does not parse use equivalent
//! forms it compiles, annotated with the original: `tellTarget (p)` becomes
//! `with (eval(p))` and numeric `ifFrameLoaded (n)` becomes
//! `if (_framesloaded >= n)`. Anything that cannot be rendered exactly (unknown
//! selector values, mismatched blocks, ANSI text needing a code page, AS2/AS3
//! declarations) is kept as a `// NeoFlash: untranslated` comment carrying its
//! typed fields; a body whose opener is untranslated is commented out with it
//! rather than run unconditionally.
use super::*;

const HISTORICAL_OPEN: [(u32, u32); 5] = [(14, 15), (18, 19), (20, 21), (22, 24), (25, 26)];
const COMPILED_OPEN: [(u32, u32); 22] = [
    (8, 9),
    (10, 19),
    (11, 12),
    (13, 14),
    (15, 18),
    (20, 21),
    (37, 38),
    (42, 43),
    (44, 45),
    (47, 48),
    (50, 51),
    (53, 54),
    (57, 58),
    (60, 61),
    (63, 64),
    (65, 66),
    (67, 68),
    (71, 72),
    (73, 74),
    (76, 77),
    (80, 81),
    (84, 85),
];
const PROPERTIES: [&str; 22] = [
    "_x",
    "_y",
    "_xscale",
    "_yscale",
    "_currentframe",
    "_totalframes",
    "_alpha",
    "_visible",
    "_width",
    "_height",
    "_rotation",
    "_target",
    "_framesloaded",
    "_name",
    "_droptarget",
    "_url",
    "_highquality",
    "_focusrect",
    "_soundbuftime",
    "_quality",
    "_xmouse",
    "_ymouse",
];
const MOUSE_EVENTS: [&str; 7] = [
    "press",
    "release",
    "releaseOutside",
    "rollOver",
    "rollOut",
    "dragOver",
    "dragOut",
];
const CLIP_EVENTS: [&str; 9] = [
    "load",
    "enterFrame",
    "unload",
    "mouseMove",
    "mouseDown",
    "mouseUp",
    "keyDown",
    "keyUp",
    "data",
];

type Fail = String;
type Translation = std::result::Result<Emit, Fail>;

enum Emit {
    Nothing,
    Stmt(String),
    /// Opening line and the command kind that closes it.
    Open(String, u32),
    /// Statement whose closing command is consumed without output.
    OpenSilent(String, u32),
    /// Branch line valid only directly inside the given closer's block.
    Branch(String, u32),
    /// Close the block; optional custom closing line.
    Close(u32, Option<String>),
}

struct Cmd<'a> {
    data: &'a [u8],
    command: &'a TimelineActionCommand,
}
impl Cmd<'_> {
    fn field(&self, name: &str) -> Option<&TimelineActionValue> {
        self.command
            .fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| &f.value)
    }
    fn number(&self, name: &str) -> i64 {
        match self.field(name) {
            Some(TimelineActionValue::Byte(v)) => (*v).into(),
            Some(TimelineActionValue::Word(v)) => (*v).into(),
            Some(TimelineActionValue::Signed(v)) => (*v).into(),
            Some(TimelineActionValue::Dword(v)) => (*v).into(),
            _ => 0,
        }
    }
    fn text(&self, name: &str) -> std::result::Result<String, Fail> {
        match self.field(name) {
            Some(TimelineActionValue::String(s)) => decode(self.data, s),
            _ => Ok(String::new()),
        }
    }
    fn parameter(&self, n: usize) -> std::result::Result<(String, u32), Fail> {
        let Some(TimelineActionValue::Parameter { text, flags }) = self
            .command
            .fields
            .iter()
            .filter(|f| matches!(f.value, TimelineActionValue::Parameter { .. }))
            .nth(n)
            .map(|f| &f.value)
        else {
            return Err("missing parameter".into());
        };
        Ok((decode(self.data, text)?, *flags))
    }
    /// Literal (flags 0) parameters are quoted strings; others are expressions.
    fn param(&self, n: usize) -> std::result::Result<String, Fail> {
        let (text, flags) = self.parameter(n)?;
        Ok(if flags == 0 { quote(&text) } else { text })
    }
    fn param_text(&self, n: usize) -> std::result::Result<String, Fail> {
        Ok(self.parameter(n)?.0)
    }
    /// Historical operand: an expression from revision 7 when flagged, else a literal.
    fn operand(&self, name: &str) -> std::result::Result<String, Fail> {
        let text = self.text(name)?;
        Ok(if self.number(&format!("{name}_is_expression")) != 0 {
            text
        } else {
            quote(&text)
        })
    }
    fn expression(&self, name: &str) -> std::result::Result<String, Fail> {
        let text = self.text(name)?;
        if text.trim().is_empty() {
            return Err(format!("empty {name} expression"));
        }
        Ok(text)
    }
}

fn decode(data: &[u8], value: &TimelineString) -> std::result::Result<String, Fail> {
    let raw = data
        .get(value.data.start..value.data.end)
        .ok_or("string span outside stream")?;
    if value.utf16 {
        String::from_utf16(
            &raw.as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "invalid UTF-16 text".into())
    } else if raw.is_ascii() {
        Ok(String::from_utf8(raw.to_vec()).expect("ASCII"))
    } else {
        Err("ANSI text needs an explicit source code page".into())
    }
}

pub(super) fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 32 || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !matches!(
            name,
            "this"
                | "var"
                | "function"
                | "if"
                | "else"
                | "while"
                | "for"
                | "return"
                | "new"
                | "delete"
                | "typeof"
                | "void"
                | "with"
                | "in"
                | "true"
                | "false"
                | "null"
                | "undefined"
                | "break"
                | "continue"
                | "do"
                | "switch"
                | "case"
                | "default"
                | "instanceof"
                | "and"
                | "or"
                | "not"
                | "eq"
                | "ne"
                | "lt"
                | "gt"
                | "le"
                | "ge"
                | "add"
                | "on"
                | "onClipEvent"
                | "tellTarget"
                | "ifFrameLoaded"
        )
}

/// Button key code names used by both the historical and compiled forms.
fn key(code: i64) -> std::result::Result<String, Fail> {
    let name = match code {
        1 => "<Left>",
        2 => "<Right>",
        3 => "<Home>",
        4 => "<End>",
        5 => "<Insert>",
        6 => "<Delete>",
        8 => "<Backspace>",
        13 => "<Enter>",
        14 => "<Up>",
        15 => "<Down>",
        16 => "<PageUp>",
        17 => "<PageDown>",
        18 => "<Tab>",
        19 => "<Escape>",
        32 => "<Space>",
        33..=126 => return Ok(quote(&char::from(code as u8).to_string())),
        _ => return Err(format!("unassigned key code {code}")),
    };
    Ok(quote(name))
}

fn assign(name: String, literal: bool, value: String) -> String {
    if literal && identifier(&name) {
        format!("{name} = {value};")
    } else {
        let name = if literal { quote(&name) } else { name };
        format!("set({name}, {value});")
    }
}

/// `tellTarget` in the form the AVM1 frontend accepts: the target becomes the
/// scope object, so unqualified timeline calls address it.
fn tell_target(target: String, literal: bool) -> String {
    let original = if literal {
        quote(&target)
    } else {
        target.clone()
    };
    let scope = if literal && target.is_empty() {
        "this".to_owned()
    } else {
        format!("eval({original})")
    };
    format!("with ({scope}) {{ // tellTarget ({original})")
}

fn frame_loaded(scene: &str, frame: String, numeric: bool) -> String {
    if scene.is_empty() && numeric {
        format!("if (_framesloaded >= {frame}) {{ // ifFrameLoaded ({frame})")
    } else if scene.is_empty() {
        format!("ifFrameLoaded ({frame}) {{")
    } else {
        format!("ifFrameLoaded ({}, {frame}) {{", quote(scene))
    }
}

fn historical_frame(c: &Cmd) -> std::result::Result<(String, String, bool), Fail> {
    let page = c.text("page")?;
    let (value, numeric) = if !c.text("arg0")?.is_empty() {
        (c.operand("arg0")?, false)
    } else if !c.text("label")?.is_empty() {
        (quote(&c.text("label")?), false)
    } else {
        (c.number("frame_number").to_string(), true)
    };
    Ok((page, value, numeric))
}

fn goto(page: &str, frame: &str, play: bool) -> String {
    let call = if play { "gotoAndPlay" } else { "gotoAndStop" };
    if page.is_empty() {
        format!("{call}({frame});")
    } else {
        format!("{call}({}, {frame});", quote(page))
    }
}

fn historical(c: &Cmd) -> Translation {
    let url = || -> std::result::Result<String, Fail> {
        let method = c.text("method")?;
        Ok(format!(
            "getURL({}, {}{});",
            c.operand("url")?,
            c.operand("window")?,
            if method.is_empty() {
                String::new()
            } else {
                format!(", {}", quote(&method))
            }
        ))
    };
    let go = |play: bool| -> std::result::Result<String, Fail> {
        let (page, frame, _) = historical_frame(c)?;
        Ok(goto(&page, &frame, play))
    };
    Ok(match c.command.kind {
        0 => Emit::Nothing,
        1 => Emit::Stmt("play();".into()),
        2 => Emit::Stmt("stop();".into()),
        3 => Emit::Stmt("nextFrame();".into()),
        4 => Emit::Stmt("prevFrame();".into()),
        5 => Emit::Stmt("nextScene();".into()),
        6 => Emit::Stmt("prevScene();".into()),
        7 => Emit::Stmt("toggleHighQuality();".into()),
        8 => Emit::Stmt(go(false)?),
        10 => Emit::Stmt(go(true)?),
        9 => Emit::Stmt(url()?),
        11 => Emit::Stmt(format!("{}\n{}", url()?, go(false)?)),
        12 => Emit::Stmt(format!("{}\n{}", url()?, go(true)?)),
        13 => Emit::Stmt("stopAllSounds();".into()),
        14 => {
            let literal = c.number("target_is_expression") == 0;
            Emit::Open(tell_target(c.text("target")?, literal), 15)
        }
        15 => Emit::Close(15, None),
        16 => Emit::Stmt(format!(
            "fscommand({}, {});",
            c.operand("arg0")?,
            c.operand("arg1")?
        )),
        17 => {
            let target = c.text("target")?;
            let function = if c.number("load_variables") != 0 {
                "loadVariables"
            } else {
                "loadMovie"
            };
            let destination = if !target.is_empty() {
                c.operand("target")?
            } else if c.text("arg2")?.is_empty() {
                c.number("level").to_string()
            } else {
                c.operand("arg2")?
            };
            let method = c.text("method")?;
            Emit::Stmt(format!(
                "{function}{}({}, {destination}{});",
                if target.is_empty() { "Num" } else { "" },
                c.operand("url")?,
                if method.is_empty() {
                    String::new()
                } else {
                    format!(", {}", quote(&method))
                }
            ))
        }
        18 => {
            let (page, frame, numeric) = historical_frame(c)?;
            Emit::Open(frame_loaded(&page, frame, numeric), 19)
        }
        19 => Emit::Close(19, None),
        20 => {
            let mut events: Vec<String> = [
                ("press", "press"),
                ("release", "release"),
                ("release_outside", "releaseOutside"),
                ("roll_over", "rollOver"),
                ("roll_out", "rollOut"),
                ("drag_over", "dragOver"),
                ("drag_out", "dragOut"),
            ]
            .into_iter()
            .filter(|(field, _)| c.number(field) != 0)
            .map(|(_, event)| event.to_owned())
            .collect();
            if c.number("on_key_press") != 0 {
                events.push(format!("keyPress {}", key(c.number("key"))?));
            }
            if events.is_empty() {
                return Err("mouse event without events".into());
            }
            Emit::Open(format!("on ({}) {{", events.join(", ")), 21)
        }
        21 => Emit::Close(21, None),
        22 => Emit::Open(format!("if ({}) {{", c.expression("arg0")?), 24),
        23 => Emit::Branch("} else {".into(), 24),
        32 => Emit::Branch(format!("}} else if ({}) {{", c.expression("arg0")?), 24),
        24 => Emit::Close(24, None),
        25 => Emit::Open(format!("while ({}) {{", c.expression("arg0")?), 26),
        26 => Emit::Close(26, None),
        28 => Emit::Stmt(format!("call({});", c.operand("arg0")?)),
        29 => Emit::Stmt(assign(
            c.text("variable_name")?,
            c.number("variable_name_is_expression") == 0,
            c.operand("arg0")?,
        )),
        30 if c.field("property").is_none() => {
            return Err("setProperty without a property selector".into());
        }
        30 => {
            let property = usize::try_from(c.number("property"))
                .ok()
                .and_then(|p| PROPERTIES.get(p))
                .ok_or("unassigned property selector")?;
            Emit::Stmt(format!(
                "setProperty({}, {property}, {});",
                c.operand("target")?,
                c.operand("arg0")?
            ))
        }
        34 if c.number("start") == 0 => Emit::Stmt("stopDrag();".into()),
        34 => {
            let mut args = format!(
                "{}, {}",
                c.operand("target")?,
                c.number("lock_mouse_to_center") != 0
            );
            if c.number("constrain_to_rectangle") != 0 {
                for name in ["arg0", "arg1", "arg2", "arg3"] {
                    args.push_str(&format!(", {}", c.expression(name)?));
                }
            }
            Emit::Stmt(format!("startDrag({args});"))
        }
        35 => Emit::Stmt(format!(
            "duplicateMovieClip({}, {}, {});",
            c.operand("target")?,
            c.operand("arg0")?,
            c.expression("depth")?
        )),
        36 => Emit::Stmt(format!("removeMovieClip({});", c.operand("target")?)),
        37 => Emit::Stmt(comment(&c.text("arg0")?)),
        38 => Emit::Stmt(format!("trace({});", c.operand("arg0")?)),
        kind => return Err(format!("unassigned historical command kind {kind}")),
    })
}

fn comment(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    text.split('\n')
        .map(|line| format!("// {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn request_method(id: i64) -> std::result::Result<&'static str, Fail> {
    match id {
        0 => Ok(""),
        1 => Ok(", \"GET\""),
        2 => Ok(", \"POST\""),
        _ => Err(format!("unassigned request method {id}")),
    }
}

fn compiled(c: &Cmd) -> Translation {
    let t = |name: &str| c.text(name);
    Ok(match c.command.kind {
        0 => {
            let mode = c.number("navigation_mode");
            if let 3..=6 = mode {
                let call =
                    ["nextFrame", "prevFrame", "nextScene", "prevScene"][(mode - 3) as usize];
                return Ok(Emit::Stmt(format!("{call}();")));
            }
            let frame = t("frame")?;
            let frame = match mode {
                0 if !frame.is_empty() && frame.bytes().all(|b| b.is_ascii_digit()) => frame,
                1 => quote(&frame),
                2 if !frame.trim().is_empty() => frame,
                _ => return Err(format!("unassigned navigation mode {mode}")),
            };
            Emit::Stmt(goto(&t("scene")?, &frame, c.number("play") != 0))
        }
        1 => Emit::Stmt("play();".into()),
        2 => Emit::Stmt("stop();".into()),
        3 => Emit::Stmt("toggleHighQuality();".into()),
        4 => Emit::Stmt("stopAllSounds();".into()),
        5 => {
            let value = |name: &str, flags: &str| -> std::result::Result<String, Fail> {
                let text = t(name)?;
                Ok(if c.number(flags) == 0 {
                    quote(&text)
                } else {
                    text
                })
            };
            Emit::Stmt(format!(
                "getURL({}, {}{});",
                value("url", "url_flags")?,
                value("window", "window_flags")?,
                request_method(c.number("method"))?
            ))
        }
        6 => Emit::Stmt(format!("fscommand({}, {});", c.param(0)?, c.param(1)?)),
        7 | 32 => {
            let level = c.number("target_mode") == 0;
            let base = if c.command.kind == 7 {
                "loadMovie"
            } else {
                "loadVariables"
            };
            Emit::Stmt(format!(
                "{base}{}({}, {}{});",
                if level { "Num" } else { "" },
                c.param(0)?,
                if level { c.param_text(1)? } else { c.param(1)? },
                request_method(c.number("request_method"))?
            ))
        }
        8 => {
            let (target, flags) = c.parameter(0)?;
            Emit::Open(tell_target(target, flags == 0), 9)
        }
        10 => {
            let frame = t("frame")?;
            let (frame, numeric) = match c.number("frame_mode") {
                0 if !frame.is_empty() && frame.bytes().all(|b| b.is_ascii_digit()) => {
                    (frame, true)
                }
                1 => (quote(&frame), false),
                2 if !frame.trim().is_empty() => (frame, false),
                mode => return Err(format!("unassigned frame-loaded mode {mode}")),
            };
            Emit::Open(frame_loaded(&t("scene")?, frame, numeric), 19)
        }
        11 => {
            let flags = c.number("event_flags");
            let events = if flags & 0x100 != 0 {
                let text = t("event_string")?;
                if text.trim().is_empty() {
                    return Err("empty event string".into());
                }
                text
            } else {
                if flags & !0xff != 0 {
                    return Err(format!("unassigned mouse event bits {flags:#x}"));
                }
                let mut events: Vec<String> = (0..7)
                    .filter(|bit| flags & (1 << bit) != 0)
                    .map(|bit| MOUSE_EVENTS[bit].to_owned())
                    .collect();
                if flags & 0x80 != 0 {
                    events.push(format!("keyPress {}", key(c.number("key_code"))?));
                }
                if events.is_empty() {
                    return Err("mouse event without events".into());
                }
                events.join(", ")
            };
            Emit::Open(format!("on ({events}) {{"), 12)
        }
        13 => {
            let flags = c.number("event_flags");
            if flags == 0 || flags & !0x1ff != 0 {
                return Err(format!("unassigned clip event bits {flags:#x}"));
            }
            let events: Vec<_> = (0..9)
                .filter(|bit| flags & (1 << bit) != 0)
                .map(|bit| CLIP_EVENTS[bit])
                .collect();
            Emit::Open(format!("onClipEvent ({}) {{", events.join(", ")), 14)
        }
        15 => Emit::Open(format!("if ({}) {{", c.expression("condition")?), 18),
        16 => Emit::Branch("} else {".into(), 18),
        17 => Emit::Branch(
            format!("}} else if ({}) {{", c.expression("condition")?),
            18,
        ),
        20 if c.number("empty_body") != 0 => {
            Emit::OpenSilent(format!("while ({});", c.expression("condition")?), 21)
        }
        20 => Emit::Open(format!("while ({}) {{", c.expression("condition")?), 21),
        22 => Emit::Stmt(format!("call({});", c.param(0)?)),
        23 => {
            let property = usize::try_from(c.number("property_index"))
                .ok()
                .and_then(|p| PROPERTIES.get(p))
                .ok_or("unassigned property selector")?;
            Emit::Stmt(format!(
                "setProperty({}, {property}, {});",
                c.param(0)?,
                c.param(1)?
            ))
        }
        24 => {
            let (name, flags) = c.parameter(0)?;
            Emit::Stmt(assign(name, flags == 0, c.param(1)?))
        }
        25 => Emit::Stmt(format!(
            "duplicateMovieClip({}, {}, {});",
            c.param(0)?,
            c.param(1)?,
            c.expression("depth")?
        )),
        26 => {
            let lock = c.number("lock_center") != 0;
            let constrain = c.number("constrain") != 0;
            let mut args = c.param(0)?;
            if lock || constrain {
                args.push_str(&format!(", {lock}"));
            }
            if constrain {
                for bound in ["left", "top", "right", "bottom"] {
                    args.push_str(&format!(", {}", c.expression(bound)?));
                }
            }
            Emit::Stmt(format!("startDrag({args});"))
        }
        27 => Emit::Stmt(format!("trace({});", c.param(0)?)),
        28 if c.number("comment_style") != 0 => {
            let text = t("comment")?;
            if text.contains("*/") {
                return Err("block comment contains its terminator".into());
            }
            Emit::Stmt(format!("/*{text}*/"))
        }
        28 => Emit::Stmt(comment(&t("comment")?)),
        29 => Emit::Stmt("stopDrag();".into()),
        30 => Emit::Stmt(format!("removeMovieClip({});", c.param(0)?)),
        31 => Emit::Stmt(if c.number("target_mode") == 0 {
            format!("unloadMovieNum({});", c.param_text(0)?)
        } else {
            format!("unloadMovie({});", c.param(0)?)
        }),
        33 => Emit::Stmt(format!("#include {}", quote(&t("include_path")?))),
        34 => {
            let bounds = ["bmovie", "bframe", "bmax"]
                .get(c.number("bounds_mode") as usize)
                .ok_or("unassigned print bounds")?;
            let level = c.number("target_mode") == 0;
            Emit::Stmt(format!(
                "print{}{}({}, \"{bounds}\");",
                if c.number("print_as_bitmap") != 0 {
                    "AsBitmap"
                } else {
                    ""
                },
                if level { "Num" } else { "" },
                if level { c.param_text(0)? } else { c.param(0)? }
            ))
        }
        36 => Emit::Stmt(format!("delete {};", c.param_text(0)?)),
        37 => {
            let flags = c.number("function_flags");
            if flags != 0 {
                return Err("class member function flags require ActionScript 2".into());
            }
            let signature = t("function_signature")?;
            if signature.contains(':') {
                return Err("typed function signature requires ActionScript 2".into());
            }
            let head = format!("function {signature}({})", t("parameters")?);
            if c.number("declaration_only") != 0 {
                return Err("function declaration without body".into());
            }
            Emit::Open(format!("{head} {{"), 38)
        }
        39 if c.number("variable_flags") == 0 => {
            Emit::Stmt(format!("var {};", c.expression("declaration")?))
        }
        40 => {
            let value = t("return_value")?;
            Emit::Stmt(if value.trim().is_empty() {
                "return;".into()
            } else {
                format!("return {value};")
            })
        }
        41 => {
            let expression = c.expression("expression")?;
            Emit::Stmt(if expression.trim_end().ends_with(';') {
                expression
            } else {
                format!("{expression};")
            })
        }
        42 => Emit::Open("do {".into(), 43),
        43 => Emit::Close(
            43,
            Some(format!("}} while ({});", c.expression("condition")?)),
        ),
        44 if c.command.revision == Some(0) => {
            Emit::Open(format!("for (; {}; ) {{", t("condition")?), 45)
        }
        44 => Emit::Open(
            format!(
                "for ({}; {}; {}) {{",
                t("initializer")?,
                t("condition")?,
                t("increment")?
            ),
            45,
        ),
        46 if c.parameter(0).is_ok_and(|(label, _)| !label.is_empty()) => {
            return Err("labelled break requires ActionScript 3".into());
        }
        46 => Emit::Stmt("break;".into()),
        47 => Emit::Open(
            format!(
                "for ({} in {}) {{",
                c.expression("iterator")?,
                c.expression("collection")?
            ),
            48,
        ),
        49 => Emit::Stmt(format!("swapDepths({}, {});", c.param(0)?, c.param(1)?)),
        50 => Emit::Open(format!("with ({}) {{", c.expression("object")?), 51),
        52 => Emit::Stmt("continue;".into()),
        53 => Emit::Open(format!("switch ({}) {{", c.expression("expression")?), 54),
        55 | 56
            if c.number("is_default_case") != 0
                || c.command.kind == 56 && c.field("is_default_case").is_none() =>
        {
            Emit::Branch("default:".into(), 54)
        }
        55 | 56 => Emit::Branch(format!("case {}:", c.expression("case_expression")?), 54),
        57 => {
            let index = t("init_clip_index")?;
            Emit::Open(
                if index.is_empty() {
                    "#initclip".into()
                } else {
                    format!("#initclip {index}")
                },
                58,
            )
        }
        58 => Emit::Close(58, Some("#endinitclip".into())),
        59 => Emit::Stmt(invoke(c)?),
        60 => {
            let object = c.expression("object")?;
            Emit::Open(
                format!(
                    "{object}.{} = function({}) {{",
                    c.expression("method")?,
                    t("parameters")?
                ),
                61,
            )
        }
        61 => Emit::Close(61, Some("};".into())),
        62 | 79 => {
            let object = t("object")?;
            let mut name = t("method")?;
            if c.command.kind == 62 && name.starts_with('.') {
                if object.is_empty() {
                    return Err("relative call without an object".into());
                }
                name.remove(0);
            }
            if name.trim().is_empty() {
                return Err("call without a function name".into());
            }
            Emit::Stmt(format!(
                "{}{name}({});",
                if object.is_empty() {
                    String::new()
                } else {
                    format!("{object}.")
                },
                t("arguments")?
            ))
        }
        63 => Emit::Open("try {".into(), 64),
        65 => Emit::Open(format!("catch ({}) {{", c.expression("exception")?), 66),
        67 => Emit::Open("finally {".into(), 68),
        69 => Emit::Stmt(format!("throw {};", c.expression("expression")?)),
        kind @ (9 | 12 | 14 | 18 | 19 | 21 | 38 | 45 | 48 | 51 | 54 | 64 | 66 | 68) => {
            Emit::Close(kind, None)
        }
        kind => {
            return Err(format!(
                "compiled command {kind} has no ActionScript 1 translation"
            ));
        }
    })
}

fn invoke(c: &Cmd) -> std::result::Result<String, Fail> {
    let signature = c.text("invocation_signature")?;
    let method = signature.starts_with('.') || signature.starts_with('@');
    let mut name = signature.as_str();
    if name.starts_with('@') {
        name = &name[name
            .find('.')
            .ok_or("qualified invocation without method")?..];
    }
    name = name.strip_prefix('.').unwrap_or(name);
    if let Some(open) = name.find('(') {
        name = &name[..open];
    }
    if name.is_empty() {
        return Err("invocation without a function name".into());
    }
    let has_arguments = signature
        .find('(')
        .and_then(|open| signature[open + 1..].chars().next())
        .is_some_and(|c| c != ')');
    let object = if method {
        let object = c.text("object")?;
        if c.number("object_expression_flags") == 0 {
            format!("{}.", quote(&object))
        } else {
            format!("{object}.")
        }
    } else {
        String::new()
    };
    Ok(format!(
        "{object}{name}({});",
        if has_arguments {
            c.text("arguments")?
        } else {
            String::new()
        }
    ))
}

/// Kind that closes a block opened by this command, independent of whether its
/// operands translate, used to comment out the body of an untranslated opener.
fn closer(form: TimelineActionForm, command: &TimelineActionCommand) -> Option<u32> {
    let table: &[(u32, u32)] = if form == TimelineActionForm::Compiled {
        &COMPILED_OPEN
    } else {
        &HISTORICAL_OPEN
    };
    if form == TimelineActionForm::Compiled
        && command.kind == 37
        && command.fields.iter().any(|f| {
            f.name == "declaration_only" && !matches!(f.value, TimelineActionValue::Byte(0))
        })
    {
        return None;
    }
    table
        .iter()
        .find(|(open, _)| *open == command.kind)
        .map(|(_, close)| *close)
}

fn closes(form: TimelineActionForm, kind: u32) -> bool {
    let table: &[(u32, u32)] = if form == TimelineActionForm::Compiled {
        &COMPILED_OPEN
    } else {
        &HISTORICAL_OPEN
    };
    table.iter().any(|(_, close)| *close == kind)
}

fn disassembly(data: &[u8], command: &TimelineActionCommand) -> String {
    let mut parts = vec![];
    for field in &command.fields {
        let value = match &field.value {
            TimelineActionValue::Byte(0)
            | TimelineActionValue::Word(0)
            | TimelineActionValue::Signed(0)
            | TimelineActionValue::Dword(0) => continue,
            TimelineActionValue::Byte(v) => v.to_string(),
            TimelineActionValue::Word(v) => v.to_string(),
            TimelineActionValue::Signed(v) => v.to_string(),
            TimelineActionValue::Dword(v) => v.to_string(),
            TimelineActionValue::String(s) => {
                if s.data.start == s.data.end {
                    continue;
                }
                raw_text(data, s)
            }
            TimelineActionValue::Parameter { text, flags } => {
                format!("{}/{flags}", raw_text(data, text))
            }
            TimelineActionValue::Bytes(span) => format!("<{} bytes>", span.end - span.start),
        };
        parts.push(format!("{}={value}", field.name));
    }
    parts.join(" ")
}

/// Single-line rendering for comments; non-ASCII ANSI bytes stay escaped.
fn raw_text(data: &[u8], value: &TimelineString) -> String {
    match decode(data, value) {
        Ok(text) => quote(&text),
        Err(_) => {
            let bytes = data.get(value.data.start..value.data.end).unwrap_or(&[]);
            let mut out = String::from("\"");
            for &b in bytes {
                match b {
                    b'"' | b'\\' => {
                        out.push('\\');
                        out.push(b as char);
                    }
                    0x20..=0x7e => out.push(b as char),
                    _ => out.push_str(&format!("\\x{b:02x}")),
                }
            }
            out.push('"');
            out
        }
    }
}

struct Block {
    closer: u32,
    silent: bool,
}

struct Output<'a> {
    data: &'a [u8],
    form: TimelineActionForm,
    lines: Vec<String>,
    stack: Vec<Block>,
    untranslated: Vec<usize>,
    needs_review: bool,
}
impl Output<'_> {
    fn line(&mut self, text: &str) {
        let indent = "    ".repeat(self.stack.iter().filter(|b| !b.silent).count());
        for line in text.split('\n') {
            self.lines.push(format!("{indent}{line}"));
        }
    }
    fn untranslated(&mut self, index: usize, command: &TimelineActionCommand, reason: &str) {
        let form = match self.form {
            TimelineActionForm::Compiled => "compiled",
            _ => "historical",
        };
        let text = format!(
            "// NeoFlash: untranslated FLA action @0x{:x} ({form} command {}{}): {reason}; {}",
            command.encoded.start,
            command.kind,
            command
                .revision
                .map(|r| format!(" r{r}"))
                .unwrap_or_default(),
            disassembly(self.data, command)
        );
        self.line(text.trim_end());
        if self.untranslated.last() != Some(&index) {
            self.untranslated.push(index);
        }
        self.needs_review = true;
    }
}

pub(super) fn translate(data: &[u8], decoded: &mut TimelineDecodedActions) {
    let form = decoded.form;
    let mut out = Output {
        data,
        form,
        lines: vec![],
        stack: vec![],
        untranslated: vec![],
        needs_review: false,
    };
    if form == TimelineActionForm::HistoricalCommand {
        single(&mut out, decoded);
    } else {
        // Depth of commented-out commands below an untranslated opener/branch;
        // `real` marks a branch whose enclosing block is still open.
        let mut ghost: Option<(usize, bool)> = None;
        for (index, command) in decoded.commands.iter().enumerate() {
            if let Some((depth, real)) = ghost.as_mut() {
                if closer(form, command).is_some() {
                    *depth += 1;
                } else if closes(form, command.kind) {
                    *depth -= 1;
                }
                if *depth > 0 || !*real {
                    out.untranslated(index, command, "inside an untranslated block");
                    if *depth == 0 {
                        ghost = None;
                    }
                    continue;
                }
                ghost = None;
            }
            let c = Cmd { data, command };
            let emitted = if form == TimelineActionForm::Compiled {
                compiled(&c)
            } else {
                historical(&c)
            };
            let fallback = |out: &mut Output, reason: &str| {
                out.untranslated(index, command, reason);
            };
            match emitted {
                Ok(Emit::Nothing) => {}
                Ok(Emit::Stmt(text)) => out.line(&text),
                Ok(Emit::Open(text, closer)) => {
                    out.line(&text);
                    out.stack.push(Block {
                        closer,
                        silent: false,
                    });
                }
                Ok(Emit::OpenSilent(text, closer)) => {
                    out.line(&text);
                    out.stack.push(Block {
                        closer,
                        silent: true,
                    });
                }
                Ok(Emit::Branch(text, closer)) => {
                    if out
                        .stack
                        .last()
                        .is_some_and(|b| b.closer == closer && !b.silent)
                    {
                        let block = out.stack.pop().unwrap();
                        out.line(&text);
                        out.stack.push(block);
                    } else {
                        fallback(&mut out, "branch outside its block");
                    }
                }
                Ok(Emit::Close(kind, text)) => {
                    if out.stack.last().is_some_and(|b| b.closer == kind) {
                        let block = out.stack.pop().unwrap();
                        if !block.silent {
                            out.line(text.as_deref().unwrap_or("}"));
                        }
                    } else {
                        fallback(&mut out, "block delimiter without its opener");
                    }
                }
                Err(reason) => {
                    fallback(&mut out, &reason);
                    if closer(form, command).is_some() {
                        ghost = Some((1, false));
                    } else if is_branch(form, command.kind) {
                        ghost = Some((1, true));
                    }
                }
            }
        }
        if ghost.is_some() {
            out.needs_review = true;
        }
        while let Some(block) = out.stack.pop() {
            out.needs_review = true;
            if !block.silent {
                out.line("} // NeoFlash: block left open in the source was closed by the importer");
            }
        }
    }
    decoded.untranslated = out.untranslated;
    decoded.needs_review = out.needs_review;
    decoded.source = out.lines.join("\n");
    if !decoded.source.is_empty() {
        decoded.source.push('\n');
    }
}

fn is_branch(form: TimelineActionForm, kind: u32) -> bool {
    match form {
        TimelineActionForm::Compiled => matches!(kind, 16 | 17 | 55 | 56),
        _ => matches!(kind, 23 | 32),
    }
}

/// A lone historical command carries its own optional button events and
/// "wait for frame" count, which become enclosing `on`/frame-loaded blocks.
fn single(out: &mut Output, decoded: &TimelineDecodedActions) {
    let Some(command) = decoded.commands.first() else {
        return;
    };
    let c = Cmd {
        data: out.data,
        command,
    };
    let statement = match historical(&c) {
        Ok(Emit::Nothing) => return,
        Ok(Emit::Stmt(text)) => text,
        Ok(_) => {
            out.untranslated(0, command, "block command without a body");
            return;
        }
        Err(reason) => {
            out.untranslated(0, command, &reason);
            return;
        }
    };
    let mut events: Vec<&str> = vec![];
    for (field, event) in [
        ("press", "press"),
        ("release", "release"),
        ("release_outside", "releaseOutside"),
        ("roll_over", "rollOver"),
        ("roll_out", "rollOut"),
        ("drag_over", "dragOver"),
        ("drag_out", "dragOut"),
    ] {
        if c.number(field) != 0 {
            events.push(event);
        }
    }
    let wait = match c.field("wait_for_extra_frames") {
        Some(TimelineActionValue::Signed(v)) if *v != i32::from(NO_WAIT) => Some(*v),
        _ => None,
    };
    if !events.is_empty() {
        out.line(&format!("on ({}) {{", events.join(", ")));
        out.stack.push(Block {
            closer: 0,
            silent: false,
        });
    }
    if let Some(wait) = wait {
        let frame = c.number("frame_number") + i64::from(wait);
        out.line(&frame_loaded("", frame.to_string(), true));
        out.stack.push(Block {
            closer: 0,
            silent: false,
        });
    }
    out.line(&statement);
    while out.stack.pop().is_some() {
        out.line("}");
    }
}
