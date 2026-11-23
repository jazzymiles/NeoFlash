use super::*;

/// Timeout in seconds after pressing `<ESC>` before a pending two-key chord expires automatically.
pub(crate) const ESC_CHORD_TIMEOUT_SECS: f64 = 5.0;

pub(crate) const ESC_CHORD_ARMED_PROMPT: &str =
    "Type two shortcut keys (fn, tr, st…) or press Tab / Shift+Tab to leave the editor.";

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
    let mut leave_editor = None;

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
                    key: egui::Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } if matches!(phase, EscChordPhase::Armed { .. })
                    && (modifiers.is_none() || modifiers == egui::Modifiers::SHIFT) =>
                {
                    leave_editor = Some(if modifiers.shift {
                        egui::FocusDirection::Previous
                    } else {
                        egui::FocusDirection::Next
                    });
                    *phase = EscChordPhase::Idle;
                    status = Some("Keyboard focus left the code editor.".into());
                }
                egui::Event::Text(ref text) if leave_editor.is_some() && text == "\t" => {}
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

    if let Some(direction) = leave_editor {
        ctx.memory_mut(|memory| memory.move_focus(direction));
    }
    if let Some(armed_at) = phase.armed_at() {
        let remaining = (ESC_CHORD_TIMEOUT_SECS - (now - armed_at)).max(0.05);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
    }

    EscChordStepOutcome {
        edit: did_edit.then(|| (cur_text.into_owned(), cur_sel)),
        status,
    }
}
