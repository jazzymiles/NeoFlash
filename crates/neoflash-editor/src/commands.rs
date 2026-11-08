//! Editor command and keyboard shortcut registry (spec E-25 / W3.7 / DL-01 Shortcuts).
//!
//! Centralises command identifiers, menu/category titles, default NF2 key bindings, effective
//! overrides from [`ShortcutPrefs`], conflict detection, key-event matching and HTML export.

use crate::app::{Editor, Tool};
use crate::timeline_edit::FrameCommand;
use crate::workspace::{PanelId, ShortcutPrefs};
use eframe::egui::{self, Event, Key, Modifiers};
use std::collections::BTreeMap;

/// Command grouping used by the category filter in `Preferences ▸ Shortcuts` and HTML export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandCategory {
    File,
    Edit,
    View,
    Insert,
    Modify,
    Control,
    Document,
    Window,
    Tools,
}

impl CommandCategory {
    pub const ALL: [Self; 9] = [
        Self::File,
        Self::Edit,
        Self::View,
        Self::Insert,
        Self::Modify,
        Self::Control,
        Self::Document,
        Self::Window,
        Self::Tools,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Insert => "Insert",
            Self::Modify => "Modify",
            Self::Control => "Control",
            Self::Document => "Document",
            Self::Window => "Window",
            Self::Tools => "Tools",
        }
    }
}

/// Normalized keyboard chord (`Modifiers` + `Key`).
///
/// Equality and hashing ignore `shift_before_alt` so `"⇧⌥F12"` and `"⌥⇧F12"` represent the exact
/// same chord for dispatch and conflict detection while preserving NF2's `"⇧⌥F12"` display order
/// on `File ▸ Publish`.
#[derive(Clone, Copy, Debug)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Key,
    pub shift_before_alt: bool,
}

impl PartialEq for Shortcut {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_tuple() == other.canonical_tuple()
    }
}

impl Eq for Shortcut {}

impl PartialOrd for Shortcut {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Shortcut {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical_tuple().cmp(&other.canonical_tuple())
    }
}

impl std::hash::Hash for Shortcut {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.canonical_tuple().hash(state);
    }
}

impl Shortcut {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self {
            modifiers,
            key,
            shift_before_alt: false,
        }
    }

    pub const fn shift_alt(key: Key) -> Self {
        Self {
            modifiers: Modifiers {
                alt: true,
                ctrl: false,
                shift: true,
                mac_cmd: false,
                command: false,
            },
            key,
            shift_before_alt: true,
        }
    }

    pub const fn none(key: Key) -> Self {
        Self::new(Modifiers::NONE, key)
    }

    pub const fn cmd(key: Key) -> Self {
        Self::new(Modifiers::COMMAND, key)
    }

    pub const fn shift(key: Key) -> Self {
        Self::new(Modifiers::SHIFT, key)
    }

    pub const fn alt(key: Key) -> Self {
        Self::new(Modifiers::ALT, key)
    }

    pub const fn shift_cmd(key: Key) -> Self {
        Self::new(
            Modifiers {
                alt: false,
                ctrl: false,
                shift: true,
                mac_cmd: false,
                command: true,
            },
            key,
        )
    }

    pub const fn alt_cmd(key: Key) -> Self {
        Self::new(
            Modifiers {
                alt: true,
                ctrl: false,
                shift: false,
                mac_cmd: false,
                command: true,
            },
            key,
        )
    }

    pub const fn alt_shift(key: Key) -> Self {
        Self::new(
            Modifiers {
                alt: true,
                ctrl: false,
                shift: true,
                mac_cmd: false,
                command: false,
            },
            key,
        )
    }

    pub const fn alt_shift_cmd(key: Key) -> Self {
        Self::new(
            Modifiers {
                alt: true,
                ctrl: false,
                shift: true,
                mac_cmd: false,
                command: true,
            },
            key,
        )
    }

    pub const fn norm_cmd(&self) -> bool {
        self.modifiers.command || self.modifiers.mac_cmd
    }

    pub const fn norm_ctrl(&self) -> bool {
        self.modifiers.ctrl && !self.norm_cmd()
    }

    fn canonical_tuple(&self) -> (bool, bool, bool, bool, Key) {
        (
            self.norm_ctrl(),
            self.modifiers.alt,
            self.modifiers.shift,
            self.norm_cmd(),
            canonical_key(self.key),
        )
    }

    /// Compact NF2 shortcut string (e.g. `"⌘S"`, `"⇧⌘S"`, `"⌥⇧⌘K"`, `"⇧⌥F12"`, `"⌘−"`).
    pub fn format(&self) -> String {
        let mut out = String::new();
        for cap in self.key_caps() {
            out.push_str(&cap);
        }
        out
    }

    /// Individual NF2 key-cap tokens for `c::kbd` (e.g. `["⇧", "F6"]`, `["⌥", "⇧", "⌘", "K"]`).
    pub fn key_caps(&self) -> Vec<String> {
        let mut caps = Vec::with_capacity(5);
        if self.norm_ctrl() {
            caps.push("⌃".to_owned());
        }
        if self.shift_before_alt {
            if self.modifiers.shift {
                caps.push("⇧".to_owned());
            }
            if self.modifiers.alt {
                caps.push("⌥".to_owned());
            }
        } else {
            if self.modifiers.alt {
                caps.push("⌥".to_owned());
            }
            if self.modifiers.shift {
                caps.push("⇧".to_owned());
            }
        }
        if self.norm_cmd() {
            caps.push("⌘".to_owned());
        }
        caps.push(format_key(canonical_key(self.key)).to_owned());
        caps
    }

    /// Parse an NF2 or human-written shortcut label into a [`Shortcut`].
    pub fn parse(raw: &str) -> Option<Self> {
        let mut rest = raw.trim();
        if rest.is_empty() {
            return None;
        }
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut cmd = false;
        let mut shift_before_alt = false;

        loop {
            rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix('⌃') {
                ctrl = true;
                rest = after;
            } else if let Some(after) = rest.strip_prefix('⌥') {
                alt = true;
                if shift {
                    shift_before_alt = true;
                }
                rest = after;
            } else if let Some(after) = rest.strip_prefix('⇧') {
                shift = true;
                rest = after;
            } else if let Some(after) = rest.strip_prefix('⌘') {
                cmd = true;
                rest = after;
            } else if let Some(after) = strip_ascii_prefix_ci(rest, "ctrl+")
                .or_else(|| strip_ascii_prefix_ci(rest, "control+"))
            {
                ctrl = true;
                rest = after;
            } else if let Some(after) = strip_ascii_prefix_ci(rest, "alt+")
                .or_else(|| strip_ascii_prefix_ci(rest, "opt+"))
                .or_else(|| strip_ascii_prefix_ci(rest, "option+"))
            {
                alt = true;
                if shift {
                    shift_before_alt = true;
                }
                rest = after;
            } else if let Some(after) = strip_ascii_prefix_ci(rest, "shift+") {
                shift = true;
                rest = after;
            } else if let Some(after) = strip_ascii_prefix_ci(rest, "cmd+")
                .or_else(|| strip_ascii_prefix_ci(rest, "command+"))
            {
                cmd = true;
                rest = after;
            } else {
                break;
            }
        }

        let key = parse_key(rest.trim())?;
        let modifiers = Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd: false,
            command: cmd,
        };
        Some(Self {
            modifiers,
            key: canonical_key_with_shift(key, shift),
            shift_before_alt: shift_before_alt && shift && alt && !cmd,
        })
    }

    /// Construct a [`Shortcut`] from a recorded `Event::Key` press, preferring the physical base
    /// key when Option/Shift remaps punctuation on macOS (`⇧;` -> `;`, `⌥R` -> `R`).
    pub fn from_key_event(modifiers: Modifiers, key: Key, physical_key: Option<Key>) -> Self {
        let cmd = modifiers.command || modifiers.mac_cmd;
        let ctrl = modifiers.ctrl && !cmd;
        let base_key = physical_key.unwrap_or(key);
        let clean_key = canonical_key_with_shift(base_key, modifiers.shift);
        Self {
            modifiers: Modifiers {
                alt: modifiers.alt,
                ctrl,
                shift: modifiers.shift,
                mac_cmd: false,
                command: cmd,
            },
            key: clean_key,
            shift_before_alt: false,
        }
    }

    /// Exact modifier + logical/physical key match against a pressed `Event::Key`.
    pub fn matches_key_event(
        &self,
        event_modifiers: Modifiers,
        key: Key,
        physical_key: Option<Key>,
    ) -> bool {
        if !modifiers_match(event_modifiers, self.modifiers) {
            return false;
        }
        let target = canonical_key(self.key);
        key_matches(target, key) || physical_key.is_some_and(|pk| key_matches(target, pk))
    }

    /// Whether this chord can safely fire while a text field has keyboard focus (`⌘`, `⌃`, `⌥` or
    /// function keys F1–F20; never plain typing characters or Enter/Backspace/Arrows).
    pub fn is_safe_while_typing(&self) -> bool {
        self.norm_cmd() || self.norm_ctrl() || self.modifiers.alt || is_function_key(self.key)
    }
}

pub fn modifiers_is_none(actual: Modifiers) -> bool {
    !actual.alt && !actual.ctrl && !actual.shift && !actual.command && !actual.mac_cmd
}

pub fn modifiers_match(actual: Modifiers, expected: Modifiers) -> bool {
    let actual_cmd = actual.command || actual.mac_cmd;
    let expected_cmd = expected.command || expected.mac_cmd;
    let actual_ctrl = actual.ctrl && !actual_cmd;
    let expected_ctrl = expected.ctrl && !expected_cmd;
    actual.alt == expected.alt
        && actual.shift == expected.shift
        && actual_cmd == expected_cmd
        && actual_ctrl == expected_ctrl
}

fn canonical_key(key: Key) -> Key {
    match key {
        Key::Plus => Key::Equals,
        Key::Colon => Key::Semicolon,
        other => other,
    }
}

fn canonical_key_with_shift(key: Key, shift: bool) -> Key {
    if shift { canonical_key(key) } else { key }
}

fn key_matches(target: Key, candidate: Key) -> bool {
    target == candidate
        || (target == Key::Backspace && candidate == Key::Delete)
        || (target == Key::Equals && candidate == Key::Plus)
        || (target == Key::Semicolon && candidate == Key::Colon)
}

fn is_function_key(key: Key) -> bool {
    matches!(
        key,
        Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12
            | Key::F13
            | Key::F14
            | Key::F15
            | Key::F16
            | Key::F17
            | Key::F18
            | Key::F19
            | Key::F20
    )
}

fn strip_ascii_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) {
        let (head, tail) = s.split_at(prefix.len());
        if head.eq_ignore_ascii_case(prefix) {
            return Some(tail);
        }
    }
    None
}

fn format_key(key: Key) -> &'static str {
    match key {
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::F13 => "F13",
        Key::F14 => "F14",
        Key::F15 => "F15",
        Key::F16 => "F16",
        Key::F17 => "F17",
        Key::F18 => "F18",
        Key::F19 => "F19",
        Key::F20 => "F20",
        Key::Enter => "↵",
        Key::Backspace => "⌫",
        Key::Delete => "⌦",
        Key::Tab => "⇥",
        Key::Space => "Space",
        Key::Escape => "Esc",
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PgUp",
        Key::PageDown => "PgDn",
        Key::Insert => "Ins",
        Key::Minus => "−",
        Key::Equals | Key::Plus => "=",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Semicolon | Key::Colon => ";",
        Key::Quote => "'",
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Backtick => "`",
        _ => key.name(),
    }
}

fn parse_key(token: &str) -> Option<Key> {
    if token.len() == 1 {
        let ch = token.chars().next()?.to_ascii_uppercase();
        return match ch {
            'A' => Some(Key::A),
            'B' => Some(Key::B),
            'C' => Some(Key::C),
            'D' => Some(Key::D),
            'E' => Some(Key::E),
            'F' => Some(Key::F),
            'G' => Some(Key::G),
            'H' => Some(Key::H),
            'I' => Some(Key::I),
            'J' => Some(Key::J),
            'K' => Some(Key::K),
            'L' => Some(Key::L),
            'M' => Some(Key::M),
            'N' => Some(Key::N),
            'O' => Some(Key::O),
            'P' => Some(Key::P),
            'Q' => Some(Key::Q),
            'R' => Some(Key::R),
            'S' => Some(Key::S),
            'T' => Some(Key::T),
            'U' => Some(Key::U),
            'V' => Some(Key::V),
            'W' => Some(Key::W),
            'X' => Some(Key::X),
            'Y' => Some(Key::Y),
            'Z' => Some(Key::Z),
            '0' => Some(Key::Num0),
            '1' => Some(Key::Num1),
            '2' => Some(Key::Num2),
            '3' => Some(Key::Num3),
            '4' => Some(Key::Num4),
            '5' => Some(Key::Num5),
            '6' => Some(Key::Num6),
            '7' => Some(Key::Num7),
            '8' => Some(Key::Num8),
            '9' => Some(Key::Num9),
            '-' => Some(Key::Minus),
            '=' | '+' => Some(Key::Equals),
            ',' => Some(Key::Comma),
            '.' => Some(Key::Period),
            ';' | ':' => Some(Key::Semicolon),
            '\'' => Some(Key::Quote),
            '/' => Some(Key::Slash),
            '\\' => Some(Key::Backslash),
            '[' => Some(Key::OpenBracket),
            ']' => Some(Key::CloseBracket),
            '`' => Some(Key::Backtick),
            _ => None,
        };
    }
    match token {
        "↵" | "⌤" => return Some(Key::Enter),
        "⌫" => return Some(Key::Backspace),
        "⌦" => return Some(Key::Delete),
        "⇥" => return Some(Key::Tab),
        "␣" => return Some(Key::Space),
        "⎋" => return Some(Key::Escape),
        "←" => return Some(Key::ArrowLeft),
        "→" => return Some(Key::ArrowRight),
        "↑" => return Some(Key::ArrowUp),
        "↓" => return Some(Key::ArrowDown),
        "−" => return Some(Key::Minus),
        "↖" => return Some(Key::Home),
        "↘" => return Some(Key::End),
        "⇞" => return Some(Key::PageUp),
        "⇟" => return Some(Key::PageDown),
        _ => {}
    }
    match token.to_ascii_lowercase().as_str() {
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        "f13" => Some(Key::F13),
        "f14" => Some(Key::F14),
        "f15" => Some(Key::F15),
        "f16" => Some(Key::F16),
        "f17" => Some(Key::F17),
        "f18" => Some(Key::F18),
        "f19" => Some(Key::F19),
        "f20" => Some(Key::F20),
        "enter" | "return" => Some(Key::Enter),
        "backspace" => Some(Key::Backspace),
        "delete" | "del" => Some(Key::Delete),
        "tab" => Some(Key::Tab),
        "space" => Some(Key::Space),
        "esc" | "escape" => Some(Key::Escape),
        "left" | "arrowleft" => Some(Key::ArrowLeft),
        "right" | "arrowright" => Some(Key::ArrowRight),
        "up" | "arrowup" => Some(Key::ArrowUp),
        "down" | "arrowdown" => Some(Key::ArrowDown),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pgup" | "pageup" => Some(Key::PageUp),
        "pgdn" | "pagedown" => Some(Key::PageDown),
        "ins" | "insert" => Some(Key::Insert),
        "minus" => Some(Key::Minus),
        "equals" | "plus" => Some(Key::Equals),
        "comma" => Some(Key::Comma),
        "period" => Some(Key::Period),
        "semicolon" | "colon" => Some(Key::Semicolon),
        "quote" => Some(Key::Quote),
        "slash" => Some(Key::Slash),
        "backslash" => Some(Key::Backslash),
        "openbracket" => Some(Key::OpenBracket),
        "closebracket" => Some(Key::CloseBracket),
        "backtick" => Some(Key::Backtick),
        _ => None,
    }
}

/// Stable identifier of every bindable editor command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandId {
    // File
    FileNew,
    FileOpen,
    FileClose,
    FileSave,
    FileSaveAs,
    FileRevert,
    FileImportGraphics,
    FileImportLegacyFlash,
    FileExportVideo,
    FileExportAudio,
    FileExportLottie,
    FilePublishSettings,
    FilePublish,
    // Edit
    EditUndo,
    EditRedo,
    EditCut,
    EditCopy,
    EditPaste,
    EditPasteInPlace,
    EditClear,
    EditDuplicate,
    EditSelectAll,
    EditDeselect,
    EditFindAndReplace,
    EditTimelineCutFrames,
    EditTimelineCopyFrames,
    EditTimelinePasteFrames,
    EditTimelineClearFrames,
    EditTimelineRemoveFrames,
    EditTimelineSelectAllFrames,
    EditTimelineReverseFrames,
    EditTimelineSyncSymbols,
    EditTimelineCopyMotion,
    EditTimelinePasteMotion,
    EditSymbols,
    EditPreferences,
    EditKeyboardShortcuts,
    // View
    ViewZoomIn,
    ViewZoomOut,
    ViewZoom100,
    ViewFitInWindow,
    ViewRulers,
    ViewGridShow,
    ViewGridSnap,
    ViewGridEdit,
    ViewGuidesShow,
    ViewGuidesLock,
    ViewGuidesSnap,
    // Insert
    InsertNewSymbol,
    InsertLayer,
    InsertLayerFolder,
    InsertMotionGuide,
    InsertFrame,
    InsertKeyframe,
    InsertBlankKeyframe,
    InsertClassicTween,
    InsertShapeTween,
    InsertMotionTween,
    InsertScene,
    // Modify
    ModifyDocument,
    ModifyConvertToSymbol,
    ModifyBreakApart,
    ModifyTraceBitmap,
    ModifyDistributeToLayers,
    ModifyTimelineLayerProperties,
    ModifyTimelineClearKeyframe,
    ModifyTimelineConvertToKeyframes,
    ModifyTimelineConvertToBlankKeyframes,
    ModifyArrangeFront,
    ModifyArrangeForward,
    ModifyArrangeBackward,
    ModifyArrangeBack,
    ModifyGroup,
    ModifyUngroup,
    // Control
    ControlPlay,
    ControlRewind,
    ControlGoToEnd,
    ControlStepForward,
    ControlStepBack,
    ControlTestMovie,
    ControlTestScene,
    ControlLoopPlayback,
    ControlPlayAllScenes,
    ControlEnableSimpleButtons,
    ControlMuteSounds,
    // Window
    WindowTimeline,
    WindowTools,
    WindowProperties,
    WindowLibrary,
    WindowActions,
    WindowAlign,
    WindowColor,
    WindowSwatches,
    WindowFilters,
    WindowTransform,
    WindowInfo,
    WindowEase,
    WindowHistory,
    WindowScene,
    WindowCompilerErrors,
    WindowOutput,
    WindowCodeSnippets,
    WindowMovieExplorer,
    WindowAccessibility,
    WindowMotionEditor,
    WindowProject,
    WindowBrushes,
    WindowHidePanels,
    WindowInspectorPriority,
    WindowLibraryPriority,
    // Tools
    ToolSelect,
    ToolSubselect,
    ToolTransform,
    ToolGradient,
    ToolLasso,
    ToolPen,
    ToolLine,
    ToolText,
    ToolRectangle,
    ToolOval,
    ToolPolyStar,
    ToolPencil,
    ToolBrush,
    ToolBucket,
    ToolInk,
    ToolEyedropper,
    ToolEraser,
    ToolHand,
    ToolZoom,
    ToolObjectDrawing,
}

struct CommandMeta {
    id: &'static str,
    category: CommandCategory,
    title: &'static str,
    keywords: &'static str,
    defaults: &'static [Shortcut],
    allow_when_typing: bool,
}

const S_CMD_N: &[Shortcut] = &[Shortcut::cmd(Key::N)];
const S_CMD_O: &[Shortcut] = &[Shortcut::cmd(Key::O)];
const S_CMD_W: &[Shortcut] = &[Shortcut::cmd(Key::W)];
const S_CMD_S: &[Shortcut] = &[Shortcut::cmd(Key::S)];
const S_SHIFT_CMD_S: &[Shortcut] = &[Shortcut::shift_cmd(Key::S)];
const S_ALT_SHIFT_CMD_L: &[Shortcut] = &[Shortcut::alt_shift_cmd(Key::L)];
const S_SHIFT_F12: &[Shortcut] = &[Shortcut::shift(Key::F12)];
const S_PUBLISH: &[Shortcut] = &[Shortcut::shift_alt(Key::F12)];

const S_CMD_Z: &[Shortcut] = &[Shortcut::cmd(Key::Z)];
const S_SHIFT_CMD_Z: &[Shortcut] = &[Shortcut::shift_cmd(Key::Z)];
const S_CMD_X: &[Shortcut] = &[Shortcut::cmd(Key::X)];
const S_CMD_C: &[Shortcut] = &[Shortcut::cmd(Key::C)];
const S_CMD_V: &[Shortcut] = &[Shortcut::cmd(Key::V)];
const S_SHIFT_CMD_V: &[Shortcut] = &[Shortcut::shift_cmd(Key::V)];
const S_BACKSPACE: &[Shortcut] = &[Shortcut::none(Key::Backspace)];
const S_CMD_D: &[Shortcut] = &[Shortcut::cmd(Key::D)];
const S_CMD_A: &[Shortcut] = &[Shortcut::cmd(Key::A)];
const S_SHIFT_CMD_A: &[Shortcut] = &[Shortcut::shift_cmd(Key::A)];
const S_CMD_F: &[Shortcut] = &[Shortcut::cmd(Key::F)];
const S_ALT_CMD_X: &[Shortcut] = &[Shortcut::alt_cmd(Key::X)];
const S_ALT_CMD_C: &[Shortcut] = &[Shortcut::alt_cmd(Key::C)];
const S_ALT_CMD_V: &[Shortcut] = &[Shortcut::alt_cmd(Key::V)];
const S_ALT_BACKSPACE: &[Shortcut] = &[Shortcut::alt(Key::Backspace)];
const S_SHIFT_F5: &[Shortcut] = &[Shortcut::shift(Key::F5)];
const S_ALT_CMD_A: &[Shortcut] = &[Shortcut::alt_cmd(Key::A)];
const S_CMD_E: &[Shortcut] = &[Shortcut::cmd(Key::E)];
const S_CMD_U: &[Shortcut] = &[Shortcut::cmd(Key::U)];
const S_ALT_SHIFT_CMD_K: &[Shortcut] = &[Shortcut::alt_shift_cmd(Key::K)];

const S_CMD_EQUALS: &[Shortcut] = &[Shortcut::cmd(Key::Equals)];
const S_CMD_MINUS: &[Shortcut] = &[Shortcut::cmd(Key::Minus)];
const S_CMD_1: &[Shortcut] = &[Shortcut::cmd(Key::Num1)];
const S_CMD_0: &[Shortcut] = &[Shortcut::cmd(Key::Num0)];
const S_ALT_SHIFT_CMD_R: &[Shortcut] = &[Shortcut::alt_shift_cmd(Key::R)];
const S_CMD_QUOTE: &[Shortcut] = &[Shortcut::cmd(Key::Quote)];
const S_SHIFT_CMD_QUOTE: &[Shortcut] = &[Shortcut::shift_cmd(Key::Quote)];
const S_ALT_CMD_G: &[Shortcut] = &[Shortcut::alt_cmd(Key::G)];
const S_CMD_SEMI: &[Shortcut] = &[Shortcut::cmd(Key::Semicolon)];
const S_ALT_CMD_SEMI: &[Shortcut] = &[Shortcut::alt_cmd(Key::Semicolon)];
const S_SHIFT_CMD_SEMI: &[Shortcut] = &[Shortcut::shift_cmd(Key::Semicolon)];

const S_CMD_F8: &[Shortcut] = &[Shortcut::cmd(Key::F8)];
const S_F5: &[Shortcut] = &[Shortcut::none(Key::F5)];
const S_F6: &[Shortcut] = &[Shortcut::none(Key::F6)];
const S_F7: &[Shortcut] = &[Shortcut::none(Key::F7)];

const S_CMD_J: &[Shortcut] = &[Shortcut::cmd(Key::J)];
const S_F8: &[Shortcut] = &[Shortcut::none(Key::F8)];
const S_CMD_B: &[Shortcut] = &[Shortcut::cmd(Key::B)];
const S_SHIFT_CMD_D: &[Shortcut] = &[Shortcut::shift_cmd(Key::D)];
const S_SHIFT_F6: &[Shortcut] = &[Shortcut::shift(Key::F6)];
const S_ARRANGE_FRONT: &[Shortcut] = &[
    Shortcut::shift_cmd(Key::ArrowUp),
    Shortcut::shift_cmd(Key::CloseBracket),
];
const S_ARRANGE_FWD: &[Shortcut] = &[
    Shortcut::cmd(Key::ArrowUp),
    Shortcut::cmd(Key::CloseBracket),
];
const S_ARRANGE_BWD: &[Shortcut] = &[
    Shortcut::cmd(Key::ArrowDown),
    Shortcut::cmd(Key::OpenBracket),
];
const S_ARRANGE_BACK: &[Shortcut] = &[
    Shortcut::shift_cmd(Key::ArrowDown),
    Shortcut::shift_cmd(Key::OpenBracket),
];
const S_CMD_G: &[Shortcut] = &[Shortcut::cmd(Key::G)];
const S_SHIFT_CMD_G: &[Shortcut] = &[Shortcut::shift_cmd(Key::G)];

const S_ENTER: &[Shortcut] = &[Shortcut::none(Key::Enter)];
const S_ALT_SHIFT_R: &[Shortcut] = &[Shortcut::alt_shift(Key::R)];
const S_ALT_SHIFT_PERIOD: &[Shortcut] = &[Shortcut::alt_shift(Key::Period)];
const S_STEP_FWD: &[Shortcut] = &[Shortcut::none(Key::Period), Shortcut::none(Key::ArrowRight)];
const S_STEP_BACK: &[Shortcut] = &[Shortcut::none(Key::Comma), Shortcut::none(Key::ArrowLeft)];
const S_CMD_ENTER: &[Shortcut] = &[Shortcut::cmd(Key::Enter)];
const S_ALT_CMD_ENTER: &[Shortcut] = &[Shortcut::alt_cmd(Key::Enter)];
const S_ALT_CMD_B: &[Shortcut] = &[Shortcut::alt_cmd(Key::B)];
const S_ALT_CMD_M: &[Shortcut] = &[Shortcut::alt_cmd(Key::M)];

const S_ALT_CMD_T: &[Shortcut] = &[Shortcut::alt_cmd(Key::T)];
const S_CMD_F2: &[Shortcut] = &[Shortcut::cmd(Key::F2)];
const S_CMD_F3: &[Shortcut] = &[Shortcut::cmd(Key::F3)];
const S_CMD_L: &[Shortcut] = &[Shortcut::cmd(Key::L)];
const S_ALT_CMD_L: &[Shortcut] = &[Shortcut::alt_cmd(Key::L)];
const S_ALT_CMD_F3: &[Shortcut] = &[Shortcut::alt_cmd(Key::F3)];
const S_F9: &[Shortcut] = &[Shortcut::none(Key::F9)];
const S_CMD_K: &[Shortcut] = &[Shortcut::cmd(Key::K)];
const S_SHIFT_CMD_F9: &[Shortcut] = &[Shortcut::shift_cmd(Key::F9)];
const S_CMD_F9: &[Shortcut] = &[Shortcut::cmd(Key::F9)];
const S_CMD_T: &[Shortcut] = &[Shortcut::cmd(Key::T)];
const S_CMD_I: &[Shortcut] = &[Shortcut::cmd(Key::I)];
const S_CMD_F10: &[Shortcut] = &[Shortcut::cmd(Key::F10)];
const S_SHIFT_F2: &[Shortcut] = &[Shortcut::shift(Key::F2)];
const S_ALT_F2: &[Shortcut] = &[Shortcut::alt(Key::F2)];
const S_F2: &[Shortcut] = &[Shortcut::none(Key::F2)];
const S_SHIFT_F11: &[Shortcut] = &[Shortcut::shift(Key::F11)];
const S_F4: &[Shortcut] = &[Shortcut::none(Key::F4)];

const S_TOOL_V: &[Shortcut] = &[Shortcut::none(Key::V)];
const S_TOOL_A: &[Shortcut] = &[Shortcut::none(Key::A)];
const S_TOOL_Q: &[Shortcut] = &[Shortcut::none(Key::Q)];
const S_TOOL_F: &[Shortcut] = &[Shortcut::none(Key::F)];
const S_TOOL_L: &[Shortcut] = &[Shortcut::none(Key::L)];
const S_TOOL_P: &[Shortcut] = &[Shortcut::none(Key::P)];
const S_TOOL_N: &[Shortcut] = &[Shortcut::none(Key::N)];
const S_TOOL_T: &[Shortcut] = &[Shortcut::none(Key::T)];
const S_TOOL_R: &[Shortcut] = &[Shortcut::none(Key::R)];
const S_TOOL_O: &[Shortcut] = &[Shortcut::none(Key::O)];
const S_TOOL_Y: &[Shortcut] = &[Shortcut::none(Key::Y)];
const S_TOOL_B: &[Shortcut] = &[Shortcut::none(Key::B)];
const S_TOOL_K: &[Shortcut] = &[Shortcut::none(Key::K)];
const S_TOOL_S: &[Shortcut] = &[Shortcut::none(Key::S)];
const S_TOOL_I: &[Shortcut] = &[Shortcut::none(Key::I)];
const S_TOOL_E: &[Shortcut] = &[Shortcut::none(Key::E)];
const S_TOOL_H: &[Shortcut] = &[Shortcut::none(Key::H)];
const S_TOOL_ZM: &[Shortcut] = &[Shortcut::none(Key::Z), Shortcut::none(Key::M)];
const S_TOOL_J: &[Shortcut] = &[Shortcut::none(Key::J)];

impl CommandId {
    pub const ALL: &'static [Self] = &[
        Self::FileNew,
        Self::FileOpen,
        Self::FileClose,
        Self::FileSave,
        Self::FileSaveAs,
        Self::FileRevert,
        Self::FileImportGraphics,
        Self::FileImportLegacyFlash,
        Self::FileExportVideo,
        Self::FileExportAudio,
        Self::FileExportLottie,
        Self::FilePublishSettings,
        Self::FilePublish,
        Self::EditUndo,
        Self::EditRedo,
        Self::EditCut,
        Self::EditCopy,
        Self::EditPaste,
        Self::EditPasteInPlace,
        Self::EditClear,
        Self::EditDuplicate,
        Self::EditSelectAll,
        Self::EditDeselect,
        Self::EditFindAndReplace,
        Self::EditTimelineCutFrames,
        Self::EditTimelineCopyFrames,
        Self::EditTimelinePasteFrames,
        Self::EditTimelineClearFrames,
        Self::EditTimelineRemoveFrames,
        Self::EditTimelineSelectAllFrames,
        Self::EditTimelineReverseFrames,
        Self::EditTimelineSyncSymbols,
        Self::EditTimelineCopyMotion,
        Self::EditTimelinePasteMotion,
        Self::EditSymbols,
        Self::EditPreferences,
        Self::EditKeyboardShortcuts,
        Self::ViewZoomIn,
        Self::ViewZoomOut,
        Self::ViewZoom100,
        Self::ViewFitInWindow,
        Self::ViewRulers,
        Self::ViewGridShow,
        Self::ViewGridSnap,
        Self::ViewGridEdit,
        Self::ViewGuidesShow,
        Self::ViewGuidesLock,
        Self::ViewGuidesSnap,
        Self::InsertNewSymbol,
        Self::InsertLayer,
        Self::InsertLayerFolder,
        Self::InsertMotionGuide,
        Self::InsertFrame,
        Self::InsertKeyframe,
        Self::InsertBlankKeyframe,
        Self::InsertClassicTween,
        Self::InsertShapeTween,
        Self::InsertMotionTween,
        Self::InsertScene,
        Self::ModifyDocument,
        Self::ModifyConvertToSymbol,
        Self::ModifyBreakApart,
        Self::ModifyTraceBitmap,
        Self::ModifyDistributeToLayers,
        Self::ModifyTimelineLayerProperties,
        Self::ModifyTimelineClearKeyframe,
        Self::ModifyTimelineConvertToKeyframes,
        Self::ModifyTimelineConvertToBlankKeyframes,
        Self::ModifyArrangeFront,
        Self::ModifyArrangeForward,
        Self::ModifyArrangeBackward,
        Self::ModifyArrangeBack,
        Self::ModifyGroup,
        Self::ModifyUngroup,
        Self::ControlPlay,
        Self::ControlRewind,
        Self::ControlGoToEnd,
        Self::ControlStepForward,
        Self::ControlStepBack,
        Self::ControlTestMovie,
        Self::ControlTestScene,
        Self::ControlLoopPlayback,
        Self::ControlPlayAllScenes,
        Self::ControlEnableSimpleButtons,
        Self::ControlMuteSounds,
        Self::WindowTimeline,
        Self::WindowTools,
        Self::WindowProperties,
        Self::WindowLibrary,
        Self::WindowActions,
        Self::WindowAlign,
        Self::WindowColor,
        Self::WindowSwatches,
        Self::WindowFilters,
        Self::WindowTransform,
        Self::WindowInfo,
        Self::WindowEase,
        Self::WindowHistory,
        Self::WindowScene,
        Self::WindowCompilerErrors,
        Self::WindowOutput,
        Self::WindowCodeSnippets,
        Self::WindowMovieExplorer,
        Self::WindowAccessibility,
        Self::WindowMotionEditor,
        Self::WindowProject,
        Self::WindowBrushes,
        Self::WindowHidePanels,
        Self::WindowInspectorPriority,
        Self::WindowLibraryPriority,
        Self::ToolSelect,
        Self::ToolSubselect,
        Self::ToolTransform,
        Self::ToolGradient,
        Self::ToolLasso,
        Self::ToolPen,
        Self::ToolLine,
        Self::ToolText,
        Self::ToolRectangle,
        Self::ToolOval,
        Self::ToolPolyStar,
        Self::ToolPencil,
        Self::ToolBrush,
        Self::ToolBucket,
        Self::ToolInk,
        Self::ToolEyedropper,
        Self::ToolEraser,
        Self::ToolHand,
        Self::ToolZoom,
        Self::ToolObjectDrawing,
    ];

    const fn meta(self) -> CommandMeta {
        use CommandCategory as C;
        match self {
            Self::FileNew => CommandMeta {
                id: "file.new",
                category: C::File,
                title: "File ▸ New",
                keywords: "",
                defaults: S_CMD_N,
                allow_when_typing: true,
            },
            Self::FileOpen => CommandMeta {
                id: "file.open",
                category: C::File,
                title: "File ▸ Open…",
                keywords: "",
                defaults: S_CMD_O,
                allow_when_typing: true,
            },
            Self::FileClose => CommandMeta {
                id: "file.close",
                category: C::File,
                title: "File ▸ Close",
                keywords: "",
                defaults: S_CMD_W,
                allow_when_typing: false,
            },
            Self::FileSave => CommandMeta {
                id: "file.save",
                category: C::File,
                title: "File ▸ Save",
                keywords: "",
                defaults: S_CMD_S,
                allow_when_typing: true,
            },
            Self::FileSaveAs => CommandMeta {
                id: "file.save_as",
                category: C::File,
                title: "File ▸ Save as…",
                keywords: "",
                defaults: S_SHIFT_CMD_S,
                allow_when_typing: true,
            },
            Self::FileRevert => CommandMeta {
                id: "file.revert",
                category: C::File,
                title: "File ▸ Revert",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::FileImportGraphics => CommandMeta {
                id: "file.import.graphics",
                category: C::File,
                title: "File ▸ Import ▸ Import graphics to Stage…",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::FileImportLegacyFlash => CommandMeta {
                id: "file.import.legacy_flash",
                category: C::File,
                title: "File ▸ Import ▸ Import legacy Flash…",
                keywords: "fla xfl swf",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::FileExportVideo => CommandMeta {
                id: "file.export.video",
                category: C::File,
                title: "File ▸ Export ▸ Export video…",
                keywords: "mp4 webm mov gif png sequence movie",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::FileExportAudio => CommandMeta {
                id: "file.export.audio",
                category: C::File,
                title: "File ▸ Export ▸ Export audio…",
                keywords: "wav mp3 ogg aac sound",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::FileExportLottie => CommandMeta {
                id: "file.export.lottie",
                category: C::File,
                title: "File ▸ Export ▸ Export Lottie…",
                keywords: "bodymovin json animation vector",
                defaults: S_ALT_SHIFT_CMD_L,
                allow_when_typing: false,
            },
            Self::FilePublishSettings => CommandMeta {
                id: "file.publish_settings",
                category: C::File,
                title: "File ▸ Publish settings…",
                keywords: "",
                defaults: S_SHIFT_F12,
                allow_when_typing: false,
            },
            Self::FilePublish => CommandMeta {
                id: "file.publish",
                category: C::File,
                title: "File ▸ Publish",
                keywords: "export html5",
                defaults: S_PUBLISH,
                allow_when_typing: false,
            },
            Self::EditUndo => CommandMeta {
                id: "edit.undo",
                category: C::Edit,
                title: "Edit ▸ Undo",
                keywords: "",
                defaults: S_CMD_Z,
                allow_when_typing: false,
            },
            Self::EditRedo => CommandMeta {
                id: "edit.redo",
                category: C::Edit,
                title: "Edit ▸ Redo",
                keywords: "",
                defaults: S_SHIFT_CMD_Z,
                allow_when_typing: false,
            },
            Self::EditCut => CommandMeta {
                id: "edit.cut",
                category: C::Edit,
                title: "Edit ▸ Cut",
                keywords: "",
                defaults: S_CMD_X,
                allow_when_typing: false,
            },
            Self::EditCopy => CommandMeta {
                id: "edit.copy",
                category: C::Edit,
                title: "Edit ▸ Copy",
                keywords: "",
                defaults: S_CMD_C,
                allow_when_typing: false,
            },
            Self::EditPaste => CommandMeta {
                id: "edit.paste",
                category: C::Edit,
                title: "Edit ▸ Paste in centre",
                keywords: "paste",
                defaults: S_CMD_V,
                allow_when_typing: false,
            },
            Self::EditPasteInPlace => CommandMeta {
                id: "edit.paste_in_place",
                category: C::Edit,
                title: "Edit ▸ Paste in place",
                keywords: "",
                defaults: S_SHIFT_CMD_V,
                allow_when_typing: false,
            },
            Self::EditClear => CommandMeta {
                id: "edit.clear",
                category: C::Edit,
                title: "Edit ▸ Clear",
                keywords: "delete",
                defaults: S_BACKSPACE,
                allow_when_typing: false,
            },
            Self::EditDuplicate => CommandMeta {
                id: "edit.duplicate",
                category: C::Edit,
                title: "Edit ▸ Duplicate",
                keywords: "",
                defaults: S_CMD_D,
                allow_when_typing: false,
            },
            Self::EditSelectAll => CommandMeta {
                id: "edit.select_all",
                category: C::Edit,
                title: "Edit ▸ Select all",
                keywords: "",
                defaults: S_CMD_A,
                allow_when_typing: false,
            },
            Self::EditDeselect => CommandMeta {
                id: "edit.deselect",
                category: C::Edit,
                title: "Edit ▸ Deselect",
                keywords: "",
                defaults: S_SHIFT_CMD_A,
                allow_when_typing: false,
            },
            Self::EditFindAndReplace => CommandMeta {
                id: "edit.find_and_replace",
                category: C::Edit,
                title: "Edit ▸ Find and replace",
                keywords: "",
                defaults: S_CMD_F,
                allow_when_typing: false,
            },
            Self::EditTimelineCutFrames => CommandMeta {
                id: "edit.timeline.cut_frames",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Cut frames",
                keywords: "",
                defaults: S_ALT_CMD_X,
                allow_when_typing: false,
            },
            Self::EditTimelineCopyFrames => CommandMeta {
                id: "edit.timeline.copy_frames",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Copy frames",
                keywords: "",
                defaults: S_ALT_CMD_C,
                allow_when_typing: false,
            },
            Self::EditTimelinePasteFrames => CommandMeta {
                id: "edit.timeline.paste_frames",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Paste frames",
                keywords: "",
                defaults: S_ALT_CMD_V,
                allow_when_typing: false,
            },
            Self::EditTimelineClearFrames => CommandMeta {
                id: "edit.timeline.clear_frames",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Clear frames",
                keywords: "",
                defaults: S_ALT_BACKSPACE,
                allow_when_typing: false,
            },
            Self::EditTimelineRemoveFrames => CommandMeta {
                id: "edit.timeline.remove_frames",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Remove frames",
                keywords: "keyframe",
                defaults: S_SHIFT_F5,
                allow_when_typing: false,
            },
            Self::EditTimelineSelectAllFrames => CommandMeta {
                id: "edit.timeline.select_all_frames",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Select all frames",
                keywords: "",
                defaults: S_ALT_CMD_A,
                allow_when_typing: false,
            },
            Self::EditTimelineReverseFrames => CommandMeta {
                id: "edit.timeline.reverse_frames",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Reverse frames",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::EditTimelineSyncSymbols => CommandMeta {
                id: "edit.timeline.sync_symbols",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Synchronise symbols",
                keywords: "synchronize",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::EditTimelineCopyMotion => CommandMeta {
                id: "edit.timeline.copy_motion",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Copy motion",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::EditTimelinePasteMotion => CommandMeta {
                id: "edit.timeline.paste_motion",
                category: C::Edit,
                title: "Edit ▸ Timeline ▸ Paste motion",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::EditSymbols => CommandMeta {
                id: "edit.edit_symbols",
                category: C::Edit,
                title: "Edit ▸ Edit symbols",
                keywords: "",
                defaults: S_CMD_E,
                allow_when_typing: false,
            },
            Self::EditPreferences => CommandMeta {
                id: "edit.preferences",
                category: C::Edit,
                title: "Edit ▸ Preferences…",
                keywords: "",
                defaults: S_CMD_U,
                allow_when_typing: false,
            },
            Self::EditKeyboardShortcuts => CommandMeta {
                id: "edit.keyboard_shortcuts",
                category: C::Edit,
                title: "Edit ▸ Keyboard shortcuts…",
                keywords: "",
                defaults: S_ALT_SHIFT_CMD_K,
                allow_when_typing: false,
            },
            Self::ViewZoomIn => CommandMeta {
                id: "view.zoom_in",
                category: C::View,
                title: "View ▸ Zoom in",
                keywords: "",
                defaults: S_CMD_EQUALS,
                allow_when_typing: false,
            },
            Self::ViewZoomOut => CommandMeta {
                id: "view.zoom_out",
                category: C::View,
                title: "View ▸ Zoom out",
                keywords: "",
                defaults: S_CMD_MINUS,
                allow_when_typing: false,
            },
            Self::ViewZoom100 => CommandMeta {
                id: "view.zoom_100",
                category: C::View,
                title: "View ▸ Magnification ▸ 100 %",
                keywords: "actual size",
                defaults: S_CMD_1,
                allow_when_typing: false,
            },
            Self::ViewFitInWindow => CommandMeta {
                id: "view.fit_in_window",
                category: C::View,
                title: "View ▸ Magnification ▸ Fit in window",
                keywords: "",
                defaults: S_CMD_0,
                allow_when_typing: false,
            },
            Self::ViewRulers => CommandMeta {
                id: "view.rulers",
                category: C::View,
                title: "View ▸ Rulers",
                keywords: "",
                defaults: S_ALT_SHIFT_CMD_R,
                allow_when_typing: false,
            },
            Self::ViewGridShow => CommandMeta {
                id: "view.grid.show",
                category: C::View,
                title: "View ▸ Grid ▸ Show grid",
                keywords: "",
                defaults: S_CMD_QUOTE,
                allow_when_typing: false,
            },
            Self::ViewGridSnap => CommandMeta {
                id: "view.grid.snap",
                category: C::View,
                title: "View ▸ Grid ▸ Snap to grid",
                keywords: "",
                defaults: S_SHIFT_CMD_QUOTE,
                allow_when_typing: false,
            },
            Self::ViewGridEdit => CommandMeta {
                id: "view.grid.edit",
                category: C::View,
                title: "View ▸ Grid ▸ Edit grid…",
                keywords: "view settings",
                defaults: S_ALT_CMD_G,
                allow_when_typing: false,
            },
            Self::ViewGuidesShow => CommandMeta {
                id: "view.guides.show",
                category: C::View,
                title: "View ▸ Guides ▸ Show guides",
                keywords: "",
                defaults: S_CMD_SEMI,
                allow_when_typing: false,
            },
            Self::ViewGuidesLock => CommandMeta {
                id: "view.guides.lock",
                category: C::View,
                title: "View ▸ Guides ▸ Lock guides",
                keywords: "",
                defaults: S_ALT_CMD_SEMI,
                allow_when_typing: false,
            },
            Self::ViewGuidesSnap => CommandMeta {
                id: "view.guides.snap",
                category: C::View,
                title: "View ▸ Guides ▸ Snap to guides",
                keywords: "",
                defaults: S_SHIFT_CMD_SEMI,
                allow_when_typing: false,
            },
            Self::InsertNewSymbol => CommandMeta {
                id: "insert.new_symbol",
                category: C::Insert,
                title: "Insert ▸ New symbol…",
                keywords: "",
                defaults: S_CMD_F8,
                allow_when_typing: false,
            },
            Self::InsertLayer => CommandMeta {
                id: "insert.layer",
                category: C::Insert,
                title: "Insert ▸ Layer",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::InsertLayerFolder => CommandMeta {
                id: "insert.layer_folder",
                category: C::Insert,
                title: "Insert ▸ Layer folder",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::InsertMotionGuide => CommandMeta {
                id: "insert.motion_guide",
                category: C::Insert,
                title: "Insert ▸ Motion guide",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::InsertFrame => CommandMeta {
                id: "insert.frame",
                category: C::Insert,
                title: "Insert ▸ Frame",
                keywords: "keyframe",
                defaults: S_F5,
                allow_when_typing: false,
            },
            Self::InsertKeyframe => CommandMeta {
                id: "insert.keyframe",
                category: C::Insert,
                title: "Insert ▸ Keyframe",
                keywords: "",
                defaults: S_F6,
                allow_when_typing: false,
            },
            Self::InsertBlankKeyframe => CommandMeta {
                id: "insert.blank_keyframe",
                category: C::Insert,
                title: "Insert ▸ Blank keyframe",
                keywords: "",
                defaults: S_F7,
                allow_when_typing: false,
            },
            Self::InsertClassicTween => CommandMeta {
                id: "insert.classic_tween",
                category: C::Insert,
                title: "Insert ▸ Classic tween",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::InsertShapeTween => CommandMeta {
                id: "insert.shape_tween",
                category: C::Insert,
                title: "Insert ▸ Shape tween",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::InsertMotionTween => CommandMeta {
                id: "insert.motion_tween",
                category: C::Insert,
                title: "Insert ▸ Motion tween",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::InsertScene => CommandMeta {
                id: "insert.scene",
                category: C::Insert,
                title: "Insert ▸ Scene",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ModifyDocument => CommandMeta {
                id: "modify.document",
                category: C::Document,
                title: "Document ▸ Document settings…",
                keywords: "",
                defaults: S_CMD_J,
                allow_when_typing: false,
            },
            Self::ModifyConvertToSymbol => CommandMeta {
                id: "modify.convert_to_symbol",
                category: C::Modify,
                title: "Modify ▸ Convert to symbol…",
                keywords: "",
                defaults: S_F8,
                allow_when_typing: false,
            },
            Self::ModifyBreakApart => CommandMeta {
                id: "modify.break_apart",
                category: C::Modify,
                title: "Modify ▸ Break apart",
                keywords: "",
                defaults: S_CMD_B,
                allow_when_typing: false,
            },
            Self::ModifyTraceBitmap => CommandMeta {
                id: "modify.bitmap.trace_bitmap",
                category: C::Modify,
                title: "Modify ▸ Bitmap ▸ Trace bitmap…",
                keywords: "vectorize image raster",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ModifyDistributeToLayers => CommandMeta {
                id: "modify.timeline.distribute_to_layers",
                category: C::Modify,
                title: "Modify ▸ Timeline ▸ Distribute to layers",
                keywords: "",
                defaults: S_SHIFT_CMD_D,
                allow_when_typing: false,
            },
            Self::ModifyTimelineLayerProperties => CommandMeta {
                id: "modify.timeline.layer_properties",
                category: C::Modify,
                title: "Modify ▸ Timeline ▸ Layer properties…",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ModifyTimelineClearKeyframe => CommandMeta {
                id: "modify.timeline.clear_keyframe",
                category: C::Modify,
                title: "Clear keyframe",
                keywords: "modify timeline",
                defaults: S_SHIFT_F6,
                allow_when_typing: false,
            },
            Self::ModifyTimelineConvertToKeyframes => CommandMeta {
                id: "modify.timeline.convert_to_keyframes",
                category: C::Modify,
                title: "Convert to keyframes",
                keywords: "modify timeline",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ModifyTimelineConvertToBlankKeyframes => CommandMeta {
                id: "modify.timeline.convert_to_blank_keyframes",
                category: C::Modify,
                title: "Modify ▸ Timeline ▸ Convert to blank keyframes",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ModifyArrangeFront => CommandMeta {
                id: "modify.arrange.front",
                category: C::Modify,
                title: "Modify ▸ Arrange ▸ Bring to front",
                keywords: "depth z-order stack",
                defaults: S_ARRANGE_FRONT,
                allow_when_typing: false,
            },
            Self::ModifyArrangeForward => CommandMeta {
                id: "modify.arrange.forward",
                category: C::Modify,
                title: "Modify ▸ Arrange ▸ Bring forward",
                keywords: "depth z-order stack",
                defaults: S_ARRANGE_FWD,
                allow_when_typing: false,
            },
            Self::ModifyArrangeBackward => CommandMeta {
                id: "modify.arrange.backward",
                category: C::Modify,
                title: "Modify ▸ Arrange ▸ Send backward",
                keywords: "depth z-order stack",
                defaults: S_ARRANGE_BWD,
                allow_when_typing: false,
            },
            Self::ModifyArrangeBack => CommandMeta {
                id: "modify.arrange.back",
                category: C::Modify,
                title: "Modify ▸ Arrange ▸ Send to back",
                keywords: "depth z-order stack",
                defaults: S_ARRANGE_BACK,
                allow_when_typing: false,
            },
            Self::ModifyGroup => CommandMeta {
                id: "modify.group",
                category: C::Modify,
                title: "Modify ▸ Group",
                keywords: "",
                defaults: S_CMD_G,
                allow_when_typing: false,
            },
            Self::ModifyUngroup => CommandMeta {
                id: "modify.ungroup",
                category: C::Modify,
                title: "Modify ▸ Ungroup",
                keywords: "",
                defaults: S_SHIFT_CMD_G,
                allow_when_typing: false,
            },
            Self::ControlPlay => CommandMeta {
                id: "control.play",
                category: C::Control,
                title: "Control ▸ Play",
                keywords: "stop pause preview",
                defaults: S_ENTER,
                allow_when_typing: false,
            },
            Self::ControlRewind => CommandMeta {
                id: "control.rewind",
                category: C::Control,
                title: "Control ▸ Rewind",
                keywords: "",
                defaults: S_ALT_SHIFT_R,
                allow_when_typing: false,
            },
            Self::ControlGoToEnd => CommandMeta {
                id: "control.go_to_end",
                category: C::Control,
                title: "Control ▸ Go to end",
                keywords: "",
                defaults: S_ALT_SHIFT_PERIOD,
                allow_when_typing: false,
            },
            Self::ControlStepForward => CommandMeta {
                id: "control.step_forward",
                category: C::Control,
                title: "Control ▸ Step forward one frame",
                keywords: "next frame",
                defaults: S_STEP_FWD,
                allow_when_typing: false,
            },
            Self::ControlStepBack => CommandMeta {
                id: "control.step_back",
                category: C::Control,
                title: "Control ▸ Step back one frame",
                keywords: "previous frame",
                defaults: S_STEP_BACK,
                allow_when_typing: false,
            },
            Self::ControlTestMovie => CommandMeta {
                id: "control.test_movie",
                category: C::Control,
                title: "Control ▸ Test movie",
                keywords: "",
                defaults: S_CMD_ENTER,
                allow_when_typing: true,
            },
            Self::ControlTestScene => CommandMeta {
                id: "control.test_scene",
                category: C::Control,
                title: "Control ▸ Test scene",
                keywords: "",
                defaults: S_ALT_CMD_ENTER,
                allow_when_typing: true,
            },
            Self::ControlLoopPlayback => CommandMeta {
                id: "control.loop_playback",
                category: C::Control,
                title: "Control ▸ Loop playback",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ControlPlayAllScenes => CommandMeta {
                id: "control.play_all_scenes",
                category: C::Control,
                title: "Control ▸ Play all scenes",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ControlEnableSimpleButtons => CommandMeta {
                id: "control.enable_simple_buttons",
                category: C::Control,
                title: "Control ▸ Enable simple buttons",
                keywords: "",
                defaults: S_ALT_CMD_B,
                allow_when_typing: false,
            },
            Self::ControlMuteSounds => CommandMeta {
                id: "control.mute_sounds",
                category: C::Control,
                title: "Control ▸ Mute sounds",
                keywords: "",
                defaults: S_ALT_CMD_M,
                allow_when_typing: false,
            },
            Self::WindowTimeline => CommandMeta {
                id: "window.timeline",
                category: C::Window,
                title: "Window ▸ Panels ▸ Timeline",
                keywords: "",
                defaults: S_ALT_CMD_T,
                allow_when_typing: false,
            },
            Self::WindowTools => CommandMeta {
                id: "window.tools",
                category: C::Window,
                title: "Window ▸ Panels ▸ Tools",
                keywords: "",
                defaults: S_CMD_F2,
                allow_when_typing: false,
            },
            Self::WindowProperties => CommandMeta {
                id: "window.properties",
                category: C::Window,
                title: "Window ▸ Panels ▸ Properties",
                keywords: "",
                defaults: S_CMD_F3,
                allow_when_typing: false,
            },
            Self::WindowLibrary => CommandMeta {
                id: "window.library",
                category: C::Window,
                title: "Window ▸ Panels ▸ Library",
                keywords: "",
                defaults: S_CMD_L,
                allow_when_typing: false,
            },
            Self::WindowActions => CommandMeta {
                id: "window.actions",
                category: C::Window,
                title: "Window ▸ Panels ▸ Actions",
                keywords: "script code",
                defaults: S_F9,
                allow_when_typing: true,
            },
            Self::WindowAlign => CommandMeta {
                id: "window.align",
                category: C::Window,
                title: "Window ▸ Panels ▸ Align",
                keywords: "",
                defaults: S_CMD_K,
                allow_when_typing: false,
            },
            Self::WindowColor => CommandMeta {
                id: "window.color",
                category: C::Window,
                title: "Window ▸ Panels ▸ Color",
                keywords: "",
                defaults: S_SHIFT_CMD_F9,
                allow_when_typing: false,
            },
            Self::WindowSwatches => CommandMeta {
                id: "window.swatches",
                category: C::Window,
                title: "Window ▸ Panels ▸ Swatches",
                keywords: "",
                defaults: S_CMD_F9,
                allow_when_typing: false,
            },
            Self::WindowFilters => CommandMeta {
                id: "window.filters",
                category: C::Window,
                title: "Window ▸ Panels ▸ Filters",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::WindowTransform => CommandMeta {
                id: "window.transform",
                category: C::Window,
                title: "Window ▸ Panels ▸ Transform",
                keywords: "",
                defaults: S_CMD_T,
                allow_when_typing: false,
            },
            Self::WindowInfo => CommandMeta {
                id: "window.info",
                category: C::Window,
                title: "Window ▸ Panels ▸ Info",
                keywords: "",
                defaults: S_CMD_I,
                allow_when_typing: false,
            },
            Self::WindowEase => CommandMeta {
                id: "window.ease",
                category: C::Window,
                title: "Window ▸ Panels ▸ Ease",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::WindowHistory => CommandMeta {
                id: "window.history",
                category: C::Window,
                title: "Window ▸ Panels ▸ History",
                keywords: "",
                defaults: S_CMD_F10,
                allow_when_typing: false,
            },
            Self::WindowScene => CommandMeta {
                id: "window.scene",
                category: C::Window,
                title: "Window ▸ Panels ▸ Scene",
                keywords: "",
                defaults: S_SHIFT_F2,
                allow_when_typing: false,
            },
            Self::WindowCompilerErrors => CommandMeta {
                id: "window.compiler_errors",
                category: C::Window,
                title: "Window ▸ Panels ▸ Compiler errors",
                keywords: "",
                defaults: S_ALT_F2,
                allow_when_typing: false,
            },
            Self::WindowOutput => CommandMeta {
                id: "window.output",
                category: C::Window,
                title: "Window ▸ Panels ▸ Output",
                keywords: "",
                defaults: S_F2,
                allow_when_typing: false,
            },
            Self::WindowCodeSnippets => CommandMeta {
                id: "window.code_snippets",
                category: C::Window,
                title: "Window ▸ Panels ▸ Code snippets",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::WindowMovieExplorer => CommandMeta {
                id: "window.movie_explorer",
                category: C::Window,
                title: "Window ▸ Panels ▸ Movie Explorer",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::WindowAccessibility => CommandMeta {
                id: "window.accessibility",
                category: C::Window,
                title: "Window ▸ Panels ▸ Accessibility",
                keywords: "",
                defaults: S_SHIFT_F11,
                allow_when_typing: false,
            },
            Self::WindowMotionEditor => CommandMeta {
                id: "window.motion_editor",
                category: C::Window,
                title: "Window ▸ Panels ▸ Motion editor",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::WindowBrushes => CommandMeta {
                id: "window.brushes",
                category: C::Window,
                title: "Window ▸ Panels ▸ Brushes",
                keywords: "brush presets",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::WindowProject => CommandMeta {
                id: "window.project",
                category: C::Window,
                title: "Window ▸ Panels ▸ Project",
                keywords: "",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::WindowHidePanels => CommandMeta {
                id: "window.hide_panels",
                category: C::Window,
                title: "Window ▸ Hide panels",
                keywords: "",
                defaults: S_F4,
                allow_when_typing: false,
            },
            Self::WindowInspectorPriority => CommandMeta {
                id: "window.inspector_priority",
                category: C::Window,
                title: "Window ▸ Inspector priority",
                keywords: "properties dock expand focus",
                defaults: S_ALT_CMD_F3,
                allow_when_typing: false,
            },
            Self::WindowLibraryPriority => CommandMeta {
                id: "window.library_priority",
                category: C::Window,
                title: "Window ▸ Library priority",
                keywords: "library dock expand focus",
                defaults: S_ALT_CMD_L,
                allow_when_typing: false,
            },
            Self::ToolSelect => CommandMeta {
                id: "tool.select",
                category: C::Tools,
                title: "Tools ▸ Selection",
                keywords: "",
                defaults: S_TOOL_V,
                allow_when_typing: false,
            },
            Self::ToolSubselect => CommandMeta {
                id: "tool.subselect",
                category: C::Tools,
                title: "Tools ▸ Subselection",
                keywords: "edit points",
                defaults: S_TOOL_A,
                allow_when_typing: false,
            },
            Self::ToolTransform => CommandMeta {
                id: "tool.transform",
                category: C::Tools,
                title: "Tools ▸ Free Transform",
                keywords: "",
                defaults: S_TOOL_Q,
                allow_when_typing: false,
            },
            Self::ToolGradient => CommandMeta {
                id: "tool.gradient",
                category: C::Tools,
                title: "Tools ▸ Gradient Transform",
                keywords: "",
                defaults: S_TOOL_F,
                allow_when_typing: false,
            },
            Self::ToolLasso => CommandMeta {
                id: "tool.lasso",
                category: C::Tools,
                title: "Tools ▸ Lasso",
                keywords: "",
                defaults: S_TOOL_L,
                allow_when_typing: false,
            },
            Self::ToolPen => CommandMeta {
                id: "tool.pen",
                category: C::Tools,
                title: "Tools ▸ Pen",
                keywords: "",
                defaults: S_TOOL_P,
                allow_when_typing: false,
            },
            Self::ToolLine => CommandMeta {
                id: "tool.line",
                category: C::Tools,
                title: "Tools ▸ Line",
                keywords: "",
                defaults: S_TOOL_N,
                allow_when_typing: false,
            },
            Self::ToolText => CommandMeta {
                id: "tool.text",
                category: C::Tools,
                title: "Tools ▸ Text",
                keywords: "",
                defaults: S_TOOL_T,
                allow_when_typing: false,
            },
            Self::ToolRectangle => CommandMeta {
                id: "tool.rectangle",
                category: C::Tools,
                title: "Tools ▸ Rectangle",
                keywords: "",
                defaults: S_TOOL_R,
                allow_when_typing: false,
            },
            Self::ToolOval => CommandMeta {
                id: "tool.oval",
                category: C::Tools,
                title: "Tools ▸ Oval",
                keywords: "ellipse",
                defaults: S_TOOL_O,
                allow_when_typing: false,
            },
            Self::ToolPolyStar => CommandMeta {
                id: "tool.polystar",
                category: C::Tools,
                title: "Tools ▸ PolyStar",
                keywords: "polygon star",
                defaults: &[],
                allow_when_typing: false,
            },
            Self::ToolPencil => CommandMeta {
                id: "tool.pencil",
                category: C::Tools,
                title: "Tools ▸ Pencil",
                keywords: "",
                defaults: S_TOOL_Y,
                allow_when_typing: false,
            },
            Self::ToolBrush => CommandMeta {
                id: "tool.brush",
                category: C::Tools,
                title: "Tools ▸ Brush",
                keywords: "",
                defaults: S_TOOL_B,
                allow_when_typing: false,
            },
            Self::ToolBucket => CommandMeta {
                id: "tool.bucket",
                category: C::Tools,
                title: "Tools ▸ Paint Bucket",
                keywords: "",
                defaults: S_TOOL_K,
                allow_when_typing: false,
            },
            Self::ToolInk => CommandMeta {
                id: "tool.ink",
                category: C::Tools,
                title: "Tools ▸ Ink Bottle",
                keywords: "",
                defaults: S_TOOL_S,
                allow_when_typing: false,
            },
            Self::ToolEyedropper => CommandMeta {
                id: "tool.eyedropper",
                category: C::Tools,
                title: "Tools ▸ Eyedropper",
                keywords: "",
                defaults: S_TOOL_I,
                allow_when_typing: false,
            },
            Self::ToolEraser => CommandMeta {
                id: "tool.eraser",
                category: C::Tools,
                title: "Tools ▸ Eraser",
                keywords: "",
                defaults: S_TOOL_E,
                allow_when_typing: false,
            },
            Self::ToolHand => CommandMeta {
                id: "tool.hand",
                category: C::Tools,
                title: "Tools ▸ Hand",
                keywords: "",
                defaults: S_TOOL_H,
                allow_when_typing: false,
            },
            Self::ToolZoom => CommandMeta {
                id: "tool.zoom",
                category: C::Tools,
                title: "Tools ▸ Zoom",
                keywords: "",
                defaults: S_TOOL_ZM,
                allow_when_typing: false,
            },
            Self::ToolObjectDrawing => CommandMeta {
                id: "tool.object_drawing",
                category: C::Tools,
                title: "Tools ▸ Object Drawing",
                keywords: "merge drawing",
                defaults: S_TOOL_J,
                allow_when_typing: false,
            },
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.meta().id
    }

    pub const fn category(self) -> CommandCategory {
        self.meta().category
    }

    pub const fn title(self) -> &'static str {
        self.meta().title
    }

    /// Full `"<Menu> ▸ …"` path used by search, the recorder conflict strip and `"Go to <menu path>"`.
    pub const fn menu_path(self) -> &'static str {
        match self {
            Self::ModifyTimelineClearKeyframe => "Modify ▸ Timeline ▸ Clear keyframe",
            Self::ModifyTimelineConvertToKeyframes => "Modify ▸ Timeline ▸ Convert to keyframes",
            _ => self.meta().title,
        }
    }

    /// Command title inside its `<Menu>` group in `Preferences ▸ Shortcuts` (R-046).
    pub fn row_title(self) -> &'static str {
        let title = self.meta().title;
        if let Some((head, tail)) = title.split_once(" ▸ ")
            && head == self.category().label()
        {
            tail
        } else {
            title
        }
    }

    pub const fn search_keywords(self) -> &'static str {
        self.meta().keywords
    }

    pub const fn default_shortcuts(self) -> &'static [Shortcut] {
        self.meta().defaults
    }

    pub const fn allow_when_typing(self) -> bool {
        self.meta().allow_when_typing
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|cmd| cmd.as_str() == id)
    }

    pub const fn for_panel(panel: PanelId) -> Self {
        match panel {
            PanelId::Timeline => Self::WindowTimeline,
            PanelId::Tools => Self::WindowTools,
            PanelId::Properties => Self::WindowProperties,
            PanelId::Library => Self::WindowLibrary,
            PanelId::Actions => Self::WindowActions,
            PanelId::Align => Self::WindowAlign,
            PanelId::Color => Self::WindowColor,
            PanelId::Swatches => Self::WindowSwatches,
            PanelId::Filters => Self::WindowFilters,
            PanelId::Transform => Self::WindowTransform,
            PanelId::Info => Self::WindowInfo,
            PanelId::Ease => Self::WindowEase,
            PanelId::History => Self::WindowHistory,
            PanelId::Scene => Self::WindowScene,
            PanelId::CompilerErrors => Self::WindowCompilerErrors,
            PanelId::Output => Self::WindowOutput,
            PanelId::CodeSnippets => Self::WindowCodeSnippets,
            PanelId::MovieExplorer => Self::WindowMovieExplorer,
            PanelId::Accessibility => Self::WindowAccessibility,
            PanelId::MotionEditor => Self::WindowMotionEditor,
            PanelId::Project => Self::WindowProject,
            PanelId::Brushes => Self::WindowBrushes,
        }
    }

    pub const fn for_tool(tool: Tool) -> Self {
        match tool {
            Tool::Select => Self::ToolSelect,
            Tool::Subselect => Self::ToolSubselect,
            Tool::Transform => Self::ToolTransform,
            Tool::Gradient => Self::ToolGradient,
            Tool::Lasso => Self::ToolLasso,
            Tool::Pen => Self::ToolPen,
            Tool::Line => Self::ToolLine,
            Tool::Text => Self::ToolText,
            Tool::Rectangle => Self::ToolRectangle,
            Tool::Ellipse => Self::ToolOval,
            Tool::PolyStar => Self::ToolPolyStar,
            Tool::Pencil => Self::ToolPencil,
            Tool::Brush => Self::ToolBrush,
            Tool::Bucket => Self::ToolBucket,
            Tool::Ink => Self::ToolInk,
            Tool::Eyedropper => Self::ToolEyedropper,
            Tool::Eraser => Self::ToolEraser,
            Tool::Hand => Self::ToolHand,
            Tool::Zoom => Self::ToolZoom,
        }
    }

    pub const fn for_frame_command(command: FrameCommand) -> Self {
        match command {
            FrameCommand::Cut => Self::EditTimelineCutFrames,
            FrameCommand::Copy => Self::EditTimelineCopyFrames,
            FrameCommand::Paste => Self::EditTimelinePasteFrames,
            FrameCommand::Clear => Self::EditTimelineClearFrames,
            FrameCommand::Remove => Self::EditTimelineRemoveFrames,
            FrameCommand::SelectAll => Self::EditTimelineSelectAllFrames,
        }
    }
}

/// Effective shortcuts for `cmd`: active set's override if present (even empty = unbound),
/// otherwise [`CommandId::default_shortcuts`].
pub fn effective_shortcuts(prefs: &ShortcutPrefs, cmd: CommandId) -> Vec<Shortcut> {
    if let Some(raw_list) = prefs.overrides.get(cmd.as_str()) {
        raw_list.iter().filter_map(|s| Shortcut::parse(s)).collect()
    } else {
        cmd.default_shortcuts().to_vec()
    }
}

/// Whether `prefs` stores an explicit override differing from `cmd`'s default bindings.
pub fn has_override(prefs: &ShortcutPrefs, cmd: CommandId) -> bool {
    prefs.overrides.contains_key(cmd.as_str())
        && effective_shortcuts(prefs, cmd) != cmd.default_shortcuts()
}

/// Primary NF2 shortcut label for menu items, tooltips and status chips (`""` when unbound).
pub fn primary_shortcut_label(prefs: &ShortcutPrefs, cmd: CommandId) -> String {
    effective_shortcuts(prefs, cmd)
        .first()
        .map(Shortcut::format)
        .unwrap_or_default()
}

/// Shortcut label for timeline transport `Previous frame` / `Next frame` buttons: uses the arrow
/// chord (`"←"` / `"→"`) when un-overridden, or the user's primary override when customized.
pub fn transport_step_label(prefs: &ShortcutPrefs, cmd: CommandId) -> String {
    if has_override(prefs, cmd) {
        primary_shortcut_label(prefs, cmd)
    } else {
        cmd.default_shortcuts()
            .get(1)
            .or_else(|| cmd.default_shortcuts().first())
            .map(Shortcut::format)
            .unwrap_or_default()
    }
}

/// Maps every command that shares an effective chord with at least one other command to the first
/// conflicting command in [`CommandId::ALL`] order.
pub fn conflicts(prefs: &ShortcutPrefs) -> BTreeMap<CommandId, CommandId> {
    let mut owners: BTreeMap<Shortcut, Vec<CommandId>> = BTreeMap::new();
    for &cmd in CommandId::ALL {
        for chord in effective_shortcuts(prefs, cmd) {
            let list = owners.entry(chord).or_default();
            if !list.contains(&cmd) {
                list.push(cmd);
            }
        }
    }
    let mut out = BTreeMap::new();
    for &cmd in CommandId::ALL {
        for chord in effective_shortcuts(prefs, cmd) {
            if let Some(list) = owners.get(&chord)
                && let Some(&other) = list.iter().find(|&&other| other != cmd)
            {
                out.entry(cmd).or_insert(other);
                break;
            }
        }
    }
    out
}

/// First command other than `cmd` in [`CommandId::ALL`] order whose effective shortcuts in `prefs`
/// include `chord`.
pub fn first_conflict_for_chord(
    prefs: &ShortcutPrefs,
    cmd: CommandId,
    chord: Shortcut,
) -> Option<CommandId> {
    CommandId::ALL
        .iter()
        .copied()
        .find(|&other| other != cmd && effective_shortcuts(prefs, other).contains(&chord))
}

fn escape_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Pure HTML reference sheet generator for `Preferences ▸ Shortcuts ▸ Export as HTML…`.
pub fn export_shortcuts_html(prefs: &ShortcutPrefs) -> String {
    let set_name = escape_html(prefs.active_set_label());
    let mut html = String::with_capacity(8192);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<title>NeoFlash Keyboard Shortcuts — ");
    html.push_str(&set_name);
    html.push_str("</title>\n<style>\n");
    html.push_str(
        "body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px;color:#18181b;background:#fafafa}\n\
         h1{font-size:20px;margin:0 0 24px}\n\
         h2{font-size:14px;text-transform:uppercase;letter-spacing:.04em;color:#52525b;margin:24px 0 8px}\n\
         table{width:100%;max-width:720px;border-collapse:collapse;background:#fff;border:1px solid #e4e4e7}\n\
         th,td{text-align:left;padding:8px 12px;border-bottom:1px solid #f4f4f5;font-size:13px}\n\
         th{background:#f4f4f5;font-weight:600}\n\
         td.sc{width:180px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;text-align:right}\n",
    );
    html.push_str("</style>\n</head>\n<body>\n<h1>NeoFlash Keyboard Shortcuts — ");
    html.push_str(&set_name);
    html.push_str("</h1>\n");

    for category in CommandCategory::ALL {
        html.push_str("<h2>");
        html.push_str(escape_html(category.label()).as_str());
        html.push_str("</h2>\n<table>\n<thead><tr><th>Command</th><th class=\"sc\">Shortcut</th></tr></thead>\n<tbody>\n");
        for &cmd in CommandId::ALL {
            if cmd.category() != category {
                continue;
            }
            let shortcuts = effective_shortcuts(prefs, cmd)
                .iter()
                .map(Shortcut::format)
                .collect::<Vec<_>>()
                .join(" / ");
            html.push_str("<tr><td>");
            html.push_str(&escape_html(cmd.title()));
            html.push_str("</td><td class=\"sc\">");
            html.push_str(&escape_html(&shortcuts));
            html.push_str("</td></tr>\n");
        }
        html.push_str("</tbody>\n</table>\n");
    }

    html.push_str("</body>\n</html>\n");
    html
}

impl Editor {
    /// Effective primary shortcut label for `cmd` (e.g. `"⌘S"`, `""` if unbound).
    pub(crate) fn sc(&self, cmd: CommandId) -> String {
        primary_shortcut_label(&self.workspace.shortcuts, cmd)
    }

    /// Shortcut string for button/tool hover tooltips, honouring `General ▸ Show tooltips with shortcuts`.
    pub(crate) fn tooltip_sc(&self, cmd: CommandId) -> Option<String> {
        if !self.workspace.general.tooltip_shortcuts {
            return None;
        }
        let s = self.sc(cmd);
        (!s.is_empty()).then_some(s)
    }

    /// Consume a matching pressed key event for `cmd` according to the active shortcut set and
    /// text-editing focus rules.
    pub(crate) fn take_command_shortcut(&self, ctx: &egui::Context, cmd: CommandId) -> bool {
        let typing = ctx.wants_keyboard_input();
        if typing && !cmd.allow_when_typing() {
            return false;
        }
        let chords = effective_shortcuts(&self.workspace.shortcuts, cmd);
        if chords.is_empty() {
            return false;
        }
        ctx.input_mut(|i| {
            let before = i.events.len();
            let mut found = false;
            i.events.retain(|event| {
                let hit = !found
                    && matches!(
                        event,
                        Event::Key {
                            key,
                            physical_key,
                            pressed: true,
                            modifiers,
                            ..
                        } if chords.iter().any(|chord| {
                            (!typing || chord.is_safe_while_typing())
                                && chord.matches_key_event(*modifiers, *key, *physical_key)
                        })
                    );
                found |= hit;
                !hit
            });
            i.events.len() != before
        })
    }

    /// Check without consuming whether `event` is a key press matching `cmd`.
    pub(crate) fn command_matches_event(&self, cmd: CommandId, event: &Event) -> bool {
        let Event::Key {
            key,
            physical_key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            return false;
        };
        effective_shortcuts(&self.workspace.shortcuts, cmd)
            .iter()
            .any(|chord| chord.matches_key_event(*modifiers, *key, *physical_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_unique_ids_and_no_default_conflicts() {
        let mut ids = std::collections::BTreeSet::new();
        for &cmd in CommandId::ALL {
            assert!(
                ids.insert(cmd.as_str()),
                "duplicate command id {}",
                cmd.as_str()
            );
            assert_eq!(CommandId::from_id(cmd.as_str()), Some(cmd));
            for chord in cmd.default_shortcuts() {
                let formatted = chord.format();
                let reparsed = Shortcut::parse(&formatted)
                    .unwrap_or_else(|| panic!("cannot reparse {formatted:?} for {cmd:?}"));
                assert_eq!(*chord, reparsed, "round-trip mismatch for {cmd:?}");
                assert_eq!(reparsed.format(), formatted, "format mismatch for {cmd:?}");
            }
        }
        let prefs = ShortcutPrefs::default();
        assert!(
            conflicts(&prefs).is_empty(),
            "built-in default shortcuts must not conflict: {:?}",
            conflicts(&prefs)
        );
    }

    #[test]
    fn default_labels_match_nf2_menu_and_tool_strings() {
        let p = ShortcutPrefs::default();
        let cases = [
            (CommandId::FileNew, "⌘N"),
            (CommandId::FileOpen, "⌘O"),
            (CommandId::FileSave, "⌘S"),
            (CommandId::FileSaveAs, "⇧⌘S"),
            (CommandId::FilePublishSettings, "⇧F12"),
            (CommandId::FilePublish, "⇧⌥F12"),
            (CommandId::EditUndo, "⌘Z"),
            (CommandId::EditRedo, "⇧⌘Z"),
            (CommandId::EditPasteInPlace, "⇧⌘V"),
            (CommandId::EditClear, "⌫"),
            (CommandId::EditTimelineClearFrames, "⌥⌫"),
            (CommandId::EditTimelineRemoveFrames, "⇧F5"),
            (CommandId::EditPreferences, "⌘U"),
            (CommandId::EditKeyboardShortcuts, "⌥⇧⌘K"),
            (CommandId::ViewZoomIn, "⌘="),
            (CommandId::ViewZoomOut, "⌘−"),
            (CommandId::ViewRulers, "⌥⇧⌘R"),
            (CommandId::ViewGridShow, "⌘'"),
            (CommandId::ViewGridSnap, "⇧⌘'"),
            (CommandId::ViewGridEdit, "⌥⌘G"),
            (CommandId::ViewGuidesShow, "⌘;"),
            (CommandId::ViewGuidesLock, "⌥⌘;"),
            (CommandId::ViewGuidesSnap, "⇧⌘;"),
            (CommandId::InsertNewSymbol, "⌘F8"),
            (CommandId::InsertFrame, "F5"),
            (CommandId::InsertKeyframe, "F6"),
            (CommandId::InsertBlankKeyframe, "F7"),
            (CommandId::ModifyConvertToSymbol, "F8"),
            (CommandId::ModifyTimelineClearKeyframe, "⇧F6"),
            (CommandId::ModifyTimelineConvertToKeyframes, ""),
            (CommandId::ModifyGroup, "⌘G"),
            (CommandId::ModifyUngroup, "⇧⌘G"),
            (CommandId::ControlPlay, "↵"),
            (CommandId::ControlRewind, "⌥⇧R"),
            (CommandId::ControlGoToEnd, "⌥⇧."),
            (CommandId::ControlStepForward, "."),
            (CommandId::ControlStepBack, ","),
            (CommandId::ControlTestMovie, "⌘↵"),
            (CommandId::ControlTestScene, "⌥⌘↵"),
            (CommandId::ControlMuteSounds, "⌥⌘M"),
            (CommandId::WindowTimeline, "⌥⌘T"),
            (CommandId::WindowActions, "F9"),
            (CommandId::WindowColor, "⇧⌘F9"),
            (CommandId::WindowSwatches, "⌘F9"),
            (CommandId::WindowCompilerErrors, "⌥F2"),
            (CommandId::ToolSelect, "V"),
            (CommandId::ToolPencil, "Y"),
            (CommandId::ToolZoom, "Z"),
        ];
        for (cmd, expected) in cases {
            assert_eq!(primary_shortcut_label(&p, cmd), expected, "{cmd:?}");
        }
        assert_eq!(transport_step_label(&p, CommandId::ControlStepBack), "←");
        assert_eq!(transport_step_label(&p, CommandId::ControlStepForward), "→");
    }

    #[test]
    fn conflict_detection_flags_both_commands_sharing_a_chord() {
        let mut prefs = ShortcutPrefs::default();
        prefs.set_shortcuts(
            CommandId::ModifyTimelineConvertToKeyframes,
            vec![Shortcut::none(Key::F6)],
        );
        let map = conflicts(&prefs);
        assert_eq!(
            map.get(&CommandId::ModifyTimelineConvertToKeyframes),
            Some(&CommandId::InsertKeyframe)
        );
        assert_eq!(
            map.get(&CommandId::InsertKeyframe),
            Some(&CommandId::ModifyTimelineConvertToKeyframes)
        );

        // Clearing InsertKeyframe resolves the conflict.
        prefs.set_shortcuts(CommandId::InsertKeyframe, vec![]);
        assert!(conflicts(&prefs).is_empty());
    }

    #[test]
    fn html_export_renders_escaped_table_with_overrides() {
        let mut prefs = ShortcutPrefs {
            set: "Studio <A&B>".to_owned(),
            ..Default::default()
        };
        prefs.set_shortcuts(CommandId::InsertKeyframe, vec![Shortcut::alt(Key::F6)]);
        let html = export_shortcuts_html(&prefs);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("Studio &lt;A&amp;B&gt;"));
        assert!(html.contains("<td>Insert ▸ Keyframe</td><td class=\"sc\">⌥F6</td>"));
        assert!(html.contains("<h2>Tools</h2>"));
    }
}
