use crate::app::{Editor, Pending};
use crate::commands::{CommandId, Shortcut};
use crate::macos_menu::{
    MACOS_MOD_COMMAND, MACOS_MOD_OPTION, MACOS_MOD_SHIFT, NativeLifecycleQueue, NativeMenuAction,
    NativeMenuItemSpec, NativeMenuSpec, NativeSubmenuRole, NativeSystemItemRole,
    macos_key_equivalent, should_route_key_equivalent_via_appkit, startup_file_args_from,
};
use crate::theme::ThemeMode;
use crate::workspace::{DensityPref, PanelId, WorkspaceTemplate};
use eframe::egui::{self, Key};
use neoflash_core::Project;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn setup(name: &str) -> (Editor, egui::Context, PathBuf) {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/t309-macos-menu")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    editor.recovery.set_dir(Some(dir.join("Recovery")));
    (editor, ctx, dir)
}

fn collect_command_items<'a>(
    items: &'a [NativeMenuItemSpec],
    out: &mut Vec<(CommandId, &'a NativeMenuItemSpec)>,
) {
    for item in items {
        match item {
            NativeMenuItemSpec::Action {
                command_id: Some(cmd),
                ..
            } => out.push((*cmd, item)),
            NativeMenuItemSpec::Submenu { items: sub, .. } => collect_command_items(sub, out),
            _ => {}
        }
    }
}

fn find_submenu<'a>(
    items: &'a [NativeMenuItemSpec],
    label: &str,
) -> Option<&'a [NativeMenuItemSpec]> {
    for item in items {
        if let NativeMenuItemSpec::Submenu {
            label: found,
            items: sub,
            ..
        } = item
            && found == label
        {
            return Some(sub);
        }
    }
    None
}

#[test]
fn macos_menu_structure_covers_all_app_and_authoring_menus_and_services() {
    let (editor, ctx, _dir) = setup("structure");
    let spec = NativeMenuSpec::build(&editor, Some(&ctx));

    let titles: Vec<&str> = spec.menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "NeoFlash", "File", "Edit", "View", "Insert", "Modify", "Text", "Control", "Debug",
            "Window", "Help"
        ]
    );

    let app_menu = spec.menu("NeoFlash").expect("NeoFlash app menu");
    assert!(app_menu.items.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Action {
            label,
            action: NativeMenuAction::About,
            enabled: true,
            ..
        } if label == "About NeoFlash"
    )));
    assert!(app_menu.items.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Submenu {
            label,
            role: Some(NativeSubmenuRole::Services),
            enabled: true,
            ..
        } if label == "Services"
    )));
    for expected_role in [
        NativeSystemItemRole::HideApp,
        NativeSystemItemRole::HideOthers,
        NativeSystemItemRole::ShowAll,
    ] {
        assert!(
            app_menu.items.iter().any(|item| matches!(
                item,
                NativeMenuItemSpec::System { role, enabled: true, .. } if *role == expected_role
            )),
            "missing system role {expected_role:?} in NeoFlash menu"
        );
    }
    assert!(app_menu.items.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Action {
            label,
            shortcut,
            action: NativeMenuAction::Quit,
            enabled: true,
            ..
        } if label == "Quit NeoFlash" && *shortcut == Some(Shortcut::cmd(Key::Q))
    )));

    let window_menu = spec.menu("Window").expect("Window menu");
    for expected_role in [
        NativeSystemItemRole::MinimizeWindow,
        NativeSystemItemRole::ZoomWindow,
    ] {
        assert!(
            window_menu.items.iter().any(|item| matches!(
                item,
                NativeMenuItemSpec::System { role, enabled: true, .. } if *role == expected_role
            )),
            "missing window system role {expected_role:?}"
        );
    }

    let help_menu = spec.menu("Help").expect("Help menu");
    for expected_label in [
        "Search functions…",
        "NeoFlash Help",
        "Scripting Reference…",
        "Open example",
        "Check for Updates…",
        "About NeoFlash",
    ] {
        assert!(
            help_menu.items.iter().any(|item| matches!(
                item,
                NativeMenuItemSpec::Action { label, enabled: true, .. } if label == expected_label
            )),
            "missing Help item {expected_label:?}"
        );
    }
}

#[test]
fn macos_menu_single_source_of_truth_matches_command_registry_and_custom_shortcuts() {
    let (mut editor, ctx, _dir) = setup("registry-parity");

    // Check parity with active document + selection, and again with no document open.
    editor.select_all();
    for pass in 0..2 {
        if pass == 1 {
            editor.sessions.clear();
        }
        let spec = NativeMenuSpec::build(&editor, Some(&ctx));
        let mut collected = Vec::new();
        for top in &spec.menus {
            collect_command_items(&top.items, &mut collected);
        }
        assert!(
            collected.len() >= 60,
            "expected rich CommandId coverage in NativeMenuSpec, got {}",
            collected.len()
        );
        for (cmd, item) in collected {
            let expected = editor.command_menu_state(Some(&ctx), cmd);
            let registry_sc =
                crate::commands::effective_shortcuts(&editor.workspace.shortcuts, cmd)
                    .into_iter()
                    .next();
            assert_eq!(
                expected.shortcut, registry_sc,
                "command_menu_state shortcut drifted from registry for {cmd:?}"
            );
            let NativeMenuItemSpec::Action {
                shortcut,
                shortcut_label,
                enabled,
                checked,
                action,
                ..
            } = item
            else {
                panic!("expected Action item for {cmd:?}");
            };
            assert_eq!(
                *shortcut, expected.shortcut,
                "shortcut mismatch for {cmd:?} (pass {pass})"
            );
            assert_eq!(
                *shortcut_label, expected.shortcut_label,
                "shortcut_label mismatch for {cmd:?} (pass {pass})"
            );
            assert_eq!(
                *enabled, expected.enabled,
                "enabled mismatch for {cmd:?} (pass {pass})"
            );
            assert_eq!(
                *checked, expected.checked,
                "checked mismatch for {cmd:?} (pass {pass})"
            );
            assert_eq!(*action, NativeMenuAction::Command(cmd));
        }
    }

    // Reopen a document and override FileSave shortcut in WorkspacePrefs: NativeMenuSpec and
    // macos_key_equivalent must reflect the customized shortcut immediately.
    editor.new_document_session(&ctx);
    let custom_save = Shortcut::alt_cmd(Key::S);
    editor
        .workspace
        .shortcuts
        .set_shortcuts(CommandId::FileSave, vec![custom_save]);

    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let Some(NativeMenuItemSpec::Action {
        shortcut,
        shortcut_label,
        ..
    }) = spec.find_command(CommandId::FileSave)
    else {
        panic!("FileSave missing from NativeMenuSpec");
    };
    assert_eq!(*shortcut, Some(custom_save));
    assert_eq!(shortcut_label, &custom_save.format());
    assert_eq!(
        macos_key_equivalent(custom_save),
        Some(("s".to_owned(), MACOS_MOD_OPTION | MACOS_MOD_COMMAND))
    );
    let redo_sc = Shortcut::shift_cmd(Key::Z);
    assert_eq!(
        macos_key_equivalent(redo_sc),
        Some(("z".to_owned(), MACOS_MOD_SHIFT | MACOS_MOD_COMMAND))
    );
}

#[test]
fn macos_menu_edit_undo_redo_labels_track_named_steps_dynamically() {
    let (mut editor, ctx, _dir) = setup("undo-redo-labels");

    let initial = NativeMenuSpec::build(&editor, Some(&ctx));
    assert!(matches!(
        initial.find_command(CommandId::EditUndo),
        Some(NativeMenuItemSpec::Action { label, enabled: false, .. }) if label == "Undo"
    ));
    assert!(matches!(
        initial.find_command(CommandId::EditRedo),
        Some(NativeMenuItemSpec::Action { label, enabled: false, .. }) if label == "Redo"
    ));

    editor.transact_named("Move selection", |ed| {
        ed.project.name = "Moved".to_owned();
    });

    let after_edit = NativeMenuSpec::build(&editor, Some(&ctx));
    assert!(matches!(
        after_edit.find_command(CommandId::EditUndo),
        Some(NativeMenuItemSpec::Action { label, enabled: true, .. }) if label == "Undo Move selection"
    ));
    assert!(matches!(
        after_edit.find_command(CommandId::EditRedo),
        Some(NativeMenuItemSpec::Action { label, enabled: false, .. }) if label == "Redo"
    ));

    editor.execute_native_menu_action(&ctx, NativeMenuAction::Command(CommandId::EditUndo));
    let after_undo = NativeMenuSpec::build(&editor, Some(&ctx));
    assert!(matches!(
        after_undo.find_command(CommandId::EditUndo),
        Some(NativeMenuItemSpec::Action { label, enabled: false, .. }) if label == "Undo"
    ));
    assert!(matches!(
        after_undo.find_command(CommandId::EditRedo),
        Some(NativeMenuItemSpec::Action { label, enabled: true, .. }) if label == "Redo Move selection"
    ));

    // Switching to a fresh tab immediately resets Edit -> Undo/Redo menu titles to that tab's history.
    editor.new_document_session(&ctx);
    let clean_tab = NativeMenuSpec::build(&editor, Some(&ctx));
    assert!(matches!(
        clean_tab.find_command(CommandId::EditUndo),
        Some(NativeMenuItemSpec::Action { label, enabled: false, .. }) if label == "Undo"
    ));
    assert!(matches!(
        clean_tab.find_command(CommandId::EditRedo),
        Some(NativeMenuItemSpec::Action { label, enabled: false, .. }) if label == "Redo"
    ));

    // Returning to tab 0 restores its Redo step label.
    editor.execute_native_menu_action(&ctx, NativeMenuAction::ActivateDocumentTab(0));
    let restored_tab = NativeMenuSpec::build(&editor, Some(&ctx));
    assert!(matches!(
        restored_tab.find_command(CommandId::EditRedo),
        Some(NativeMenuItemSpec::Action { label, enabled: true, .. }) if label == "Redo Move selection"
    ));
}

#[test]
fn macos_menu_open_recent_lists_existing_files_disambiguates_duplicates_and_clears() {
    let (mut editor, ctx, dir) = setup("open-recent");
    let sub_a = dir.join("client-a");
    let sub_b = dir.join("client-b");
    std::fs::create_dir_all(&sub_a).unwrap();
    std::fs::create_dir_all(&sub_b).unwrap();

    let file_a = sub_a.join("banner.nflash");
    let file_b = sub_b.join("banner.nflash");
    let missing = dir.join("deleted.nflash");

    assert!(editor.save_to_path(file_a.clone()));
    editor.new_document_session(&ctx);
    assert!(editor.save_to_path(file_b.clone()));
    editor.workspace.general.push_recent(&missing);

    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let file_menu = spec.menu("File").expect("File menu");
    let recent_items = find_submenu(&file_menu.items, "Open recent").expect("Open recent submenu");

    let labels: Vec<&str> = recent_items
        .iter()
        .filter_map(|item| match item {
            NativeMenuItemSpec::Action { label, .. } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        labels,
        vec![
            "banner.nflash — client-b",
            "banner.nflash — client-a",
            "Clear Menu"
        ]
    );

    // Clicking OpenRecent for already-open file_a activates tab 0 instead of duplicating it.
    assert_eq!(editor.active_session, 1);
    editor.execute_native_menu_action(&ctx, NativeMenuAction::OpenRecent(file_a.clone()));
    assert_eq!(editor.session_count(), 2);
    assert_eq!(editor.active_session, 0);

    // Clearing recent files empties the list and disables Clear Menu.
    editor.execute_native_menu_action(&ctx, NativeMenuAction::ClearRecent);
    let cleared_spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let cleared_recent =
        find_submenu(&cleared_spec.menu("File").unwrap().items, "Open recent").unwrap();
    assert_eq!(cleared_recent.len(), 1);
    assert!(matches!(
        &cleared_recent[0],
        NativeMenuItemSpec::Action {
            label,
            enabled: false,
            action: NativeMenuAction::ClearRecent,
            ..
        } if label == "Clear Menu"
    ));
}

#[test]
fn macos_menu_finder_open_files_and_cli_args_never_duplicate_open_tabs() {
    // 1. Verify CLI argument filtering (LaunchServices -psn_*, AppKit -NS* flags, --headless).
    let parsed = startup_file_args_from([
        OsString::from("neoflash-editor"),
        OsString::from("-psn_0_12345"),
        OsString::from("-NSDocumentRevisionsDebugMode"),
        OsString::from("YES"),
        OsString::from("-ApplePersistenceIgnoreState"),
        OsString::from("NO"),
        OsString::from("first.nflash"),
        OsString::from("--"),
        OsString::from("-literal-name.nflash"),
    ]);
    assert_eq!(
        parsed,
        vec![
            PathBuf::from("first.nflash"),
            PathBuf::from("-literal-name.nflash")
        ]
    );
    assert!(
        startup_file_args_from([
            OsString::from("neoflash-editor"),
            OsString::from("--headless"),
            OsString::from("first.nflash"),
        ])
        .is_empty(),
        "--headless mode must never consume CLI args as GUI documents"
    );

    // 2. Verify Finder openFile:/openFiles: deduplication across relative paths, symlinks, and XFL folders.
    let (mut editor, ctx, dir) = setup("finder-dedup");
    let first_path = dir.join("first.nflash");
    let second_path = dir.join("second.nflash");
    assert!(editor.save_to_path(first_path.clone()));
    editor.transact_named("Edit first", |ed| ed.project.name = "First unsaved".into());

    editor.new_document_session(&ctx);
    assert!(editor.save_to_path(second_path.clone()));

    let xfl_dir = dir.join("hero-xfl");
    std::fs::create_dir_all(&xfl_dir).unwrap();
    let dom_xml = xfl_dir.join("DOMDocument.xml");
    let xml = r#"<DOMDocument width="200" height="100" frameRate="24"><timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="Layer 1"><frames><DOMFrame index="0" duration="1"><elements><DOMRectangleObject objectWidth="20" objectHeight="20"/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#;
    std::fs::write(&dom_xml, xml).unwrap();
    assert!(editor.try_open_path(dom_xml.clone(), &ctx));
    assert_eq!(editor.session_count(), 3);
    assert_eq!(editor.active_session, 2);

    #[cfg(unix)]
    let symlink_second = {
        let link = dir.join("second-alias.nflash");
        std::os::unix::fs::symlink(&second_path, &link).unwrap();
        link
    };
    #[cfg(not(unix))]
    let symlink_second = second_path.clone();

    let mut queue = NativeLifecycleQueue::new();
    // Queue dot-segment alias to first.nflash: must focus tab 0 without losing unsaved edits!
    queue.push_open_file(dir.join(".").join("first.nflash"));
    assert!(queue.drain_into(&mut editor, &ctx));
    assert_eq!(editor.session_count(), 3);
    assert_eq!(editor.active_session, 0);
    assert_eq!(editor.project.name, "First unsaved");
    assert!(editor.dirty());

    // Queue symlink to second.nflash: must focus tab 1 without opening a duplicate tab!
    queue.push_open_file(symlink_second);
    assert!(queue.drain_into(&mut editor, &ctx));
    assert_eq!(editor.session_count(), 3);
    assert_eq!(editor.active_session, 1);

    // Queue XFL parent directory while DOMDocument.xml was opened: must focus tab 2!
    queue.push_open_file(xfl_dir.clone());
    assert!(queue.drain_into(&mut editor, &ctx));
    assert_eq!(editor.session_count(), 3);
    assert_eq!(editor.active_session, 2);

    // While a modal confirmation prompt is active, Finder open requests stay buffered safely.
    let third_path = dir.join("third.nflash");
    Project::blank();
    std::fs::write(&third_path, Project::blank().to_json().unwrap()).unwrap();
    editor.request_quit(&ctx);
    assert!(editor.confirm.is_some());
    queue.push_open_files([third_path.clone(), third_path.clone()]);
    assert!(!queue.drain_into(&mut editor, &ctx));
    assert!(
        !queue.is_empty(),
        "queued files must remain buffered during modal prompt"
    );
    assert_eq!(editor.session_count(), 3);

    editor.cancel_confirmation(&ctx);
    assert!(queue.drain_into(&mut editor, &ctx));
    assert!(queue.is_empty());
    assert_eq!(
        editor.session_count(),
        4,
        "duplicate paths in one openFiles: batch must open only one new tab"
    );
}

#[test]
fn macos_menu_dock_reopen_creates_session_only_when_no_document_is_open() {
    let (mut editor, ctx, _dir) = setup("dock-reopen");
    editor.sessions.clear();
    assert!(!editor.has_document());

    let mut queue = NativeLifecycleQueue::new();
    queue.request_reopen(false);
    assert!(queue.drain_into(&mut editor, &ctx));
    assert_eq!(editor.session_count(), 1);

    // Reopening from Dock when tabs already exist never spawns spurious extra tabs.
    queue.request_reopen(false);
    assert!(queue.drain_into(&mut editor, &ctx));
    assert_eq!(editor.session_count(), 1);
}

#[test]
fn macos_menu_window_state_dirty_dot_represented_file_and_quit_save_cancel_across_tabs() {
    let (mut editor, ctx, dir) = setup("window-doc-state");
    let alpha_path = dir.join("alpha.nflash");
    let beta_path = dir.join("beta.nflash");
    let gamma_path = dir.join("gamma.nflash");

    // Tab 0: saved & clean
    assert!(editor.save_to_path(alpha_path.clone()));
    let canon_alpha = crate::recovery::canonical_or_normalized_path(&alpha_path)
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        editor.native_window_state(),
        crate::macos_menu::NativeWindowDocState {
            title: "alpha.nflash — NeoFlash".to_owned(),
            document_edited: false,
            represented_filename: canon_alpha.clone(),
        }
    );

    // Tab 1: saved then dirtied
    editor.new_document_session(&ctx);
    assert!(editor.save_to_path(beta_path.clone()));
    editor.transact_named("Edit beta", |ed| ed.project.fps = 30);
    let canon_beta = crate::recovery::canonical_or_normalized_path(&beta_path)
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        editor.native_window_state(),
        crate::macos_menu::NativeWindowDocState {
            title: "beta.nflash • — NeoFlash".to_owned(),
            document_edited: true,
            represented_filename: canon_beta.clone(),
        }
    );

    // Tab 2: untitled & dirty (represented_filename must be empty)
    editor.new_document_session(&ctx);
    editor.transact_named("Edit untitled", |ed| ed.project.name = "Draft".into());
    assert_eq!(
        editor.native_window_state(),
        crate::macos_menu::NativeWindowDocState {
            title: "Draft • — NeoFlash".to_owned(),
            document_edited: true,
            represented_filename: String::new(),
        }
    );

    // Window menu lists all 3 tabs with active checkmark on Tab 2.
    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let tab_items: Vec<(String, bool)> = spec
        .menu("Window")
        .unwrap()
        .items
        .iter()
        .filter_map(|item| match item {
            NativeMenuItemSpec::Action {
                label,
                checked,
                action: NativeMenuAction::ActivateDocumentTab(_),
                ..
            } => Some((label.clone(), *checked)),
            _ => None,
        })
        .collect();
    assert_eq!(
        tab_items,
        vec![
            ("alpha.nflash".to_owned(), false),
            ("beta.nflash — unsaved".to_owned(), false),
            ("Draft — unsaved".to_owned(), true),
        ]
    );

    // Switching to Tab 0 via Window menu clears dirty dot and updates representedFilename immediately.
    editor.execute_native_menu_action(&ctx, NativeMenuAction::ActivateDocumentTab(0));
    let state_tab0 = editor.native_window_state();
    assert!(!state_tab0.document_edited);
    assert_eq!(state_tab0.represented_filename, canon_alpha);

    // Quit flow with dirty tabs: prompts on dirty tab, Cancel preserves all tabs & dirty state.
    editor.execute_native_menu_action(&ctx, NativeMenuAction::Quit);
    assert_eq!(editor.confirm, Some(Pending::Close));
    assert!(!editor.allow_close);
    assert!(editor.native_window_state().document_edited);

    editor.cancel_confirmation(&ctx);
    assert!(editor.confirm.is_none());
    assert!(!editor.allow_close);
    assert_eq!(editor.session_count(), 3);

    // Quit flow -> Save each dirty tab -> completes Quit cleanly with allow_close = true.
    editor.execute_native_menu_action(&ctx, NativeMenuAction::Quit);
    assert_eq!(editor.confirm, Some(Pending::Close));
    assert_eq!(editor.active_session, 1);
    assert!(editor.save_to_path(beta_path));
    assert!(!editor.native_window_state().document_edited);
    editor.continue_confirmation(&ctx);

    assert_eq!(editor.confirm, Some(Pending::Close));
    assert_eq!(editor.active_session, 2);
    assert!(editor.save_to_path(gamma_path));
    assert!(!editor.native_window_state().document_edited);
    editor.continue_confirmation(&ctx);
    assert!(editor.allow_close);

    // Empty state (no document open): clean title, no dirty dot, empty representedFilename.
    editor.allow_close = false;
    editor.sessions.clear();
    assert_eq!(
        editor.native_window_state(),
        crate::macos_menu::NativeWindowDocState {
            title: "No document open — NeoFlash".to_owned(),
            document_edited: false,
            represented_filename: String::new(),
        }
    );
}

#[test]
fn macos_menu_shortcut_deduplication_and_single_execution_guarantee() {
    // Only OS responder-chain shortcuts (⌘H, ⌥⌘H, ⌘M, ⌘Q) route through NeoFlashMainMenu;
    // all authoring and text-editing chords return false so winit/egui handles them exactly once.
    assert!(should_route_key_equivalent_via_appkit(
        true, false, false, false, "h"
    ));
    assert!(should_route_key_equivalent_via_appkit(
        true, true, false, false, "h"
    ));
    assert!(should_route_key_equivalent_via_appkit(
        true, false, false, false, "m"
    ));
    assert!(should_route_key_equivalent_via_appkit(
        true, false, false, false, "q"
    ));

    for (cmd, alt, shift, ctrl, key) in [
        (true, false, false, false, "s"),         // Save
        (true, false, true, false, "s"),          // Save As
        (true, false, false, false, "z"),         // Undo / TextEdit Undo
        (true, false, true, false, "z"),          // Redo / TextEdit Redo
        (true, false, false, false, "n"),         // New Document
        (true, false, false, false, "o"),         // Open
        (true, false, false, false, "w"),         // Close
        (true, true, false, false, "w"),          // Close All
        (true, false, false, false, "c"),         // Copy
        (true, false, false, false, "v"),         // Paste
        (true, false, false, false, "x"),         // Cut
        (true, false, false, false, "a"),         // Select All
        (true, false, false, false, ","),         // Preferences
        (false, false, false, false, "\u{f708}"), // F5
        (true, false, true, false, "q"),          // ⇧⌘Q is not Quit
        (true, false, false, true, "h"),          // ⌃⌘H is not Hide
    ] {
        assert!(
            !should_route_key_equivalent_via_appkit(cmd, alt, shift, ctrl, key),
            "authoring/text shortcut ({cmd},{alt},{shift},{ctrl},{key:?}) must not be intercepted by AppKit menu"
        );
    }

    // Simulate a full frame with ⌘N and another with ⌘Z: each fires once (never twice).
    let (mut editor, ctx, _dir) = setup("single-shortcut-dispatch");
    assert_eq!(editor.session_count(), 1);

    let cmd_mods = egui::Modifiers {
        command: true,
        mac_cmd: true,
        ..Default::default()
    };
    let mut raw_new = egui::RawInput {
        modifiers: cmd_mods,
        ..Default::default()
    };
    raw_new.events.push(egui::Event::Key {
        key: Key::N,
        physical_key: Some(Key::N),
        pressed: true,
        repeat: false,
        modifiers: cmd_mods,
    });
    let _ = ctx.run(raw_new, |ctx| {
        crate::macos_menu::drain_events(&mut editor, ctx);
        editor.shortcuts(ctx);
        let _ = crate::macos_menu::sync(&editor, ctx);
    });
    assert_eq!(
        editor.session_count(),
        2,
        "⌘N must open exactly one new tab, never two"
    );

    editor.transact_named("Step 1", |ed| ed.project.fps = 30);
    editor.transact_named("Step 2", |ed| ed.project.fps = 60);
    assert_eq!(editor.project.fps, 60);

    let mut raw_undo = egui::RawInput {
        modifiers: cmd_mods,
        ..Default::default()
    };
    raw_undo.events.push(egui::Event::Key {
        key: Key::Z,
        physical_key: Some(Key::Z),
        pressed: true,
        repeat: false,
        modifiers: cmd_mods,
    });
    let _ = ctx.run(raw_undo, |ctx| {
        crate::macos_menu::drain_events(&mut editor, ctx);
        editor.shortcuts(ctx);
        let _ = crate::macos_menu::sync(&editor, ctx);
    });
    assert_eq!(
        editor.project.fps, 30,
        "⌘Z must undo exactly one step (60 -> 30), never two (60 -> 24)"
    );
}

#[test]
fn macos_menu_workspace_density_appearance_and_panel_actions_roundtrip() {
    let (mut editor, ctx, _dir) = setup("workspace-density-panels");

    editor.execute_native_menu_action(
        &ctx,
        NativeMenuAction::ApplyWorkspaceTemplate(WorkspaceTemplate::Developer),
    );
    assert_eq!(editor.workspace.template, WorkspaceTemplate::Developer);

    editor.execute_native_menu_action(&ctx, NativeMenuAction::SetDensity(DensityPref::Compact));
    assert_eq!(editor.workspace.density, DensityPref::Compact);

    editor.execute_native_menu_action(&ctx, NativeMenuAction::SetTheme(ThemeMode::Light));
    assert_eq!(editor.workspace.theme, ThemeMode::Light);

    assert!(!editor.workspace.is_panel_active(PanelId::Ease));
    let mut queue = NativeLifecycleQueue::new();
    queue.push_action(NativeMenuAction::Command(CommandId::for_panel(
        PanelId::Ease,
    )));
    assert!(queue.drain_into(&mut editor, &ctx));
    assert!(editor.workspace.is_panel_active(PanelId::Ease));

    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    assert!(matches!(
        spec.find_command(CommandId::for_panel(PanelId::Ease)),
        Some(NativeMenuItemSpec::Action { checked: true, .. })
    ));

    let win_items = &spec.menu("Window").unwrap().items;
    let ws_items = find_submenu(win_items, "Workspaces").unwrap();
    assert!(ws_items.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Action {
            label,
            checked: true,
            ..
        } if label == WorkspaceTemplate::Developer.label()
    )));
    let density_items = find_submenu(win_items, "Density").unwrap();
    assert!(density_items.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Action {
            label,
            checked: true,
            ..
        } if label == DensityPref::Compact.label()
    )));
    let appearance_items = find_submenu(win_items, "Appearance").unwrap();
    assert!(appearance_items.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Action {
            label,
            checked: true,
            ..
        } if label == ThemeMode::Light.label()
    )));
}

#[test]
fn t322_brushes_uses_common_visibility_and_native_detach_dock_routes() {
    let (mut editor, ctx, _dir) = setup("brushes-panel-routes");
    editor.set_panel_visible(PanelId::Brushes, true);
    editor.set_panels_hidden(true);

    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::WindowBrushes)
            .checked
    );
    assert!(editor.execute_menu_command(&ctx, CommandId::WindowBrushes));
    assert!(!editor.panels_hidden);
    assert!(editor.workspace.is_panel_active(PanelId::Brushes));

    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let window = spec.menu("Window").expect("Window menu");
    let detach = find_submenu(&window.items, "Detach panel").expect("Detach panel submenu");
    assert!(detach.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Action {
            label,
            action: NativeMenuAction::DetachPanel(PanelId::Brushes),
            ..
        } if label == "Brushes"
    )));

    editor.execute_native_menu_action(&ctx, NativeMenuAction::DetachPanel(PanelId::Brushes));
    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let window = spec.menu("Window").expect("Window menu");
    let dock = find_submenu(&window.items, "Dock panel").expect("Dock panel submenu");
    assert!(dock.iter().any(|item| matches!(
        item,
        NativeMenuItemSpec::Action {
            label,
            action: NativeMenuAction::DockPanel(PanelId::Brushes),
            ..
        } if label == "Brushes"
    )));
}

#[test]
fn t311_zero_document_queued_commands_shortcuts_and_function_search_execute_cleanly() {
    let (mut editor, ctx, _dir) = setup("zero-doc-wiring");
    editor.sessions.clear();
    assert!(!editor.has_document());
    assert!(!editor.workspace.is_panel_active(PanelId::Output));

    // Queued zero-doc-enabled commands execute in `shortcuts(&ctx)`, while disabled authoring
    // commands are ignored rather than panicking or mutating state.
    crate::commands::queue_command(&ctx, CommandId::WindowOutput);
    crate::commands::queue_command(&ctx, CommandId::EditSelectAll);
    if crate::panels::beta_feedback::is_beta_build() {
        crate::commands::queue_command(&ctx, CommandId::HelpSendBetaFeedback);
    }
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        editor.shortcuts(ctx);
    });
    assert!(
        editor.workspace.is_panel_active(PanelId::Output),
        "queued WindowOutput must execute even when 0 documents are open"
    );
    if crate::panels::beta_feedback::is_beta_build() {
        assert!(
            editor.beta_feedback.open,
            "queued HelpSendBetaFeedback must open Beta feedback with 0 documents open"
        );
        editor.close_beta_feedback();
    }

    // Pressing F1 in zero-document state opens Function search via customizable HelpSearchFunctions.
    assert!(!crate::panels::function_search::is_open(&ctx));
    let raw_f1 = egui::RawInput {
        events: vec![egui::Event::Key {
            key: Key::F1,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_f1, |ctx| {
        editor.shortcuts(ctx);
    });
    assert!(
        crate::panels::function_search::is_open(&ctx),
        "F1 shortcut must open Function search even when 0 documents are open"
    );

    // `results_for_editor` in zero-doc mode must align with `command_menu_state`.
    let results = crate::panels::function_search::results_for_editor(&editor, Some(&ctx), "");
    assert!(results.contains(&CommandId::FileNew));
    assert!(results.contains(&CommandId::FileOpen));
    assert!(results.contains(&CommandId::FileImportLegacyFlash));
    assert!(results.contains(&CommandId::EditPreferences));
    assert!(results.contains(&CommandId::EditKeyboardShortcuts));
    assert!(results.contains(&CommandId::HelpManual));
    assert!(results.contains(&CommandId::HelpCheckForUpdates));
    assert!(!results.contains(&CommandId::HelpSearchFunctions));
    for cmd in &results {
        assert!(
            editor.command_menu_state(Some(&ctx), *cmd).enabled,
            "Function search returned disabled command {cmd:?} in zero-document mode"
        );
    }
}

#[test]
fn t311_swf_export_profiles_and_as1_as2_gui_export_succeed() {
    let (editor, ctx, dir) = setup("swf-profiles");
    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let file_items = &spec.menu("File").expect("File menu").items;
    let export_items = find_submenu(file_items, "Export").expect("Export submenu");
    let swf_target_items =
        find_submenu(export_items, "Export SWF Target").expect("Export SWF Target submenu");
    for (expected_cmd, expected_label) in [
        (CommandId::FileExportSwfFp8, "Flash Player 8 (AVM1)…"),
        (CommandId::FileExportSwfFp9, "Flash Player 9 (AVM2)…"),
        (CommandId::FileExportSwfFp10, "Flash Player 10 (AVM2)…"),
        (
            CommandId::FileExportSwfFp10Scriptless,
            "Flash Player 10 (Visuals Only)…",
        ),
    ] {
        assert!(
            swf_target_items.iter().any(|item| matches!(
                item,
                NativeMenuItemSpec::Action {
                    label,
                    command_id: Some(cmd),
                    enabled: true,
                    ..
                } if *cmd == expected_cmd && label == expected_label
            )),
            "missing SWF target item {expected_label}"
        );
    }

    for (lang, tag) in [
        (neoflash_core::ScriptLanguage::ActionScript1, "as1"),
        (neoflash_core::ScriptLanguage::ActionScript2, "as2"),
    ] {
        let mut proj = Project::blank();
        proj.script_language = Some(lang);
        proj.symbols[0]
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
                layer: None,
                frame: 0,
                script: neoflash_core::Script {
                    language: lang,
                    source: "stop();".into(),
                },
            });
        let mut ed = Editor::from_project(&ctx, proj);
        let swf_out = dir.join(format!("movie-{tag}.swf"));
        assert!(
            ed.export_swf_dialog(Some(swf_out.clone())),
            "default Export Movie (.swf) must succeed for {lang:?}"
        );
        let bytes = std::fs::read(&swf_out).unwrap();
        assert!(bytes.len() > 8);
        assert_eq!(
            bytes[3], 8,
            "AS1/AS2 default SWF export must target SWF v8 (AVM1)"
        );

        let visuals_out = dir.join(format!("visuals-{tag}.swf"));
        assert!(
            ed.export_swf_profile_dialog(
                neoflash_export::SwfPublishProfile::FlashPlayer10Avm2,
                true,
                Some(visuals_out.clone()),
            ),
            "Flash Player 10 (Visuals Only) export must succeed on {lang:?} projects"
        );
        let vbytes = std::fs::read(&visuals_out).unwrap();
        assert_eq!(vbytes[3], 10);
    }
}

#[test]
fn macos_startup_open_files_drain_deduplicates_and_clears_cold_launch_queue() {
    let (_editor, _ctx, dir) = setup("cold_launch_drain");
    let first = dir.join("first.nflash");
    let second = dir.join("second.nflash");
    std::fs::write(&first, Project::blank().to_json().unwrap()).unwrap();
    std::fs::write(&second, Project::blank().to_json().unwrap()).unwrap();

    let mut queue = NativeLifecycleQueue::default();
    queue.push_open_file(first.clone());
    queue.push_open_file(dir.join(".").join("first.nflash"));
    queue.push_open_file(second.clone());

    let mut arg_open = vec![dir.join(".").join("first.nflash")];
    queue.drain_startup_open_files(&mut arg_open);

    assert_eq!(
        arg_open.len(),
        2,
        "cold-launch odoc queue must deduplicate against existing startup paths"
    );
    assert_eq!(
        std::fs::canonicalize(&arg_open[0]).unwrap(),
        std::fs::canonicalize(&first).unwrap()
    );
    assert_eq!(
        std::fs::canonicalize(&arg_open[1]).unwrap(),
        std::fs::canonicalize(&second).unwrap()
    );
    assert!(
        queue.is_empty(),
        "draining cold-launch open_files must leave the lifecycle queue empty"
    );
}

#[test]
fn t377_native_macos_menu_and_in_window_menu_bar_match_one_to_one() {
    fn expected_accessible_label(label: &str, shortcut: &str) -> String {
        if shortcut.is_empty() {
            label.to_owned()
        } else {
            format!("{label} ({shortcut})")
        }
    }

    let (mut editor, ctx, _dir) = setup("menu-1to1-parity");
    ctx.enable_accesskit();
    let clock = std::cell::Cell::new(0.0_f64);

    let step = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
        let t = clock.get() + 0.1;
        clock.set(t);
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1920.0, 1080.0),
                )),
                time: Some(t),
                events,
                ..Default::default()
            },
            |ctx| {
                ed.menu(ctx);
                egui::CentralPanel::default().show(ctx, |_| {});
            },
        )
    };

    let spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let expected_top: Vec<&str> = spec.menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(
        expected_top,
        vec![
            "NeoFlash", "File", "Edit", "View", "Insert", "Modify", "Text", "Control", "Debug",
            "Window", "Help",
        ]
    );
    assert!(!expected_top.contains(&"Document"));

    // Verify Preferences… and Keyboard shortcuts… appear in BOTH `NeoFlash` AND `Edit`.
    for menu_name in ["NeoFlash", "Edit"] {
        let menu = spec.menu(menu_name).expect(menu_name);
        for (cmd, expected_label) in [
            (CommandId::EditPreferences, "Preferences…"),
            (CommandId::EditKeyboardShortcuts, "Keyboard shortcuts…"),
        ] {
            assert!(
                menu.items.iter().any(|item| matches!(
                    item,
                    NativeMenuItemSpec::Action {
                        label,
                        command_id: Some(found),
                        enabled: true,
                        ..
                    } if *found == cmd && label == expected_label
                )),
                "{menu_name} menu must include {expected_label}"
            );
        }
    }

    // Render the in-window NF2 top bar and verify top-level menu buttons match 1:1 left-to-right
    // and never expose a standalone `Document` menu.
    let _ = step(&mut editor, vec![]);
    let bar_out = step(&mut editor, vec![]);
    let ak = bar_out
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("accesskit enabled");
    assert!(
        !ak.nodes.iter().any(|(_, n)| n.label() == Some("Document")),
        "in-window menu bar must not expose top-level Document menu"
    );

    let mut top_buttons: Vec<(&str, egui::Pos2)> = Vec::new();
    for &title in &expected_top {
        let bounds = ak
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.label() == Some(title)
                    && n.bounds()
                        .is_some_and(|b| b.y1 <= f64::from(neoflash_ui::metrics::top_bar()) + 2.0)
            })
            .filter_map(|(_, n)| n.bounds())
            .min_by(|a, b| a.x0.total_cmp(&b.x0))
            .unwrap_or_else(|| panic!("missing top-level menu button {title:?} in NF2 top bar"));
        top_buttons.push((
            title,
            egui::pos2(
                ((bounds.x0 + bounds.x1) * 0.5) as f32,
                ((bounds.y0 + bounds.y1) * 0.5) as f32,
            ),
        ));
    }
    for pair in top_buttons.windows(2) {
        assert!(
            pair[0].1.x < pair[1].1.x,
            "in-window menu order mismatch: {:?} ({}) must precede {:?} ({})",
            pair[0].0,
            pair[0].1.x,
            pair[1].0,
            pair[1].1.x
        );
    }

    // Open every single one of the 11 top-level menus in the in-window bar and verify every
    // item, submenu, shortcut, enabled state, and checkmark matches `NativeMenuSpec` 1:1!
    for (title, btn_pos) in top_buttons {
        let _ = step(
            &mut editor,
            vec![
                egui::Event::PointerMoved(btn_pos),
                egui::Event::PointerButton {
                    pos: btn_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = step(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: btn_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let _ = step(&mut editor, vec![]);
        let popup_out = step(&mut editor, vec![]);
        let live_spec = NativeMenuSpec::build_top_menu(&editor, Some(&ctx), title).unwrap();
        let popup_ak = popup_out.platform_output.accesskit_update.as_ref().unwrap();

        for item in &live_spec.items {
            match item {
                NativeMenuItemSpec::Separator | NativeMenuItemSpec::Section(_) => {}
                NativeMenuItemSpec::Action {
                    label,
                    shortcut_label,
                    enabled,
                    checkable,
                    checked,
                    ..
                } => {
                    let expected = expected_accessible_label(label, shortcut_label);
                    let matching: Vec<_> = popup_ak
                        .nodes
                        .iter()
                        .filter(|(_, n)| {
                            n.label() == Some(expected.as_str())
                                && n.bounds().is_some_and(|b| b.y0 >= 28.0)
                        })
                        .collect();
                    assert!(
                        !matching.is_empty(),
                        "in-window {title} menu missing Action row {expected:?}"
                    );
                    let node = &matching[0].1;
                    assert_eq!(
                        !node.is_disabled(),
                        *enabled,
                        "enabled mismatch on {title} ▸ {expected}"
                    );
                    if *checkable {
                        let is_checked = node.toggled() == Some(egui::accesskit::Toggled::True);
                        assert_eq!(
                            is_checked, *checked,
                            "checked mismatch on {title} ▸ {expected}"
                        );
                    }
                }
                NativeMenuItemSpec::System {
                    label,
                    shortcut_label,
                    enabled,
                    ..
                } => {
                    let expected = expected_accessible_label(label, shortcut_label);
                    let matching: Vec<_> = popup_ak
                        .nodes
                        .iter()
                        .filter(|(_, n)| {
                            n.label() == Some(expected.as_str())
                                && n.bounds().is_some_and(|b| b.y0 >= 28.0)
                        })
                        .collect();
                    assert!(
                        !matching.is_empty(),
                        "in-window {title} menu missing System row {expected:?}"
                    );
                    assert_eq!(
                        !matching[0].1.is_disabled(),
                        *enabled,
                        "enabled mismatch on System row {title} ▸ {expected}"
                    );
                }
                NativeMenuItemSpec::Submenu {
                    label,
                    enabled,
                    role,
                    items: sub_items,
                } => {
                    let expected_enabled = !matches!(role, Some(NativeSubmenuRole::Services))
                        && (*enabled || !sub_items.is_empty())
                        && *enabled;
                    let matching: Vec<_> = popup_ak
                        .nodes
                        .iter()
                        .filter(|(_, n)| {
                            n.label() == Some(label.as_str())
                                && n.bounds().is_some_and(|b| b.y0 >= 28.0)
                        })
                        .collect();
                    assert!(
                        !matching.is_empty(),
                        "in-window {title} menu missing Submenu row {label:?}"
                    );
                    assert_eq!(
                        !matching[0].1.is_disabled(),
                        expected_enabled,
                        "enabled mismatch on Submenu row {title} ▸ {label}"
                    );
                }
            }
        }

        // Close popup via Escape before opening the next menu.
        let _ = step(
            &mut editor,
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: Some(Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let _ = step(&mut editor, vec![]);
    }
}

#[test]
fn t390_menu_sync_fingerprint_skips_idle_pan_hover_and_tracks_every_menu_mutation() {
    use crate::macos_menu::{
        MenuSyncFingerprint, TOP_MENU_COUNT, decode_menu_tag, encode_menu_tag,
    };
    use crate::workspace::PanelPlacement;

    // 1. Verify per-top-menu `(menu_idx << 16) | local_idx` tag encoding never aliases across menus.
    assert_eq!(decode_menu_tag(-1), None);
    assert_eq!(decode_menu_tag(encode_menu_tag(TOP_MENU_COUNT, 0)), None);
    for menu_idx in 0..TOP_MENU_COUNT {
        for local_idx in [0, 1, 17, 63, 255] {
            let tag = encode_menu_tag(menu_idx, local_idx);
            assert!(tag >= 0);
            assert_eq!(decode_menu_tag(tag), Some((menu_idx, local_idx)));
        }
    }

    // 2. Verify O(1) MenuSyncFingerprint stays identical across idle, stage pan, and status updates.
    let (mut editor, ctx, dir) = setup("fingerprint-stability");
    let base = MenuSyncFingerprint::capture(&editor, Some(&ctx));

    editor.pan = egui::vec2(320.0, -140.0);
    editor.status = "Hovered stage".to_owned();
    assert_eq!(
        MenuSyncFingerprint::capture(&editor, Some(&ctx)),
        base,
        "pan and non-menu status updates must NOT invalidate MenuSyncFingerprint"
    );

    // 3. Verify every menu-visible state transition changes MenuSyncFingerprint immediately.
    let mut prev = base;
    let mut assert_changed = |ed: &Editor, reason: &str| {
        let next = MenuSyncFingerprint::capture(ed, Some(&ctx));
        assert_ne!(next, prev, "MenuSyncFingerprint missed change on: {reason}");
        prev = next;
    };

    editor.transact_named("Rename project", |ed| ed.project.name = "Hero".to_owned());
    assert_changed(&editor, "Undo/history commit");

    editor.undo();
    assert_changed(&editor, "Undo step");

    editor.redo();
    assert_changed(&editor, "Redo step");

    editor.select_all();
    assert_changed(&editor, "Selection change");

    editor.set_tool(crate::app::Tool::Text);
    assert_changed(&editor, "Tool change");

    editor.frame = 3;
    assert_changed(&editor, "Playhead frame change");

    editor.zoom = 2.0;
    assert_changed(&editor, "Stage zoom change");

    editor.onion = !editor.onion;
    assert_changed(&editor, "Onion skin toggle");

    editor.stage_view.prefs.grid_show = !editor.stage_view.prefs.grid_show;
    assert_changed(&editor, "Grid show toggle");

    editor.set_panel_visible(PanelId::Align, true);
    assert_changed(&editor, "Panel visibility change");

    editor.set_panel_placement(&ctx, PanelId::Align, PanelPlacement::Detached);
    assert_changed(&editor, "Panel detach placement change");

    editor.execute_native_menu_action(
        &ctx,
        NativeMenuAction::ApplyWorkspaceTemplate(WorkspaceTemplate::Animator),
    );
    assert_changed(&editor, "Workspace template change");

    editor.execute_native_menu_action(&ctx, NativeMenuAction::SetTheme(ThemeMode::Light));
    assert_changed(&editor, "Theme change");

    editor.execute_native_menu_action(&ctx, NativeMenuAction::SetDensity(DensityPref::Compact));
    assert_changed(&editor, "Density change");

    editor
        .workspace
        .shortcuts
        .set_shortcuts(CommandId::FileSave, vec![Shortcut::alt_cmd(Key::S)]);
    assert_changed(&editor, "Custom shortcut override");

    let saved_file = dir.join("hero.nflash");
    assert!(editor.save_to_path(saved_file));
    assert_changed(&editor, "Save & recent files update");

    editor.new_document_session(&ctx);
    assert_changed(&editor, "New document session tab");
}

#[test]
fn rf02_dirty_cache_and_recent_files_cache_avoid_repeat_traversals_and_preserve_exactness() {
    use neoflash_core::{Element, ElementKind, ResourceUnit, Transform};

    let (mut editor, ctx, dir) = setup("rf02-dirty-and-recent-cache");

    // Populate 600 vector elements + binary ResourceUnits so full Project comparison is non-trivial.
    let root_id = editor.project.root.clone();
    {
        let sym = editor.project.symbol_mut(&root_id).unwrap();
        let elements = &mut sym.timeline.layers[0].keyframes[0].elements;
        for idx in 0..600 {
            elements.push(Element {
                id: format!("rf02-el-{idx}"),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: 40.0,
                    height: 30.0,
                    fill: [200, 100, 50, 255],
                    stroke: [0, 0, 0, 255],
                    stroke_width: 1.0,
                },
                visible: true,
                ..Element::default()
            });
        }
    }
    let res = ResourceUnit::from_bytes(
        "rf02-img",
        "images/banner.png",
        "image/png",
        &vec![0x5A; 32 * 1024],
    )
    .unwrap();
    editor.project.resources.push(res);

    let file_path = dir.join("heavy-doc.nflash");
    assert!(editor.save_to_path(file_path.clone()));
    editor.dirty_cache.set(Default::default());

    // 200 repeated dirty() + native_window_state() queries must run at most 1 full Project comparison!
    for _ in 0..200 {
        assert!(!editor.dirty());
        let win = editor.native_window_state();
        assert!(!win.document_edited);
        assert!(!win.represented_filename.is_empty());
    }
    assert_eq!(
        editor.dirty_cache.get().full_comparisons,
        1,
        "clean saved document must cache dirty() across repeated queries without re-traversing Project"
    );

    // Commit an edit -> dirty() becomes true immediately; Undo -> dirty() returns to false; Redo -> true.
    editor.transact_named("Change stage FPS", |ed| ed.project.fps = 60);
    assert!(editor.dirty());
    assert!(editor.native_window_state().document_edited);

    editor.undo();
    assert!(
        !editor.dirty(),
        "Undo back to saved state must clear dirty()"
    );

    editor.redo();
    assert!(
        editor.dirty(),
        "Redo away from saved state must set dirty()"
    );

    assert!(editor.save(false));
    assert!(!editor.dirty(), "Saving must clear dirty()");

    // Direct header mutation without transact (used by several legacy/export tests) still invalidates cache.
    editor.project.fps = 120;
    assert!(editor.dirty());
    editor.project.fps = 60;
    assert!(!editor.dirty());

    // Inactive session tab caches dirty() independently in O(1).
    editor.new_document_session(&ctx);
    assert_eq!(editor.active_session, 1);
    let inactive = editor.sessions[0].as_ref().expect("inactive tab 0");
    inactive.dirty_cache.set(Default::default());
    for _ in 0..100 {
        assert!(!inactive.dirty());
    }
    assert_eq!(inactive.dirty_cache.get().full_comparisons, 1);

    // Recent-files cache preserves GeneralPrefs PartialEq symmetry and invalidates on clear.
    let uncached_clone = crate::workspace::GeneralPrefs {
        recent_files: editor.workspace.general.recent_files.clone(),
        ..editor.workspace.general.clone()
    };
    uncached_clone.invalidate_recent_files_cache();
    let cached_paths = editor.workspace.general.existing_recent_files_cached();
    assert_eq!(cached_paths.len(), 1);
    assert_eq!(editor.workspace.general, uncached_clone);

    editor.workspace.clear_recent_files();
    assert!(
        editor
            .workspace
            .general
            .existing_recent_files_cached()
            .is_empty()
    );
}
