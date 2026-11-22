//! Native macOS application menu bar, Finder open/reopen lifecycle, and `NSWindow` document state
//! (`T-309`).
//!
//! `NativeMenuSpec` and `NativeWindowDocState` are pure, OS-independent data models derived from
//! the live [`Editor`] and the [`crate::commands`] registry so every menu hierarchy, dynamic
//! `Undo <step>` / `Redo <step>` title, shortcut, checkmark, recent-files list, multi-tab document
//! list, and open/reopen transition can be unit-tested on any platform without AppKit.
//!
//! On macOS (`#[cfg(all(target_os = "macos", not(test)))]`), `install` and `sync` bridge that
//! specification onto `NSApplication.mainMenu`, `NSApplication.delegate`, and the root `NSWindow`
//! while keeping the in-window NF2 top bar and all non-macOS builds untouched.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::app::{Editor, Pending};
use crate::commands::{CommandId, Shortcut};
use crate::debugger_protocol::{DebugCommand, DebugState};
use crate::stage::StageAbComparisonMode;
use crate::theme::ThemeMode;
use crate::workspace::{DensityPref, PanelId, PanelPlacement, StageToolbarMode, WorkspaceTemplate};
use crate::workspace_dialogs::WorkspaceDialog;
use eframe::egui::{self, Key};
use neoflash_core::{BooleanOp, ElementKind, TextAlign};
use neoflash_export::FlaTargetProfile;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Panels exposed in `Window ▸ Panels`, in NF2 §8 order.
pub const NATIVE_WINDOW_PANELS: &[(&str, PanelId)] = &[
    ("Timeline", PanelId::Timeline),
    ("Tools", PanelId::Tools),
    ("Properties", PanelId::Properties),
    ("Library", PanelId::Library),
    ("Actions", PanelId::Actions),
    ("Align", PanelId::Align),
    ("Color", PanelId::Color),
    ("Swatches", PanelId::Swatches),
    ("Transform", PanelId::Transform),
    ("Info", PanelId::Info),
    ("Ease", PanelId::Ease),
    ("History", PanelId::History),
    ("Scene", PanelId::Scene),
    ("Output", PanelId::Output),
    ("Compiler errors", PanelId::CompilerErrors),
    ("Accessibility", PanelId::Accessibility),
    ("Motion editor", PanelId::MotionEditor),
    ("Debugger", PanelId::Debugger),
    ("Project", PanelId::Project),
    ("Code snippets", PanelId::CodeSnippets),
    ("Movie Explorer", PanelId::MovieExplorer),
    ("Filters", PanelId::Filters),
];

fn native_detachable_panels() -> impl Iterator<Item = (&'static str, PanelId)> {
    std::iter::once(("Brushes", PanelId::Brushes)).chain(NATIVE_WINDOW_PANELS.iter().copied())
}

const TEXT_MENU_SIZES: &[u16] = &[8, 10, 12, 14, 18, 24, 36, 48, 72, 96];

/// Complete cross-platform snapshot of the macOS menu bar derived from [`Editor`] state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeMenuSpec {
    pub menus: Vec<NativeTopMenuSpec>,
}

/// Top-level menu in [`NativeMenuSpec`] (`NeoFlash`, `File`, `Edit`, `View`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTopMenuSpec {
    pub title: String,
    pub enabled: bool,
    pub items: Vec<NativeMenuItemSpec>,
}

/// System-handled macOS responder-chain item role (`hide:`, `performMiniaturize:`, `performZoom:`, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSystemItemRole {
    HideApp,
    HideOthers,
    ShowAll,
    MinimizeWindow,
    ZoomWindow,
}

/// Special AppKit submenu role (`app.setServicesMenu:`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSubmenuRole {
    Services,
}

/// Individual row inside a [`NativeTopMenuSpec`] or nested submenu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeMenuItemSpec {
    Separator,
    Section(String),
    Action {
        label: String,
        command_id: Option<CommandId>,
        shortcut: Option<Shortcut>,
        shortcut_label: String,
        enabled: bool,
        checkable: bool,
        checked: bool,
        tooltip: Option<String>,
        action: NativeMenuAction,
    },
    System {
        label: String,
        role: NativeSystemItemRole,
        shortcut: Option<Shortcut>,
        shortcut_label: String,
        enabled: bool,
    },
    Submenu {
        label: String,
        enabled: bool,
        role: Option<NativeSubmenuRole>,
        items: Vec<NativeMenuItemSpec>,
    },
}

/// Editor action dispatched when a native or in-window menu item is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeMenuAction {
    Command(CommandId),
    Unavailable,
    About,
    Quit,
    OpenRecent(PathBuf),
    ClearRecent,
    CloseAllDocuments,
    CloseOtherDocuments,
    ImportToLibrary,
    ImportPalette,
    ImportVideo,
    InsertEmptyVideoObject,
    ExportImage,
    ExportFlaProfile(FlaTargetProfile),
    PasteMotionSpecial,
    EditSelectedSymbol,
    EditSymbolInPlace,
    SetMagnification(u32),
    ToggleGridOverObjects,
    OpenViewSettings,
    ToggleObjectSnap,
    TogglePixelSnap,
    ToggleOnionSkin,
    ToggleOnionOutlines,
    ToggleOnionAnchor,
    SetStageToolbar(StageToolbarMode),
    DisableStageAb,
    CombineObjects { op: BooleanOp, crop: bool },
    DistributeToKeyframes,
    TweenToKeys(u32),
    ConvertToKeyframesStepped2s,
    CopyKeyframeEase,
    ApplyCopiedEase,
    DeleteEmptyLayers,
    RenameLayersFromContent,
    AddActionsStopLayer,
    RunJsflDialog,
    OpenBatchRenameLibrary,
    OpenBatchLinkage,
    OpenBatchBitmapCompression,
    DeleteUnusedLibraryItems,
    OpenAutoInstanceNamer,
    OpenRandomizeSelection,
    PixelSnapSelection,
    AlignSelection { axis: usize, edge: usize },
    ToggleAlignToStage,
    ShowPanel(PanelId),
    TextSetSize(u16),
    TextSetStyle { bold: bool, italic: bool },
    TextSetAlign(TextAlign),
    TextAdjustTracking(i16),
    TextOpenFontEmbedding,
    ToggleLiveActionScript,
    DebugContinueOrStart,
    DebugStop,
    DebugStepOver,
    DebugStepInto,
    DebugStepOut,
    DebugToggleBreakpoint,
    DebugClearBreakpoints,
    ApplyWorkspaceTemplate(WorkspaceTemplate),
    ApplyNamedWorkspace(String),
    SaveWorkspaceDialog,
    UpdateActiveWorkspace,
    RenameActiveWorkspaceDialog,
    DeleteActiveWorkspaceDialog,
    ResetActiveWorkspace,
    SetDensity(DensityPref),
    SetTheme(ThemeMode),
    DetachPanel(PanelId),
    DockPanel(PanelId),
    OpenCustomizeTools,
    ToggleUiLayoutStudio,
    ActivateDocumentTab(usize),
    NextDocumentTab,
    PreviousDocumentTab,
}

/// Synchronized root `NSWindow` document presentation state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeWindowDocState {
    pub title: String,
    pub document_edited: bool,
    pub represented_filename: String,
}

/// AppKit modifier bitmasks matching `NSEventModifierFlags` (kept pure-Rust for cross-OS tests).
pub const MACOS_MOD_SHIFT: usize = 1 << 17;
pub const MACOS_MOD_CONTROL: usize = 1 << 18;
pub const MACOS_MOD_OPTION: usize = 1 << 19;
pub const MACOS_MOD_COMMAND: usize = 1 << 20;

/// Thread-safe queue of native Finder open-file, Dock reopen, and menu-bar click events.
#[derive(Default, Debug)]
pub struct NativeLifecycleQueue {
    open_files: Vec<PathBuf>,
    reopen_has_visible_windows: Option<bool>,
    actions: Vec<NativeMenuAction>,
}

impl NativeLifecycleQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_open_file(&mut self, path: PathBuf) {
        if !path.as_os_str().is_empty() {
            self.open_files.push(path);
        }
    }

    pub fn push_open_files<I>(&mut self, paths: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        for path in paths {
            self.push_open_file(path);
        }
    }

    pub fn request_reopen(&mut self, has_visible_windows: bool) {
        self.reopen_has_visible_windows = Some(has_visible_windows);
    }

    pub fn push_action(&mut self, action: NativeMenuAction) {
        self.actions.push(action);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.open_files.is_empty()
            && self.reopen_has_visible_windows.is_none()
            && self.actions.is_empty()
    }

    /// Drain queued cold-launch `open_files` into `into` during `Editor::new`, deduplicating
    /// canonical/identical paths so Finder double-click / `Open With` cold-launch opens only the
    /// requested document(s) without creating an extra Start/Demo tab.
    pub fn drain_startup_open_files(&mut self, into: &mut Vec<PathBuf>) {
        let raw_paths = std::mem::take(&mut self.open_files);
        for path in raw_paths {
            if !into
                .iter()
                .any(|seen| crate::document_sessions::same_path(seen, &path))
            {
                into.push(path);
            }
        }
    }

    /// Drain queued Finder open-file requests, Dock reopen events, and clicked menu actions into
    /// `editor`. If a modal confirmation or timeline drag currently blocks opening files, queued
    /// paths remain buffered for the next frame instead of being dropped.
    pub fn drain_into(&mut self, editor: &mut Editor, ctx: &egui::Context) -> bool {
        let mut changed = false;

        let can_mutate_sessions = editor.confirm.is_none()
            && editor.close_origin.is_none()
            && !editor.timeline_edit.dragging();

        if can_mutate_sessions && !self.open_files.is_empty() {
            let raw_paths = std::mem::take(&mut self.open_files);
            let mut deduped: Vec<PathBuf> = Vec::with_capacity(raw_paths.len());
            for path in raw_paths {
                if !deduped
                    .iter()
                    .any(|seen| crate::document_sessions::same_path(seen, &path))
                {
                    deduped.push(path);
                }
            }
            let mut opened_or_focused = false;
            for path in deduped {
                if editor.try_open_path(path, ctx) {
                    opened_or_focused = true;
                }
            }
            if opened_or_focused {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                changed = true;
            }
        }

        if let Some(has_visible_windows) = self.reopen_has_visible_windows.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if !has_visible_windows && !editor.has_document() && can_mutate_sessions {
                editor.new_document_session(ctx);
            }
            changed = true;
        }

        if !self.actions.is_empty() {
            let actions = std::mem::take(&mut self.actions);
            for action in actions {
                editor.execute_native_menu_action(ctx, action);
                changed = true;
            }
        }

        changed
    }
}

static GLOBAL_QUEUE: OnceLock<Mutex<NativeLifecycleQueue>> = OnceLock::new();

pub(crate) fn global_lifecycle_queue() -> &'static Mutex<NativeLifecycleQueue> {
    GLOBAL_QUEUE.get_or_init(|| Mutex::new(NativeLifecycleQueue::new()))
}

/// Returns `true` ONLY for OS-level application/window menu chords (`⌘H`, `⌥⌘H`, `⌘M`, `⌘Q`)
/// that AppKit handles directly via the responder chain.
///
/// All editor authoring shortcuts (`⌘S`, `⌘Z`, `⇧⌘Z`, `⌘N`, `⌘O`, `⌘W`, `⌘C`, `⌘V`, `⌘X`, `⌘A`,
/// `F5`, `F6`, `⌘↵`, etc.) return `false` so `NeoFlashMainMenu::performKeyEquivalent:` leaves the
/// `KeyDown` event untouched for `winit` -> `egui-winit` -> `Editor::shortcuts`, guaranteeing:
/// 1. Shortcuts never fire twice (once via `NSMenu` and once via `egui`).
/// 2. Focused `egui` `TextEdit` widgets keep full `⌘C` / `⌘V` / `⌘X` / `⌘A` / `⌘Z` editing.
pub fn should_route_key_equivalent_via_appkit(
    cmd: bool,
    alt: bool,
    shift: bool,
    ctrl: bool,
    characters_ignoring_modifiers: &str,
) -> bool {
    if ctrl || shift || !cmd {
        return false;
    }
    let mut chars = characters_ignoring_modifiers.chars();
    let Some(ch) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    let lower = ch.to_ascii_lowercase();
    if alt {
        lower == 'h'
    } else {
        matches!(lower, 'h' | 'm' | 'q')
    }
}

/// Convert a registry [`Shortcut`] into AppKit `(keyEquivalent, keyEquivalentModifierMask)`.
pub fn macos_key_equivalent(shortcut: Shortcut) -> Option<(String, usize)> {
    let key_str: String = match shortcut.key {
        Key::A => "a".into(),
        Key::B => "b".into(),
        Key::C => "c".into(),
        Key::D => "d".into(),
        Key::E => "e".into(),
        Key::F => "f".into(),
        Key::G => "g".into(),
        Key::H => "h".into(),
        Key::I => "i".into(),
        Key::J => "j".into(),
        Key::K => "k".into(),
        Key::L => "l".into(),
        Key::M => "m".into(),
        Key::N => "n".into(),
        Key::O => "o".into(),
        Key::P => "p".into(),
        Key::Q => "q".into(),
        Key::R => "r".into(),
        Key::S => "s".into(),
        Key::T => "t".into(),
        Key::U => "u".into(),
        Key::V => "v".into(),
        Key::W => "w".into(),
        Key::X => "x".into(),
        Key::Y => "y".into(),
        Key::Z => "z".into(),
        Key::Num0 => "0".into(),
        Key::Num1 => "1".into(),
        Key::Num2 => "2".into(),
        Key::Num3 => "3".into(),
        Key::Num4 => "4".into(),
        Key::Num5 => "5".into(),
        Key::Num6 => "6".into(),
        Key::Num7 => "7".into(),
        Key::Num8 => "8".into(),
        Key::Num9 => "9".into(),
        Key::Minus => "-".into(),
        Key::Equals | Key::Plus => "=".into(),
        Key::Comma => ",".into(),
        Key::Period => ".".into(),
        Key::Semicolon | Key::Colon => ";".into(),
        Key::Quote => "'".into(),
        Key::Slash => "/".into(),
        Key::Backslash => "\\".into(),
        Key::OpenBracket => "[".into(),
        Key::CloseBracket => "]".into(),
        Key::Backtick => "`".into(),
        Key::Enter => "\r".into(),
        Key::Backspace => "\u{0008}".into(),
        Key::Delete => "\u{f728}".into(),
        Key::Tab => "\t".into(),
        Key::Space => " ".into(),
        Key::Escape => "\u{001b}".into(),
        Key::ArrowUp => "\u{f700}".into(),
        Key::ArrowDown => "\u{f701}".into(),
        Key::ArrowLeft => "\u{f702}".into(),
        Key::ArrowRight => "\u{f703}".into(),
        Key::F1 => "\u{f704}".into(),
        Key::F2 => "\u{f705}".into(),
        Key::F3 => "\u{f706}".into(),
        Key::F4 => "\u{f707}".into(),
        Key::F5 => "\u{f708}".into(),
        Key::F6 => "\u{f709}".into(),
        Key::F7 => "\u{f70a}".into(),
        Key::F8 => "\u{f70b}".into(),
        Key::F9 => "\u{f70c}".into(),
        Key::F10 => "\u{f70d}".into(),
        Key::F11 => "\u{f70e}".into(),
        Key::F12 => "\u{f70f}".into(),
        Key::F13 => "\u{f710}".into(),
        Key::F14 => "\u{f711}".into(),
        Key::F15 => "\u{f712}".into(),
        Key::F16 => "\u{f713}".into(),
        Key::F17 => "\u{f714}".into(),
        Key::F18 => "\u{f715}".into(),
        Key::F19 => "\u{f716}".into(),
        Key::F20 => "\u{f717}".into(),
        Key::Insert => "\u{f727}".into(),
        Key::Home => "\u{f729}".into(),
        Key::End => "\u{f72b}".into(),
        Key::PageUp => "\u{f72c}".into(),
        Key::PageDown => "\u{f72d}".into(),
        _ => return None,
    };

    let mut mask = 0usize;
    if shortcut.norm_ctrl() {
        mask |= MACOS_MOD_CONTROL;
    }
    if shortcut.modifiers.alt {
        mask |= MACOS_MOD_OPTION;
    }
    if shortcut.modifiers.shift {
        mask |= MACOS_MOD_SHIFT;
    }
    if shortcut.norm_cmd() {
        mask |= MACOS_MOD_COMMAND;
    }
    Some((key_str, mask))
}

/// Extract positional startup document arguments, skipping `argv[0]`, macOS LaunchServices
/// `-psn_*` process serial numbers, AppKit `-NS*` / `-Apple*` system flag pairs, and returning
/// empty whenever `--headless` is active.
pub fn startup_file_args_from<I>(args: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = OsString>,
{
    let raw: Vec<OsString> = args.into_iter().collect();
    if raw.len() <= 1 {
        return Vec::new();
    }
    if raw[1..]
        .iter()
        .any(|arg| arg.to_string_lossy() == "--headless")
    {
        return Vec::new();
    }

    let mut files = Vec::new();
    let mut index = 1usize;
    let mut passthrough = false;
    while index < raw.len() {
        let arg = &raw[index];
        let lossy = arg.to_string_lossy();
        if !passthrough {
            if lossy == "--" {
                passthrough = true;
                index += 1;
                continue;
            }
            if lossy.starts_with("-psn_") {
                index += 1;
                continue;
            }
            if lossy.starts_with("-NS") || lossy.starts_with("-Apple") {
                // AppKit launch flags such as `-NSDocumentRevisionsDebugMode YES` carry a value.
                index += 1;
                if index < raw.len() {
                    let next = raw[index].to_string_lossy();
                    if matches!(next.as_ref(), "YES" | "NO" | "true" | "false" | "1" | "0") {
                        index += 1;
                    }
                }
                continue;
            }
            if lossy.starts_with('-') {
                index += 1;
                continue;
            }
        }
        if !arg.is_empty() {
            files.push(PathBuf::from(arg));
        }
        index += 1;
    }
    files
}

/// Extract positional startup document arguments from `std::env::args_os()`.
pub fn startup_file_args() -> Vec<PathBuf> {
    startup_file_args_from(std::env::args_os())
}

fn is_command_checkable(cmd: CommandId) -> bool {
    matches!(
        cmd,
        CommandId::ViewZoom100
            | CommandId::ViewRulers
            | CommandId::ViewGridShow
            | CommandId::ViewGridSnap
            | CommandId::ViewGuidesShow
            | CommandId::ViewGuidesLock
            | CommandId::ViewGuidesSnap
            | CommandId::ViewShowShapeHints
            | CommandId::ToggleStageAbSplit
            | CommandId::ModifyFreeTransform
            | CommandId::ModifyDistort
            | CommandId::ModifyEnvelope
            | CommandId::ModifyScale
            | CommandId::ModifyRotateAndSkew
            | CommandId::ToggleMotionSketch
            | CommandId::ControlLoopPlayback
            | CommandId::ControlPlayAllScenes
            | CommandId::ControlEnableSimpleButtons
            | CommandId::ControlMuteSounds
            | CommandId::WindowHidePanels
            | CommandId::WindowInspectorPriority
            | CommandId::WindowLibraryPriority
            | CommandId::WindowBrushes
            | CommandId::WindowTimeline
            | CommandId::WindowTools
            | CommandId::WindowProperties
            | CommandId::WindowLibrary
            | CommandId::WindowActions
            | CommandId::WindowAlign
            | CommandId::WindowColor
            | CommandId::WindowSwatches
            | CommandId::WindowFilters
            | CommandId::WindowTransform
            | CommandId::WindowInfo
            | CommandId::WindowEase
            | CommandId::WindowHistory
            | CommandId::WindowScene
            | CommandId::WindowCompilerErrors
            | CommandId::WindowOutput
            | CommandId::WindowCodeSnippets
            | CommandId::WindowMovieExplorer
            | CommandId::WindowAccessibility
            | CommandId::WindowMotionEditor
            | CommandId::WindowDebugger
            | CommandId::WindowProject
            | CommandId::ToolObjectDrawing
    )
}

fn cmd_item(editor: &Editor, ctx: Option<&egui::Context>, cmd: CommandId) -> NativeMenuItemSpec {
    let state = editor.command_menu_state(ctx, cmd);
    NativeMenuItemSpec::Action {
        label: state.label,
        command_id: Some(cmd),
        shortcut: state.shortcut,
        shortcut_label: state.shortcut_label,
        enabled: state.enabled,
        checkable: is_command_checkable(cmd),
        checked: state.checked,
        tooltip: None,
        action: NativeMenuAction::Command(cmd),
    }
}

fn cmd_item_labeled(
    editor: &Editor,
    ctx: Option<&egui::Context>,
    label: impl Into<String>,
    cmd: CommandId,
) -> NativeMenuItemSpec {
    let state = editor.command_menu_state(ctx, cmd);
    NativeMenuItemSpec::Action {
        label: label.into(),
        command_id: Some(cmd),
        shortcut: state.shortcut,
        shortcut_label: state.shortcut_label,
        enabled: state.enabled,
        checkable: is_command_checkable(cmd),
        checked: state.checked,
        tooltip: None,
        action: NativeMenuAction::Command(cmd),
    }
}

fn custom_item(
    label: impl Into<String>,
    shortcut: Option<Shortcut>,
    enabled: bool,
    checked: bool,
    action: NativeMenuAction,
) -> NativeMenuItemSpec {
    let shortcut_label = shortcut.map(|sc| sc.format()).unwrap_or_default();
    NativeMenuItemSpec::Action {
        label: label.into(),
        command_id: None,
        shortcut,
        shortcut_label,
        enabled,
        checkable: false,
        checked,
        tooltip: None,
        action,
    }
}

fn custom_check_item(
    label: impl Into<String>,
    shortcut: Option<Shortcut>,
    enabled: bool,
    checked: bool,
    action: NativeMenuAction,
) -> NativeMenuItemSpec {
    let shortcut_label = shortcut.map(|sc| sc.format()).unwrap_or_default();
    NativeMenuItemSpec::Action {
        label: label.into(),
        command_id: None,
        shortcut,
        shortcut_label,
        enabled,
        checkable: true,
        checked,
        tooltip: None,
        action,
    }
}

fn unavailable_action_item(
    label: impl Into<String>,
    shortcut: Option<Shortcut>,
    checkable: bool,
    checked: bool,
) -> NativeMenuItemSpec {
    let shortcut_label = shortcut.map(|sc| sc.format()).unwrap_or_default();
    NativeMenuItemSpec::Action {
        label: label.into(),
        command_id: None,
        shortcut,
        shortcut_label,
        enabled: false,
        checkable,
        checked,
        tooltip: Some("Not available yet".to_owned()),
        action: NativeMenuAction::Unavailable,
    }
}

fn unavailable_submenu_item(label: impl Into<String>) -> NativeMenuItemSpec {
    NativeMenuItemSpec::Submenu {
        label: label.into(),
        enabled: false,
        role: None,
        items: Vec::new(),
    }
}

fn recent_file_display_labels(paths: &[PathBuf]) -> Vec<String> {
    let names: Vec<String> = paths
        .iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| p.display().to_string())
        })
        .collect();
    paths
        .iter()
        .enumerate()
        .map(|(idx, path)| {
            let name = &names[idx];
            let duplicate = names.iter().filter(|other| *other == name).count() > 1;
            if duplicate
                && let Some(folder) = path
                    .parent()
                    .and_then(Path::file_name)
                    .map(|f| f.to_string_lossy())
                && !folder.is_empty()
            {
                format!("{name} — {folder}")
            } else {
                name.clone()
            }
        })
        .collect()
}

impl NativeMenuSpec {
    /// Canonical top-level menu bar order shared 1:1 by the native macOS menu bar and the in-window
    /// NF2 top bar (`T-377`).
    pub const TOP_LEVEL_TITLES: [&str; 11] = [
        "NeoFlash", "File", "Edit", "View", "Insert", "Modify", "Text", "Control", "Debug",
        "Window", "Help",
    ];

    /// Build a single top-level menu specification by title (`Self::TOP_LEVEL_TITLES`).
    pub fn build_top_menu(
        editor: &Editor,
        ctx: Option<&egui::Context>,
        title: &str,
    ) -> Option<NativeTopMenuSpec> {
        Some(match title {
            "NeoFlash" => Self::build_app_menu(editor, ctx),
            "File" => Self::build_file_menu(editor, ctx),
            "Edit" => Self::build_edit_menu(editor, ctx),
            "View" => Self::build_view_menu(editor, ctx),
            "Insert" => Self::build_insert_menu(editor, ctx),
            "Modify" => Self::build_modify_menu(editor, ctx),
            "Text" => Self::build_text_menu(editor),
            "Control" => Self::build_control_menu(editor, ctx),
            "Debug" => Self::build_debug_menu(editor, ctx),
            "Window" => Self::build_window_menu(editor, ctx),
            "Help" => Self::build_help_menu(editor, ctx),
            _ => return None,
        })
    }

    /// Derive the complete application menu bar specification from `editor` and the command registry.
    pub fn build(editor: &Editor, ctx: Option<&egui::Context>) -> Self {
        Self {
            menus: Self::TOP_LEVEL_TITLES
                .into_iter()
                .map(|title| {
                    Self::build_top_menu(editor, ctx, title).expect("known top-level menu")
                })
                .collect(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn menu(&self, title: &str) -> Option<&NativeTopMenuSpec> {
        self.menus.iter().find(|m| m.title == title)
    }

    /// Recursively find the first `Action` item registered for `cmd`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn find_command(&self, cmd: CommandId) -> Option<&NativeMenuItemSpec> {
        fn search(items: &[NativeMenuItemSpec], cmd: CommandId) -> Option<&NativeMenuItemSpec> {
            for item in items {
                match item {
                    NativeMenuItemSpec::Action {
                        command_id: Some(found),
                        ..
                    } if *found == cmd => return Some(item),
                    NativeMenuItemSpec::Submenu { items: sub, .. } => {
                        if let Some(hit) = search(sub, cmd) {
                            return Some(hit);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        self.menus.iter().find_map(|m| search(&m.items, cmd))
    }

    fn build_app_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        NativeTopMenuSpec {
            title: "NeoFlash".to_owned(),
            enabled: true,
            items: vec![
                custom_item("About NeoFlash", None, true, false, NativeMenuAction::About),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::EditPreferences),
                cmd_item(editor, ctx, CommandId::EditKeyboardShortcuts),
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Services".to_owned(),
                    enabled: true,
                    role: Some(NativeSubmenuRole::Services),
                    items: Vec::new(),
                },
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::System {
                    label: "Hide NeoFlash".to_owned(),
                    role: NativeSystemItemRole::HideApp,
                    shortcut: Some(Shortcut::cmd(Key::H)),
                    shortcut_label: "⌘H".to_owned(),
                    enabled: true,
                },
                NativeMenuItemSpec::System {
                    label: "Hide Others".to_owned(),
                    role: NativeSystemItemRole::HideOthers,
                    shortcut: Some(Shortcut::alt_cmd(Key::H)),
                    shortcut_label: "⌥⌘H".to_owned(),
                    enabled: true,
                },
                NativeMenuItemSpec::System {
                    label: "Show All".to_owned(),
                    role: NativeSystemItemRole::ShowAll,
                    shortcut: None,
                    shortcut_label: String::new(),
                    enabled: true,
                },
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Quit NeoFlash",
                    Some(Shortcut::cmd(Key::Q)),
                    true,
                    false,
                    NativeMenuAction::Quit,
                ),
            ],
        }
    }

    fn build_file_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_authoring_doc = has_doc && !editor.is_standalone_script_session();
        let recent = editor.workspace.general.existing_recent_files_cached();
        let recent_labels = recent_file_display_labels(&recent);
        let mut recent_items = Vec::with_capacity(recent.len() + 2);
        for (path, label) in recent.iter().zip(recent_labels) {
            recent_items.push(NativeMenuItemSpec::Action {
                label,
                command_id: None,
                shortcut: None,
                shortcut_label: String::new(),
                enabled: true,
                checkable: false,
                checked: false,
                tooltip: Some(path.display().to_string()),
                action: NativeMenuAction::OpenRecent(path.clone()),
            });
        }
        if !recent.is_empty() {
            recent_items.push(NativeMenuItemSpec::Separator);
        }
        recent_items.push(custom_item(
            "Clear Menu",
            None,
            !recent.is_empty(),
            false,
            NativeMenuAction::ClearRecent,
        ));

        let fla_items: Vec<NativeMenuItemSpec> = FlaTargetProfile::ALL
            .into_iter()
            .map(|profile| {
                custom_item(
                    format!("{}…", profile.label()),
                    None,
                    has_authoring_doc,
                    false,
                    NativeMenuAction::ExportFlaProfile(profile),
                )
            })
            .collect();

        let import_items = vec![
            cmd_item(editor, ctx, CommandId::FileImportGraphics),
            custom_item(
                "Import to Library…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::ImportToLibrary,
            ),
            custom_item(
                "Import colour palette…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::ImportPalette,
            ),
            custom_item(
                "Import video…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::ImportVideo,
            ),
            custom_item(
                "Empty video object",
                None,
                has_authoring_doc && editor.layer_editable(),
                false,
                NativeMenuAction::InsertEmptyVideoObject,
            ),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::FileImportLegacyFlash),
        ];

        let export_items = vec![
            custom_item(
                "Export image…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::ExportImage,
            ),
            cmd_item(editor, ctx, CommandId::FileExportVideo),
            cmd_item(editor, ctx, CommandId::FileExportAudio),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::FileExportLottie),
            cmd_item(editor, ctx, CommandId::FileExportAfterEffects),
            cmd_item(editor, ctx, CommandId::FileExportXcodeSwift),
            cmd_item(editor, ctx, CommandId::FileExportTextureAtlas),
            cmd_item(editor, ctx, CommandId::FileExportSwf),
            NativeMenuItemSpec::Submenu {
                label: "Export SWF Target".to_owned(),
                enabled: has_authoring_doc,
                role: None,
                items: vec![
                    cmd_item(editor, ctx, CommandId::FileExportSwfFp8),
                    cmd_item(editor, ctx, CommandId::FileExportSwfFp9),
                    cmd_item(editor, ctx, CommandId::FileExportSwfFp10),
                    cmd_item(editor, ctx, CommandId::FileExportSwfFp10Scriptless),
                ],
            },
            NativeMenuItemSpec::Submenu {
                label: "Export FLA / XFL Target".to_owned(),
                enabled: has_authoring_doc,
                role: None,
                items: fla_items,
            },
        ];

        let mut publish_item = cmd_item(editor, ctx, CommandId::FilePublish);
        if let NativeMenuItemSpec::Action { tooltip, .. } = &mut publish_item {
            *tooltip = Some("Choose an output folder and publish the project as HTML5".to_owned());
        }

        NativeTopMenuSpec {
            title: "File".to_owned(),
            enabled: true,
            items: vec![
                cmd_item(editor, ctx, CommandId::FileNew),
                cmd_item(editor, ctx, CommandId::FileNewXcodeSwiftScene),
                cmd_item(editor, ctx, CommandId::FileOpen),
                NativeMenuItemSpec::Submenu {
                    label: "Open recent".to_owned(),
                    enabled: true,
                    role: None,
                    items: recent_items,
                },
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::FileClose),
                custom_item(
                    "Close All",
                    Some(Shortcut::alt_cmd(Key::W)),
                    has_doc,
                    false,
                    NativeMenuAction::CloseAllDocuments,
                ),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::FileSave),
                cmd_item(editor, ctx, CommandId::FileSaveAs),
                cmd_item(editor, ctx, CommandId::FileRevert),
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Import".to_owned(),
                    enabled: true,
                    role: None,
                    items: import_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Export".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: export_items,
                },
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::FilePublishSettings),
                publish_item,
                NativeMenuItemSpec::Separator,
                unavailable_action_item("Print…", Some(Shortcut::cmd(Key::P)), false, false),
            ],
        }
    }

    fn build_edit_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_authoring_doc = has_doc && !editor.is_standalone_script_session();
        let has_instance = has_authoring_doc && editor.selected_instance_symbol().is_some();
        let timeline_items = vec![
            cmd_item(editor, ctx, CommandId::EditTimelineCutFrames),
            cmd_item(editor, ctx, CommandId::EditTimelineCopyFrames),
            cmd_item(editor, ctx, CommandId::EditTimelinePasteFrames),
            cmd_item(editor, ctx, CommandId::EditTimelineClearFrames),
            cmd_item(editor, ctx, CommandId::EditTimelineRemoveFrames),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::EditTimelineCopyMotion),
            cmd_item(editor, ctx, CommandId::EditTimelinePasteMotion),
            custom_item(
                "Paste motion special…",
                None,
                has_authoring_doc && editor.can_paste_motion(),
                false,
                NativeMenuAction::PasteMotionSpecial,
            ),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::EditTimelineSelectAllFrames),
            cmd_item(editor, ctx, CommandId::EditTimelineReverseFrames),
            cmd_item(editor, ctx, CommandId::EditTimelineSyncSymbols),
        ];

        let mut items = vec![
            cmd_item(editor, ctx, CommandId::EditUndo),
            cmd_item(editor, ctx, CommandId::EditRedo),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::EditCut),
            cmd_item(editor, ctx, CommandId::EditCopy),
            cmd_item(editor, ctx, CommandId::EditPaste),
            cmd_item(editor, ctx, CommandId::EditPasteInPlace),
            cmd_item(editor, ctx, CommandId::EditDuplicate),
            cmd_item(editor, ctx, CommandId::EditClear),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::EditSelectAll),
            cmd_item(editor, ctx, CommandId::EditDeselect),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::EditFindAndReplace),
            NativeMenuItemSpec::Submenu {
                label: "Timeline".to_owned(),
                enabled: has_authoring_doc,
                role: None,
                items: timeline_items,
            },
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::EditSymbols),
            custom_item(
                "Edit selected",
                None,
                has_instance,
                false,
                NativeMenuAction::EditSelectedSymbol,
            ),
            custom_item(
                "Edit in place",
                None,
                has_instance,
                false,
                NativeMenuAction::EditSymbolInPlace,
            ),
        ];
        if editor.has_ready_ai_connection() {
            items.push(NativeMenuItemSpec::Separator);
            items.push(cmd_item(editor, ctx, CommandId::EditSendToAi));
        }
        items.push(NativeMenuItemSpec::Separator);
        items.push(cmd_item(editor, ctx, CommandId::EditPreferences));
        items.push(cmd_item(editor, ctx, CommandId::EditKeyboardShortcuts));

        NativeTopMenuSpec {
            title: "Edit".to_owned(),
            enabled: true,
            items,
        }
    }

    fn build_view_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_authoring_doc = has_doc && !editor.is_standalone_script_session();
        let v = &editor.stage_view.prefs;
        let zoom_pct = (editor.zoom * 100.0).round() as u32;
        let ab_mode = editor.stage_ab_mode();

        let magnification_items = vec![
            custom_check_item(
                "25 %",
                None,
                has_authoring_doc,
                has_authoring_doc && zoom_pct == 25,
                NativeMenuAction::SetMagnification(25),
            ),
            custom_check_item(
                "50 %",
                None,
                has_authoring_doc,
                has_authoring_doc && zoom_pct == 50,
                NativeMenuAction::SetMagnification(50),
            ),
            cmd_item(editor, ctx, CommandId::ViewZoom100),
            custom_check_item(
                "200 %",
                None,
                has_authoring_doc,
                has_authoring_doc && zoom_pct == 200,
                NativeMenuAction::SetMagnification(200),
            ),
            custom_check_item(
                "400 %",
                None,
                has_authoring_doc,
                has_authoring_doc && zoom_pct == 400,
                NativeMenuAction::SetMagnification(400),
            ),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ViewFitInWindow),
        ];

        let grid_items = vec![
            cmd_item(editor, ctx, CommandId::ViewGridShow),
            cmd_item(editor, ctx, CommandId::ViewGridSnap),
            custom_check_item(
                "Show over objects",
                None,
                has_authoring_doc,
                has_authoring_doc && v.grid_over_objects,
                NativeMenuAction::ToggleGridOverObjects,
            ),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ViewGridEdit),
        ];

        let guides_items = vec![
            cmd_item(editor, ctx, CommandId::ViewGuidesShow),
            cmd_item(editor, ctx, CommandId::ViewGuidesLock),
            cmd_item(editor, ctx, CommandId::ViewGuidesSnap),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Edit guides…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::OpenViewSettings,
            ),
        ];

        let snapping_items = vec![
            custom_check_item(
                "Snap to objects",
                None,
                has_authoring_doc,
                has_authoring_doc && v.objects_snap,
                NativeMenuAction::ToggleObjectSnap,
            ),
            custom_check_item(
                "Snap to pixels",
                None,
                has_authoring_doc,
                has_authoring_doc && v.pixels_snap,
                NativeMenuAction::TogglePixelSnap,
            ),
            cmd_item_labeled(editor, ctx, "Snap to grid", CommandId::ViewGridSnap),
            cmd_item_labeled(editor, ctx, "Snap to guides", CommandId::ViewGuidesSnap),
        ];

        let onion_items = vec![
            custom_check_item(
                "Onion skin",
                None,
                has_authoring_doc,
                has_authoring_doc && editor.onion,
                NativeMenuAction::ToggleOnionSkin,
            ),
            custom_check_item(
                "Onion skin outlines",
                None,
                has_authoring_doc,
                has_authoring_doc && editor.onion && editor.onion_outlines,
                NativeMenuAction::ToggleOnionOutlines,
            ),
            custom_check_item(
                "Anchor onion markers",
                None,
                has_authoring_doc,
                has_authoring_doc && editor.onion_anchor,
                NativeMenuAction::ToggleOnionAnchor,
            ),
        ];

        let stage_toolbar_items: Vec<NativeMenuItemSpec> = StageToolbarMode::ALL
            .into_iter()
            .map(|mode| {
                custom_check_item(
                    mode.label(),
                    None,
                    has_authoring_doc,
                    has_authoring_doc && editor.workspace.stage_toolbar == mode,
                    NativeMenuAction::SetStageToolbar(mode),
                )
            })
            .collect();

        let stage_ab_items = vec![
            cmd_item(editor, ctx, CommandId::CaptureStageA),
            cmd_item(editor, ctx, CommandId::CaptureStageB),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ToggleStageAbSplit),
            custom_item(
                "Disable A/B View",
                None,
                has_authoring_doc && ab_mode != StageAbComparisonMode::Off,
                false,
                NativeMenuAction::DisableStageAb,
            ),
        ];

        NativeTopMenuSpec {
            title: "View".to_owned(),
            enabled: true,
            items: vec![
                cmd_item(editor, ctx, CommandId::ViewZoomIn),
                cmd_item(editor, ctx, CommandId::ViewZoomOut),
                cmd_item(editor, ctx, CommandId::ViewZoom100),
                cmd_item(editor, ctx, CommandId::ViewFitInWindow),
                NativeMenuItemSpec::Submenu {
                    label: "Magnification".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: magnification_items,
                },
                NativeMenuItemSpec::Separator,
                unavailable_submenu_item("Preview mode"),
                unavailable_action_item(
                    "Pasteboard",
                    Some(Shortcut::shift_cmd(Key::W)),
                    true,
                    true,
                ),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ViewRulers),
                NativeMenuItemSpec::Submenu {
                    label: "Grid".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: grid_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Guides".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: guides_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Snapping".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: snapping_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Onion Skin".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: onion_items,
                },
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Rulers, guides & grid…",
                    None,
                    has_authoring_doc,
                    false,
                    NativeMenuAction::OpenViewSettings,
                ),
                cmd_item(editor, ctx, CommandId::ViewShowShapeHints),
                NativeMenuItemSpec::Submenu {
                    label: "Stage toolbar".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: stage_toolbar_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Stage A/B".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: stage_ab_items,
                },
            ],
        }
    }

    fn build_insert_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        NativeTopMenuSpec {
            title: "Insert".to_owned(),
            enabled: true,
            items: vec![
                cmd_item(editor, ctx, CommandId::InsertNewSymbol),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::InsertLayer),
                cmd_item(editor, ctx, CommandId::InsertLayerFolder),
                cmd_item(editor, ctx, CommandId::InsertMotionGuide),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::InsertFrame),
                cmd_item(editor, ctx, CommandId::InsertKeyframe),
                cmd_item(editor, ctx, CommandId::InsertBlankKeyframe),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::InsertClassicTween),
                cmd_item(editor, ctx, CommandId::InsertShapeTween),
                cmd_item(editor, ctx, CommandId::InsertMotionTween),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::AddTimelineMarker),
                cmd_item(editor, ctx, CommandId::InsertScene),
            ],
        }
    }

    fn build_modify_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_authoring_doc = has_doc && !editor.is_standalone_script_session();
        let can_combine = has_authoring_doc && editor.can_combine_objects();
        let can_stepped = has_authoring_doc && editor.can_convert_to_keyframes_stepped();
        let can_align = has_authoring_doc && editor.can_align_selection();

        let bitmap_items = vec![cmd_item(editor, ctx, CommandId::ModifyTraceBitmap)];
        let shape_items = vec![
            cmd_item(editor, ctx, CommandId::ModifyAddShapeHint),
            cmd_item(editor, ctx, CommandId::ModifyRemoveAllShapeHints),
            cmd_item(editor, ctx, CommandId::ExpandShapeAppearance),
        ];
        let combine_items = vec![
            cmd_item(editor, ctx, CommandId::ModifyDeleteEnvelope),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Union",
                None,
                can_combine,
                false,
                NativeMenuAction::CombineObjects {
                    op: BooleanOp::Union,
                    crop: false,
                },
            ),
            custom_item(
                "Intersect",
                None,
                can_combine,
                false,
                NativeMenuAction::CombineObjects {
                    op: BooleanOp::Intersection,
                    crop: false,
                },
            ),
            custom_item(
                "Punch",
                None,
                can_combine,
                false,
                NativeMenuAction::CombineObjects {
                    op: BooleanOp::Difference,
                    crop: false,
                },
            ),
            custom_item(
                "Crop",
                None,
                can_combine,
                false,
                NativeMenuAction::CombineObjects {
                    op: BooleanOp::Intersection,
                    crop: true,
                },
            ),
        ];

        let timeline_items = vec![
            cmd_item(editor, ctx, CommandId::ModifyDistributeToLayers),
            custom_item(
                "Distribute to keyframes",
                None,
                has_authoring_doc && editor.can_distribute_to_keyframes(),
                false,
                NativeMenuAction::DistributeToKeyframes,
            ),
            cmd_item(editor, ctx, CommandId::ModifyTimelineLayerProperties),
            cmd_item(editor, ctx, CommandId::EditTimelineReverseFrames),
            cmd_item(editor, ctx, CommandId::EditTimelineSyncSymbols),
            cmd_item(editor, ctx, CommandId::ModifyTimelineSplitClip),
            cmd_item(editor, ctx, CommandId::ModifyTimelineLinkTracks),
            cmd_item(editor, ctx, CommandId::ModifyTimelineUnlinkTracks),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ModifyTimelineConvertToKeyframes),
            custom_item(
                "Tween2Keys (On 2s)",
                None,
                can_stepped,
                false,
                NativeMenuAction::TweenToKeys(2),
            ),
            custom_item(
                "Tween2Keys (On 3s)",
                None,
                can_stepped,
                false,
                NativeMenuAction::TweenToKeys(3),
            ),
            custom_item(
                "Convert to keyframes (Stepped 2s)",
                None,
                can_stepped,
                false,
                NativeMenuAction::ConvertToKeyframesStepped2s,
            ),
            cmd_item(editor, ctx, CommandId::ModifyTimelineClearKeyframe),
            cmd_item(
                editor,
                ctx,
                CommandId::ModifyTimelineConvertToBlankKeyframes,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Copy ease",
                None,
                has_authoring_doc && editor.can_copy_active_keyframe_ease(),
                false,
                NativeMenuAction::CopyKeyframeEase,
            ),
            custom_item(
                "Apply copied ease",
                None,
                has_authoring_doc && editor.has_copied_keyframe_ease(),
                false,
                NativeMenuAction::ApplyCopiedEase,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Delete empty layers",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::DeleteEmptyLayers,
            ),
            custom_item(
                "Rename layers from content",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::RenameLayersFromContent,
            ),
            custom_item(
                "Add actions & stop() layer",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::AddActionsStopLayer,
            ),
        ];

        let jsfl_items = vec![
            custom_item(
                "Run JSFL script (.jsfl)…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::RunJsflDialog,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Batch rename library items…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::OpenBatchRenameLibrary,
            ),
            custom_item(
                "Batch linkage (Export for ActionScript)…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::OpenBatchLinkage,
            ),
            custom_item(
                "Batch bitmap compression & smoothing…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::OpenBatchBitmapCompression,
            ),
            custom_item(
                "Delete unused library items",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::DeleteUnusedLibraryItems,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Auto Instance Namer…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::OpenAutoInstanceNamer,
            ),
            custom_item(
                "Randomize Selection…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::OpenRandomizeSelection,
            ),
            custom_item(
                "Pixel snap (round to whole pixels)",
                None,
                has_authoring_doc && !editor.editable_targets().is_empty(),
                false,
                NativeMenuAction::PixelSnapSelection,
            ),
        ];

        let transform_panel_sc = editor
            .command_menu_state(ctx, CommandId::WindowTransform)
            .shortcut;
        let transform_items = vec![
            cmd_item(editor, ctx, CommandId::ModifyFreeTransform),
            cmd_item(editor, ctx, CommandId::ModifyDistort),
            cmd_item(editor, ctx, CommandId::ModifyEnvelope),
            cmd_item(editor, ctx, CommandId::ModifyScale),
            cmd_item(editor, ctx, CommandId::ModifyRotateAndSkew),
            cmd_item(editor, ctx, CommandId::ModifyScaleAndRotate),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ModifyRotate90Cw),
            cmd_item(editor, ctx, CommandId::ModifyRotate90Ccw),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ModifyFlipVertical),
            cmd_item(editor, ctx, CommandId::ModifyFlipHorizontal),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ModifyRemoveTransform),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Transform panel",
                transform_panel_sc,
                has_authoring_doc,
                false,
                NativeMenuAction::ShowPanel(PanelId::Transform),
            ),
        ];

        let arrange_items = vec![
            cmd_item(editor, ctx, CommandId::ModifyArrangeFront),
            cmd_item(editor, ctx, CommandId::ModifyArrangeForward),
            cmd_item(editor, ctx, CommandId::ModifyArrangeBackward),
            cmd_item(editor, ctx, CommandId::ModifyArrangeBack),
        ];

        let align_panel_sc = editor
            .command_menu_state(ctx, CommandId::WindowAlign)
            .shortcut;
        let align_items = vec![
            custom_item(
                "Left",
                None,
                can_align,
                false,
                NativeMenuAction::AlignSelection { axis: 0, edge: 0 },
            ),
            custom_item(
                "Horizontal centre",
                None,
                can_align,
                false,
                NativeMenuAction::AlignSelection { axis: 0, edge: 1 },
            ),
            custom_item(
                "Right",
                None,
                can_align,
                false,
                NativeMenuAction::AlignSelection { axis: 0, edge: 2 },
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Top",
                None,
                can_align,
                false,
                NativeMenuAction::AlignSelection { axis: 1, edge: 0 },
            ),
            custom_item(
                "Vertical middle",
                None,
                can_align,
                false,
                NativeMenuAction::AlignSelection { axis: 1, edge: 1 },
            ),
            custom_item(
                "Bottom",
                None,
                can_align,
                false,
                NativeMenuAction::AlignSelection { axis: 1, edge: 2 },
            ),
            NativeMenuItemSpec::Separator,
            custom_check_item(
                "To stage",
                None,
                has_authoring_doc,
                has_authoring_doc && editor.authoring_modify.to_stage,
                NativeMenuAction::ToggleAlignToStage,
            ),
            custom_item(
                "Align panel",
                align_panel_sc,
                has_authoring_doc,
                false,
                NativeMenuAction::ShowPanel(PanelId::Align),
            ),
        ];

        NativeTopMenuSpec {
            title: "Modify".to_owned(),
            enabled: true,
            items: vec![
                cmd_item_labeled(editor, ctx, "Document…", CommandId::ModifyDocument),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ModifyConvertToSymbol),
                unavailable_action_item("Convert to Bitmap", None, false, false),
                cmd_item(editor, ctx, CommandId::ModifyBreakApart),
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Bitmap".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: bitmap_items,
                },
                unavailable_submenu_item("Symbol"),
                NativeMenuItemSpec::Submenu {
                    label: "Shape".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: shape_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Combine objects".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: combine_items,
                },
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Timeline".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: timeline_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Commands & JSFL".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: jsfl_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Transform".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: transform_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Arrange".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: arrange_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Align".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: align_items,
                },
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ModifyGroup),
                cmd_item(editor, ctx, CommandId::ModifyUngroup),
            ],
        }
    }

    fn build_text_menu(editor: &Editor) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_authoring_doc = has_doc && !editor.is_standalone_script_session();
        let selected_text = if has_authoring_doc && editor.selection.len() == 1 {
            editor.selected_element().map(|el| &el.kind)
        } else {
            None
        };
        let (has_text_el, has_text_box, cur_size, bold, italic, align) = match selected_text {
            Some(ElementKind::Text { font_size, .. }) => {
                (true, false, *font_size, false, false, TextAlign::Left)
            }
            Some(ElementKind::TextBox { format, .. }) => (
                true,
                true,
                format.size,
                format.bold,
                format.italic,
                format.align,
            ),
            _ => (
                false,
                false,
                editor.workspace.text_size,
                false,
                false,
                TextAlign::Left,
            ),
        };
        let can_set_size =
            has_authoring_doc && (has_text_el || editor.tool == crate::app::Tool::Text);

        let size_items: Vec<NativeMenuItemSpec> = TEXT_MENU_SIZES
            .iter()
            .copied()
            .map(|pt| {
                custom_check_item(
                    format!("{pt} pt"),
                    None,
                    can_set_size,
                    can_set_size && (cur_size - f64::from(pt)).abs() < 0.05,
                    NativeMenuAction::TextSetSize(pt),
                )
            })
            .collect();

        let style_items = vec![
            custom_check_item(
                "Regular",
                None,
                has_text_box,
                has_text_box && !bold && !italic,
                NativeMenuAction::TextSetStyle {
                    bold: false,
                    italic: false,
                },
            ),
            custom_check_item(
                "Bold",
                None,
                has_text_box,
                has_text_box && bold && !italic,
                NativeMenuAction::TextSetStyle {
                    bold: true,
                    italic: false,
                },
            ),
            custom_check_item(
                "Italic",
                None,
                has_text_box,
                has_text_box && !bold && italic,
                NativeMenuAction::TextSetStyle {
                    bold: false,
                    italic: true,
                },
            ),
            custom_check_item(
                "Bold Italic",
                None,
                has_text_box,
                has_text_box && bold && italic,
                NativeMenuAction::TextSetStyle {
                    bold: true,
                    italic: true,
                },
            ),
        ];

        let align_items = vec![
            custom_check_item(
                "Align Left",
                None,
                has_text_box,
                has_text_box && align == TextAlign::Left,
                NativeMenuAction::TextSetAlign(TextAlign::Left),
            ),
            custom_check_item(
                "Align Center",
                None,
                has_text_box,
                has_text_box && align == TextAlign::Center,
                NativeMenuAction::TextSetAlign(TextAlign::Center),
            ),
            custom_check_item(
                "Align Right",
                None,
                has_text_box,
                has_text_box && align == TextAlign::Right,
                NativeMenuAction::TextSetAlign(TextAlign::Right),
            ),
        ];

        let tracking_items = vec![
            custom_item(
                "Increase",
                None,
                has_text_box,
                false,
                NativeMenuAction::TextAdjustTracking(1),
            ),
            custom_item(
                "Decrease",
                None,
                has_text_box,
                false,
                NativeMenuAction::TextAdjustTracking(-1),
            ),
            custom_item(
                "Reset",
                None,
                has_text_box,
                false,
                NativeMenuAction::TextAdjustTracking(0),
            ),
        ];

        NativeTopMenuSpec {
            title: "Text".to_owned(),
            enabled: true,
            items: vec![
                NativeMenuItemSpec::Submenu {
                    label: "Size".to_owned(),
                    enabled: can_set_size,
                    role: None,
                    items: size_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Style".to_owned(),
                    enabled: has_text_box,
                    role: None,
                    items: style_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Align".to_owned(),
                    enabled: has_text_box,
                    role: None,
                    items: align_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Letter Spacing".to_owned(),
                    enabled: has_text_box,
                    role: None,
                    items: tracking_items,
                },
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Font Embedding…",
                    None,
                    has_authoring_doc,
                    false,
                    NativeMenuAction::TextOpenFontEmbedding,
                ),
            ],
        }
    }

    fn build_control_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_authoring_doc = has_doc && !editor.is_standalone_script_session();
        let work_area_items = vec![
            cmd_item(editor, ctx, CommandId::SetWorkAreaIn),
            cmd_item(editor, ctx, CommandId::SetWorkAreaOut),
            cmd_item(editor, ctx, CommandId::ClearWorkArea),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::NextTimelineMarker),
            cmd_item(editor, ctx, CommandId::PreviousTimelineMarker),
        ];

        NativeTopMenuSpec {
            title: "Control".to_owned(),
            enabled: true,
            items: vec![
                cmd_item(editor, ctx, CommandId::ControlPlay),
                cmd_item(editor, ctx, CommandId::ControlRewind),
                cmd_item(editor, ctx, CommandId::ControlGoToEnd),
                cmd_item(editor, ctx, CommandId::ControlStepForward),
                cmd_item(editor, ctx, CommandId::ControlStepBack),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ControlTestMovie),
                cmd_item(editor, ctx, CommandId::ControlTestScene),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ControlLoopPlayback),
                cmd_item(editor, ctx, CommandId::ControlPlayAllScenes),
                cmd_item(editor, ctx, CommandId::ControlMuteSounds),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ControlEnableSimpleButtons),
                custom_check_item(
                    "Live ActionScript",
                    None,
                    has_authoring_doc,
                    has_authoring_doc && editor.live_script_preview,
                    NativeMenuAction::ToggleLiveActionScript,
                ),
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Work Area".to_owned(),
                    enabled: has_authoring_doc,
                    role: None,
                    items: work_area_items,
                },
            ],
        }
    }

    fn build_debug_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_authoring_doc = has_doc && !editor.is_standalone_script_session();
        let snapshot = editor.debugger.snapshot();
        let idle_ready = snapshot.is_none() && editor.debugger.startup_status.is_none();
        let paused = snapshot
            .as_ref()
            .is_some_and(|s| s.state == DebugState::Paused && !s.command_pending);
        let run_label = if snapshot.is_some() {
            "Continue"
        } else {
            "Debug Movie"
        };

        NativeTopMenuSpec {
            title: "Debug".to_owned(),
            enabled: true,
            items: vec![
                custom_item(
                    run_label,
                    None,
                    has_authoring_doc && (paused || idle_ready),
                    false,
                    NativeMenuAction::DebugContinueOrStart,
                ),
                custom_item(
                    "Stop Debugging",
                    None,
                    has_authoring_doc && !idle_ready,
                    false,
                    NativeMenuAction::DebugStop,
                ),
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Step Over",
                    None,
                    has_authoring_doc && paused,
                    false,
                    NativeMenuAction::DebugStepOver,
                ),
                custom_item(
                    "Step In",
                    None,
                    has_authoring_doc && paused,
                    false,
                    NativeMenuAction::DebugStepInto,
                ),
                custom_item(
                    "Step Out",
                    None,
                    has_authoring_doc && paused,
                    false,
                    NativeMenuAction::DebugStepOut,
                ),
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Toggle Breakpoint",
                    None,
                    has_authoring_doc && (snapshot.is_some() || idle_ready),
                    false,
                    NativeMenuAction::DebugToggleBreakpoint,
                ),
                custom_item(
                    "Remove All Breakpoints",
                    None,
                    has_authoring_doc
                        && (snapshot.is_some()
                            || (idle_ready && !editor.debugger.breakpoints.is_empty())),
                    false,
                    NativeMenuAction::DebugClearBreakpoints,
                ),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::WindowDebugger),
                cmd_item(editor, ctx, CommandId::WindowCompilerErrors),
                cmd_item(editor, ctx, CommandId::WindowOutput),
            ],
        }
    }

    fn build_window_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let mut panels_items = Vec::with_capacity(NATIVE_WINDOW_PANELS.len() + 4);
        panels_items.push(cmd_item_labeled(
            editor,
            ctx,
            "Brushes",
            CommandId::WindowBrushes,
        ));
        panels_items.push(NativeMenuItemSpec::Separator);
        for &(name, panel) in &NATIVE_WINDOW_PANELS[..16] {
            panels_items.push(cmd_item_labeled(
                editor,
                ctx,
                name,
                CommandId::for_panel(panel),
            ));
        }
        panels_items.push(NativeMenuItemSpec::Separator);
        for &(name, panel) in &NATIVE_WINDOW_PANELS[16..] {
            panels_items.push(cmd_item_labeled(
                editor,
                ctx,
                name,
                CommandId::for_panel(panel),
            ));
        }

        let mut workspace_items = Vec::new();
        workspace_items.push(NativeMenuItemSpec::Section("Built-in".to_owned()));
        for template in WorkspaceTemplate::ALL {
            let checked = editor.workspace.active_named_preset.is_none()
                && !editor.workspace.unsaved_layout
                && editor.workspace.template == template;
            workspace_items.push(custom_check_item(
                template.label(),
                None,
                true,
                checked,
                NativeMenuAction::ApplyWorkspaceTemplate(template),
            ));
        }
        if !editor.workspace.named_presets.is_empty() {
            workspace_items.push(NativeMenuItemSpec::Separator);
            workspace_items.push(NativeMenuItemSpec::Section("My workspaces".to_owned()));
            for name in editor.workspace.named_presets.keys() {
                let checked =
                    editor.workspace.active_named_preset.as_deref() == Some(name.as_str());
                workspace_items.push(custom_check_item(
                    name.clone(),
                    None,
                    true,
                    checked,
                    NativeMenuAction::ApplyNamedWorkspace(name.clone()),
                ));
            }
        }
        workspace_items.push(NativeMenuItemSpec::Separator);
        workspace_items.push(custom_item(
            "Save workspace…",
            None,
            true,
            false,
            NativeMenuAction::SaveWorkspaceDialog,
        ));
        let active_named = editor.workspace.active_named_preset.is_some();
        let active_ws_label = editor.workspace.active_workspace_label().to_owned();
        workspace_items.push(custom_item(
            format!("Update “{active_ws_label}”"),
            None,
            active_named,
            false,
            NativeMenuAction::UpdateActiveWorkspace,
        ));
        workspace_items.push(custom_item(
            format!("Rename “{active_ws_label}”…"),
            None,
            active_named,
            false,
            NativeMenuAction::RenameActiveWorkspaceDialog,
        ));
        workspace_items.push(custom_item(
            format!("Delete “{active_ws_label}”…"),
            None,
            active_named,
            false,
            NativeMenuAction::DeleteActiveWorkspaceDialog,
        ));
        workspace_items.push(NativeMenuItemSpec::Separator);
        workspace_items.push(custom_item(
            format!("Reset “{active_ws_label}”"),
            None,
            !editor.workspace.unsaved_layout,
            false,
            NativeMenuAction::ResetActiveWorkspace,
        ));

        let appearance_items: Vec<NativeMenuItemSpec> = ThemeMode::ALL
            .into_iter()
            .map(|theme| {
                custom_check_item(
                    theme.label(),
                    None,
                    true,
                    editor.workspace.theme == theme,
                    NativeMenuAction::SetTheme(theme),
                )
            })
            .collect();

        let density_items: Vec<NativeMenuItemSpec> = DensityPref::ALL
            .into_iter()
            .map(|density| {
                custom_check_item(
                    density.label(),
                    None,
                    true,
                    editor.workspace.density == density,
                    NativeMenuAction::SetDensity(density),
                )
            })
            .collect();

        let has_authoring_doc = editor.has_document() && !editor.is_standalone_script_session();
        let detach_items: Vec<NativeMenuItemSpec> = native_detachable_panels()
            .filter(|(_, panel)| editor.workspace.placement(*panel) == PanelPlacement::Docked)
            .map(|(name, panel)| {
                custom_item(
                    name,
                    None,
                    has_authoring_doc,
                    false,
                    NativeMenuAction::DetachPanel(panel),
                )
            })
            .collect();
        let dock_items: Vec<NativeMenuItemSpec> = native_detachable_panels()
            .filter(|(_, panel)| editor.workspace.placement(*panel) == PanelPlacement::Detached)
            .map(|(name, panel)| {
                custom_item(
                    name,
                    None,
                    has_authoring_doc,
                    false,
                    NativeMenuAction::DockPanel(panel),
                )
            })
            .collect();

        let session_count = editor.session_count();
        let next_doc_sc = Shortcut::new(
            egui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
            Key::Tab,
        );
        let prev_doc_sc = Shortcut::new(
            egui::Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
            Key::Tab,
        );

        let mut items = vec![
            NativeMenuItemSpec::System {
                label: "Minimize".to_owned(),
                role: NativeSystemItemRole::MinimizeWindow,
                shortcut: Some(Shortcut::cmd(Key::M)),
                shortcut_label: "⌘M".to_owned(),
                enabled: true,
            },
            NativeMenuItemSpec::System {
                label: "Zoom".to_owned(),
                role: NativeSystemItemRole::ZoomWindow,
                shortcut: None,
                shortcut_label: String::new(),
                enabled: true,
            },
            unavailable_action_item(
                "Duplicate window",
                Some(Shortcut::alt_cmd(Key::K)),
                false,
                false,
            ),
            NativeMenuItemSpec::Separator,
            NativeMenuItemSpec::Submenu {
                label: "Panels".to_owned(),
                enabled: true,
                role: None,
                items: panels_items,
            },
            cmd_item(editor, ctx, CommandId::WindowHidePanels),
            cmd_item(editor, ctx, CommandId::WindowInspectorPriority),
            cmd_item(editor, ctx, CommandId::WindowLibraryPriority),
            NativeMenuItemSpec::Separator,
            NativeMenuItemSpec::Submenu {
                label: "Workspaces".to_owned(),
                enabled: true,
                role: None,
                items: workspace_items,
            },
            NativeMenuItemSpec::Submenu {
                label: "Appearance".to_owned(),
                enabled: true,
                role: None,
                items: appearance_items,
            },
            NativeMenuItemSpec::Submenu {
                label: "Density".to_owned(),
                enabled: true,
                role: None,
                items: density_items,
            },
            NativeMenuItemSpec::Submenu {
                label: "Detach panel".to_owned(),
                enabled: has_authoring_doc && !detach_items.is_empty(),
                role: None,
                items: detach_items,
            },
            NativeMenuItemSpec::Submenu {
                label: "Dock panel".to_owned(),
                enabled: has_authoring_doc && !dock_items.is_empty(),
                role: None,
                items: dock_items,
            },
            custom_item(
                "Customize tools…",
                None,
                has_authoring_doc,
                false,
                NativeMenuAction::OpenCustomizeTools,
            ),
            custom_check_item(
                "UI Layout Studio…",
                None,
                true,
                ctx.is_some_and(crate::panels::ui_studio::is_studio_open),
                NativeMenuAction::ToggleUiLayoutStudio,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Next document",
                Some(next_doc_sc),
                session_count > 1,
                false,
                NativeMenuAction::NextDocumentTab,
            ),
            custom_item(
                "Previous document",
                Some(prev_doc_sc),
                session_count > 1,
                false,
                NativeMenuAction::PreviousDocumentTab,
            ),
            custom_item(
                "Close other documents",
                None,
                session_count > 1,
                false,
                NativeMenuAction::CloseOtherDocuments,
            ),
        ];

        if session_count > 0 {
            items.push(NativeMenuItemSpec::Separator);
            let raw_tabs: Vec<(String, Option<PathBuf>, bool)> = (0..session_count)
                .map(|index| {
                    let (project, path, disk_path, dirty) = if index == editor.active_session {
                        (
                            &editor.project,
                            &editor.path,
                            editor.tracked_disk_path().map(Path::to_path_buf),
                            editor.dirty(),
                        )
                    } else {
                        let session = editor.sessions[index].as_ref().expect("inactive session");
                        (
                            &session.project,
                            &session.path,
                            session.tracked_disk_path().map(Path::to_path_buf),
                            session.dirty(),
                        )
                    };
                    let name = path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| project.name.clone());
                    (name, disk_path, dirty)
                })
                .collect();

            for (index, (name, disk_path, dirty)) in raw_tabs.iter().enumerate() {
                let duplicate = raw_tabs
                    .iter()
                    .filter(|(other_name, _, _)| other_name == name)
                    .count()
                    > 1;
                let base_label = if duplicate
                    && let Some(folder) = disk_path
                        .as_deref()
                        .and_then(Path::parent)
                        .and_then(Path::file_name)
                        .map(|f| f.to_string_lossy())
                    && !folder.is_empty()
                {
                    format!("{name} ({folder})")
                } else {
                    name.clone()
                };
                let label = format!("{}{}", base_label, if *dirty { " — unsaved" } else { "" });
                let tooltip = disk_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| name.clone());
                items.push(NativeMenuItemSpec::Action {
                    label,
                    command_id: None,
                    shortcut: None,
                    shortcut_label: String::new(),
                    enabled: true,
                    checkable: true,
                    checked: index == editor.active_session,
                    tooltip: Some(tooltip),
                    action: NativeMenuAction::ActivateDocumentTab(index),
                });
            }
        }

        NativeTopMenuSpec {
            title: "Window".to_owned(),
            enabled: true,
            items,
        }
    }

    fn build_help_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let mut items = vec![
            cmd_item(editor, ctx, CommandId::HelpSearchFunctions),
            cmd_item(editor, ctx, CommandId::HelpManual),
            cmd_item(editor, ctx, CommandId::HelpScriptingReference),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::HelpOpenExample),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::HelpCheckForUpdates),
        ];
        if crate::panels::beta_feedback::is_beta_build() {
            items.push(cmd_item(editor, ctx, CommandId::HelpSendBetaFeedback));
        }
        items.push(NativeMenuItemSpec::Separator);
        items.push(cmd_item(editor, ctx, CommandId::HelpAbout));
        NativeTopMenuSpec {
            title: "Help".to_owned(),
            enabled: true,
            items,
        }
    }
}

pub(crate) const TOP_MENU_COUNT: usize = NativeMenuSpec::TOP_LEVEL_TITLES.len();

/// Encode `(menu_idx, local_idx)` into a non-negative AppKit `NSMenuItem.tag` so rebuilding one
/// top-level menu in `menuNeedsUpdate:` never invalidates tags of any other top-level menu (`T-390`).
pub(crate) const fn encode_menu_tag(menu_idx: usize, local_idx: usize) -> isize {
    (((menu_idx & 0xFF) << 16) | (local_idx & 0xFFFF)) as isize
}

/// Decode a tag produced by [`encode_menu_tag`] back into `(menu_idx, local_idx)`.
pub(crate) const fn decode_menu_tag(tag: isize) -> Option<(usize, usize)> {
    if tag < 0 {
        return None;
    }
    let raw = tag as usize;
    let menu_idx = (raw >> 16) & 0xFF;
    let local_idx = raw & 0xFFFF;
    if menu_idx < TOP_MENU_COUNT {
        Some((menu_idx, local_idx))
    } else {
        None
    }
}

/// O(1) editor + context fingerprint governing [`NativeMenuSpec`] presentation (`T-390`).
/// When unchanged across idle, hover, pan, or scrub frames, `macos_menu::sync` skips
/// [`NativeMenuSpec::build`] and Objective-C synchronization completely (`< 1 µs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuSyncFingerprint {
    pub has_doc: bool,
    pub standalone_script: bool,
    pub session_count: usize,
    pub active_session: usize,
    pub history_revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub dirty: bool,
    pub playing: bool,
    pub live_script_preview: bool,
    pub enable_simple_buttons: bool,
    pub audio_muted: bool,
    pub panels_hidden: bool,
    pub active_panels_mask: u32,
    pub visible_panels_mask: u32,
    pub detached_panels_mask: u32,
    pub priority_mask: u8,
    pub zoom_pct: u32,
    pub zoom_is_100: bool,
    pub layer: usize,
    pub frame: u32,
    pub selection_len: usize,
    pub modal_open: bool,
    pub studio_open: bool,
    pub text_focus: bool,
    pub digest_hi: u64,
    pub digest_lo: u64,
}

impl MenuSyncFingerprint {
    pub fn capture(editor: &Editor, ctx: Option<&egui::Context>) -> Self {
        use std::hash::{Hash, Hasher};

        let has_doc = editor.has_document();
        let session_count = editor.session_count();
        let dirty = editor.dirty();
        let mut active_panels_mask = 0u32;
        let mut visible_panels_mask = 0u32;
        let mut detached_panels_mask = 0u32;
        for (bit, panel) in PanelId::DETACHABLE.into_iter().enumerate() {
            if editor.workspace.is_panel_active(panel) {
                active_panels_mask |= 1u32 << bit;
            }
            if editor.workspace.is_panel_visible(panel) {
                visible_panels_mask |= 1u32 << bit;
            }
            if editor.workspace.placement(panel) == PanelPlacement::Detached {
                detached_panels_mask |= 1u32 << bit;
            }
        }
        let priority_mask = u8::from(editor.workspace.dock.has_priority(PanelId::Properties))
            | (u8::from(editor.workspace.dock.has_priority(PanelId::Library)) << 1);
        let zoom_pct = (editor.zoom * 100.0).round() as u32;
        let zoom_is_100 = (editor.zoom - 1.0).abs() < 0.005;
        let modal_open = ctx.map_or_else(
            || editor.has_modal_dialog(),
            |c| editor.has_modal_dialog_in(c),
        );
        let studio_open = ctx.is_some_and(crate::panels::ui_studio::is_studio_open);
        let text_focus = ctx
            .is_some_and(|c| c.wants_keyboard_input() && c.memory(|m| m.focused().is_some()))
            || editor
                .clipboard
                .menu_target
                .as_ref()
                .is_some_and(|t| t.is_text());

        let mut h1 = std::collections::hash_map::DefaultHasher::new();
        let mut h2 = std::collections::hash_map::DefaultHasher::new();
        0x9E37_79B9_7F4A_7C15_u64.hash(&mut h2);

        macro_rules! mix {
            ($val:expr) => {{
                let v = $val;
                v.hash(&mut h1);
                v.hash(&mut h2);
            }};
        }

        mix!(std::mem::discriminant(&editor.tool));
        mix!(std::mem::discriminant(&editor.active_free_transform_mode()));
        mix!(editor.is_motion_sketch_armed());
        mix!(editor.vector.draft.as_ref().map(|d| d.anchors.len()));
        mix!(editor.history.undo_label());
        mix!(editor.history.redo_label());
        mix!(editor.onion);
        mix!(editor.onion_outlines);
        mix!(editor.onion_anchor);
        mix!(editor.timeline_shape_hints.show);
        mix!(editor.authoring_modify.to_stage);
        mix!(std::mem::discriminant(&editor.stage_ab_mode()));

        let v = &editor.stage_view.prefs;
        mix!(v.rulers);
        mix!(v.grid_show);
        mix!(v.grid_snap);
        mix!(v.grid_over_objects);
        mix!(v.guides_show);
        mix!(v.guides_lock);
        mix!(v.guides_snap);
        mix!(v.objects_snap);
        mix!(v.pixels_snap);

        mix!(std::mem::discriminant(&editor.workspace.template));
        mix!(&editor.workspace.active_named_preset);
        mix!(editor.workspace.unsaved_layout);
        mix!(editor.workspace.named_presets.len());
        for name in editor.workspace.named_presets.keys() {
            mix!(name);
        }
        mix!(std::mem::discriminant(&editor.workspace.theme));
        mix!(std::mem::discriminant(&editor.workspace.density));
        mix!(std::mem::discriminant(&editor.workspace.stage_toolbar));
        mix!(editor.workspace.timeline.loop_playback);
        mix!(editor.workspace.drawing.object_drawing);
        mix!(editor.workspace.text_size.to_bits());
        mix!(&editor.workspace.shortcuts.set);
        mix!(editor.workspace.shortcuts.overrides.len());
        for (k, chords) in &editor.workspace.shortcuts.overrides {
            mix!(k);
            mix!(chords);
        }
        mix!(&editor.workspace.general.recent_files);
        mix!(editor.has_ready_ai_connection());
        mix!(ctx.is_some_and(crate::clipboard::has_egui_label_selection));

        mix!(editor.debugger.startup_status.is_some());
        mix!(editor.debugger.breakpoints.len());
        if let Some(snap) = editor.debugger.snapshot() {
            mix!(true);
            mix!(std::mem::discriminant(&snap.state));
            mix!(snap.command_pending);
        } else {
            mix!(false);
        }

        if has_doc {
            mix!(editor.symbol_id());
            mix!(editor.editing_scene());
            mix!(editor.layer_editable());
            mix!(editor.can_revert());
            mix!(
                editor
                    .timeline_edit
                    .range
                    .as_ref()
                    .map(|r| (r.start, r.end, &r.layers))
            );
            mix!(editor.timeline_edit.play_all_scenes);
            mix!(editor.timeline_edit.focused);
            mix!(editor.shared_frame_clipboard.is_some());
            mix!(editor.can_paste_motion());
            mix!(editor.has_copied_keyframe_ease());
            mix!(editor.can_copy_active_keyframe_ease());
            mix!(editor.current_tween().map(|t| std::mem::discriminant(&t)));
            mix!(editor.can_create_tween());
            mix!(editor.can_add_motion_guide_to(editor.layer));
            mix!(editor.can_link_av_layers());
            mix!(editor.can_unlink_av_layers());
            mix!(
                editor
                    .active_timeline_work_area()
                    .map(|w| (w.in_frame, w.out_frame))
            );
            mix!(!editor.active_timeline_markers().is_empty());

            mix!(&editor.selection.symbol);
            let targets = &editor.selection.targets;
            for t in targets.iter().take(8) {
                mix!(&t.layer);
                mix!(&t.element);
            }
            if let Some(last) = targets.last() {
                mix!(&last.layer);
                mix!(&last.element);
            }
            if editor.selection.len() == 1
                && let Some(el) = editor.selected_element()
            {
                mix!(std::mem::discriminant(&el.kind));
                match &el.kind {
                    ElementKind::Instance { symbol_id, .. } => mix!(symbol_id),
                    ElementKind::Text { font_size, .. } => mix!(font_size.to_bits()),
                    ElementKind::TextBox { format, .. } => {
                        mix!(format.size.to_bits());
                        mix!(format.bold);
                        mix!(format.italic);
                        mix!(std::mem::discriminant(&format.align));
                    }
                    _ => {}
                }
            }

            for index in 0..session_count {
                if index == editor.active_session {
                    mix!(&editor.project.name);
                    mix!(&editor.path);
                    mix!(&editor.import_source);
                    mix!(editor.standalone_script);
                    mix!(dirty);
                } else if let Some(session) = &editor.sessions[index] {
                    mix!(&session.project.name);
                    mix!(&session.path);
                    mix!(&session.import_source);
                    mix!(session.standalone_script);
                    mix!(session.dirty());
                }
            }
        }

        Self {
            has_doc,
            standalone_script: editor.is_standalone_script_session(),
            session_count,
            active_session: editor.active_session,
            history_revision: editor.history.revision(),
            can_undo: editor.history.can_undo(),
            can_redo: editor.history.can_redo(),
            dirty,
            playing: editor.playing,
            live_script_preview: editor.live_script_preview,
            enable_simple_buttons: editor.enable_simple_buttons,
            audio_muted: editor.timeline_audio.muted,
            panels_hidden: editor.panels_hidden,
            active_panels_mask,
            visible_panels_mask,
            detached_panels_mask,
            priority_mask,
            zoom_pct,
            zoom_is_100,
            layer: editor.layer,
            frame: editor.frame,
            selection_len: editor.selection.len(),
            modal_open,
            studio_open,
            text_focus,
            digest_hi: h1.finish(),
            digest_lo: h2.finish(),
        }
    }
}

thread_local! {
    static REPRESENTED_FILENAME_CACHE: std::cell::RefCell<Option<(PathBuf, String)>> =
        const { std::cell::RefCell::new(None) };
}

fn cached_canonical_represented_filename(raw_path: &Path) -> String {
    if let Some(hit) = REPRESENTED_FILENAME_CACHE.with(|cell| {
        let borrow = cell.borrow();
        borrow.as_ref().and_then(|(cached_raw, cached_canon)| {
            (cached_raw == raw_path).then(|| cached_canon.clone())
        })
    }) {
        return hit;
    }
    let resolved = crate::recovery::canonical_or_normalized_path(raw_path)
        .to_string_lossy()
        .into_owned();
    REPRESENTED_FILENAME_CACHE.with(|cell| {
        *cell.borrow_mut() = Some((raw_path.to_path_buf(), resolved.clone()));
    });
    resolved
}

impl Editor {
    /// Compute the root window's title, macOS `documentEdited` dirty indicator, and canonical
    /// `representedFilename` (`""` when untitled, unsaved-imported, or when no document is open).
    pub fn native_window_state(&self) -> NativeWindowDocState {
        if !self.has_document() {
            return NativeWindowDocState {
                title: "No document open — NeoFlash".to_owned(),
                document_edited: false,
                represented_filename: String::new(),
            };
        }
        let dirty = self.dirty();
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.project.name.clone());
        let title = format!("{name}{} — NeoFlash", if dirty { " •" } else { "" });
        let represented_filename = self
            .path
            .as_deref()
            .map(cached_canonical_represented_filename)
            .unwrap_or_default();
        NativeWindowDocState {
            title,
            document_edited: dirty,
            represented_filename,
        }
    }

    /// Execute any [`NativeMenuAction`] against the live [`Editor`].
    pub fn execute_native_menu_action(&mut self, ctx: &egui::Context, action: NativeMenuAction) {
        let has_authoring_doc = self.has_document() && !self.is_standalone_script_session();
        match action {
            NativeMenuAction::Command(cmd) => {
                let _ = self.execute_menu_command(ctx, cmd);
            }
            NativeMenuAction::Unavailable => {}
            NativeMenuAction::About => {
                self.show_about = true;
            }
            NativeMenuAction::Quit => {
                self.request_quit(ctx);
            }
            NativeMenuAction::OpenRecent(path) => {
                if !path.exists() {
                    self.workspace.general.invalidate_recent_files_cache();
                }
                self.request(Pending::OpenPath(path), ctx);
            }
            NativeMenuAction::ClearRecent => {
                self.workspace.clear_recent_files();
            }
            NativeMenuAction::CloseAllDocuments => {
                if self.has_document() {
                    self.request_close_documents(ctx, false);
                }
            }
            NativeMenuAction::CloseOtherDocuments => {
                if self.session_count() > 1 {
                    self.request_close_documents(ctx, true);
                }
            }
            NativeMenuAction::ImportToLibrary => {
                if has_authoring_doc {
                    self.import_to_library_dialog();
                }
            }
            NativeMenuAction::ImportPalette => {
                if has_authoring_doc {
                    self.import_palette_dialog();
                }
            }
            NativeMenuAction::ImportVideo => {
                if has_authoring_doc {
                    self.open_import_video(ctx);
                }
            }
            NativeMenuAction::InsertEmptyVideoObject => {
                if has_authoring_doc && self.layer_editable() {
                    self.finish_edits();
                    self.add_element(
                        ElementKind::Video {
                            width: 320.,
                            height: 240.,
                        },
                        80.,
                        80.,
                    );
                }
            }
            NativeMenuAction::ExportImage => {
                if has_authoring_doc {
                    self.open_export_image(ctx);
                }
            }
            NativeMenuAction::ExportFlaProfile(profile) => {
                if has_authoring_doc {
                    self.export_fla_profile_dialog(profile, None);
                }
            }
            NativeMenuAction::PasteMotionSpecial => {
                if has_authoring_doc && self.can_paste_motion() {
                    self.open_paste_motion_special();
                }
            }
            NativeMenuAction::EditSelectedSymbol => {
                if has_authoring_doc && let Some(id) = self.selected_instance_symbol() {
                    self.enter_symbol_isolated(id);
                }
            }
            NativeMenuAction::EditSymbolInPlace => {
                if has_authoring_doc && self.selected_instance_symbol().is_some() {
                    self.enter_selected_symbol_in_place();
                }
            }
            NativeMenuAction::SetMagnification(percent) => {
                if has_authoring_doc {
                    self.finish_edits();
                    self.zoom = (percent as f32 / 100.0).clamp(0.05, 64.0);
                    self.mark_manual_stage_view();
                }
            }
            NativeMenuAction::ToggleGridOverObjects => {
                if has_authoring_doc {
                    self.toggle_grid_over_objects();
                }
            }
            NativeMenuAction::OpenViewSettings => {
                if has_authoring_doc {
                    self.open_view_settings();
                }
            }
            NativeMenuAction::ToggleObjectSnap => {
                if has_authoring_doc {
                    self.toggle_object_snap();
                }
            }
            NativeMenuAction::TogglePixelSnap => {
                if has_authoring_doc {
                    self.toggle_pixel_snap();
                }
            }
            NativeMenuAction::ToggleOnionSkin => {
                if has_authoring_doc {
                    self.onion = !self.onion;
                }
            }
            NativeMenuAction::ToggleOnionOutlines => {
                if has_authoring_doc {
                    if !self.onion {
                        self.onion = true;
                        self.onion_outlines = true;
                    } else {
                        self.onion_outlines = !self.onion_outlines;
                    }
                }
            }
            NativeMenuAction::ToggleOnionAnchor => {
                if has_authoring_doc {
                    self.onion_anchor = !self.onion_anchor;
                }
            }
            NativeMenuAction::SetStageToolbar(mode) => {
                if has_authoring_doc {
                    self.workspace.stage_toolbar = mode;
                }
            }
            NativeMenuAction::DisableStageAb => {
                if has_authoring_doc {
                    self.set_stage_ab_mode(StageAbComparisonMode::Off);
                }
            }
            NativeMenuAction::CombineObjects { op, crop } => {
                if has_authoring_doc && self.can_combine_objects() {
                    self.combine_objects_selection(op, crop);
                }
            }
            NativeMenuAction::DistributeToKeyframes => {
                if has_authoring_doc && self.can_distribute_to_keyframes() {
                    let _ = self.distribute_selection_to_keyframes();
                }
            }
            NativeMenuAction::TweenToKeys(step) => {
                if has_authoring_doc && self.can_convert_to_keyframes_stepped() {
                    match step {
                        2 => {
                            let _ = self.tween_to_keys_on_twos();
                        }
                        3 => {
                            let _ = self.tween_to_keys_on_threes();
                        }
                        other => {
                            self.convert_to_keyframes_stepped(other.max(1), false);
                        }
                    }
                }
            }
            NativeMenuAction::ConvertToKeyframesStepped2s => {
                if has_authoring_doc && self.can_convert_to_keyframes_stepped() {
                    self.convert_to_keyframes_stepped(2, false);
                }
            }
            NativeMenuAction::CopyKeyframeEase => {
                if has_authoring_doc && self.can_copy_active_keyframe_ease() {
                    let _ = self.copy_active_keyframe_ease();
                }
            }
            NativeMenuAction::ApplyCopiedEase => {
                if has_authoring_doc && self.has_copied_keyframe_ease() {
                    let _ = self.apply_copied_ease_to_selection();
                }
            }
            NativeMenuAction::DeleteEmptyLayers => {
                if has_authoring_doc {
                    let _ = self.delete_empty_layers();
                }
            }
            NativeMenuAction::RenameLayersFromContent => {
                if has_authoring_doc {
                    let _ = self.rename_layers_from_content();
                }
            }
            NativeMenuAction::AddActionsStopLayer => {
                if has_authoring_doc {
                    let _ = self.add_actions_and_stop_layer();
                }
            }
            NativeMenuAction::RunJsflDialog => {
                if has_authoring_doc {
                    let _ = self.run_jsfl_script_dialog(None);
                }
            }
            NativeMenuAction::OpenBatchRenameLibrary => {
                if has_authoring_doc {
                    self.open_batch_rename_library_modal(ctx);
                }
            }
            NativeMenuAction::OpenBatchLinkage => {
                if has_authoring_doc {
                    self.open_batch_linkage_modal(ctx);
                }
            }
            NativeMenuAction::OpenBatchBitmapCompression => {
                if has_authoring_doc {
                    self.open_batch_bitmap_compression_modal(ctx);
                }
            }
            NativeMenuAction::DeleteUnusedLibraryItems => {
                if has_authoring_doc {
                    self.find_and_delete_unused_library_items();
                }
            }
            NativeMenuAction::OpenAutoInstanceNamer => {
                if has_authoring_doc {
                    self.open_auto_instance_namer_modal(ctx);
                }
            }
            NativeMenuAction::OpenRandomizeSelection => {
                if has_authoring_doc {
                    self.open_randomize_selection_modal(ctx);
                }
            }
            NativeMenuAction::PixelSnapSelection => {
                if has_authoring_doc && !self.editable_targets().is_empty() {
                    let _ = self.pixel_snap_selection();
                }
            }
            NativeMenuAction::AlignSelection { axis, edge } => {
                if has_authoring_doc && self.can_align_selection() {
                    self.align_selection(ctx, axis, edge);
                }
            }
            NativeMenuAction::ToggleAlignToStage => {
                if has_authoring_doc {
                    self.authoring_modify.to_stage = !self.authoring_modify.to_stage;
                }
            }
            NativeMenuAction::ShowPanel(panel) => {
                if !self.is_standalone_script_session() {
                    self.show_panel(panel);
                }
            }
            NativeMenuAction::TextSetSize(pt) => {
                if has_authoring_doc {
                    let size = f64::from(pt).clamp(1.0, 10000.0);
                    self.workspace.text_size = size;
                    if self.selection.len() == 1 && self.layer_editable() {
                        let is_text = self.selected_element().is_some_and(|el| {
                            matches!(
                                el.kind,
                                ElementKind::Text { .. } | ElementKind::TextBox { .. }
                            )
                        });
                        if is_text {
                            self.finish_edits();
                            self.transact_named("Change text size", |s| {
                                s.ensure_selected_key();
                                if let Some(el) = s.selected_element_mut() {
                                    match &mut el.kind {
                                        ElementKind::Text { font_size, .. } => *font_size = size,
                                        ElementKind::TextBox { format, .. } => format.size = size,
                                        _ => {}
                                    }
                                }
                            });
                        }
                    }
                }
            }
            NativeMenuAction::TextSetStyle { bold, italic } => {
                if has_authoring_doc && self.selection.len() == 1 && self.layer_editable() {
                    let is_box = self
                        .selected_element()
                        .is_some_and(|el| matches!(el.kind, ElementKind::TextBox { .. }));
                    if is_box {
                        self.finish_edits();
                        self.transact_named("Change text style", |s| {
                            s.ensure_selected_key();
                            if let Some(ElementKind::TextBox { format, .. }) =
                                s.selected_element_mut().map(|el| &mut el.kind)
                            {
                                format.bold = bold;
                                format.italic = italic;
                            }
                        });
                    }
                }
            }
            NativeMenuAction::TextSetAlign(align) => {
                if has_authoring_doc && self.selection.len() == 1 && self.layer_editable() {
                    let is_box = self
                        .selected_element()
                        .is_some_and(|el| matches!(el.kind, ElementKind::TextBox { .. }));
                    if is_box {
                        self.finish_edits();
                        self.transact_named("Align text", |s| {
                            s.ensure_selected_key();
                            if let Some(ElementKind::TextBox { format, .. }) =
                                s.selected_element_mut().map(|el| &mut el.kind)
                            {
                                format.align = align;
                            }
                        });
                    }
                }
            }
            NativeMenuAction::TextAdjustTracking(delta) => {
                if has_authoring_doc && self.selection.len() == 1 && self.layer_editable() {
                    let is_box = self
                        .selected_element()
                        .is_some_and(|el| matches!(el.kind, ElementKind::TextBox { .. }));
                    if is_box {
                        self.finish_edits();
                        self.transact_named("Adjust letter spacing", |s| {
                            s.ensure_selected_key();
                            if let Some(ElementKind::TextBox { format, .. }) =
                                s.selected_element_mut().map(|el| &mut el.kind)
                            {
                                format.letter_spacing = if delta == 0 {
                                    0.0
                                } else {
                                    (format.letter_spacing + f64::from(delta))
                                        .clamp(-1000.0, 1000.0)
                                };
                            }
                        });
                    }
                }
            }
            NativeMenuAction::TextOpenFontEmbedding => {
                if has_authoring_doc {
                    self.open_font_embedding(ctx);
                }
            }
            NativeMenuAction::ToggleLiveActionScript => {
                if has_authoring_doc {
                    self.set_live_script_preview(!self.live_script_preview);
                }
            }
            NativeMenuAction::DebugContinueOrStart => {
                if has_authoring_doc {
                    if self.debugger.snapshot().is_some() {
                        let _ = self.debug_command(DebugCommand::Resume);
                    } else {
                        self.start_debug_movie(ctx);
                    }
                }
            }
            NativeMenuAction::DebugStop => {
                if has_authoring_doc {
                    self.debugger.stop();
                }
            }
            NativeMenuAction::DebugStepOver => {
                if has_authoring_doc {
                    let _ = self.debug_command(DebugCommand::StepOver);
                }
            }
            NativeMenuAction::DebugStepInto => {
                if has_authoring_doc {
                    let _ = self.debug_command(DebugCommand::StepInto);
                }
            }
            NativeMenuAction::DebugStepOut => {
                if has_authoring_doc {
                    let _ = self.debug_command(DebugCommand::StepOut);
                }
            }
            NativeMenuAction::DebugToggleBreakpoint => {
                if has_authoring_doc {
                    self.toggle_debug_breakpoint(ctx);
                }
            }
            NativeMenuAction::DebugClearBreakpoints => {
                if has_authoring_doc {
                    self.clear_all_session_breakpoints();
                    if self.debugger.snapshot().is_some() {
                        let _ = self.debug_command(DebugCommand::SetBreakpoints(Vec::new()));
                    }
                }
            }
            NativeMenuAction::ApplyWorkspaceTemplate(template) => {
                self.apply_workspace_template(ctx, template);
            }
            NativeMenuAction::ApplyNamedWorkspace(name) => {
                self.apply_named_workspace(ctx, &name);
            }
            NativeMenuAction::SaveWorkspaceDialog => {
                self.workspace_dialog = Some(WorkspaceDialog::Save {
                    name: String::new(),
                    focus: true,
                });
            }
            NativeMenuAction::UpdateActiveWorkspace => {
                if let Some(name) = self.workspace.active_named_preset.clone() {
                    let _ = self.save_named_workspace(&name, true);
                }
            }
            NativeMenuAction::RenameActiveWorkspaceDialog => {
                if let Some(name) = self.workspace.active_named_preset.clone() {
                    self.workspace_dialog = Some(WorkspaceDialog::Rename {
                        original: name.clone(),
                        name,
                        focus: true,
                    });
                }
            }
            NativeMenuAction::DeleteActiveWorkspaceDialog => {
                if let Some(name) = self.workspace.active_named_preset.clone() {
                    self.workspace_dialog = Some(WorkspaceDialog::Delete { name });
                }
            }
            NativeMenuAction::ResetActiveWorkspace => {
                self.reset_active_workspace(ctx);
            }
            NativeMenuAction::SetDensity(density) => {
                self.set_density(ctx, density);
            }
            NativeMenuAction::SetTheme(theme) => {
                self.set_theme(ctx, theme);
            }
            NativeMenuAction::DetachPanel(panel) => {
                if self.has_document() && !self.is_standalone_script_session() {
                    self.set_panel_placement(ctx, panel, PanelPlacement::Detached);
                }
            }
            NativeMenuAction::DockPanel(panel) => {
                if self.has_document() && !self.is_standalone_script_session() {
                    self.set_panel_placement(ctx, panel, PanelPlacement::Docked);
                }
            }
            NativeMenuAction::OpenCustomizeTools => {
                if self.has_document() && !self.is_standalone_script_session() {
                    self.open_customize_tools(ctx);
                }
            }
            NativeMenuAction::ToggleUiLayoutStudio => {
                let open = crate::panels::ui_studio::is_studio_open(ctx);
                crate::panels::ui_studio::set_studio_open(ctx, !open);
            }
            NativeMenuAction::ActivateDocumentTab(index) => {
                let _ = self.switch_session_tab(ctx, index);
            }
            NativeMenuAction::NextDocumentTab => {
                self.cycle_document(ctx, false);
            }
            NativeMenuAction::PreviousDocumentTab => {
                self.cycle_document(ctx, true);
            }
        }
    }

    /// Execute a [`NativeSystemItemRole`] from the in-window NF2 menu bar (`T-377`).
    pub fn execute_native_system_role(&self, ctx: &egui::Context, role: NativeSystemItemRole) {
        match role {
            NativeSystemItemRole::MinimizeWindow => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            NativeSystemItemRole::ZoomWindow => {
                let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            NativeSystemItemRole::HideApp
            | NativeSystemItemRole::HideOthers
            | NativeSystemItemRole::ShowAll => {
                #[cfg(all(target_os = "macos", not(test)))]
                native::perform_app_hide_role(role);
            }
        }
    }
}

/// Drain pending Finder open-file, Dock reopen, and clicked native menu actions on the root frame.
pub(crate) fn drain_events(editor: &mut Editor, ctx: &egui::Context) {
    if let Ok(mut queue) = global_lifecycle_queue().lock() {
        let _ = queue.drain_into(editor, ctx);
    }
}

/// Drain any Finder `odoc` paths queued during `NSApplication` cold-launch before `Editor::new`
/// decides whether to apply `OnLaunch` Start/Demo defaults.
pub(crate) fn drain_startup_open_files(into: &mut Vec<PathBuf>) {
    if let Ok(mut queue) = global_lifecycle_queue().lock() {
        queue.drain_startup_open_files(into);
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod native {
    use super::*;
    use objc2::{
        Encode, MainThreadMarker, ffi, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
        sel,
    };
    use objc2_app_kit::{
        NSApplication, NSEvent, NSEventModifierFlags, NSFloatingWindowLevel, NSWindow,
    };
    use std::cell::RefCell;
    use std::ffi::{CStr, CString, c_char};

    const NS_CONTROL_STATE_ON: isize = 1;
    const NS_CONTROL_STATE_OFF: isize = 0;
    const NS_APP_DELEGATE_REPLY_SUCCESS: usize = 0;
    const NS_APP_DELEGATE_REPLY_FAILURE: usize = 2;

    type MenuActionSlot = Vec<(NativeMenuAction, bool)>;
    type MenuActionSlots = [MenuActionSlot; TOP_MENU_COUNT];

    static CONTEXT: Mutex<Option<egui::Context>> = Mutex::new(None);
    static ACTION_TABLE: OnceLock<Mutex<MenuActionSlots>> = OnceLock::new();

    struct MainThreadCache {
        installed: bool,
        target: Option<Retained<AnyObject>>,
        services_menu: Option<Retained<AnyObject>>,
        #[allow(dead_code)]
        main_menu: Option<Retained<AnyObject>>,
        top_items: Vec<Retained<AnyObject>>,
        top_menus: Vec<Retained<AnyObject>>,
        top_enabled: [bool; TOP_MENU_COUNT],
        specs: [Option<NativeTopMenuSpec>; TOP_MENU_COUNT],
        dirty: [bool; TOP_MENU_COUNT],
        last_fingerprint: Option<MenuSyncFingerprint>,
        last_window_state: Option<(isize, bool, String)>,
    }

    thread_local! {
        static CACHE: RefCell<MainThreadCache> = const {
            RefCell::new(MainThreadCache {
                installed: false,
                target: None,
                services_menu: None,
                main_menu: None,
                top_items: Vec::new(),
                top_menus: Vec::new(),
                top_enabled: [true; TOP_MENU_COUNT],
                specs: [const { None }; TOP_MENU_COUNT],
                dirty: [true; TOP_MENU_COUNT],
                last_fingerprint: None,
                last_window_state: None,
            })
        };
    }

    fn action_table() -> &'static Mutex<MenuActionSlots> {
        ACTION_TABLE.get_or_init(|| Mutex::new(std::array::from_fn(|_| Vec::new())))
    }

    fn wake_root_viewport() {
        let ctx = CONTEXT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(ctx) = ctx {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    fn ns_string(text: &str) -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"NSString")?;
        let clean: std::borrow::Cow<'_, str> = if text.contains('\0') {
            std::borrow::Cow::Owned(text.replace('\0', ""))
        } else {
            std::borrow::Cow::Borrowed(text)
        };
        let cstr = CString::new(clean.as_ref()).ok()?;
        let ptr: *const c_char = cstr.as_ptr();
        unsafe {
            let raw: *mut AnyObject = msg_send![cls, alloc];
            if raw.is_null() {
                return None;
            }
            let initialized: *mut AnyObject = msg_send![raw, initWithUTF8String: ptr];
            Retained::from_raw(initialized)
        }
    }

    unsafe fn ns_string_to_rust(obj: &AnyObject) -> Option<String> {
        let utf8: *const c_char = unsafe { msg_send![obj, UTF8String] };
        if utf8.is_null() {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(utf8) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn find_root_window(app: &NSApplication) -> Option<Retained<NSWindow>> {
        let windows = app.windows();
        let tracked = crate::native_windows::root_window_number();
        if tracked > 0
            && let Some(found) = windows.iter().find(|w| w.windowNumber() == tracked)
        {
            return Some(found.clone());
        }
        windows
            .iter()
            .find(|w| w.level() != NSFloatingWindowLevel)
            .or_else(|| windows.iter().next())
    }

    type OpenFileFn =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, Option<&AnyObject>) -> Bool;
    type OpenFilesFn =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, Option<&AnyObject>);
    type ShouldHandleReopenFn =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, Bool) -> Bool;
    type InvokeMenuActionFn = unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>);
    type ValidateMenuItemFn =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>) -> Bool;
    type MenuNeedsUpdateFn = unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>);
    type PerformKeyEquivalentFn =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&NSEvent>) -> Bool;
    type PrelaunchNotificationFn = unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>);

    unsafe extern "C-unwind" fn prelaunch_will_finish_launching(
        _observer: &AnyObject,
        _sel: Sel,
        _notification: Option<&AnyObject>,
    ) {
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = install_delegate_hooks(mtm);
        }
    }

    unsafe extern "C-unwind" fn application_open_file(
        _delegate: &AnyObject,
        _sel: Sel,
        app: &NSApplication,
        filename: Option<&AnyObject>,
    ) -> Bool {
        let Some(filename) = filename else {
            return Bool::NO;
        };
        let Some(path_str) = (unsafe { ns_string_to_rust(filename) }) else {
            return Bool::NO;
        };
        if path_str.is_empty() {
            return Bool::NO;
        }
        if let Ok(mut queue) = global_lifecycle_queue().lock() {
            queue.push_open_file(PathBuf::from(path_str));
        }
        if let Some(root) = find_root_window(app) {
            if root.isMiniaturized() {
                root.deminiaturize(None);
            }
            root.makeKeyAndOrderFront(None);
        }
        wake_root_viewport();
        Bool::YES
    }

    unsafe extern "C-unwind" fn application_open_files(
        _delegate: &AnyObject,
        _sel: Sel,
        app: &NSApplication,
        filenames: Option<&AnyObject>,
    ) {
        let mut accepted = Vec::new();
        if let Some(array) = filenames {
            let count: usize = unsafe { msg_send![array, count] };
            for index in 0..count {
                let item: *const AnyObject = unsafe { msg_send![array, objectAtIndex: index] };
                if !item.is_null()
                    && let Some(path_str) = unsafe { ns_string_to_rust(&*item) }
                    && !path_str.is_empty()
                {
                    accepted.push(PathBuf::from(path_str));
                }
            }
        }
        let reply = if accepted.is_empty() {
            NS_APP_DELEGATE_REPLY_FAILURE
        } else {
            if let Ok(mut queue) = global_lifecycle_queue().lock() {
                queue.push_open_files(accepted);
            }
            if let Some(root) = find_root_window(app) {
                if root.isMiniaturized() {
                    root.deminiaturize(None);
                }
                root.makeKeyAndOrderFront(None);
            }
            wake_root_viewport();
            NS_APP_DELEGATE_REPLY_SUCCESS
        };
        let _: () = unsafe { msg_send![app, replyToOpenOrPrint: reply] };
    }

    unsafe extern "C-unwind" fn application_should_handle_reopen(
        _delegate: &AnyObject,
        _sel: Sel,
        app: &NSApplication,
        has_visible_windows: Bool,
    ) -> Bool {
        if let Some(root) = find_root_window(app) {
            if root.isMiniaturized() {
                root.deminiaturize(None);
            }
            root.makeKeyAndOrderFront(None);
        }
        if let Ok(mut queue) = global_lifecycle_queue().lock() {
            queue.request_reopen(has_visible_windows.as_bool());
        }
        wake_root_viewport();
        Bool::NO
    }

    unsafe extern "C-unwind" fn invoke_menu_action(
        _target: &AnyObject,
        _sel: Sel,
        sender: Option<&AnyObject>,
    ) {
        let Some(sender) = sender else {
            return;
        };
        let tag: isize = unsafe { msg_send![sender, tag] };
        let Some((menu_idx, local_idx)) = decode_menu_tag(tag) else {
            return;
        };
        let found = action_table()
            .lock()
            .ok()
            .and_then(|table| table.get(menu_idx)?.get(local_idx).cloned());
        if let Some((action, enabled)) = found
            && enabled
        {
            if let Ok(mut queue) = global_lifecycle_queue().lock() {
                queue.push_action(action);
            }
            wake_root_viewport();
        }
    }

    unsafe extern "C-unwind" fn validate_menu_item(
        _target: &AnyObject,
        _sel: Sel,
        menu_item: Option<&AnyObject>,
    ) -> Bool {
        let Some(menu_item) = menu_item else {
            return Bool::NO;
        };
        let tag: isize = unsafe { msg_send![menu_item, tag] };
        if let Some((menu_idx, local_idx)) = decode_menu_tag(tag)
            && let Some(enabled) = action_table().lock().ok().and_then(|table| {
                table
                    .get(menu_idx)?
                    .get(local_idx)
                    .map(|(_, enabled)| *enabled)
            })
        {
            return Bool::new(enabled);
        }
        unsafe { msg_send![menu_item, isEnabled] }
    }

    unsafe extern "C-unwind" fn menu_needs_update(
        _target: &AnyObject,
        _sel: Sel,
        menu: Option<&AnyObject>,
    ) {
        let Some(menu) = menu else {
            return;
        };
        let menu_ptr = menu as *const AnyObject;
        let pending = CACHE.with(|cache| {
            let mut state = cache.borrow_mut();
            let idx = state
                .top_menus
                .iter()
                .position(|m| std::ptr::eq(&**m, menu_ptr))?;
            if !state.dirty[idx] {
                return None;
            }
            let spec = state.specs[idx].clone()?;
            let target = state.target.clone()?;
            let services = state.services_menu.clone()?;
            state.dirty[idx] = false;
            Some((idx, spec, target, services))
        });
        let Some((menu_idx, spec, target, services_menu)) = pending else {
            return;
        };
        rebuild_single_top_menu(menu, menu_idx, &spec, &target, &services_menu);
    }

    unsafe extern "C-unwind" fn main_menu_perform_key_equivalent(
        this: &AnyObject,
        _sel: Sel,
        event: Option<&NSEvent>,
    ) -> Bool {
        let Some(event) = event else {
            return Bool::NO;
        };
        let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
        let cmd = flags.contains(NSEventModifierFlags::Command);
        let alt = flags.contains(NSEventModifierFlags::Option);
        let shift = flags.contains(NSEventModifierFlags::Shift);
        let ctrl = flags.contains(NSEventModifierFlags::Control);
        let chars_ptr: *const AnyObject = unsafe { msg_send![event, charactersIgnoringModifiers] };
        let chars = if chars_ptr.is_null() {
            String::new()
        } else {
            unsafe { ns_string_to_rust(&*chars_ptr) }.unwrap_or_default()
        };
        if !should_route_key_equivalent_via_appkit(cmd, alt, shift, ctrl, &chars) {
            return Bool::NO;
        }
        let item_count: isize = unsafe { msg_send![this, numberOfItems] };
        for idx in 0..item_count.max(0) {
            let item: *const AnyObject = unsafe { msg_send![this, itemAtIndex: idx] };
            if item.is_null() {
                continue;
            }
            let submenu: *const AnyObject = unsafe { msg_send![&*item, submenu] };
            if submenu.is_null() {
                continue;
            }
            let handled: Bool = unsafe { msg_send![&*submenu, performKeyEquivalent: event] };
            if handled.as_bool() {
                return Bool::YES;
            }
        }
        Bool::NO
    }

    fn ensure_target_class() -> Result<&'static AnyClass, String> {
        static TARGET_CLASS: OnceLock<Result<&'static AnyClass, String>> = OnceLock::new();
        TARGET_CLASS
            .get_or_init(|| {
                if let Some(existing) = AnyClass::get(c"NeoFlashMacMenuTarget") {
                    return Ok(existing);
                }
                let superclass = AnyClass::get(c"NSObject")
                    .ok_or_else(|| "Could not find NSObject class.".to_owned())?;
                let cls = unsafe {
                    ffi::objc_allocateClassPair(superclass, c"NeoFlashMacMenuTarget".as_ptr(), 0)
                };
                if cls.is_null() {
                    return Err("Could not allocate NeoFlashMacMenuTarget class.".to_owned());
                }
                let invoke_imp =
                    unsafe { std::mem::transmute::<InvokeMenuActionFn, Imp>(invoke_menu_action) };
                let invoke_added = unsafe {
                    ffi::class_addMethod(cls, sel!(invokeMenuAction:), invoke_imp, c"v@:@".as_ptr())
                };
                if !invoke_added.as_bool() {
                    return Err("Could not add invokeMenuAction: to NeoFlashMacMenuTarget.".into());
                }
                let validate_enc = CString::new(format!("{}@:@", Bool::ENCODING))
                    .map_err(|_| "Invalid validateMenuItem: encoding.".to_owned())?;
                let validate_imp =
                    unsafe { std::mem::transmute::<ValidateMenuItemFn, Imp>(validate_menu_item) };
                let validate_added = unsafe {
                    ffi::class_addMethod(
                        cls,
                        sel!(validateMenuItem:),
                        validate_imp,
                        validate_enc.as_ptr(),
                    )
                };
                if !validate_added.as_bool() {
                    return Err("Could not add validateMenuItem: to NeoFlashMacMenuTarget.".into());
                }
                let update_imp =
                    unsafe { std::mem::transmute::<MenuNeedsUpdateFn, Imp>(menu_needs_update) };
                let update_added = unsafe {
                    ffi::class_addMethod(cls, sel!(menuNeedsUpdate:), update_imp, c"v@:@".as_ptr())
                };
                if !update_added.as_bool() {
                    return Err("Could not add menuNeedsUpdate: to NeoFlashMacMenuTarget.".into());
                }
                unsafe { ffi::objc_registerClassPair(cls) };
                AnyClass::get(c"NeoFlashMacMenuTarget")
                    .ok_or_else(|| "Registered NeoFlashMacMenuTarget class not found.".to_owned())
            })
            .clone()
    }

    fn ensure_main_menu_class() -> Result<&'static AnyClass, String> {
        static MAIN_MENU_CLASS: OnceLock<Result<&'static AnyClass, String>> = OnceLock::new();
        MAIN_MENU_CLASS
            .get_or_init(|| {
                if let Some(existing) = AnyClass::get(c"NeoFlashMainMenu") {
                    return Ok(existing);
                }
                let superclass = AnyClass::get(c"NSMenu")
                    .ok_or_else(|| "Could not find NSMenu class.".to_owned())?;
                let cls = unsafe {
                    ffi::objc_allocateClassPair(superclass, c"NeoFlashMainMenu".as_ptr(), 0)
                };
                if cls.is_null() {
                    return Err("Could not allocate NeoFlashMainMenu class.".to_owned());
                }
                let perf_enc = CString::new(format!("{}@:@", Bool::ENCODING))
                    .map_err(|_| "Invalid performKeyEquivalent: encoding.".to_owned())?;
                let perf_imp = unsafe {
                    std::mem::transmute::<PerformKeyEquivalentFn, Imp>(
                        main_menu_perform_key_equivalent,
                    )
                };
                let added = unsafe {
                    ffi::class_addMethod(
                        cls,
                        sel!(performKeyEquivalent:),
                        perf_imp,
                        perf_enc.as_ptr(),
                    )
                };
                if !added.as_bool() {
                    return Err("Could not add performKeyEquivalent: to NeoFlashMainMenu.".into());
                }
                unsafe { ffi::objc_registerClassPair(cls) };
                AnyClass::get(c"NeoFlashMainMenu")
                    .ok_or_else(|| "Registered NeoFlashMainMenu class not found.".to_owned())
            })
            .clone()
    }

    fn add_delegate_method(
        class: &AnyClass,
        selector: Sel,
        imp: Imp,
        encoding: &str,
    ) -> Result<bool, String> {
        if let Some(method) = class.instance_method(selector) {
            if std::ptr::fn_addr_eq(method.implementation(), imp) {
                return Ok(false);
            }
            return Err(format!(
                "Native macOS lifecycle bridge cannot overwrite existing {} on {}.",
                selector.name().to_string_lossy(),
                class.name().to_string_lossy()
            ));
        }
        let enc_cstr = CString::new(encoding)
            .map_err(|_| "Could not encode Objective-C method signature.".to_owned())?;
        let added = unsafe {
            ffi::class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                selector,
                imp,
                enc_cstr.as_ptr(),
            )
        };
        if !added.as_bool() {
            return Err(format!(
                "Native macOS lifecycle bridge could not install {}.",
                selector.name().to_string_lossy()
            ));
        }
        Ok(true)
    }

    fn ensure_prelaunch_observer_class() -> Result<&'static AnyClass, String> {
        static OBSERVER_CLASS: OnceLock<Result<&'static AnyClass, String>> = OnceLock::new();
        OBSERVER_CLASS
            .get_or_init(|| {
                if let Some(existing) = AnyClass::get(c"NeoFlashPrelaunchObserver") {
                    return Ok(existing);
                }
                let superclass = AnyClass::get(c"NSObject")
                    .ok_or_else(|| "Could not find NSObject class.".to_owned())?;
                let cls = unsafe {
                    ffi::objc_allocateClassPair(
                        superclass,
                        c"NeoFlashPrelaunchObserver".as_ptr(),
                        0,
                    )
                };
                if cls.is_null() {
                    return Err("Could not allocate NeoFlashPrelaunchObserver class.".to_owned());
                }
                let imp = unsafe {
                    std::mem::transmute::<PrelaunchNotificationFn, Imp>(
                        prelaunch_will_finish_launching,
                    )
                };
                let added = unsafe {
                    ffi::class_addMethod(cls, sel!(willFinishLaunching:), imp, c"v@:@".as_ptr())
                };
                if !added.as_bool() {
                    return Err(
                        "Could not add willFinishLaunching: to NeoFlashPrelaunchObserver.".into(),
                    );
                }
                unsafe { ffi::objc_registerClassPair(cls) };
                AnyClass::get(c"NeoFlashPrelaunchObserver")
                    .ok_or_else(|| "Registered NeoFlashPrelaunchObserver class not found.".into())
            })
            .clone()
    }

    fn install_delegate_hooks(mtm: MainThreadMarker) -> Result<(), String> {
        let app = NSApplication::sharedApplication(mtm);
        let delegate = app.delegate().ok_or_else(|| {
            "Native macOS menu bridge could not find the application delegate.".to_owned()
        })?;
        let object: &AnyObject = (*delegate).as_ref();
        let class = object.class();

        let open_file_enc = format!("{}@:@@", Bool::ENCODING);
        let open_file_imp =
            unsafe { std::mem::transmute::<OpenFileFn, Imp>(application_open_file) };
        let added_open_file = add_delegate_method(
            class,
            sel!(application:openFile:),
            open_file_imp,
            &open_file_enc,
        )?;

        let open_files_imp =
            unsafe { std::mem::transmute::<OpenFilesFn, Imp>(application_open_files) };
        let added_open_files =
            add_delegate_method(class, sel!(application:openFiles:), open_files_imp, "v@:@@")?;

        let reopen_enc = format!("{}@:@{}", Bool::ENCODING, Bool::ENCODING);
        let reopen_imp = unsafe {
            std::mem::transmute::<ShouldHandleReopenFn, Imp>(application_should_handle_reopen)
        };
        let added_reopen = add_delegate_method(
            class,
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            reopen_imp,
            &reopen_enc,
        )?;

        if added_open_file || added_open_files || added_reopen {
            // Refresh NSApplication's cached delegate selector flags without altering winit's
            // delegate instance or notifications.
            app.setDelegate(None);
            app.setDelegate(Some(&delegate));
        }
        Ok(())
    }

    pub(super) fn install_prelaunch_hooks() {
        static PRELAUNCH_HOOKS: OnceLock<()> = OnceLock::new();
        PRELAUNCH_HOOKS.get_or_init(|| {
            let Ok(obs_cls) = ensure_prelaunch_observer_class() else {
                return;
            };
            let Some(notif_name) = ns_string("NSApplicationWillFinishLaunchingNotification") else {
                return;
            };
            let Some(nc_cls) = AnyClass::get(c"NSNotificationCenter") else {
                return;
            };
            unsafe {
                let raw_obs: *mut AnyObject = msg_send![obs_cls, new];
                let Some(observer) = Retained::from_raw(raw_obs) else {
                    return;
                };
                let center: *const AnyObject = msg_send![nc_cls, defaultCenter];
                if !center.is_null() {
                    let nil_obj: Option<&AnyObject> = None;
                    let _: () = msg_send![
                        &*center,
                        addObserver: &*observer,
                        selector: sel!(willFinishLaunching:),
                        name: &*notif_name,
                        object: nil_obj
                    ];
                    // Keep the singleton observer alive for the lifetime of the GUI process.
                    std::mem::forget(observer);
                }
            }
            if let Some(mtm) = MainThreadMarker::new() {
                let app = NSApplication::sharedApplication(mtm);
                if app.delegate().is_some() {
                    let _ = install_delegate_hooks(mtm);
                }
            }
        });
    }

    pub(super) fn install(ctx: &egui::Context) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "Native macOS menu bridge must be installed on the main thread.".to_owned()
        })?;
        let _ = ensure_target_class()?;
        let _ = ensure_main_menu_class()?;
        install_delegate_hooks(mtm)?;

        *CONTEXT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ctx.clone());
        let app = NSApplication::sharedApplication(mtm);
        ensure_persistent_main_menu(&app)?;
        Ok(())
    }

    pub(super) fn perform_app_hide_role(role: NativeSystemItemRole) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        match role {
            NativeSystemItemRole::HideApp => app.hide(None),
            NativeSystemItemRole::HideOthers => app.hideOtherApplications(None),
            NativeSystemItemRole::ShowAll => app.unhideAllApplications(None),
            NativeSystemItemRole::MinimizeWindow | NativeSystemItemRole::ZoomWindow => {}
        }
    }

    fn alloc_menu(cls: &AnyClass, title: &str) -> Option<Retained<AnyObject>> {
        let title_ns = ns_string(title)?;
        let menu = unsafe {
            let raw: *mut AnyObject = msg_send![cls, alloc];
            if raw.is_null() {
                return None;
            }
            let initialized: *mut AnyObject = msg_send![raw, initWithTitle: &*title_ns];
            Retained::from_raw(initialized)?
        };
        let _: () = unsafe { msg_send![&*menu, setAutoenablesItems: Bool::NO] };
        Some(menu)
    }

    fn alloc_menu_item(
        item_cls: &AnyClass,
        title: &str,
        action: Option<Sel>,
        key_equiv: &str,
    ) -> Option<Retained<AnyObject>> {
        let title_ns = ns_string(title)?;
        let key_ns = ns_string(key_equiv)?;
        unsafe {
            let raw: *mut AnyObject = msg_send![item_cls, alloc];
            if raw.is_null() {
                return None;
            }
            let initialized: *mut AnyObject = msg_send![
                raw,
                initWithTitle: &*title_ns,
                action: action,
                keyEquivalent: &*key_ns
            ];
            Retained::from_raw(initialized)
        }
    }

    fn alloc_separator_item(item_cls: &AnyClass) -> Option<Retained<AnyObject>> {
        unsafe {
            let sep: *mut AnyObject = msg_send![item_cls, separatorItem];
            if sep.is_null() {
                None
            } else {
                Retained::retain(sep)
            }
        }
    }

    fn alloc_section_item(item_cls: &AnyClass, title: &str) -> Option<Retained<AnyObject>> {
        let supports_section: Bool =
            unsafe { msg_send![item_cls, respondsToSelector: sel!(sectionHeaderWithTitle:)] };
        if supports_section.as_bool()
            && let Some(title_ns) = ns_string(title)
        {
            let header: *mut AnyObject =
                unsafe { msg_send![item_cls, sectionHeaderWithTitle: &*title_ns] };
            if !header.is_null()
                && let Some(retained) = unsafe { Retained::retain(header) }
            {
                return Some(retained);
            }
        }
        let fallback = alloc_menu_item(item_cls, title, None, "")?;
        let _: () = unsafe { msg_send![&*fallback, setEnabled: Bool::NO] };
        let _: () = unsafe { msg_send![&*fallback, setTag: -1isize] };
        Some(fallback)
    }

    struct PopulateCtx<'a> {
        menu_cls: &'a AnyClass,
        item_cls: &'a AnyClass,
        target: &'a AnyObject,
        services_menu: &'a Retained<AnyObject>,
        menu_idx: usize,
    }

    fn populate_menu(
        menu: &AnyObject,
        pctx: &PopulateCtx<'_>,
        items: &[NativeMenuItemSpec],
        actions: &mut Vec<(NativeMenuAction, bool)>,
    ) {
        let item_cls = pctx.item_cls;
        for spec in items {
            match spec {
                NativeMenuItemSpec::Separator => {
                    if let Some(sep) = alloc_separator_item(item_cls) {
                        let _: () = unsafe { msg_send![menu, addItem: &*sep] };
                    }
                }
                NativeMenuItemSpec::Section(title) => {
                    if let Some(header) = alloc_section_item(item_cls, title) {
                        let _: () = unsafe { msg_send![menu, addItem: &*header] };
                    }
                }
                NativeMenuItemSpec::System {
                    label,
                    role,
                    shortcut,
                    enabled,
                    ..
                } => {
                    let selector = match role {
                        NativeSystemItemRole::HideApp => sel!(hide:),
                        NativeSystemItemRole::HideOthers => sel!(hideOtherApplications:),
                        NativeSystemItemRole::ShowAll => sel!(unhideAllApplications:),
                        NativeSystemItemRole::MinimizeWindow => sel!(performMiniaturize:),
                        NativeSystemItemRole::ZoomWindow => sel!(performZoom:),
                    };
                    let (key_str, mask) = shortcut
                        .and_then(macos_key_equivalent)
                        .unwrap_or_else(|| (String::new(), 0));
                    let Some(item) = alloc_menu_item(item_cls, label, Some(selector), &key_str)
                    else {
                        continue;
                    };
                    if !key_str.is_empty() {
                        let _: () =
                            unsafe { msg_send![&*item, setKeyEquivalentModifierMask: mask] };
                    }
                    let _: () = unsafe { msg_send![&*item, setTarget: None::<&AnyObject>] };
                    let _: () = unsafe { msg_send![&*item, setEnabled: Bool::new(*enabled)] };
                    let _: () = unsafe { msg_send![&*item, setTag: -1isize] };
                    let _: () = unsafe { msg_send![menu, addItem: &*item] };
                }
                NativeMenuItemSpec::Action {
                    label,
                    shortcut,
                    enabled,
                    checked,
                    tooltip,
                    action,
                    ..
                } => {
                    let (key_str, mask) = shortcut
                        .and_then(macos_key_equivalent)
                        .unwrap_or_else(|| (String::new(), 0));
                    let Some(item) =
                        alloc_menu_item(item_cls, label, Some(sel!(invokeMenuAction:)), &key_str)
                    else {
                        continue;
                    };
                    if !key_str.is_empty() {
                        let _: () =
                            unsafe { msg_send![&*item, setKeyEquivalentModifierMask: mask] };
                    }
                    let local_idx = actions.len();
                    let tag = encode_menu_tag(pctx.menu_idx, local_idx);
                    actions.push((action.clone(), *enabled));
                    let _: () = unsafe { msg_send![&*item, setTag: tag] };
                    let _: () = unsafe { msg_send![&*item, setTarget: Some(pctx.target)] };
                    let _: () = unsafe { msg_send![&*item, setEnabled: Bool::new(*enabled)] };
                    let state = if *checked {
                        NS_CONTROL_STATE_ON
                    } else {
                        NS_CONTROL_STATE_OFF
                    };
                    let _: () = unsafe { msg_send![&*item, setState: state] };
                    if let Some(tip) = tooltip
                        && let Some(tip_ns) = ns_string(tip)
                    {
                        let _: () = unsafe { msg_send![&*item, setToolTip: Some(&*tip_ns)] };
                    }
                    let _: () = unsafe { msg_send![menu, addItem: &*item] };
                }
                NativeMenuItemSpec::Submenu {
                    label,
                    enabled,
                    role,
                    items: sub_items,
                } => {
                    let Some(item) = alloc_menu_item(item_cls, label, None, "") else {
                        continue;
                    };
                    let _: () = unsafe { msg_send![&*item, setEnabled: Bool::new(*enabled)] };
                    let _: () = unsafe { msg_send![&*item, setTag: -1isize] };
                    if matches!(role, Some(NativeSubmenuRole::Services)) {
                        let _: () =
                            unsafe { msg_send![&*item, setSubmenu: Some(&**pctx.services_menu)] };
                    } else if let Some(sub) = alloc_menu(pctx.menu_cls, label) {
                        populate_menu(&sub, pctx, sub_items, actions);
                        let _: () = unsafe { msg_send![&*item, setSubmenu: Some(&*sub)] };
                    }
                    let _: () = unsafe { msg_send![menu, addItem: &*item] };
                }
            }
        }
    }

    /// Detach `services_menu` from any existing `NSMenuItem` in `menu` before `[menu removeAllItems]`
    /// so AppKit never raises `NSInternalInconsistencyException` while an autoreleased item still
    /// points to `services_menu`.
    fn detach_services_menu_from(menu: &AnyObject, services_menu: &AnyObject) {
        let count: isize = unsafe { msg_send![menu, numberOfItems] };
        for idx in 0..count.max(0) {
            let item: *mut AnyObject = unsafe { msg_send![menu, itemAtIndex: idx] };
            if !item.is_null() {
                let sub: *const AnyObject = unsafe { msg_send![item, submenu] };
                if std::ptr::eq(sub, services_menu as *const AnyObject) {
                    let _: () = unsafe { msg_send![item, setSubmenu: None::<&AnyObject>] };
                }
            }
        }
    }

    fn rebuild_single_top_menu(
        menu: &AnyObject,
        menu_idx: usize,
        spec: &NativeTopMenuSpec,
        target: &AnyObject,
        services_menu: &Retained<AnyObject>,
    ) {
        let (Some(menu_cls), Some(item_cls)) =
            (AnyClass::get(c"NSMenu"), AnyClass::get(c"NSMenuItem"))
        else {
            return;
        };
        detach_services_menu_from(menu, services_menu);
        let _: () = unsafe { msg_send![menu, removeAllItems] };
        let mut local_actions = Vec::with_capacity(spec.items.len() * 2);
        let pctx = PopulateCtx {
            menu_cls,
            item_cls,
            target,
            services_menu,
            menu_idx,
        };
        populate_menu(menu, &pctx, &spec.items, &mut local_actions);
        if let Ok(mut table) = action_table().lock()
            && let Some(slot) = table.get_mut(menu_idx)
        {
            *slot = local_actions;
        }
    }

    fn bootstrap_app_menu_items() -> NativeTopMenuSpec {
        NativeTopMenuSpec {
            title: "NeoFlash".to_owned(),
            enabled: true,
            items: vec![
                custom_item("About NeoFlash", None, true, false, NativeMenuAction::About),
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Services".to_owned(),
                    enabled: true,
                    role: Some(NativeSubmenuRole::Services),
                    items: Vec::new(),
                },
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::System {
                    label: "Hide NeoFlash".to_owned(),
                    role: NativeSystemItemRole::HideApp,
                    shortcut: Some(Shortcut::cmd(Key::H)),
                    shortcut_label: "⌘H".to_owned(),
                    enabled: true,
                },
                NativeMenuItemSpec::System {
                    label: "Hide Others".to_owned(),
                    role: NativeSystemItemRole::HideOthers,
                    shortcut: Some(Shortcut::alt_cmd(Key::H)),
                    shortcut_label: "⌥⌘H".to_owned(),
                    enabled: true,
                },
                NativeMenuItemSpec::System {
                    label: "Show All".to_owned(),
                    role: NativeSystemItemRole::ShowAll,
                    shortcut: None,
                    shortcut_label: String::new(),
                    enabled: true,
                },
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Quit NeoFlash",
                    Some(Shortcut::cmd(Key::Q)),
                    true,
                    false,
                    NativeMenuAction::Quit,
                ),
            ],
        }
    }

    fn bootstrap_window_menu_items() -> NativeTopMenuSpec {
        NativeTopMenuSpec {
            title: "Window".to_owned(),
            enabled: true,
            items: vec![
                NativeMenuItemSpec::System {
                    label: "Minimize".to_owned(),
                    role: NativeSystemItemRole::MinimizeWindow,
                    shortcut: Some(Shortcut::cmd(Key::M)),
                    shortcut_label: "⌘M".to_owned(),
                    enabled: true,
                },
                NativeMenuItemSpec::System {
                    label: "Zoom".to_owned(),
                    role: NativeSystemItemRole::ZoomWindow,
                    shortcut: None,
                    shortcut_label: String::new(),
                    enabled: true,
                },
            ],
        }
    }

    /// Install `NSApplication.mainMenu` ONCE with 11 persistent top-level `NSMenu` objects whose
    /// delegate is `NeoFlashMacMenuTarget` (`menuNeedsUpdate:`), pre-populating `NeoFlash` and
    /// `Window` so responder-chain chords (`⌘H`, `⌥⌘H`, `⌘M`, `⌘Q`) work immediately (`T-390`).
    fn ensure_persistent_main_menu(app: &NSApplication) -> Result<(), String> {
        if CACHE.with(|cache| cache.borrow().installed) {
            return Ok(());
        }
        let target_cls = ensure_target_class()?;
        let main_menu_cls = ensure_main_menu_class()?;
        let menu_cls =
            AnyClass::get(c"NSMenu").ok_or_else(|| "Could not find NSMenu class.".to_owned())?;
        let item_cls = AnyClass::get(c"NSMenuItem")
            .ok_or_else(|| "Could not find NSMenuItem class.".to_owned())?;

        let target = unsafe {
            let raw: *mut AnyObject = msg_send![target_cls, alloc];
            if raw.is_null() {
                return Err("Could not allocate NeoFlashMacMenuTarget instance.".into());
            }
            let init: *mut AnyObject = msg_send![raw, init];
            Retained::from_raw(init)
                .ok_or_else(|| "Could not init NeoFlashMacMenuTarget instance.".to_owned())?
        };
        let services_menu = alloc_menu(menu_cls, "Services")
            .ok_or_else(|| "Could not allocate Services NSMenu.".to_owned())?;
        let main_menu = alloc_menu(main_menu_cls, "MainMenu")
            .ok_or_else(|| "Could not allocate NeoFlashMainMenu.".to_owned())?;

        let mut top_items = Vec::with_capacity(TOP_MENU_COUNT);
        let mut top_menus = Vec::with_capacity(TOP_MENU_COUNT);
        for title in NativeMenuSpec::TOP_LEVEL_TITLES {
            let top_item = alloc_menu_item(item_cls, title, None, "")
                .ok_or_else(|| format!("Could not allocate top-level NSMenuItem {title}."))?;
            let _: () = unsafe { msg_send![&*top_item, setEnabled: Bool::YES] };
            let submenu = alloc_menu(menu_cls, title)
                .ok_or_else(|| format!("Could not allocate top-level NSMenu {title}."))?;
            let _: () = unsafe { msg_send![&*submenu, setDelegate: Some(&*target)] };
            let _: () = unsafe { msg_send![&*top_item, setSubmenu: Some(&*submenu)] };
            let _: () = unsafe { msg_send![&*main_menu, addItem: &*top_item] };
            top_items.push(top_item);
            top_menus.push(submenu);
        }

        // Pre-populate `NeoFlash` (idx 0) and `Window` (idx 9) so responder-chain shortcuts
        // (`⌘H`, `⌥⌘H`, `⌘M`, `⌘Q`) work immediately before the user ever opens a menu.
        let app_bootstrap = bootstrap_app_menu_items();
        rebuild_single_top_menu(&top_menus[0], 0, &app_bootstrap, &target, &services_menu);
        let window_bootstrap = bootstrap_window_menu_items();
        rebuild_single_top_menu(&top_menus[9], 9, &window_bootstrap, &target, &services_menu);

        let _: () = unsafe { msg_send![app, setServicesMenu: Some(&*services_menu)] };
        let _: () = unsafe { msg_send![app, setMainMenu: Some(&*main_menu)] };

        CACHE.with(|cache| {
            let mut state = cache.borrow_mut();
            state.installed = true;
            state.target = Some(target);
            state.services_menu = Some(services_menu);
            state.main_menu = Some(main_menu);
            state.top_items = top_items;
            state.top_menus = top_menus;
            state.top_enabled = [true; TOP_MENU_COUNT];
            state.dirty = [true; TOP_MENU_COUNT];
        });
        Ok(())
    }

    pub(super) fn sync(editor: &Editor, ctx: &egui::Context, win_state: &NativeWindowDocState) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        if ensure_persistent_main_menu(&app).is_err() {
            return;
        }

        // 1. Sync root NSWindow documentEdited + representedFilename only when changed (skipping
        //    even `find_root_window` + `String::clone` when the tracked root window matches).
        let tracked_root = crate::native_windows::root_window_number();
        let win_unchanged = tracked_root > 0
            && CACHE.with(|cache| {
                cache
                    .borrow()
                    .last_window_state
                    .as_ref()
                    .is_some_and(|(num, edited, path)| {
                        *num == tracked_root
                            && *edited == win_state.document_edited
                            && path == &win_state.represented_filename
                    })
            });
        if !win_unchanged && let Some(root) = find_root_window(&app) {
            let win_num = root.windowNumber();
            let needs_win_sync = CACHE.with(|cache| {
                cache
                    .borrow()
                    .last_window_state
                    .as_ref()
                    .is_none_or(|(num, edited, path)| {
                        *num != win_num
                            || *edited != win_state.document_edited
                            || path != &win_state.represented_filename
                    })
            });
            if needs_win_sync {
                root.setDocumentEdited(win_state.document_edited);
                if let Some(path_ns) = ns_string(&win_state.represented_filename) {
                    let _: () = unsafe { msg_send![&*root, setRepresentedFilename: &*path_ns] };
                }
                CACHE.with(|cache| {
                    cache.borrow_mut().last_window_state = Some((
                        win_num,
                        win_state.document_edited,
                        win_state.represented_filename.clone(),
                    ));
                });
            }
        }

        // 2. O(1) fingerprint check: skip `NativeMenuSpec::build` completely (`< 1 µs`) on idle,
        //    hover, pan, and mouse-move frames (`T-390`).
        let fingerprint = MenuSyncFingerprint::capture(editor, Some(ctx));
        if CACHE.with(|cache| cache.borrow().last_fingerprint == Some(fingerprint)) {
            return;
        }

        // 3. Store updated `NativeTopMenuSpec`s in `MainThreadCache` and mark ONLY changed top-level
        //    menus dirty for lazy `menuNeedsUpdate:` population — ZERO Objective-C item rebuilds
        //    during `Editor::update` and ZERO repeat `[app setMainMenu:]` calls!
        let spec = NativeMenuSpec::build(editor, Some(ctx));
        let mut enabled_updates: Vec<(Retained<AnyObject>, bool)> = Vec::new();
        CACHE.with(|cache| {
            let mut state = cache.borrow_mut();
            for (idx, top_spec) in spec.menus.into_iter().enumerate().take(TOP_MENU_COUNT) {
                if state.top_enabled[idx] != top_spec.enabled {
                    state.top_enabled[idx] = top_spec.enabled;
                    if let Some(item) = state.top_items.get(idx) {
                        enabled_updates.push((item.clone(), top_spec.enabled));
                    }
                }
                if state.specs[idx].as_ref() != Some(&top_spec) {
                    state.specs[idx] = Some(top_spec);
                    state.dirty[idx] = true;
                }
            }
            state.last_fingerprint = Some(fingerprint);
        });
        for (item, enabled) in enabled_updates {
            let _: () = unsafe { msg_send![&*item, setEnabled: Bool::new(enabled)] };
        }
    }
}

/// Register an `NSApplicationWillFinishLaunchingNotification` observer prior to `eframe::run_native`
/// so `application:openFile:` / `application:openFiles:` / `applicationShouldHandleReopen:hasVisibleWindows:`
/// are attached onto `WinitApplicationDelegate` BEFORE `[NSApplication finishLaunching]` dispatches
/// cold-launch Finder `kAEOpenDocuments` (`odoc`) AppleEvents.
pub(crate) fn install_prelaunch_hooks() {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        native::install_prelaunch_hooks();
    }
}

/// Install the macOS native menu bar and Finder/Dock application delegate hooks once on startup.
/// No-op on non-macOS builds and worker-thread unit tests.
pub(crate) fn install(ctx: &egui::Context) -> Result<(), String> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        native::install(ctx)
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        let _ = ctx;
        Ok(())
    }
}

/// Sync the macOS menu bar and root `NSWindow` document state when changed, returning the computed
/// [`NativeWindowDocState`] so the caller can update the viewport title from one source of truth.
pub(crate) fn sync(editor: &Editor, ctx: &egui::Context) -> NativeWindowDocState {
    let win_state = editor.native_window_state();
    #[cfg(all(target_os = "macos", not(test)))]
    {
        native::sync(editor, ctx, &win_state);
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        let _ = ctx;
    }
    win_state
}

#[cfg(test)]
mod t414_macos_menu_tests {
    use super::*;
    use neoflash_core::Project;

    fn collect_actions(
        items: &[NativeMenuItemSpec],
        out: &mut Vec<(String, NativeMenuAction, bool)>,
    ) {
        for item in items {
            match item {
                NativeMenuItemSpec::Action {
                    label,
                    enabled,
                    action,
                    ..
                } => out.push((label.clone(), action.clone(), *enabled)),
                NativeMenuItemSpec::Submenu { items, .. } => collect_actions(items, out),
                _ => {}
            }
        }
    }

    #[test]
    fn t414_standalone_as_session_disables_and_noops_authoring_native_menu_actions() {
        let tmp = std::env::temp_dir().join(format!(
            "neoflash-t414-menu-as-{}-{}",
            std::process::id(),
            neoflash_core::new_id("dir")
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let as_path = tmp.join("Main.as");
        std::fs::write(&as_path, "class Main {}\n").unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        editor.new_document_session(&ctx);
        // Detach Brushes on the authoring session first so `DockPanel(PanelId::Brushes)` is present in `Window ▸ Dock panel`.
        editor.set_panel_placement(&ctx, PanelId::Brushes, PanelPlacement::Detached);
        let fp_authoring = MenuSyncFingerprint::capture(&editor, Some(&ctx));
        assert!(!fp_authoring.standalone_script);

        assert!(editor.open_standalone_script_session(&ctx, as_path));
        assert_eq!(editor.session_count(), 2);
        assert!(editor.is_standalone_script_session());

        let fp_standalone = MenuSyncFingerprint::capture(&editor, Some(&ctx));
        assert!(fp_standalone.standalone_script);
        assert_ne!(fp_authoring, fp_standalone);

        let spec = NativeMenuSpec::build(&editor, Some(&ctx));
        let win_menu = spec.menu("Window").expect("Window menu");
        for sub_title in ["Detach panel", "Dock panel"] {
            assert!(
                win_menu.items.iter().any(|item| matches!(
                    item,
                    NativeMenuItemSpec::Submenu { label, enabled: false, .. } if label == sub_title
                )),
                "Window ▸ {sub_title} submenu must be disabled on standalone .as tabs"
            );
        }
        let mut flat = Vec::new();
        for menu in &spec.menus {
            collect_actions(&menu.items, &mut flat);
        }

        // Document lifecycle multi-tab actions remain enabled on standalone `.as` tabs.
        assert!(
            flat.iter()
                .any(|(_, act, en)| { matches!(act, NativeMenuAction::CloseAllDocuments) && *en }),
            "Close All must stay enabled on standalone .as tabs"
        );
        assert!(
            flat.iter().any(|(_, act, en)| {
                matches!(act, NativeMenuAction::CloseOtherDocuments) && *en
            }),
            "Close other documents must stay enabled when multiple tabs are open"
        );

        let authoring_actions = [
            NativeMenuAction::ImportToLibrary,
            NativeMenuAction::ImportPalette,
            NativeMenuAction::ImportVideo,
            NativeMenuAction::InsertEmptyVideoObject,
            NativeMenuAction::ExportImage,
            NativeMenuAction::ExportFlaProfile(FlaTargetProfile::AnimateCC),
            NativeMenuAction::PasteMotionSpecial,
            NativeMenuAction::EditSelectedSymbol,
            NativeMenuAction::EditSymbolInPlace,
            NativeMenuAction::SetMagnification(200),
            NativeMenuAction::ToggleGridOverObjects,
            NativeMenuAction::OpenViewSettings,
            NativeMenuAction::ToggleObjectSnap,
            NativeMenuAction::TogglePixelSnap,
            NativeMenuAction::ToggleOnionSkin,
            NativeMenuAction::ToggleOnionOutlines,
            NativeMenuAction::ToggleOnionAnchor,
            NativeMenuAction::SetStageToolbar(StageToolbarMode::ALL[0]),
            NativeMenuAction::DisableStageAb,
            NativeMenuAction::CombineObjects {
                op: BooleanOp::Union,
                crop: false,
            },
            NativeMenuAction::DistributeToKeyframes,
            NativeMenuAction::TweenToKeys(2),
            NativeMenuAction::ConvertToKeyframesStepped2s,
            NativeMenuAction::CopyKeyframeEase,
            NativeMenuAction::ApplyCopiedEase,
            NativeMenuAction::DeleteEmptyLayers,
            NativeMenuAction::RenameLayersFromContent,
            NativeMenuAction::AddActionsStopLayer,
            NativeMenuAction::RunJsflDialog,
            NativeMenuAction::OpenBatchRenameLibrary,
            NativeMenuAction::OpenBatchLinkage,
            NativeMenuAction::OpenBatchBitmapCompression,
            NativeMenuAction::DeleteUnusedLibraryItems,
            NativeMenuAction::OpenAutoInstanceNamer,
            NativeMenuAction::OpenRandomizeSelection,
            NativeMenuAction::PixelSnapSelection,
            NativeMenuAction::AlignSelection { axis: 0, edge: 0 },
            NativeMenuAction::ToggleAlignToStage,
            NativeMenuAction::ShowPanel(PanelId::Transform),
            NativeMenuAction::ShowPanel(PanelId::Align),
            NativeMenuAction::TextSetSize(24),
            NativeMenuAction::TextSetStyle {
                bold: true,
                italic: false,
            },
            NativeMenuAction::TextSetAlign(TextAlign::Center),
            NativeMenuAction::TextAdjustTracking(2),
            NativeMenuAction::TextOpenFontEmbedding,
            NativeMenuAction::ToggleLiveActionScript,
            NativeMenuAction::DebugContinueOrStart,
            NativeMenuAction::DebugStop,
            NativeMenuAction::DebugStepOver,
            NativeMenuAction::DebugStepInto,
            NativeMenuAction::DebugStepOut,
            NativeMenuAction::DebugToggleBreakpoint,
            NativeMenuAction::DebugClearBreakpoints,
            NativeMenuAction::DetachPanel(PanelId::Timeline),
            NativeMenuAction::DockPanel(PanelId::Brushes),
            NativeMenuAction::OpenCustomizeTools,
        ];

        for target in &authoring_actions {
            for (label, act, enabled) in &flat {
                if act == target {
                    assert!(
                        !*enabled,
                        "authoring menu item '{label}' ({act:?}) must be disabled on standalone .as tab"
                    );
                }
            }
        }

        let before_project = editor.project.clone();
        let before_zoom = editor.zoom;
        let before_onion = editor.onion;
        let before_text_size = editor.workspace.text_size;
        let before_toolbar = editor.workspace.stage_toolbar;
        assert!(!editor.dirty());
        assert!(!editor.history.can_undo());

        for action in authoring_actions {
            editor.execute_native_menu_action(&ctx, action);
        }
        editor.open_export_image(&ctx);
        assert!(
            !editor
                .export_fla_profile_dialog(FlaTargetProfile::AnimateCC, Some(tmp.join("out.fla")),)
        );
        assert!(!tmp.join("out.fla").exists());

        assert_eq!(editor.project, before_project);
        assert_eq!(editor.zoom, before_zoom);
        assert_eq!(editor.onion, before_onion);
        assert_eq!(editor.workspace.text_size, before_text_size);
        assert_eq!(editor.workspace.stage_toolbar, before_toolbar);
        assert_eq!(
            editor.workspace.placement(PanelId::Timeline),
            PanelPlacement::Docked
        );
        assert_eq!(
            editor.workspace.placement(PanelId::Brushes),
            PanelPlacement::Detached
        );
        assert!(!editor.customize_tools_open(&ctx));
        assert!(!editor.dirty(), "standalone .as tab must stay clean");
        assert!(
            !editor.history.can_undo(),
            "authoring actions must not push Undo entries onto standalone .as History"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
