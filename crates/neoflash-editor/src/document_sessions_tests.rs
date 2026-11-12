use crate::app::{Editor, Pending, ScriptTarget};
use eframe::egui;
use neoflash_core::{FrameRange, Project};
use std::path::PathBuf;

fn setup(name: &str) -> (Editor, egui::Context, PathBuf) {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/t006-documents")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    editor.recovery.set_dir(Some(dir.join("Recovery")));
    editor.workspace.general.autosave = true;
    editor.workspace.general.autosave_minutes = 1;
    (editor, ctx, dir)
}

fn recovery_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir.join("Recovery"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "nflash"))
        .count()
}

#[test]
fn document_sessions_preserve_selection_history_views_and_independent_recovery() {
    let (mut editor, ctx, dir) = setup("switch");
    let original = editor.project.clone();
    editor.select_all();
    let selected = editor.selection.clone();
    editor.vector.selected_anchors = Some(("anchor-selection".into(), vec![1, 3]));
    editor.stage_view.zero = [13., 29.];
    editor.zoom = 1.75;
    editor.pan = egui::vec2(12., -19.);
    editor.onion = true;
    editor.script_target = ScriptTarget::Source;
    editor.selected_source = Some("first-source".into());
    let first_widget = editor.actions_editor_id();
    let mut caret = egui::text_edit::TextEditState::default();
    caret
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(7),
        )));
    caret.store(&ctx, first_widget);
    let first_identity = editor.clipboard.session.clone();
    editor.transact(|e| e.project.name = "First dirty".into());
    let first_id = editor.recovery.active_id().to_owned();
    editor.tick_autosave(0., 1000);
    let mut second = original.clone(); // Identical authored IDs must not mix selection/history.
    second.name = "Second".into();
    editor.open_export_lottie(None);
    editor.last_lottie_report = Some(Default::default());
    editor.open_new_session_tab(&ctx, second.clone(), None);
    assert!(editor.export_lottie.is_none());
    assert!(editor.last_lottie_report.is_none());
    assert_eq!(editor.session_count(), 2);
    assert_ne!(editor.clipboard.session, first_identity);
    assert_ne!(editor.actions_editor_id(), first_widget);
    assert_ne!(editor.recovery.active_id(), first_id);
    assert!(editor.selection.is_empty());
    editor.transact(|e| e.project.name = "Second dirty".into());
    let second_id = editor.recovery.active_id().to_owned();
    let layer = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![layer],
        start: 2,
        end: 4,
    });
    let range = editor.frame_range();
    editor.tick_autosave(10., 1010);
    editor.tick_autosave(70., 1070);
    assert_eq!(
        recovery_files(&dir),
        2,
        "inactive dirty document is autosaved too"
    );
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.recovery.active_id(), first_id);
    assert_eq!(editor.clipboard.session, first_identity);
    assert_eq!(editor.actions_editor_id(), first_widget);
    assert_eq!(
        egui::text_edit::TextEditState::load(&ctx, first_widget)
            .unwrap()
            .cursor
            .char_range()
            .unwrap()
            .primary
            .index,
        7
    );
    assert_eq!(editor.selection, selected);
    assert_eq!(
        editor.vector.selected_anchors,
        Some(("anchor-selection".into(), vec![1, 3]))
    );
    assert_eq!(editor.stage_view.zero, [13., 29.]);
    assert_eq!(editor.zoom, 1.75);
    assert_eq!(editor.pan, egui::vec2(12., -19.));
    assert!(editor.onion);
    assert!(editor.script_target == ScriptTarget::Source);
    assert_eq!(editor.selected_source.as_deref(), Some("first-source"));
    editor.undo();
    assert_eq!(editor.project, original);
    editor.redo();
    assert_eq!(editor.project.name, "First dirty");
    assert!(editor.save_to_path(dir.join("first.nflash")));
    assert_eq!(recovery_files(&dir), 1, "Save cleans only its own recovery");
    assert!(editor.switch_session_tab(&ctx, 1));
    assert_eq!(editor.recovery.active_id(), second_id);
    assert_eq!(editor.frame_range(), range);
    editor.undo();
    assert_eq!(editor.project, second);
    editor.redo();
    assert_eq!(editor.project.name, "Second dirty");
    assert!(
        !editor.save_to_path(dir.join("first.nflash")),
        "cannot overwrite another open document"
    );
    assert!(editor.open_path_in_session(&ctx, dir.join("first.nflash")));
    assert_eq!(editor.active_session, 0);
    assert!(!editor.dirty());
    assert_eq!(editor.session_count(), 2);
    let saved = editor.project.clone();
    let copy = dir.join("independent-copy.nflash");
    std::fs::copy(dir.join("first.nflash"), &copy).unwrap();
    assert!(editor.open_path_in_session(&ctx, copy));
    assert_eq!(editor.session_count(), 3);
    assert_eq!(
        editor.project, saved,
        "Save/Open round-trips authored content into an independent session"
    );
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
    assert!(editor.switch_session_tab(&ctx, 2));
    assert_eq!(editor.project.name, "Second dirty");
    assert!(editor.dirty());
    editor.project.validate().unwrap();

    // Small authored public XFL fixture: imported source identity is session state, never a save
    // destination. Reopening an alias returns to the converted document with its edits and Undo.
    let source_dir = dir.join("authored-import");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("DOMDocument.xml");
    let xml = r#"<DOMDocument platform="Windows" width="120" height="80" frameRate="24"><timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="Artwork"><frames><DOMFrame index="0" duration="1"><elements><DOMRectangleObject objectWidth="10" objectHeight="10"/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#;
    std::fs::write(&source, xml).unwrap();
    let previous_index = editor.active_session;
    let previous_output = editor.output.clone();
    assert!(editor.try_open_path(source.clone(), &ctx));
    let imported_index = editor.active_session;
    let imported = editor.project.clone();
    let imported_count = editor.session_count();
    assert_eq!(imported_count, 4);
    assert!(editor.path.is_none());
    assert_eq!(editor.import_source, Some(source.canonicalize().unwrap()));
    assert!(editor.dirty());
    assert!(
        editor
            .output
            .iter()
            .any(|line| line.contains("Authoring platform metadata")),
        "successful import diagnostics belong to the imported document"
    );
    editor.transact(|editor| editor.project.name = "Edited imported document".into());
    let edited_import = editor.project.clone();
    assert!(editor.switch_session_tab(&ctx, previous_index));
    assert_eq!(editor.output, previous_output);
    assert!(editor.import_source.is_none());
    let source_alias = source_dir.join(".").join("DOMDocument.xml");
    assert!(editor.try_open_path(source_alias.clone(), &ctx));
    assert_eq!(editor.active_session, imported_index);
    assert_eq!(editor.session_count(), imported_count);
    assert_eq!(editor.project, edited_import);
    editor.undo();
    assert_eq!(editor.project, imported);
    editor.redo();
    assert_eq!(editor.project, edited_import);
    let converted = dir.join("converted-import.nflash");
    assert!(editor.save_to_path(converted.clone()));
    assert_eq!(editor.path.as_ref(), Some(&converted));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), xml);
    assert_eq!(editor.session_for_path(&source), None);
    assert_eq!(editor.session_for_open_path(&source), Some(imported_index));
    assert!(editor.switch_session_tab(&ctx, previous_index));
    editor.import_legacy(source_alias, &ctx);
    assert_eq!(editor.active_session, imported_index);
    assert_eq!(editor.session_count(), imported_count);
    assert_eq!(editor.path.as_ref(), Some(&converted));
    assert_eq!(editor.project, edited_import);
    assert!(editor.history.can_undo());
    assert!(!editor.dirty());

    // Failed source diagnostics stay on the current tab and do not replace its imported identity.
    let failed_dir = dir.join("failed-import");
    std::fs::create_dir_all(&failed_dir).unwrap();
    let failed_source = failed_dir.join("DOMDocument.xml");
    std::fs::write(
        &failed_source,
        xml.replace("</DOMDocument>", "<UnknownFeature/></DOMDocument>"),
    )
    .unwrap();
    let output_count = editor.output.len();
    assert!(!editor.try_open_path(failed_source, &ctx));
    assert_eq!(editor.active_session, imported_index);
    assert_eq!(editor.session_count(), imported_count);
    assert_eq!(editor.project, edited_import);
    assert_eq!(editor.path.as_ref(), Some(&converted));
    assert_eq!(editor.import_source, Some(source.canonicalize().unwrap()));
    assert!(
        editor.output[output_count..]
            .iter()
            .any(|line| line.contains("UnknownFeature"))
    );
    editor.project.validate().unwrap();
}

#[test]
fn document_close_quit_cancel_and_failed_open_preserve_all_unsaved_documents() {
    let (mut editor, ctx, dir) = setup("close");
    editor.transact(|e| e.project.name = "First dirty".into());
    editor.new_document_session(&ctx);
    editor.transact(|e| e.project.name = "Second dirty".into());
    editor.tick_autosave(0., 1000);
    editor.tick_autosave(60., 1060);
    assert_eq!(recovery_files(&dir), 2);
    assert!(editor.close_session_tab(&ctx, 0));
    assert_eq!(editor.confirm, Some(Pending::CloseSession(0)));
    assert_eq!(editor.project.name, "First dirty");
    assert!(
        !editor.switch_session_tab(&ctx, 1),
        "a prompt pins its document"
    );
    editor.cancel_confirmation(&ctx);
    assert_eq!(editor.active_session, 0);
    assert_eq!(editor.project.name, "First dirty");
    assert_eq!(recovery_files(&dir), 2);
    editor.request(Pending::Close, &ctx);
    assert!(
        editor.has_modal_dialog(),
        "authoring shortcuts are blocked by the dirty prompt"
    );
    assert_eq!(editor.confirm, Some(Pending::Close));
    editor.continue_confirmation(&ctx); // Discard first prompt, but no deletion yet.
    assert_eq!(editor.confirm, Some(Pending::Close));
    assert_eq!(recovery_files(&dir), 2);
    assert!(!editor.allow_close);
    editor.cancel_confirmation(&ctx);
    assert_eq!(editor.active_session, 1);
    assert_eq!(editor.session_count(), 2);
    assert_eq!(recovery_files(&dir), 2);
    assert!(editor.dirty());
    let before = editor.project.clone();
    let id = editor.recovery.active_id().to_owned();
    editor.request(Pending::OpenPath(dir.join("missing.nflash")), &ctx);
    editor.continue_confirmation(&ctx); // Discard authorizes replacement only if load succeeds.
    assert_eq!(editor.project, before);
    assert_eq!(editor.recovery.active_id(), id);
    assert_eq!(recovery_files(&dir), 2);
    assert!(!editor.open_path_in_session(&ctx, dir.join("missing.nflash")));
    assert_eq!(editor.project, before);
    editor.transact(|e| {
        e.symbol_mut()
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
                frame: 0,
                script: neoflash_core::Script {
                    language: neoflash_core::ScriptLanguage::ActionScript2,
                    source: "var first=1;var second=2;".into(),
                },
            })
    });
    editor.workspace.code_editor.format_on_save = true;
    let before_failed_save = editor.project.clone();
    assert!(editor.close_session_tab(&ctx, 1));
    assert!(!editor.save_to_path(dir.join("missing-parent/file.nflash")));
    assert_eq!(
        editor.project, before_failed_save,
        "failed save rolls back automatic formatting"
    );
    assert_eq!(
        editor.confirm,
        Some(Pending::CloseSession(1)),
        "failed save leaves the choice pending"
    );
    editor.cancel_confirmation(&ctx);
    assert!(editor.close_session_tab(&ctx, 0));
    assert!(editor.save_to_path(dir.join("first.nflash")));
    editor.continue_confirmation(&ctx);
    assert_eq!(editor.session_count(), 1);
    assert_eq!(editor.project.name, "Second dirty");
    assert_eq!(recovery_files(&dir), 1);
    assert!(editor.close_session_tab(&ctx, 0));
    editor.continue_confirmation(&ctx);
    assert_eq!(editor.session_count(), 0);
    assert!(!editor.has_document());
    assert!(!editor.dirty());
    assert_eq!(editor.project.name, Project::blank().name);
    assert!(!editor.history.can_undo());
    assert_eq!(recovery_files(&dir), 0);
    editor.project.validate().unwrap();
}

#[test]
fn multiple_recoveries_restore_without_replacing_unsaved_work() {
    let (mut editor, ctx, dir) = setup("restore");
    editor.transact(|e| e.project.name = "Recovered one".into());
    editor.tick_autosave(0., 1000);
    editor.tick_autosave(60., 1060);
    editor.open_new_session_tab(&ctx, Project::blank(), None);
    editor.transact(|e| e.project.name = "Recovered two".into());
    editor.tick_autosave(70., 1070);
    editor.tick_autosave(130., 1130);
    assert_eq!(recovery_files(&dir), 2);
    let mut relaunched = Editor::from_project(&ctx, Project::blank());
    relaunched.recovery.set_dir(Some(dir.join("Recovery")));
    relaunched.check_recovery_on_launch();
    relaunched.restore_recovery_prompt(&ctx);
    let first = relaunched.project.clone();
    let first_index = relaunched.active_session;
    assert!(relaunched.dirty());
    assert!(relaunched.recovery.has_prompt());
    relaunched.restore_recovery_prompt(&ctx);
    assert_eq!(relaunched.session_count(), 3);
    assert_ne!(relaunched.project.name, first.name);
    assert!(relaunched.dirty());
    assert!(!relaunched.recovery.has_prompt());
    assert_eq!(recovery_files(&dir), 2);
    assert!(relaunched.switch_session_tab(&ctx, first_index));
    assert_eq!(relaunched.project, first);
    relaunched.request(Pending::Close, &ctx);
    relaunched.continue_confirmation(&ctx);
    relaunched.continue_confirmation(&ctx);
    assert!(relaunched.allow_close);
    assert_eq!(recovery_files(&dir), 0);

    // A clean original already open among several documents must not absorb/deduplicate
    // the recovery payload. Keep both the original and the recovered copy independently.
    let (mut editor, ctx, dir) = setup("restore-open-path");
    let path = dir.join("original.nflash");
    assert!(editor.save_to_path(path.clone()));
    let original = editor.project.clone();
    let mut recovered = original.clone();
    recovered.name = "Recovered separate copy".into();
    let mut recovery = editor.recovery.for_document(Some(&path));
    recovery
        .write_active(&recovered, Some(&path), &recovered.name, 2000)
        .unwrap();
    editor.new_document_session(&ctx);
    editor.switch_session_tab(&ctx, 0);
    // Closing the clean original with an unanswered launch prompt must keep the snapshot.
    let mut abandoned = Editor::from_project(&ctx, original.clone());
    abandoned.path = Some(path.clone());
    abandoned.recovery = editor.recovery.for_document(Some(&path));
    abandoned.check_recovery_on_launch();
    abandoned.request(Pending::Close, &ctx);
    assert!(abandoned.allow_close);
    assert_eq!(recovery_files(&dir), 1);
    editor.check_recovery_on_launch();
    let blocked = dir.join("blocked-recovery");
    std::fs::write(&blocked, b"not a directory").unwrap();
    editor.recovery.set_dir(Some(blocked));
    editor.restore_recovery_in_session(&ctx);
    assert_eq!(
        editor.project, original,
        "failed recovery copy cannot replace the original"
    );
    assert!(
        editor.recovery.has_prompt(),
        "failed restore retains its candidate"
    );
    editor.recovery.set_dir(Some(dir.join("Recovery")));
    editor.restore_recovery_in_session(&ctx);
    assert_eq!(editor.project, recovered);
    assert_eq!(
        editor.path, None,
        "conflicting recovered copy needs its own Save As path"
    );
    let recovered_index = editor.active_session;
    let recovered_id = editor.recovery.active_id().to_owned();
    assert_eq!(
        recovery_files(&dir),
        2,
        "conflicting recovery is copied before either document can clean it"
    );
    editor.switch_session_tab(&ctx, 0);
    assert_eq!(editor.project, original);
    assert!(!editor.dirty());
    assert_ne!(editor.recovery.active_id(), recovered_id);
    editor.transact(|e| e.project.name = "Original still being edited".into());
    editor.tick_autosave(0., 2100);
    editor.tick_autosave(60., 2160);
    editor.switch_session_tab(&ctx, recovered_index);
    assert!(editor.save_to_path(dir.join("recovered.nflash")));
    assert_eq!(
        recovery_files(&dir),
        1,
        "saving recovered copy preserves original autosave"
    );
    editor.switch_session_tab(&ctx, 0);
    assert_eq!(editor.project.name, "Original still being edited");
    assert!(editor.dirty());
}

#[test]
fn document_insert_order_close_others_and_all_are_lossless_until_every_decision() {
    let (mut editor, ctx, dir) = setup("close-batch");
    editor.transact(|e| e.project.name = "A".into());
    editor.new_document_session(&ctx);
    editor.transact(|e| e.project.name = "C".into());
    editor.switch_session_tab(&ctx, 0);
    editor.request(Pending::New, &ctx);
    editor.transact(|e| e.project.name = "B".into());
    assert_eq!(editor.active_session, 1, "New inserts after the active tab");
    assert_eq!(editor.sessions[0].as_ref().unwrap().project.name, "A");
    assert_eq!(editor.sessions[2].as_ref().unwrap().project.name, "C");
    editor.tick_autosave(0., 1000);
    editor.tick_autosave(60., 1060);
    assert_eq!(recovery_files(&dir), 3);
    editor.request_close_documents(&ctx, false);
    assert_eq!(editor.project.name, "A");
    assert_eq!(editor.confirm, Some(Pending::CloseSessions));
    editor.continue_confirmation(&ctx); // defer discard of A
    assert_eq!(editor.project.name, "B");
    editor.cancel_confirmation(&ctx);
    assert_eq!(
        editor.project.name, "B",
        "Cancel stays on the prompted document"
    );
    assert_eq!(editor.session_count(), 3);
    assert_eq!(recovery_files(&dir), 3);
    editor.request_close_documents(&ctx, true);
    assert_eq!(editor.project.name, "A");
    editor.continue_confirmation(&ctx);
    assert_eq!(editor.project.name, "C");
    editor.continue_confirmation(&ctx);
    assert_eq!(editor.session_count(), 1);
    assert_eq!(editor.project.name, "B");
    assert_eq!(recovery_files(&dir), 1);
    editor.undo();
    assert_eq!(editor.project.name, Project::blank().name);
    editor.redo();
    assert_eq!(editor.project.name, "B");
    editor.request_close_documents(&ctx, false);
    editor.continue_confirmation(&ctx);
    assert!(!editor.has_document());
    assert_eq!(recovery_files(&dir), 0);
    editor.request(Pending::New, &ctx);
    assert_eq!(editor.session_count(), 1);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
    editor.project.validate().unwrap();
}

#[test]
fn library_donor_session_tracks_tab_insertions_and_removals() {
    use crate::library::LibrarySource;

    let (mut editor, ctx, _dir) = setup("library-donor-rebase");
    editor.project.name = "A".into();
    let mut b = Project::blank();
    b.name = "B".into();
    editor.open_new_session_tab(&ctx, b, None);
    let mut c = Project::blank();
    c.name = "C donor".into();
    editor.open_new_session_tab(&ctx, c, None);
    assert_eq!(editor.active_session, 2);

    assert!(editor.switch_session_tab(&ctx, 0));
    editor.library_panel.source = LibrarySource::Session(2);
    assert_eq!(editor.library_source_project().name, "C donor");

    let mut inserted = Project::blank();
    inserted.name = "Inserted".into();
    assert_eq!(editor.open_new_session_tab(&ctx, inserted, None), 1);
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.library_panel.source, LibrarySource::Session(3));
    assert_eq!(editor.library_source_project().name, "C donor");

    assert!(editor.close_session_tab(&ctx, 1));
    assert_eq!(editor.project.name, "A");
    assert_eq!(editor.library_panel.source, LibrarySource::Session(2));
    assert_eq!(editor.library_source_project().name, "C donor");

    assert!(editor.close_session_tab(&ctx, 2));
    assert_eq!(editor.project.name, "A");
    assert_eq!(editor.library_panel.source, LibrarySource::ActiveDocument);
    assert_eq!(editor.library_source_project().name, "A");
}

#[test]
fn t231_relaunch_restores_validated_tab_set_active_tab_dedupes_and_preserves_recovery() {
    use crate::workspace::OnLaunch;

    let (mut prev, ctx, dir) = setup("t231-relaunch-tabs");
    let alpha_path = dir.join("Alpha.nflash");
    let beta_path = dir.join("Beta.nflash");
    let gamma_path = dir.join("Gamma.nflash");
    let delta_path = dir.join("Delta.nflash");

    prev.project.name = "Alpha".into();
    assert!(prev.save_to_path(alpha_path.clone()));

    let mut beta = Project::blank();
    beta.name = "Beta".into();
    prev.open_new_session_tab(&ctx, beta, None);
    assert!(prev.save_to_path(beta_path.clone()));

    let mut gamma = Project::blank();
    gamma.name = "Gamma".into();
    prev.open_new_session_tab(&ctx, gamma, None);
    assert!(prev.save_to_path(gamma_path.clone()));

    let mut delta = Project::blank();
    delta.name = "Delta".into();
    prev.open_new_session_tab(&ctx, delta, None);
    assert!(prev.save_to_path(delta_path.clone()));

    // User focuses Beta (tab 1), while Gamma (tab 2) has an unsaved edit before Quit.
    assert!(prev.switch_session_tab(&ctx, 2));
    prev.transact(|e| e.project.name = "Gamma Unsaved".into());
    assert!(prev.switch_session_tab(&ctx, 1));
    assert_eq!(prev.project.name, "Beta");
    prev.request_quit(&ctx);
    assert_eq!(prev.confirm, Some(Pending::Close));
    assert_eq!(prev.project.name, "Gamma Unsaved");
    prev.approve_quit_document(&ctx);
    assert!(prev.allow_close);

    // Saved prefs must preserve all 4 tabs in order AND keep Beta (the pre-quit active tab) active.
    let mut saved_prefs = prev.workspace.clone();
    saved_prefs.general.on_launch = OnLaunch::LastDocument;
    assert_eq!(saved_prefs.general.session_files.len(), 4);
    // Inject a duplicate non-canonical Alpha entry; delete Gamma; corrupt Delta.
    saved_prefs
        .general
        .session_files
        .push(dir.join("./Alpha.nflash").to_string_lossy().into_owned());
    std::fs::remove_file(&gamma_path).unwrap();
    std::fs::write(&delta_path, "{corrupt json").unwrap();

    // Simulate an unprompted crash-recovery snapshot for Alpha present on disk before relaunch.
    let mut crashed_alpha = Project::blank();
    crashed_alpha.name = "Alpha Crashed Copy".into();
    let mut crash_writer = prev.recovery.for_document(Some(&alpha_path));
    crash_writer
        .write_active(&crashed_alpha, Some(&alpha_path), "Alpha", 4200)
        .unwrap();
    assert_eq!(recovery_files(&dir), 1);

    let mut relaunched = Editor::from_project(&ctx, Project::demo());
    relaunched.workspace = saved_prefs;
    relaunched.recovery.set_dir(Some(dir.join("Recovery")));
    relaunched.apply_on_launch(&ctx);
    relaunched.check_recovery_on_launch();

    assert_eq!(
        relaunched.session_count(),
        2,
        "only valid unique tabs Alpha and Beta are restored"
    );
    assert_eq!(
        relaunched.active_session, 1,
        "pre-quit active tab Beta is restored as active"
    );
    assert_eq!(relaunched.project.name, "Beta");
    assert_eq!(
        relaunched.sessions[0].as_ref().unwrap().project.name,
        "Alpha"
    );
    assert!(
        relaunched.status.contains("skipped 2"),
        "expected skipped diagnostic, got: {}",
        relaunched.status
    );
    assert_eq!(
        recovery_files(&dir),
        1,
        "unprompted crash recovery file must survive session-tab restoration"
    );
    assert!(relaunched.recovery.has_prompt());

    // Restoring the crash recovery opens an independent 3rd tab without overwriting clean Alpha.
    relaunched.restore_recovery_prompt(&ctx);
    assert_eq!(relaunched.session_count(), 3);
    assert_eq!(relaunched.project.name, "Alpha Crashed Copy");
    assert!(relaunched.path.is_none());
    assert!(relaunched.switch_session_tab(&ctx, 0));
    assert_eq!(relaunched.project.name, "Alpha");
    assert!(!relaunched.dirty());
}

#[test]
fn t231_stage_scale9_guide_drag_previews_cancels_on_escape_and_commits_one_undo() {
    use neoflash_core::{Symbol, SymbolKind};

    let (mut editor, ctx, _dir) = setup("t231-scale9-drag");
    let blank_tl = Project::blank().symbols[0].timeline.clone();
    let mut sym = Symbol {
        id: "sym-card".into(),
        name: "Card".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: blank_tl,
    };
    sym.set_scale9_grid(Some([20.0, 15.0, 100.0, 60.0]));
    editor.project.symbols.push(sym);
    editor.enter_symbol("sym-card".into());
    assert_eq!(editor.symbol().id, "sym-card");
    editor.stage_view.prefs.rulers = false;
    editor.zoom = 1.0;

    let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 400.0));
    let origin = canvas.center();
    let mapping = crate::stage_mapping::StageMapping::identity();

    let run_events = |editor: &mut Editor, events: Vec<egui::Event>, hover: Option<egui::Pos2>| {
        let mut raw = egui::RawInput {
            screen_rect: Some(canvas),
            events,
            ..Default::default()
        };
        if let Some(h) = hover {
            raw.events.insert(0, egui::Event::PointerMoved(h));
        }
        let mut icon = egui::CursorIcon::Default;
        let out = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.stage_view.guide_pointer = false;
                editor.scale9_guide_input(ui, canvas, origin, mapping);
            });
            icon = ctx.output(|o| o.cursor_icon);
        });
        let _ = out;
        icon
    };

    // 1. Hover over left vertical guide (x = 20 in isolated space -> origin.x + 20) sets ResizeHorizontal.
    let left_guide_pos = egui::pos2(origin.x + 20.0, origin.y + 30.0);
    let hover_icon = run_events(&mut editor, vec![], Some(left_guide_pos));
    assert_eq!(hover_icon, egui::CursorIcon::ResizeHorizontal);

    // 2. Press on left guide, drag to x = 42, then Escape cancels preview with zero Undo commits.
    let _ = run_events(
        &mut editor,
        vec![
            egui::Event::PointerButton {
                pos: left_guide_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(egui::pos2(origin.x + 42.0, origin.y + 30.0)),
        ],
        None,
    );
    assert_eq!(
        editor.scale9_guide_drag.as_ref().map(|d| d.preview_grid),
        Some([42.0, 15.0, 78.0, 60.0])
    );
    assert_eq!(editor.history.undo_len(), 0);

    let _ = run_events(
        &mut editor,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        None,
    );
    assert!(editor.scale9_guide_drag.is_none());
    assert_eq!(
        editor.project.symbol("sym-card").unwrap().scale9_grid(),
        Some([20.0, 15.0, 100.0, 60.0])
    );
    assert_eq!(editor.history.undo_len(), 0);

    // 3. Drag right guide (x = 120) to x = 155 and release -> commits exactly 1 Undo step.
    let right_guide_pos = egui::pos2(origin.x + 120.0, origin.y + 30.0);
    let _ = run_events(
        &mut editor,
        vec![
            egui::Event::PointerButton {
                pos: right_guide_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(egui::pos2(origin.x + 140.0, origin.y + 30.0)),
            egui::Event::PointerMoved(egui::pos2(origin.x + 155.0, origin.y + 30.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(origin.x + 155.0, origin.y + 30.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        None,
    );
    assert_eq!(
        editor.project.symbol("sym-card").unwrap().scale9_grid(),
        Some([20.0, 15.0, 135.0, 60.0])
    );
    assert_eq!(editor.history.undo_len(), 1);
    assert_eq!(editor.history.undo_label(), Some("9-slice scaling"));
    editor
        .project
        .validate()
        .expect("project validates after 9-slice drag");
    editor.undo();
    assert_eq!(
        editor.project.symbol("sym-card").unwrap().scale9_grid(),
        Some([20.0, 15.0, 100.0, 60.0])
    );
    editor.redo();
    assert_eq!(
        editor.project.symbol("sym-card").unwrap().scale9_grid(),
        Some([20.0, 15.0, 135.0, 60.0])
    );
}

#[test]
fn t231_shared_library_external_nflash_mount_and_disk_url_sync_are_collision_safe_and_idempotent() {
    use neoflash_core::{
        Element, ElementKind, InstancePlayback, ResourceUnit, SharedLibraryLinkage, Symbol,
        SymbolKind,
    };

    let (mut editor, _ctx, dir) = setup("t231-shared-lib");
    let blank_tl = || Project::blank().symbols[0].timeline.clone();
    let libs_dir = dir.join("libs");
    std::fs::create_dir_all(&libs_dir).unwrap();
    let donor_path = libs_dir.join("UIKit.nflash");
    let recipient_path = dir.join("App.nflash");

    //Tiny 1x1 PNG fixtures with distinct bytes to prove resource collision safety.
    let make_png = |rgba: [u8; 4]| -> Vec<u8> {
        let mut bytes = Vec::new();
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba));
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        bytes
    };
    let local_png = make_png([220, 40, 40, 255]);
    let donor_png = make_png([30, 140, 250, 255]);

    // Recipient owns colliding IDs `sym-collide`, `layer-collide`, `el-collide`, `res-collide`.
    editor.project.resources.push(
        ResourceUnit::from_bytes("res-collide", "assets/logo.png", "image/png", &local_png)
            .unwrap(),
    );
    let mut local_unrelated = Symbol {
        id: "sym-collide".into(),
        name: "LocalHero".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: blank_tl(),
    };
    local_unrelated.timeline.layers[0].id = "layer-collide".into();
    local_unrelated.timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "el-collide".into(),
            kind: ElementKind::Bitmap {
                resource_id: "res-collide".into(),
                width: 1,
                height: 1,
                smoothing: true,
            },
            ..Element::default()
        });
    editor.project.symbols.push(local_unrelated);

    let badge_placeholder = Symbol {
        id: "sym-local-badge".into(),
        name: "Badge".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: Some("ui.Badge".into()),
        timeline: blank_tl(),
    };
    editor.project.symbols.push(badge_placeholder);
    editor.project.set_shared_library(
        "sym-local-badge",
        Some(SharedLibraryLinkage {
            symbol_id: "sym-local-badge".into(),
            url: "libs/UIKit.nflash".into(),
            export_for_runtime: false,
            import_for_runtime: true,
            always_update: false,
        }),
    );
    editor.project.upgrade_format_for_features();
    assert!(editor.save_to_path(recipient_path.clone()));

    // Donor `UIKit.nflash` exports `Badge` which nests `sym-collide` (`DonorLeaf`) and `res-collide` (`assets/logo.png`).
    let mut donor = Project::blank();
    donor.name = "UIKit".into();
    donor.resources.push(
        ResourceUnit::from_bytes("res-collide", "assets/logo.png", "image/png", &donor_png)
            .unwrap(),
    );
    let mut donor_leaf = Symbol {
        id: "sym-collide".into(),
        name: "DonorLeaf".into(),
        folder: "Shared/Icons".into(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: blank_tl(),
    };
    donor_leaf.timeline.layers[0].id = "layer-collide".into();
    donor_leaf.timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "el-collide".into(),
            kind: ElementKind::Bitmap {
                resource_id: "res-collide".into(),
                width: 1,
                height: 1,
                smoothing: true,
            },
            ..Element::default()
        });
    let _ = donor.add_library_folder("Shared/Icons");
    donor.symbols.push(donor_leaf);

    let mut donor_badge = Symbol {
        id: "donor-badge".into(),
        name: "Badge".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: Some("ui.Badge".into()),
        timeline: blank_tl(),
    };
    donor_badge.timeline.layers[0].id = "layer-badge".into();
    donor_badge.timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "el-badge".into(),
            kind: ElementKind::Instance {
                symbol_id: "sym-collide".into(),
                playback: InstancePlayback::Loop,
            },
            ..Element::default()
        });
    donor.symbols.push(donor_badge);
    donor.set_shared_library(
        "donor-badge",
        Some(SharedLibraryLinkage {
            symbol_id: "donor-badge".into(),
            url: "libs/UIKit.nflash".into(),
            export_for_runtime: true,
            import_for_runtime: false,
            always_update: false,
        }),
    );
    donor.upgrade_format_for_features();
    donor.validate().expect("donor validates");
    std::fs::write(&donor_path, donor.to_json().unwrap()).unwrap();

    // 1. Disk URL sync resolves `libs/UIKit.nflash` relative to `App.nflash` without overwriting local collisions!
    assert_eq!(editor.sync_shared_libraries_from_open_documents(), 1);
    editor
        .project
        .validate()
        .expect("recipient validates after collision-safe sync");
    assert_eq!(editor.history.undo_label(), Some("Sync shared library"));
    assert_eq!(
        editor.project.symbol("sym-collide").unwrap().name,
        "LocalHero"
    );
    let local_res_bytes = editor
        .project
        .resources
        .iter()
        .find(|r| r.id == "res-collide")
        .unwrap()
        .decoded_bytes()
        .unwrap();
    assert_eq!(local_res_bytes, local_png);
    assert!(
        editor
            .project
            .symbols
            .iter()
            .any(|s| s.name == "DonorLeaf" && s.id != "sym-collide"),
        "nested DonorLeaf imported under a non-colliding ID"
    );

    // 2. Repeated sync is idempotent (no duplicate symbols or resources added).
    let sym_count_after_first = editor.project.symbols.len();
    let res_count_after_first = editor.project.resources.len();
    assert_eq!(editor.sync_shared_libraries_from_open_documents(), 1);
    assert_eq!(editor.project.symbols.len(), sym_count_after_first);
    assert_eq!(editor.project.resources.len(), res_count_after_first);
    editor
        .project
        .validate()
        .expect("recipient validates after second sync");

    // 3. External `.nflash` path mount + self-mount guard + close_external_library.
    assert!(
        editor.open_external_library_path(&recipient_path).is_err(),
        "mounting the active document as its own external library must be rejected"
    );
    editor
        .open_external_library_path(&donor_path)
        .expect("mount UIKit.nflash");
    assert_eq!(
        Editor::format_shared_library_url_for_project(Some(&recipient_path), &donor_path),
        "libs/UIKit.nflash"
    );
    assert_eq!(editor.library_source_project().name, "UIKit");
    editor.close_external_library();
    assert!(editor.library_panel.external_library.is_none());
    assert_eq!(editor.library_source_project().name, editor.project.name);

    // Undo restores clean pre-sync state cleanly.
    editor.undo();
    editor.undo();
    assert!(!editor.project.symbols.iter().any(|s| s.name == "DonorLeaf"));
    editor.project.validate().unwrap();
}
