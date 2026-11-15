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

/// Editor action dispatched when a native menu item is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeMenuAction {
    Command(CommandId),
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

fn cmd_item(editor: &Editor, ctx: Option<&egui::Context>, cmd: CommandId) -> NativeMenuItemSpec {
    let state = editor.command_menu_state(ctx, cmd);
    NativeMenuItemSpec::Action {
        label: state.label,
        command_id: Some(cmd),
        shortcut: state.shortcut,
        shortcut_label: state.shortcut_label,
        enabled: state.enabled,
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
        checked,
        tooltip: None,
        action,
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
    /// Derive the complete macOS menu bar specification from `editor` and the command registry.
    pub fn build(editor: &Editor, ctx: Option<&egui::Context>) -> Self {
        Self {
            menus: vec![
                Self::build_app_menu(editor, ctx),
                Self::build_file_menu(editor, ctx),
                Self::build_edit_menu(editor, ctx),
                Self::build_view_menu(editor, ctx),
                Self::build_insert_menu(editor, ctx),
                Self::build_modify_menu(editor, ctx),
                Self::build_text_menu(editor),
                Self::build_control_menu(editor, ctx),
                Self::build_debug_menu(editor, ctx),
                Self::build_window_menu(editor, ctx),
                Self::build_help_menu(editor, ctx),
            ],
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
        let recent = editor.workspace.general.existing_recent_files();
        let recent_labels = recent_file_display_labels(&recent);
        let mut recent_items = Vec::with_capacity(recent.len() + 2);
        for (path, label) in recent.iter().zip(recent_labels) {
            recent_items.push(NativeMenuItemSpec::Action {
                label,
                command_id: None,
                shortcut: None,
                shortcut_label: String::new(),
                enabled: true,
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
                    has_doc,
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
                has_doc,
                false,
                NativeMenuAction::ImportToLibrary,
            ),
            custom_item(
                "Import colour palette…",
                None,
                has_doc,
                false,
                NativeMenuAction::ImportPalette,
            ),
            custom_item(
                "Import video…",
                None,
                has_doc,
                false,
                NativeMenuAction::ImportVideo,
            ),
            custom_item(
                "Empty video object",
                None,
                has_doc && editor.layer_editable(),
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
                has_doc,
                false,
                NativeMenuAction::ExportImage,
            ),
            cmd_item(editor, ctx, CommandId::FileExportVideo),
            cmd_item(editor, ctx, CommandId::FileExportAudio),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::FileExportLottie),
            cmd_item(editor, ctx, CommandId::FileExportAfterEffects),
            cmd_item(editor, ctx, CommandId::FileExportTextureAtlas),
            cmd_item(editor, ctx, CommandId::FileExportSwf),
            NativeMenuItemSpec::Submenu {
                label: "Export SWF Target".to_owned(),
                enabled: has_doc,
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
                enabled: has_doc,
                role: None,
                items: fla_items,
            },
        ];

        NativeTopMenuSpec {
            title: "File".to_owned(),
            enabled: true,
            items: vec![
                cmd_item(editor, ctx, CommandId::FileNew),
                cmd_item(editor, ctx, CommandId::FileOpen),
                NativeMenuItemSpec::Submenu {
                    label: "Open Recent".to_owned(),
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
                    enabled: has_doc,
                    role: None,
                    items: export_items,
                },
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::FilePublishSettings),
                cmd_item(editor, ctx, CommandId::FilePublish),
            ],
        }
    }

    fn build_edit_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let has_instance = has_doc && editor.selected_instance_symbol().is_some();
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
                has_doc && editor.can_paste_motion(),
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
                enabled: has_doc,
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

        NativeTopMenuSpec {
            title: "Edit".to_owned(),
            enabled: true,
            items,
        }
    }

    fn build_view_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
        let v = &editor.stage_view.prefs;
        let zoom_pct = (editor.zoom * 100.0).round() as u32;
        let ab_mode = editor.stage_ab_mode();

        let magnification_items = vec![
            custom_item(
                "25 %",
                None,
                has_doc,
                has_doc && zoom_pct == 25,
                NativeMenuAction::SetMagnification(25),
            ),
            custom_item(
                "50 %",
                None,
                has_doc,
                has_doc && zoom_pct == 50,
                NativeMenuAction::SetMagnification(50),
            ),
            cmd_item(editor, ctx, CommandId::ViewZoom100),
            custom_item(
                "200 %",
                None,
                has_doc,
                has_doc && zoom_pct == 200,
                NativeMenuAction::SetMagnification(200),
            ),
            custom_item(
                "400 %",
                None,
                has_doc,
                has_doc && zoom_pct == 400,
                NativeMenuAction::SetMagnification(400),
            ),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::ViewFitInWindow),
        ];

        let grid_items = vec![
            cmd_item(editor, ctx, CommandId::ViewGridShow),
            cmd_item(editor, ctx, CommandId::ViewGridSnap),
            custom_item(
                "Show over objects",
                None,
                has_doc,
                has_doc && v.grid_over_objects,
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
                has_doc,
                false,
                NativeMenuAction::OpenViewSettings,
            ),
        ];

        let snapping_items = vec![
            custom_item(
                "Snap to objects",
                None,
                has_doc,
                has_doc && v.objects_snap,
                NativeMenuAction::ToggleObjectSnap,
            ),
            custom_item(
                "Snap to pixels",
                None,
                has_doc,
                has_doc && v.pixels_snap,
                NativeMenuAction::TogglePixelSnap,
            ),
            cmd_item_labeled(editor, ctx, "Snap to grid", CommandId::ViewGridSnap),
            cmd_item_labeled(editor, ctx, "Snap to guides", CommandId::ViewGuidesSnap),
        ];

        let onion_items = vec![
            custom_item(
                "Onion skin",
                None,
                has_doc,
                has_doc && editor.onion,
                NativeMenuAction::ToggleOnionSkin,
            ),
            custom_item(
                "Onion skin outlines",
                None,
                has_doc,
                has_doc && editor.onion && editor.onion_outlines,
                NativeMenuAction::ToggleOnionOutlines,
            ),
            custom_item(
                "Anchor onion markers",
                None,
                has_doc,
                has_doc && editor.onion_anchor,
                NativeMenuAction::ToggleOnionAnchor,
            ),
        ];

        let stage_toolbar_items: Vec<NativeMenuItemSpec> = StageToolbarMode::ALL
            .into_iter()
            .map(|mode| {
                custom_item(
                    mode.label(),
                    None,
                    has_doc,
                    has_doc && editor.workspace.stage_toolbar == mode,
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
                has_doc && ab_mode != StageAbComparisonMode::Off,
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
                    enabled: has_doc,
                    role: None,
                    items: magnification_items,
                },
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ViewRulers),
                NativeMenuItemSpec::Submenu {
                    label: "Grid".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: grid_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Guides".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: guides_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Snapping".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: snapping_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Onion Skin".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: onion_items,
                },
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Rulers, guides & grid…",
                    None,
                    has_doc,
                    false,
                    NativeMenuAction::OpenViewSettings,
                ),
                cmd_item(editor, ctx, CommandId::ViewShowShapeHints),
                NativeMenuItemSpec::Submenu {
                    label: "Stage toolbar".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: stage_toolbar_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Stage A/B".to_owned(),
                    enabled: has_doc,
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
        let can_combine = has_doc && editor.can_combine_objects();
        let can_stepped = has_doc && editor.can_convert_to_keyframes_stepped();
        let can_align = has_doc
            && (editor.selection.len() >= 2
                || (!editor.selection.is_empty() && editor.authoring_modify.to_stage));

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
                has_doc && editor.can_distribute_to_keyframes(),
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
                has_doc && editor.can_copy_active_keyframe_ease(),
                false,
                NativeMenuAction::CopyKeyframeEase,
            ),
            custom_item(
                "Apply copied ease",
                None,
                has_doc && editor.has_copied_keyframe_ease(),
                false,
                NativeMenuAction::ApplyCopiedEase,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Delete empty layers",
                None,
                has_doc,
                false,
                NativeMenuAction::DeleteEmptyLayers,
            ),
            custom_item(
                "Rename layers from content",
                None,
                has_doc,
                false,
                NativeMenuAction::RenameLayersFromContent,
            ),
            custom_item(
                "Add actions & stop() layer",
                None,
                has_doc,
                false,
                NativeMenuAction::AddActionsStopLayer,
            ),
        ];

        let jsfl_items = vec![
            custom_item(
                "Run JSFL script (.jsfl)…",
                None,
                has_doc,
                false,
                NativeMenuAction::RunJsflDialog,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Batch rename library items…",
                None,
                has_doc,
                false,
                NativeMenuAction::OpenBatchRenameLibrary,
            ),
            custom_item(
                "Batch linkage (Export for ActionScript)…",
                None,
                has_doc,
                false,
                NativeMenuAction::OpenBatchLinkage,
            ),
            custom_item(
                "Batch bitmap compression & smoothing…",
                None,
                has_doc,
                false,
                NativeMenuAction::OpenBatchBitmapCompression,
            ),
            custom_item(
                "Delete unused library items",
                None,
                has_doc,
                false,
                NativeMenuAction::DeleteUnusedLibraryItems,
            ),
            NativeMenuItemSpec::Separator,
            custom_item(
                "Auto Instance Namer…",
                None,
                has_doc,
                false,
                NativeMenuAction::OpenAutoInstanceNamer,
            ),
            custom_item(
                "Randomize Selection…",
                None,
                has_doc,
                false,
                NativeMenuAction::OpenRandomizeSelection,
            ),
            custom_item(
                "Pixel snap (round to whole pixels)",
                None,
                has_doc && !editor.editable_targets().is_empty(),
                false,
                NativeMenuAction::PixelSnapSelection,
            ),
        ];

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
            cmd_item_labeled(editor, ctx, "Transform panel", CommandId::WindowTransform),
        ];

        let arrange_items = vec![
            cmd_item(editor, ctx, CommandId::ModifyArrangeFront),
            cmd_item(editor, ctx, CommandId::ModifyArrangeForward),
            cmd_item(editor, ctx, CommandId::ModifyArrangeBackward),
            cmd_item(editor, ctx, CommandId::ModifyArrangeBack),
        ];

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
            custom_item(
                "To stage",
                None,
                has_doc,
                has_doc && editor.authoring_modify.to_stage,
                NativeMenuAction::ToggleAlignToStage,
            ),
            cmd_item_labeled(editor, ctx, "Align panel", CommandId::WindowAlign),
        ];

        NativeTopMenuSpec {
            title: "Modify".to_owned(),
            enabled: true,
            items: vec![
                cmd_item(editor, ctx, CommandId::ModifyDocument),
                NativeMenuItemSpec::Separator,
                cmd_item(editor, ctx, CommandId::ModifyConvertToSymbol),
                cmd_item(editor, ctx, CommandId::ModifyBreakApart),
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Bitmap".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: bitmap_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Shape".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: shape_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Combine objects".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: combine_items,
                },
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Timeline".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: timeline_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Commands & JSFL".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: jsfl_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Transform".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: transform_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Arrange".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: arrange_items,
                },
                NativeMenuItemSpec::Submenu {
                    label: "Align".to_owned(),
                    enabled: has_doc,
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
        let selected_text = if has_doc && editor.selection.len() == 1 {
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
        let can_set_size = has_doc && (has_text_el || editor.tool == crate::app::Tool::Text);

        let size_items: Vec<NativeMenuItemSpec> = TEXT_MENU_SIZES
            .iter()
            .copied()
            .map(|pt| {
                custom_item(
                    format!("{pt} pt"),
                    None,
                    can_set_size,
                    can_set_size && (cur_size - f64::from(pt)).abs() < 0.05,
                    NativeMenuAction::TextSetSize(pt),
                )
            })
            .collect();

        let style_items = vec![
            custom_item(
                "Regular",
                None,
                has_text_box,
                has_text_box && !bold && !italic,
                NativeMenuAction::TextSetStyle {
                    bold: false,
                    italic: false,
                },
            ),
            custom_item(
                "Bold",
                None,
                has_text_box,
                has_text_box && bold && !italic,
                NativeMenuAction::TextSetStyle {
                    bold: true,
                    italic: false,
                },
            ),
            custom_item(
                "Italic",
                None,
                has_text_box,
                has_text_box && !bold && italic,
                NativeMenuAction::TextSetStyle {
                    bold: false,
                    italic: true,
                },
            ),
            custom_item(
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
            custom_item(
                "Align Left",
                None,
                has_text_box,
                has_text_box && align == TextAlign::Left,
                NativeMenuAction::TextSetAlign(TextAlign::Left),
            ),
            custom_item(
                "Align Center",
                None,
                has_text_box,
                has_text_box && align == TextAlign::Center,
                NativeMenuAction::TextSetAlign(TextAlign::Center),
            ),
            custom_item(
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
                    has_doc,
                    false,
                    NativeMenuAction::TextOpenFontEmbedding,
                ),
            ],
        }
    }

    fn build_control_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
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
                custom_item(
                    "Live ActionScript",
                    None,
                    has_doc,
                    has_doc && editor.live_script_preview,
                    NativeMenuAction::ToggleLiveActionScript,
                ),
                NativeMenuItemSpec::Separator,
                NativeMenuItemSpec::Submenu {
                    label: "Work Area".to_owned(),
                    enabled: has_doc,
                    role: None,
                    items: work_area_items,
                },
            ],
        }
    }

    fn build_debug_menu(editor: &Editor, ctx: Option<&egui::Context>) -> NativeTopMenuSpec {
        let has_doc = editor.has_document();
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
                    has_doc && (paused || idle_ready),
                    false,
                    NativeMenuAction::DebugContinueOrStart,
                ),
                custom_item(
                    "Stop Debugging",
                    None,
                    has_doc && !idle_ready,
                    false,
                    NativeMenuAction::DebugStop,
                ),
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Step Over",
                    None,
                    has_doc && paused,
                    false,
                    NativeMenuAction::DebugStepOver,
                ),
                custom_item(
                    "Step In",
                    None,
                    has_doc && paused,
                    false,
                    NativeMenuAction::DebugStepInto,
                ),
                custom_item(
                    "Step Out",
                    None,
                    has_doc && paused,
                    false,
                    NativeMenuAction::DebugStepOut,
                ),
                NativeMenuItemSpec::Separator,
                custom_item(
                    "Toggle Breakpoint",
                    None,
                    has_doc && (snapshot.is_some() || idle_ready),
                    false,
                    NativeMenuAction::DebugToggleBreakpoint,
                ),
                custom_item(
                    "Remove All Breakpoints",
                    None,
                    has_doc
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
            workspace_items.push(custom_item(
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
                workspace_items.push(custom_item(
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
                custom_item(
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
                custom_item(
                    density.label(),
                    None,
                    true,
                    editor.workspace.density == density,
                    NativeMenuAction::SetDensity(density),
                )
            })
            .collect();

        let detach_items: Vec<NativeMenuItemSpec> = NATIVE_WINDOW_PANELS
            .iter()
            .filter(|(_, panel)| editor.workspace.placement(*panel) == PanelPlacement::Docked)
            .map(|&(name, panel)| {
                custom_item(
                    name,
                    None,
                    true,
                    false,
                    NativeMenuAction::DetachPanel(panel),
                )
            })
            .collect();
        let dock_items: Vec<NativeMenuItemSpec> = NATIVE_WINDOW_PANELS
            .iter()
            .filter(|(_, panel)| editor.workspace.placement(*panel) == PanelPlacement::Detached)
            .map(|&(name, panel)| {
                custom_item(name, None, true, false, NativeMenuAction::DockPanel(panel))
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
                enabled: !detach_items.is_empty(),
                role: None,
                items: detach_items,
            },
            NativeMenuItemSpec::Submenu {
                label: "Dock panel".to_owned(),
                enabled: !dock_items.is_empty(),
                role: None,
                items: dock_items,
            },
            custom_item(
                "Customize tools…",
                None,
                true,
                false,
                NativeMenuAction::OpenCustomizeTools,
            ),
            custom_item(
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
            cmd_item_labeled(
                editor,
                ctx,
                "ActionScript Reference",
                CommandId::HelpScriptingReference,
            ),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::HelpOpenExample),
            NativeMenuItemSpec::Separator,
            cmd_item(editor, ctx, CommandId::HelpCheckForUpdates),
        ];
        if crate::panels::beta_feedback::is_beta_build() {
            items.push(cmd_item(editor, ctx, CommandId::HelpSendBetaFeedback));
        }
        NativeTopMenuSpec {
            title: "Help".to_owned(),
            enabled: true,
            items,
        }
    }
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
            .map(crate::recovery::canonical_or_normalized_path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        NativeWindowDocState {
            title,
            document_edited: dirty,
            represented_filename,
        }
    }

    /// Execute any [`NativeMenuAction`] against the live [`Editor`].
    pub fn execute_native_menu_action(&mut self, ctx: &egui::Context, action: NativeMenuAction) {
        match action {
            NativeMenuAction::Command(cmd) => {
                let _ = self.execute_menu_command(ctx, cmd);
            }
            NativeMenuAction::About => {
                self.show_about = true;
            }
            NativeMenuAction::Quit => {
                self.request_quit(ctx);
            }
            NativeMenuAction::OpenRecent(path) => {
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
                if self.has_document() {
                    self.import_to_library_dialog();
                }
            }
            NativeMenuAction::ImportPalette => {
                if self.has_document() {
                    self.import_palette_dialog();
                }
            }
            NativeMenuAction::ImportVideo => {
                if self.has_document() {
                    self.open_import_video(ctx);
                }
            }
            NativeMenuAction::InsertEmptyVideoObject => {
                if self.has_document() && self.layer_editable() {
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
                if self.has_document() {
                    self.open_export_image(ctx);
                }
            }
            NativeMenuAction::ExportFlaProfile(profile) => {
                if self.has_document() {
                    self.export_fla_profile_dialog(profile, None);
                }
            }
            NativeMenuAction::PasteMotionSpecial => {
                if self.has_document() && self.can_paste_motion() {
                    self.open_paste_motion_special();
                }
            }
            NativeMenuAction::EditSelectedSymbol => {
                if self.has_document()
                    && let Some(id) = self.selected_instance_symbol()
                {
                    self.enter_symbol_isolated(id);
                }
            }
            NativeMenuAction::EditSymbolInPlace => {
                if self.has_document() && self.selected_instance_symbol().is_some() {
                    self.enter_selected_symbol_in_place();
                }
            }
            NativeMenuAction::SetMagnification(percent) => {
                if self.has_document() {
                    self.finish_edits();
                    self.zoom = (percent as f32 / 100.0).clamp(0.05, 64.0);
                    self.mark_manual_stage_view();
                }
            }
            NativeMenuAction::ToggleGridOverObjects => {
                if self.has_document() {
                    self.toggle_grid_over_objects();
                }
            }
            NativeMenuAction::OpenViewSettings => {
                if self.has_document() {
                    self.open_view_settings();
                }
            }
            NativeMenuAction::ToggleObjectSnap => {
                if self.has_document() {
                    self.toggle_object_snap();
                }
            }
            NativeMenuAction::TogglePixelSnap => {
                if self.has_document() {
                    self.toggle_pixel_snap();
                }
            }
            NativeMenuAction::ToggleOnionSkin => {
                if self.has_document() {
                    self.onion = !self.onion;
                }
            }
            NativeMenuAction::ToggleOnionOutlines => {
                if self.has_document() {
                    if !self.onion {
                        self.onion = true;
                        self.onion_outlines = true;
                    } else {
                        self.onion_outlines = !self.onion_outlines;
                    }
                }
            }
            NativeMenuAction::ToggleOnionAnchor => {
                if self.has_document() {
                    self.onion_anchor = !self.onion_anchor;
                }
            }
            NativeMenuAction::SetStageToolbar(mode) => {
                self.workspace.stage_toolbar = mode;
            }
            NativeMenuAction::DisableStageAb => {
                if self.has_document() {
                    self.set_stage_ab_mode(StageAbComparisonMode::Off);
                }
            }
            NativeMenuAction::CombineObjects { op, crop } => {
                if self.has_document() && self.can_combine_objects() {
                    self.combine_objects_selection(op, crop);
                }
            }
            NativeMenuAction::DistributeToKeyframes => {
                if self.has_document() && self.can_distribute_to_keyframes() {
                    let _ = self.distribute_selection_to_keyframes();
                }
            }
            NativeMenuAction::TweenToKeys(step) => {
                if self.has_document() && self.can_convert_to_keyframes_stepped() {
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
            NativeMenuAction::CopyKeyframeEase => {
                if self.has_document() && self.can_copy_active_keyframe_ease() {
                    let _ = self.copy_active_keyframe_ease();
                }
            }
            NativeMenuAction::ApplyCopiedEase => {
                if self.has_document() && self.has_copied_keyframe_ease() {
                    let _ = self.apply_copied_ease_to_selection();
                }
            }
            NativeMenuAction::DeleteEmptyLayers => {
                if self.has_document() {
                    let _ = self.delete_empty_layers();
                }
            }
            NativeMenuAction::RenameLayersFromContent => {
                if self.has_document() {
                    let _ = self.rename_layers_from_content();
                }
            }
            NativeMenuAction::AddActionsStopLayer => {
                if self.has_document() {
                    let _ = self.add_actions_and_stop_layer();
                }
            }
            NativeMenuAction::RunJsflDialog => {
                if self.has_document() {
                    let _ = self.run_jsfl_script_dialog(None);
                }
            }
            NativeMenuAction::OpenBatchRenameLibrary => {
                if self.has_document() {
                    self.open_batch_rename_library_modal(ctx);
                }
            }
            NativeMenuAction::OpenBatchLinkage => {
                if self.has_document() {
                    self.open_batch_linkage_modal(ctx);
                }
            }
            NativeMenuAction::OpenBatchBitmapCompression => {
                if self.has_document() {
                    self.open_batch_bitmap_compression_modal(ctx);
                }
            }
            NativeMenuAction::DeleteUnusedLibraryItems => {
                if self.has_document() {
                    self.find_and_delete_unused_library_items();
                }
            }
            NativeMenuAction::OpenAutoInstanceNamer => {
                if self.has_document() {
                    self.open_auto_instance_namer_modal(ctx);
                }
            }
            NativeMenuAction::OpenRandomizeSelection => {
                if self.has_document() {
                    self.open_randomize_selection_modal(ctx);
                }
            }
            NativeMenuAction::PixelSnapSelection => {
                if self.has_document() && !self.editable_targets().is_empty() {
                    let _ = self.pixel_snap_selection();
                }
            }
            NativeMenuAction::AlignSelection { axis, edge } => {
                if self.has_document() {
                    self.align_selection(ctx, axis, edge);
                }
            }
            NativeMenuAction::ToggleAlignToStage => {
                if self.has_document() {
                    self.authoring_modify.to_stage = !self.authoring_modify.to_stage;
                }
            }
            NativeMenuAction::TextSetSize(pt) => {
                let size = f64::from(pt).clamp(1.0, 10000.0);
                self.workspace.text_size = size;
                if self.has_document() && self.selection.len() == 1 && self.layer_editable() {
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
            NativeMenuAction::TextSetStyle { bold, italic } => {
                if self.has_document() && self.selection.len() == 1 && self.layer_editable() {
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
                if self.has_document() && self.selection.len() == 1 && self.layer_editable() {
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
                if self.has_document() && self.selection.len() == 1 && self.layer_editable() {
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
                if self.has_document() {
                    self.open_font_embedding(ctx);
                }
            }
            NativeMenuAction::ToggleLiveActionScript => {
                if self.has_document() {
                    self.set_live_script_preview(!self.live_script_preview);
                }
            }
            NativeMenuAction::DebugContinueOrStart => {
                if self.has_document() {
                    if self.debugger.snapshot().is_some() {
                        let _ = self.debug_command(DebugCommand::Resume);
                    } else {
                        self.start_debug_movie(ctx);
                    }
                }
            }
            NativeMenuAction::DebugStop => {
                self.debugger.stop();
            }
            NativeMenuAction::DebugStepOver => {
                let _ = self.debug_command(DebugCommand::StepOver);
            }
            NativeMenuAction::DebugStepInto => {
                let _ = self.debug_command(DebugCommand::StepInto);
            }
            NativeMenuAction::DebugStepOut => {
                let _ = self.debug_command(DebugCommand::StepOut);
            }
            NativeMenuAction::DebugToggleBreakpoint => {
                if self.has_document() {
                    self.toggle_debug_breakpoint(ctx);
                }
            }
            NativeMenuAction::DebugClearBreakpoints => {
                if self.has_document() {
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
                self.set_panel_placement(ctx, panel, PanelPlacement::Detached);
            }
            NativeMenuAction::DockPanel(panel) => {
                self.set_panel_placement(ctx, panel, PanelPlacement::Docked);
            }
            NativeMenuAction::OpenCustomizeTools => {
                self.open_customize_tools(ctx);
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
}

/// Drain pending Finder open-file, Dock reopen, and clicked native menu actions on the root frame.
pub(crate) fn drain_events(editor: &mut Editor, ctx: &egui::Context) {
    if let Ok(mut queue) = global_lifecycle_queue().lock() {
        let _ = queue.drain_into(editor, ctx);
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

    static CONTEXT: Mutex<Option<egui::Context>> = Mutex::new(None);
    static ACTION_TABLE: OnceLock<Mutex<Vec<(NativeMenuAction, bool)>>> = OnceLock::new();

    struct MainThreadCache {
        target: Option<Retained<AnyObject>>,
        services_menu: Option<Retained<AnyObject>>,
        last_menu_spec: Option<NativeMenuSpec>,
        last_window_state: Option<(isize, bool, String)>,
    }

    thread_local! {
        static CACHE: RefCell<MainThreadCache> = const {
            RefCell::new(MainThreadCache {
                target: None,
                services_menu: None,
                last_menu_spec: None,
                last_window_state: None,
            })
        };
    }

    fn action_table() -> &'static Mutex<Vec<(NativeMenuAction, bool)>> {
        ACTION_TABLE.get_or_init(|| Mutex::new(Vec::new()))
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
    type PerformKeyEquivalentFn =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&NSEvent>) -> Bool;

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
        if tag < 0 {
            return;
        }
        let found = action_table()
            .lock()
            .ok()
            .and_then(|table| table.get(tag as usize).cloned());
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
        if tag >= 0
            && let Some(enabled) = action_table()
                .lock()
                .ok()
                .and_then(|table| table.get(tag as usize).map(|(_, enabled)| *enabled))
        {
            return Bool::new(enabled);
        }
        unsafe { msg_send![menu_item, isEnabled] }
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

    pub(super) fn install(ctx: &egui::Context) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "Native macOS menu bridge must be installed on the main thread.".to_owned()
        })?;
        let _ = ensure_target_class()?;
        let _ = ensure_main_menu_class()?;

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

        *CONTEXT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ctx.clone());
        Ok(())
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

    fn populate_menu(
        menu: &AnyObject,
        menu_cls: &AnyClass,
        item_cls: &AnyClass,
        target: &AnyObject,
        services_menu: &Retained<AnyObject>,
        items: &[NativeMenuItemSpec],
        actions: &mut Vec<(NativeMenuAction, bool)>,
    ) {
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
                    let tag = actions.len() as isize;
                    actions.push((action.clone(), *enabled));
                    let _: () = unsafe { msg_send![&*item, setTag: tag] };
                    let _: () = unsafe { msg_send![&*item, setTarget: Some(target)] };
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
                            unsafe { msg_send![&*item, setSubmenu: Some(&**services_menu)] };
                    } else if let Some(sub) = alloc_menu(menu_cls, label) {
                        populate_menu(
                            &sub,
                            menu_cls,
                            item_cls,
                            target,
                            services_menu,
                            sub_items,
                            actions,
                        );
                        let _: () = unsafe { msg_send![&*item, setSubmenu: Some(&*sub)] };
                    }
                    let _: () = unsafe { msg_send![menu, addItem: &*item] };
                }
            }
        }
    }

    pub(super) fn sync(editor: &Editor, ctx: &egui::Context, win_state: &NativeWindowDocState) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);

        // 1. Sync root NSWindow documentEdited + representedFilename only when changed.
        if let Some(root) = find_root_window(&app) {
            let win_num = root.windowNumber();
            let key = (
                win_num,
                win_state.document_edited,
                win_state.represented_filename.clone(),
            );
            let needs_win_sync =
                CACHE.with(|cache| cache.borrow().last_window_state.as_ref() != Some(&key));
            if needs_win_sync {
                root.setDocumentEdited(win_state.document_edited);
                if let Some(path_ns) = ns_string(&win_state.represented_filename) {
                    let _: () = unsafe { msg_send![&*root, setRepresentedFilename: &*path_ns] };
                }
                CACHE.with(|cache| {
                    cache.borrow_mut().last_window_state = Some(key);
                });
            }
        }

        // 2. Build NativeMenuSpec and skip Objective-C rebuild if unchanged.
        let spec = NativeMenuSpec::build(editor, Some(ctx));
        let unchanged = CACHE.with(|cache| cache.borrow().last_menu_spec.as_ref() == Some(&spec));
        if unchanged {
            return;
        }

        let (Ok(target_cls), Ok(main_menu_cls)) = (ensure_target_class(), ensure_main_menu_class())
        else {
            return;
        };
        let (Some(menu_cls), Some(item_cls)) =
            (AnyClass::get(c"NSMenu"), AnyClass::get(c"NSMenuItem"))
        else {
            return;
        };

        let (target, services_menu, first_services_install) = CACHE.with(|cache| {
            let mut state = cache.borrow_mut();
            let target = if let Some(t) = &state.target {
                t.clone()
            } else {
                let Some(created) = (unsafe {
                    let raw: *mut AnyObject = msg_send![target_cls, alloc];
                    if raw.is_null() {
                        None
                    } else {
                        let init: *mut AnyObject = msg_send![raw, init];
                        Retained::from_raw(init)
                    }
                }) else {
                    return (None, None, false);
                };
                state.target = Some(created.clone());
                created
            };
            let mut first_services = false;
            let services = if let Some(s) = &state.services_menu {
                s.clone()
            } else {
                let Some(created) = alloc_menu(menu_cls, "Services") else {
                    return (Some(target), None, false);
                };
                state.services_menu = Some(created.clone());
                first_services = true;
                created
            };
            (Some(target), Some(services), first_services)
        });

        let (Some(target), Some(services_menu)) = (target, services_menu) else {
            return;
        };

        // Detach the shared Services submenu from any previous parent item before re-attaching it.
        let prev_parent: *mut AnyObject = unsafe { msg_send![&*services_menu, supermenu] };
        if !prev_parent.is_null() {
            let count: isize = unsafe { msg_send![prev_parent, numberOfItems] };
            for idx in 0..count.max(0) {
                let item: *mut AnyObject = unsafe { msg_send![prev_parent, itemAtIndex: idx] };
                if !item.is_null() {
                    let sub: *const AnyObject = unsafe { msg_send![item, submenu] };
                    if std::ptr::eq(sub, &*services_menu as *const AnyObject) {
                        let _: () = unsafe { msg_send![item, setSubmenu: None::<&AnyObject>] };
                        break;
                    }
                }
            }
        }

        let Some(main_menu) = alloc_menu(main_menu_cls, "MainMenu") else {
            return;
        };
        let mut new_actions = Vec::with_capacity(192);
        for top in &spec.menus {
            let Some(top_item) = alloc_menu_item(item_cls, &top.title, None, "") else {
                continue;
            };
            let _: () = unsafe { msg_send![&*top_item, setEnabled: Bool::new(top.enabled)] };
            let Some(submenu) = alloc_menu(menu_cls, &top.title) else {
                continue;
            };
            populate_menu(
                &submenu,
                menu_cls,
                item_cls,
                &target,
                &services_menu,
                &top.items,
                &mut new_actions,
            );
            let _: () = unsafe { msg_send![&*top_item, setSubmenu: Some(&*submenu)] };
            let _: () = unsafe { msg_send![&*main_menu, addItem: &*top_item] };
        }

        if let Ok(mut table) = action_table().lock() {
            *table = new_actions;
        }
        if first_services_install {
            let _: () = unsafe { msg_send![&*app, setServicesMenu: Some(&*services_menu)] };
        }
        let _: () = unsafe { msg_send![&*app, setMainMenu: Some(&*main_menu)] };

        CACHE.with(|cache| {
            cache.borrow_mut().last_menu_spec = Some(spec);
        });
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
