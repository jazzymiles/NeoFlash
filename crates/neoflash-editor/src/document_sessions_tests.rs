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
fn new_document_starts_with_one_frame_and_round_trips_that_duration() {
    let (mut editor, ctx, dir) = setup("new-one-frame");
    editor.new_document_session(&ctx);

    let timeline = &editor
        .project
        .symbol(&editor.project.root)
        .unwrap()
        .timeline;
    assert_eq!(timeline.frame_count, 1);
    assert_eq!(timeline.layers.len(), 1);
    assert_eq!(timeline.layers[0].keyframes.len(), 1);
    assert_eq!(timeline.layers[0].keyframes[0].frame, 0);
    editor.project.validate().unwrap();

    let path = dir.join("one-frame.nflash");
    assert!(editor.save_to_path(path.clone()));
    let reopened = Project::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
    let reopened_timeline = &reopened.symbol(&reopened.root).unwrap().timeline;
    assert_eq!(reopened_timeline.frame_count, 1);
    assert_eq!(reopened_timeline.layers[0].keyframes.len(), 1);
    reopened.validate().unwrap();
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
                layer: None,
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

#[test]
fn t276_fresh_profile_authoring_conflict_recovery_test_movie_and_publish_journey() {
    use crate::recovery::ExternalFileChange;
    use crate::selection::SelectionTarget;
    use neoflash_core::{
        Element, ElementKind, FrameScript, InstancePlayback, Keyframe, Layer, ObjectEvent,
        ObjectScript, ResourceUnit, Script, ScriptLanguage, Symbol, SymbolKind, TextAlign,
        TextFormat, Transform, TweenKind, evaluate_scene,
    };

    let ctx = egui::Context::default();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/t006-documents/t276-fresh-profile");
    let _ = std::fs::remove_dir_all(&dir);
    let profile_dir = dir.join("Profile");
    let recovery_root = profile_dir.join("Recovery");
    let work_dir = dir.join("Workspace");
    let publish_dir = dir.join("Publish");
    std::fs::create_dir_all(&recovery_root).unwrap();
    std::fs::create_dir_all(&work_dir).unwrap();
    std::fs::create_dir_all(&publish_dir).unwrap();

    // 1. Fresh document in an isolated workspace/recovery root.
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.recovery.set_dir(Some(recovery_root.clone()));
    editor.workspace.general.autosave = true;
    editor.workspace.general.autosave_minutes = 1;
    assert_eq!(editor.project.name, Project::blank().name);
    assert!(!editor.dirty());
    assert_eq!(editor.edit_depth(), 1);
    editor.project.validate().expect("blank project validates");

    // 2. Vector & Text authoring.
    let root_layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.transact_named("Draw vector shape", |ed| {
        let root_id = ed.project.root.clone();
        let kf = &mut ed.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0];
        kf.elements.push(Element {
            id: "vec-hero".into(),
            name: "HeroRect".into(),
            transform: Transform {
                x: 180.0,
                y: 160.0,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 140.0,
                height: 80.0,
                fill: [46, 134, 222, 255],
                stroke: [20, 40, 80, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        });
    });
    editor.set_selection(vec![SelectionTarget {
        layer: root_layer_id.clone(),
        element: "vec-hero".into(),
    }]);

    editor.transact_named("Place formatted text box", |ed| {
        let root_id = ed.project.root.clone();
        let kf = &mut ed.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0];
        kf.elements.push(Element {
            id: "txt-banner".into(),
            name: "BannerText".into(),
            transform: Transform {
                x: 320.0,
                y: 90.0,
                ..Transform::default()
            },
            kind: ElementKind::TextBox {
                width: 220.0,
                height: 48.0,
                text: "Beta 1 Fresh Journey".into(),
                format: TextFormat {
                    font: "Inter".into(),
                    size: 22.0,
                    bold: true,
                    color: 0xF5F7FA,
                    align: TextAlign::Center,
                    ..TextFormat::default()
                },
                word_wrap: true,
                multiline: false,
            },
            ..Element::default()
        });
    });
    editor
        .project
        .validate()
        .expect("project validates after vector & text authoring");

    // 3. Symbol Enter & One-Level Exit (Root -> OuterClip -> InnerClip, depth 3 -> 2 -> 1).
    let blank_tl = Project::blank().symbols[0].timeline.clone();
    editor.transact_named("Create nested MovieClip hierarchy", |ed| {
        let mut inner_tl = blank_tl.clone();
        inner_tl.layers = vec![Layer {
            id: "layer-inner".into(),
            name: "Inner Layer".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "el-inner-dot".into(),
                    name: "InnerDot".into(),
                    transform: Transform {
                        x: 220.0,
                        y: 180.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 16.0,
                        height: 16.0,
                        fill: [250, 180, 40, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            }],
            properties: Default::default(),
        }];
        ed.project.symbols.push(Symbol {
            id: "sym-inner".into(),
            name: "InnerClip".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: inner_tl,
        });

        let mut outer_tl = blank_tl.clone();
        outer_tl.layers = vec![Layer {
            id: "layer-outer".into(),
            name: "Outer Layer".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "inst-inner".into(),
                    name: "innerMc".into(),
                    transform: Transform {
                        x: 10.0,
                        y: 10.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-inner".into(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            }],
            properties: Default::default(),
        }];
        ed.project.symbols.push(Symbol {
            id: "sym-outer".into(),
            name: "OuterClip".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: outer_tl,
        });

        let root_id = ed.project.root.clone();
        let kf = &mut ed.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0];
        kf.elements.push(Element {
            id: "inst-outer".into(),
            name: "outerMc".into(),
            transform: Transform {
                x: 420.0,
                y: 260.0,
                ..Transform::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym-outer".into(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });
    });

    editor.set_selection(vec![SelectionTarget {
        layer: root_layer_id.clone(),
        element: "inst-outer".into(),
    }]);
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.edit_depth(), 2);
    assert_eq!(editor.symbol_id(), "sym-outer");

    editor.set_selection(vec![SelectionTarget {
        layer: "layer-outer".into(),
        element: "inst-inner".into(),
    }]);
    assert!(editor.enter_selected_symbol_in_place());
    assert_eq!(editor.edit_depth(), 3);
    assert_eq!(editor.symbol_id(), "sym-inner");

    // Warm up Stage layout and perform empty-Stage double-click.
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut warmup_input = egui::RawInput {
        screen_rect: Some(screen_rect),
        time: Some(10.0),
        ..Default::default()
    };
    let _ = ctx.run(warmup_input.take(), |ctx| {
        editor.stage(ctx);
    });
    let empty_pos = egui::pos2(80.0, 120.0);

    let double_click_empty_stage =
        |editor: &mut Editor, ctx: &egui::Context, base_time: f64, pos: egui::Pos2| {
            for (dt, events) in [
                (
                    0.0,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                ),
                (
                    0.12,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                ),
                (
                    0.16,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                ),
            ] {
                let mut raw = egui::RawInput {
                    screen_rect: Some(screen_rect),
                    time: Some(base_time + dt),
                    events,
                    ..Default::default()
                };
                let _ = ctx.run(raw.take(), |ctx| {
                    editor.stage(ctx);
                });
            }
        };

    // First empty-Stage double-click exits ONE level: depth 3 (InnerClip) -> depth 2 (OuterClip).
    double_click_empty_stage(&mut editor, &ctx, 11.0, empty_pos);
    assert_eq!(
        editor.edit_depth(),
        2,
        "double-clicking empty Stage at depth 3 must exit exactly one level to depth 2"
    );
    assert_eq!(editor.symbol_id(), "sym-outer");

    // Second empty-Stage double-click exits from depth 2 (OuterClip) -> depth 1 (Root).
    double_click_empty_stage(&mut editor, &ctx, 12.5, empty_pos);
    assert_eq!(
        editor.edit_depth(),
        1,
        "second empty-Stage double-click at depth 2 must exit to depth 1 (Root)"
    );
    assert_eq!(editor.symbol_id(), editor.project.root);

    // 4. Tween & Frame/Object Script.
    editor.layer = 0;
    editor.frame = 9;
    editor.make_key(false);
    editor.transact_named("Move outer instance at frame 10", |ed| {
        let root_id = ed.project.root.clone();
        let kf = ed.project.symbol_mut(&root_id).unwrap().timeline.layers[0]
            .keyframes
            .iter_mut()
            .find(|k| k.frame == 9)
            .unwrap();
        let inst = kf
            .elements
            .iter_mut()
            .find(|el| el.id == "inst-outer")
            .unwrap();
        inst.transform.x = 620.0;
        inst.transform.y = 340.0;
    });
    editor.frame = 0;
    editor.create_tween(TweenKind::Classic);

    editor.transact_named("Attach frame and instance scripts", |ed| {
        let root_id = ed.project.root.clone();
        let root_sym = ed.project.symbol_mut(&root_id).unwrap();
        root_sym.timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace(\"t276_frame_ok\");".into(),
            },
        });
        let kf = &mut root_sym.timeline.layers[0].keyframes[0];
        let inst = kf
            .elements
            .iter_mut()
            .find(|el| el.id == "inst-outer")
            .unwrap();
        inst.scripts.push(ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace(\"t276_click_ok\");".into(),
            },
        });
    });

    let mid_scene = evaluate_scene(&editor.project, &editor.project.root, 5, false, true);
    let mid_outer = mid_scene
        .draws
        .iter()
        .find(|d| d.item.instance_path.first().map(String::as_str) == Some("inst-outer"))
        .expect("outer instance rendered at frame 5");
    assert!(
        mid_outer.item.transform[4] > 420.0,
        "classic tween interpolates X at mid-span ({})",
        mid_outer.item.transform[4]
    );
    editor
        .project
        .validate()
        .expect("project validates after tween & scripts");

    // 5. Multi-step Undo / Redo across authoring operations.
    let authored_snapshot = editor.project.clone();
    for step in 0..5 {
        assert!(editor.history.can_undo(), "can undo step {step}");
        editor.undo();
        editor
            .project
            .validate()
            .expect("project validates after undo");
    }
    assert_ne!(editor.project, authored_snapshot);
    for step in 0..5 {
        assert!(editor.history.can_redo(), "can redo step {step}");
        editor.redo();
        editor
            .project
            .validate()
            .expect("project validates after redo");
    }
    assert_eq!(editor.project, authored_snapshot);

    // 6. Bitmap & Video Import in single Undo steps.
    let mut rgba_img = image::RgbaImage::new(4, 4);
    for pixel in rgba_img.pixels_mut() {
        *pixel = image::Rgba([32, 201, 151, 255]);
    }
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(rgba_img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let bitmap_unit =
        ResourceUnit::from_bytes("res-badge", "badge.png", "image/png", &png_bytes).unwrap();
    editor
        .add_resource_units(vec![bitmap_unit])
        .expect("import PNG bitmap resource");
    editor
        .place_graphic_resource("res-badge", Some([120.0, 300.0]))
        .expect("place bitmap on stage");
    editor
        .project
        .validate()
        .expect("validates after bitmap placement");
    editor.undo();
    editor.redo();
    editor
        .project
        .validate()
        .expect("validates after bitmap undo/redo");

    let before_video = editor.project.clone();
    let frame_a = [40u8, 120, 240, 255].repeat(16 * 16);
    let frame_b = [240u8, 120, 40, 255].repeat(16 * 16);
    let mp4_bytes =
        neoflash_export::encode_deterministic_mp4_rgba_frames(16, 16, 12, &[&frame_a, &frame_b]);
    let mp4_path = work_dir.join("clip.mp4");
    std::fs::write(&mp4_path, &mp4_bytes).unwrap();
    editor
        .import_video_path_at(&ctx, &mp4_path, [260.0, 300.0])
        .expect("import MP4 video onto stage");
    editor
        .project
        .validate()
        .expect("validates after MP4 import");
    assert!(
        editor.project.resources.iter().any(|r| r.is_video()),
        "video resource added"
    );
    // Single-step Undo removes the entire video import transaction cleanly.
    editor.undo();
    assert_eq!(editor.project.resources.len(), before_video.resources.len());
    editor.project.validate().unwrap();
    editor.redo();
    assert!(editor.project.resources.iter().any(|r| r.is_video()));
    editor.project.validate().unwrap();

    // 7. Save -> Unsaved-close prompt protection -> Quit -> Reopen with 100% fidelity.
    let doc_path = work_dir.join("Beta1Journey.nflash");
    assert!(editor.save_to_path(doc_path.clone()));
    assert!(!editor.dirty());
    assert!(doc_path.is_file());

    editor.transact_named("Post-save title edit", |ed| {
        ed.project.name = "Beta1Journey Saved".into();
    });
    assert!(editor.dirty());

    // Unsaved-close prompt protection: Cancel preserves dirty state and blocks quit.
    editor.request_quit(&ctx);
    assert_eq!(editor.confirm, Some(Pending::Close));
    assert!(!editor.allow_close);
    editor.cancel_confirmation(&ctx);
    assert!(editor.confirm.is_none());
    assert!(!editor.allow_close);
    assert!(editor.dirty());

    // Unsaved-close prompt protection: Save writes cleanly and authorizes quit.
    editor.request_quit(&ctx);
    assert_eq!(editor.confirm, Some(Pending::Close));
    let saved_ok = editor.save(false);
    assert!(saved_ok);
    editor.resolve_confirmation_save(&ctx, saved_ok);
    assert!(!editor.dirty());
    assert!(editor.allow_close);

    let saved_fidelity_snapshot = editor.project.clone();
    eframe::App::on_exit(&mut editor);
    assert_eq!(
        recovery_files(&profile_dir),
        0,
        "clean quit removes transient recovery files"
    );

    let mut reopened = Editor::from_project(&ctx, Project::blank());
    reopened.recovery.set_dir(Some(recovery_root.clone()));
    reopened.workspace.general.autosave = true;
    reopened.workspace.general.autosave_minutes = 1;
    assert!(reopened.try_open_path(doc_path.clone(), &ctx));
    assert_eq!(reopened.project, saved_fidelity_snapshot);
    assert!(!reopened.dirty());
    reopened
        .project
        .validate()
        .expect("reopened project validates with 100% fidelity");

    // 8. External-File Conflict (Modified, Replaced, Deleted + Keep editing, Save As, Reload).
    reopened.transact_named("Local in-memory edit", |ed| {
        ed.project.name = "In-Memory Local Work".into();
    });
    assert!(reopened.dirty());

    // 8a. External Modified: in-place disk write.
    let mut disk_modified = saved_fidelity_snapshot.clone();
    disk_modified.name = "Externally Modified On Disk".into();
    std::fs::write(&doc_path, disk_modified.to_json().unwrap()).unwrap();
    let _ = reopened.check_external_file_changes();
    assert_eq!(
        reopened.external_file_change(),
        Some(ExternalFileChange::Modified)
    );
    // Ordinary Save must be guarded against silently overwriting conflicting disk state.
    assert!(
        !reopened.save(false),
        "ordinary Save must be blocked while external file conflict is unresolved"
    );
    let on_disk_after_blocked_save =
        Project::from_json(&std::fs::read_to_string(&doc_path).unwrap()).unwrap();
    assert_eq!(
        on_disk_after_blocked_save.name,
        "Externally Modified On Disk"
    );

    // Keep editing dismisses the conflict banner while preserving local in-memory changes.
    assert!(reopened.keep_editing_external_change());
    assert!(reopened.external_file_change().is_none());
    assert_eq!(reopened.project.name, "In-Memory Local Work");

    // 8b. External Replaced (atomic rename changing inode) + Save As preserving both versions.
    let mut disk_replaced = saved_fidelity_snapshot.clone();
    disk_replaced.name = "Externally Replaced Via Atomic Rename".into();
    let replacement_tmp = work_dir.join("Beta1Journey.replace.tmp");
    std::fs::write(&replacement_tmp, disk_replaced.to_json().unwrap()).unwrap();
    std::fs::rename(&replacement_tmp, &doc_path).unwrap();
    let _ = reopened.check_external_file_changes();
    assert_eq!(
        reopened.external_file_change(),
        Some(ExternalFileChange::Replaced)
    );
    let forked_path = work_dir.join("Beta1Journey-SaveAs.nflash");
    assert!(reopened.save_as_external_change(forked_path.clone()));
    assert!(reopened.external_file_change().is_none());
    assert!(!reopened.dirty());
    let disk_original = Project::from_json(&std::fs::read_to_string(&doc_path).unwrap()).unwrap();
    let disk_forked = Project::from_json(&std::fs::read_to_string(&forked_path).unwrap()).unwrap();
    assert_eq!(disk_original.name, "Externally Replaced Via Atomic Rename");
    assert_eq!(disk_forked.name, "In-Memory Local Work");

    // 8c. Reload validates disk file before replacing memory (rejects corrupt JSON, accepts valid JSON).
    assert!(reopened.open_path_in_session(&ctx, doc_path.clone()));
    std::fs::write(&doc_path, "{ corrupt json payload").unwrap();
    let _ = reopened.check_external_file_changes();
    assert!(reopened.external_file_change().is_some());
    assert!(
        !reopened.reload_external_modification(&ctx),
        "Reload must reject corrupt disk JSON and preserve in-memory document"
    );
    assert_eq!(
        reopened.project.name,
        "Externally Replaced Via Atomic Rename"
    );

    let mut disk_reloaded = saved_fidelity_snapshot.clone();
    disk_reloaded.name = "Disk Reloaded Validated".into();
    std::fs::write(&doc_path, disk_reloaded.to_json().unwrap()).unwrap();
    let _ = reopened.check_external_file_changes();
    assert!(reopened.reload_external_modification(&ctx));
    assert!(reopened.external_file_change().is_none());
    assert_eq!(reopened.project.name, "Disk Reloaded Validated");

    // 8d. External Deleted warns and allows saving back without losing memory state.
    std::fs::remove_file(&doc_path).unwrap();
    let _ = reopened.check_external_file_changes();
    assert_eq!(
        reopened.external_file_change(),
        Some(ExternalFileChange::Deleted)
    );
    assert!(reopened.keep_editing_external_change());
    assert!(reopened.dirty());
    assert!(reopened.save_to_path(doc_path.clone()));
    assert!(reopened.external_file_change().is_none());

    // 9. Crash Recovery (Restore into independent accessible tab without overwriting disk, plus Discard).
    reopened.transact_named("Unsaved pre-crash edit", |ed| {
        ed.project.name = "Recovered After Crash".into();
    });
    reopened.tick_autosave(0.0, 1_700_000_000);
    reopened.tick_autosave(120.0, 1_700_000_120);
    assert!(
        recovery_files(&profile_dir) >= 1,
        "autosave wrote recovery snapshot"
    );
    // Simulate unclean crash (do not set allow_close = true).
    drop(reopened);

    let mut after_crash = Editor::from_project(&ctx, Project::blank());
    after_crash.recovery.set_dir(Some(recovery_root.clone()));
    after_crash.workspace.general.autosave = true;
    after_crash.workspace.general.autosave_minutes = 1;
    after_crash.check_recovery_on_launch();
    assert!(
        after_crash.recovery.has_prompt(),
        "relaunch after unclean exit detects recovery snapshot"
    );
    after_crash.restore_recovery_prompt(&ctx);
    assert!(!after_crash.recovery.has_prompt());
    assert_eq!(after_crash.project.name, "Recovered After Crash");
    assert!(after_crash.dirty());
    // Existing disk file must not be overwritten by crash recovery restoration.
    let disk_untouched = Project::from_json(&std::fs::read_to_string(&doc_path).unwrap()).unwrap();
    assert_eq!(disk_untouched.name, "Disk Reloaded Validated");

    // Verify Discard on a second crash snapshot removes the recovery snapshot cleanly.
    after_crash.tick_autosave(0.0, 1_700_000_200);
    after_crash.tick_autosave(120.0, 1_700_000_320);
    drop(after_crash);
    let mut after_second_crash = Editor::from_project(&ctx, Project::blank());
    after_second_crash
        .recovery
        .set_dir(Some(recovery_root.clone()));
    after_second_crash.check_recovery_on_launch();
    assert!(after_second_crash.recovery.has_prompt());
    after_second_crash.discard_recovery_prompt();
    assert!(!after_second_crash.recovery.has_prompt());
    assert_eq!(recovery_files(&profile_dir), 0);

    // 10. Test Movie & HTML5 Publish (index.html, bundle.js, neoflash_web.js, neoflash_web_bg.wasm).
    let mut player = neoflash_core::Player::new(&saved_fidelity_snapshot);
    let dispatches = player.drain_scripts();
    assert!(
        dispatches
            .iter()
            .any(|d| d.script.source.contains("t276_frame_ok")),
        "Test Movie player dispatches frame 0 script: {dispatches:?}"
    );

    let preview_dir = dir.join("TestMoviePreview");
    let preview_index = neoflash_export::export_project_with_config(
        &saved_fidelity_snapshot,
        &preview_dir,
        &Editor::runtime_dir(),
        &neoflash_export::PublishOptions::default(),
        &after_second_crash.workspace.publish.preview_html_config(),
    )
    .expect("Test Movie preview export succeeds");
    assert!(preview_index.is_file());

    let publish_options = neoflash_export::PublishOptions {
        size: neoflash_export::SizeLevel::Balanced,
        ..Default::default()
    };
    let publish_index = neoflash_export::export_project_with_config(
        &saved_fidelity_snapshot,
        &publish_dir,
        &Editor::runtime_dir(),
        &publish_options,
        &after_second_crash.workspace.publish.publish_html_config(),
    )
    .expect("HTML5 Publish succeeds with zero errors");
    assert!(publish_index.is_file());
    assert!(publish_dir.join("index.html").is_file());
    assert!(publish_dir.join("bundle.js").is_file());
    assert!(publish_dir.join("neoflash_web.js").is_file());
    assert!(publish_dir.join("neoflash_web_bg.wasm").is_file());
}

#[test]
fn t291_document_sessions_isolate_output_actions_test_movie_and_breakpoints() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    assert!(editor.output.is_empty());

    editor.message("doc0 trace output");
    editor.open_test_movie_preview();
    assert!(editor.test_movie_preview_open);
    let tab0_editor_id = editor.actions_editor_id();
    assert!(editor.toggle_breakpoint("Scene 1 / frame 1", 3));
    assert_eq!(editor.debugger.breakpoints.len(), 1);
    assert_eq!(editor.debugger.breakpoints[0].source, "Scene 1 / frame 1");
    assert_eq!(editor.debugger.breakpoints[0].line, 3);

    let tab1_index = editor.open_new_session_tab(&ctx, Project::blank(), None);
    assert_eq!(tab1_index, 1);
    assert!(
        editor.output.is_empty(),
        "new document tab starts with empty Output"
    );
    assert!(
        !editor.test_movie_preview_open,
        "Test Movie preview does not leak to Tab 1"
    );
    assert_ne!(editor.actions_editor_id(), tab0_editor_id);
    assert!(
        editor.debugger.breakpoints.is_empty(),
        "Tab 0 breakpoints do not leak into Tab 1 runtime state"
    );
    assert!(!editor.debugger.has_presentation());

    assert!(editor.toggle_breakpoint("Main.as", 9));
    editor.message("doc1 warning");
    assert_eq!(editor.debugger.breakpoints.len(), 1);
    assert_eq!(editor.debugger.breakpoints[0].source, "Main.as");

    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.output, vec!["doc0 trace output".to_owned()]);
    assert!(editor.test_movie_preview_open);
    assert_eq!(editor.actions_editor_id(), tab0_editor_id);
    assert_eq!(editor.debugger.breakpoints.len(), 1);
    assert_eq!(editor.debugger.breakpoints[0].source, "Scene 1 / frame 1");
    assert_eq!(editor.debugger.breakpoints[0].line, 3);

    assert!(editor.switch_session_tab(&ctx, 1));
    assert_eq!(editor.output, vec!["doc1 warning".to_owned()]);
    assert!(!editor.test_movie_preview_open);
    assert_eq!(editor.debugger.breakpoints.len(), 1);
    assert_eq!(editor.debugger.breakpoints[0].source, "Main.as");
    assert_eq!(editor.debugger.breakpoints[0].line, 9);
}

#[test]
fn t318_non_native_open_save_revert_recent_and_recovery_never_overwrite_source_files() {
    let (mut editor, ctx, dir) = setup("t318-non-native-safety");

    // Minimal valid synthetic non-native fixtures across all converted document formats.
    let svg_bytes = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 80"><rect width="60" height="40" fill="#3366ff"/></svg>"##;
    let svg_path = dir.join("poster.svg");
    std::fs::write(&svg_path, svg_bytes).unwrap();

    let mut psd_bytes = Vec::new();
    psd_bytes.extend_from_slice(b"8BPS");
    psd_bytes.extend_from_slice(&1u16.to_be_bytes());
    psd_bytes.extend_from_slice(&[0u8; 6]);
    psd_bytes.extend_from_slice(&3u16.to_be_bytes()); // channels
    psd_bytes.extend_from_slice(&2u32.to_be_bytes()); // height
    psd_bytes.extend_from_slice(&2u32.to_be_bytes()); // width
    psd_bytes.extend_from_slice(&8u16.to_be_bytes()); // depth
    psd_bytes.extend_from_slice(&3u16.to_be_bytes()); // RGB
    psd_bytes.extend_from_slice(&0u32.to_be_bytes()); // color mode data
    psd_bytes.extend_from_slice(&0u32.to_be_bytes()); // resources
    psd_bytes.extend_from_slice(&0u32.to_be_bytes()); // layer/mask
    psd_bytes.extend_from_slice(&0u16.to_be_bytes()); // raw compression
    psd_bytes.extend_from_slice(&[200u8; 12]); // 3 * 2 * 2 RGB
    let psd_path = dir.join("mockup.psd");
    std::fs::write(&psd_path, &psd_bytes).unwrap();

    let pdf_bytes = b"%PDF-1.4\n1 0 obj\n<< /Type /Page /MediaBox [0 0 200 100] >>\nendobj\n2 0 obj\n<< /Length 31 >>\nstream\n0.2 0.5 0.9 rg 10 10 50 30 re f\nendstream\nendobj\n%%EOF\n";
    let pdf_path = dir.join("sheet.pdf");
    std::fs::write(&pdf_path, pdf_bytes).unwrap();

    let fxg_bytes = br##"<?xml version="1.0" encoding="utf-8"?><Graphic version="2.0" viewWidth="160" viewHeight="90" xmlns="http://ns.adobe.com/fxg/2008"><Group id="Main"><Rect x="0" y="0" width="40" height="30"><fill><SolidColor color="#22aa66"/></fill></Rect></Group></Graphic>"##;
    let fxg_path = dir.join("badge.fxg");
    std::fs::write(&fxg_path, fxg_bytes).unwrap();

    let figma_bytes = br#"{"name":"HeroBoard","document":{"id":"0:0","name":"Doc","type":"DOCUMENT","children":[{"id":"0:1","name":"Page","type":"CANVAS","children":[{"id":"1:1","name":"Card","type":"FRAME","absoluteBoundingBox":{"x":0,"y":0,"width":240,"height":120},"children":[{"id":"1:2","name":"Box","type":"RECTANGLE","absoluteBoundingBox":{"x":10,"y":10,"width":80,"height":40},"fills":[{"type":"SOLID","color":{"r":0.2,"g":0.4,"b":0.9,"a":1.0}}]}]}]}]}}"#;
    let figma_path = dir.join("hero.figma.json");
    std::fs::write(&figma_path, figma_bytes).unwrap();

    // Minimal valid uncompressed FWS v8 1-frame SWF (RECT 0..2000 twips = 100x100 px, ShowFrame + End).
    let mut swf_body = vec![
        0x60, 0x00, 0x1F, 0x40, 0x00, 0x07, 0xD0, 0x00, // RECT (nbits=12: 0, 2000, 0, 2000)
        0x00, 24, // 24.0 fps
        0x01, 0x00, // 1 frame
        0x40, 0x00, // ShowFrame (tag 1, len 0)
        0x00, 0x00, // End (tag 0, len 0)
    ];
    let swf_len = (8 + swf_body.len()) as u32;
    let mut swf_bytes = b"FWS\x08".to_vec();
    swf_bytes.extend_from_slice(&swf_len.to_le_bytes());
    swf_bytes.append(&mut swf_body);
    let swf_path = dir.join("banner.swf");
    std::fs::write(&swf_path, &swf_bytes).unwrap();

    let xfl_dir = dir.join("clip-xfl");
    std::fs::create_dir_all(&xfl_dir).unwrap();
    let xfl_doc = xfl_dir.join("DOMDocument.xml");
    let xfl_bytes = br#"<DOMDocument width="120" height="80" frameRate="24"><timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="Layer 1"><frames><DOMFrame index="0" duration="1"><elements><DOMRectangleObject objectWidth="20" objectHeight="20"/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#;
    std::fs::write(&xfl_doc, xfl_bytes).unwrap();

    let canonical_dir = dir.canonicalize().unwrap();
    for (idx, (src_path, expected_default_name, raw_bytes)) in [
        (&svg_path, "poster.nflash", &svg_bytes[..]),
        (&psd_path, "mockup.nflash", &psd_bytes[..]),
        (&pdf_path, "sheet.nflash", &pdf_bytes[..]),
        (&fxg_path, "badge.nflash", &fxg_bytes[..]),
        (&figma_path, "hero.nflash", &figma_bytes[..]),
        (&swf_path, "banner.nflash", &swf_bytes[..]),
        (&xfl_doc, "clip-xfl.nflash", &xfl_bytes[..]),
    ]
    .into_iter()
    .enumerate()
    {
        let opened = if idx % 2 == 0 {
            editor.try_open_path(src_path.clone(), &ctx)
        } else {
            editor.open_path_in_session(&ctx, src_path.clone())
        };
        assert!(opened, "must open non-native source {}", src_path.display());
        editor
            .project
            .validate()
            .expect("converted document must validate");
        assert!(
            editor.path.is_none(),
            "converted import {} must never set canonical .nflash path",
            src_path.display()
        );
        assert!(
            editor.import_source.is_some(),
            "converted import {} must record import_source",
            src_path.display()
        );
        assert!(
            editor.dirty(),
            "converted import {} starts dirty until saved as .nflash",
            src_path.display()
        );
        assert!(
            !editor.can_revert(),
            "freshly imported {} has no local history edits to revert yet",
            src_path.display()
        );
        let (save_dir, save_name) = editor.default_save_dialog_target();
        assert_eq!(save_name, expected_default_name);
        if src_path != &xfl_doc {
            assert_eq!(
                save_dir.and_then(|d| d.canonicalize().ok()),
                Some(canonical_dir.clone())
            );
        }

        // Attempting to save directly onto the non-native source file MUST fail safely and leave bytes untouched!
        if src_path != &xfl_doc {
            assert!(
                !editor.save_to_path(src_path.clone()),
                "save_to_path must refuse overwriting non-native {}",
                src_path.display()
            );
            assert_eq!(
                std::fs::read(src_path).unwrap(),
                raw_bytes,
                "source bytes on disk must remain 100% untouched for {}",
                src_path.display()
            );
            assert!(editor.path.is_none());
            assert!(editor.dirty());
        }
    }

    // Re-focus the SVG tab via `open_path_in_session`: deduplicates instead of opening a duplicate tab.
    let tab_count_before = editor.session_count();
    assert!(editor.open_path_in_session(&ctx, svg_path.clone()));
    assert_eq!(editor.session_count(), tab_count_before);
    let imported_svg_project = editor.project.clone();
    let svg_session_id = editor.clipboard.session.clone();

    // Edit the converted SVG document -> `can_revert()` becomes true; `revert_to_saved` re-imports from SVG cleanly.
    editor.transact_named("Edit converted SVG", |ed| {
        ed.project.name = "Edited SVG Working Copy".into();
    });
    assert!(editor.can_revert());
    editor.revert_to_saved(&ctx);
    assert_eq!(editor.project.width, imported_svg_project.width);
    assert_ne!(editor.project.name, "Edited SVG Working Copy");
    assert!(editor.path.is_none());
    assert!(
        editor.dirty(),
        "unsaved converted import stays dirty after Revert"
    );
    assert_eq!(editor.clipboard.session, svg_session_id);

    // Crash recovery on unsaved converted SVG: restores `path == None` and `import_source == Some(...)`.
    editor.transact_named("Pre-crash SVG edit", |ed| {
        ed.project.name = "Crash Recovered SVG".into();
    });
    editor.tick_autosave(0.0, 2_000_000);
    editor.tick_autosave(120.0, 2_000_120);
    assert!(recovery_files(&dir) >= 1);

    let mut crashed_relaunch = Editor::from_project(&ctx, Project::blank());
    crashed_relaunch
        .recovery
        .set_dir(Some(dir.join("Recovery")));
    crashed_relaunch.check_recovery_on_launch();
    assert!(crashed_relaunch.recovery.has_prompt());
    crashed_relaunch.restore_recovery_prompt(&ctx);
    let recovered_svg_idx = (0..crashed_relaunch.session_count())
        .find(|&idx| {
            crashed_relaunch.switch_session_tab(&ctx, idx);
            crashed_relaunch.project.name == "Crash Recovered SVG"
        })
        .expect("recovered converted SVG tab must be present");
    crashed_relaunch.switch_session_tab(&ctx, recovered_svg_idx);
    assert!(
        crashed_relaunch.path.is_none(),
        "restoring crash recovery of converted SVG must NOT set self.path to poster.svg"
    );
    assert_eq!(
        crashed_relaunch.import_source,
        Some(svg_path.canonicalize().unwrap())
    );
    assert!(crashed_relaunch.dirty());
    assert_eq!(std::fs::read(&svg_path).unwrap(), svg_bytes);

    // Save As without extension appends `.nflash`, clears dirty, updates Open Recent, and preserves import_source.
    let extensionless_save = dir.join("poster-converted");
    let expected_nflash = dir.join("poster-converted.nflash");
    assert!(crashed_relaunch.save_to_path(extensionless_save));
    assert_eq!(
        crashed_relaunch.path.as_deref(),
        Some(expected_nflash.as_path())
    );
    assert!(!crashed_relaunch.dirty());
    assert!(expected_nflash.is_file());
    let expected_nflash_canonical = expected_nflash.canonicalize().unwrap();
    assert_eq!(
        crashed_relaunch
            .workspace
            .general
            .existing_recent_files()
            .first(),
        Some(&expected_nflash_canonical),
        "Save As .nflash must push to Open Recent"
    );
    assert_eq!(
        crashed_relaunch.import_source,
        Some(svg_path.canonicalize().unwrap()),
        "import_source stays linked so reopening poster.svg focuses the converted tab"
    );

    // Revert on saved `.nflash` reloads clean `.nflash` state and keeps `dirty() == false`.
    let clean_session = crashed_relaunch.clipboard.session.clone();
    crashed_relaunch.transact_named("Post-save edit", |ed| {
        ed.project.name = "Unsaved Post-Save Edit".into();
    });
    assert!(crashed_relaunch.can_revert());
    crashed_relaunch.revert_to_saved(&ctx);
    assert!(!crashed_relaunch.dirty());
    assert_eq!(crashed_relaunch.project.name, "Crash Recovered SVG");
    assert_eq!(crashed_relaunch.clipboard.session, clean_session);

    // Saving with `format_on_save == false` when `format_version` is under-versioned upgrades `format_version`
    // cleanly before `to_json()`.
    crashed_relaunch.workspace.code_editor.format_on_save = false;
    crashed_relaunch.project.format_version = 1;
    let unformatted_save = dir.join("under-versioned-save.nflash");
    assert!(crashed_relaunch.save_to_path(unformatted_save.clone()));
    assert!(crashed_relaunch.project.format_version >= 3);
    Project::from_json(&std::fs::read_to_string(&unformatted_save).unwrap())
        .expect("saved .nflash with format_on_save=false must validate on reload");
}

#[test]
fn t318_multi_tab_switch_close_stops_audio_resets_transients_and_zero_tab_commands_are_safe() {
    use crate::output_pipeline::OutputSource;
    use crate::workspace::{PanelId, PublishProfilePreset};
    use neoflash_core::{FrameSound, ResourceUnit, Script, ScriptLanguage};

    let (mut editor, ctx, dir) = setup("t318-multitab-zero-tab");

    // Attach a short valid WAV resource and start both timeline playback and headless Sound audition.
    let pcm_samples: Vec<u8> = (0..1600usize)
        .flat_map(|i| (((i % 16) as i16 - 8) * 512).to_le_bytes())
        .collect();
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36 + pcm_samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | (1 << 16), 8000, 16000, 2 | (16 << 16)] {
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm_samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm_samples);

    editor.transact_named("Add beep resource", |ed| {
        ed.project
            .resources
            .push(ResourceUnit::from_bytes("beep", "audio/beep.wav", "audio/wav", &wav).unwrap());
    });
    editor.set_playing(true);
    editor
        .timeline_audio
        .audition(&editor.project, &FrameSound::new("beep"));
    assert!(editor.playing);
    assert!(
        editor.timeline_audio.has_active_preview(),
        "headless sound audition should be active before tab switch"
    );

    // Seed transient Stage/tool/dialog state on Tab 0.
    editor.authoring_tools.lasso_polygon = vec![egui::pos2(10.0, 10.0), egui::pos2(30.0, 20.0)];
    editor.authoring_modify.scale_and_rotate_dialog = Some(Default::default());
    editor.timeline_motion.paste_special_open = true;

    // Open Tab 1 -> switching away MUST stop playback & audition audio and clear all Stage transients.
    editor.open_new_session_tab(&ctx, Project::blank(), None);
    assert!(
        !editor.playing,
        "switching tabs must pause timeline playback"
    );
    assert!(
        !editor.timeline_audio.has_active_preview(),
        "switching tabs must silence timeline/audition audio"
    );
    assert!(editor.authoring_tools.lasso_polygon.is_empty());
    assert!(editor.authoring_modify.scale_and_rotate_dialog.is_none());
    assert!(!editor.timeline_motion.paste_special_open);

    // Verify surgical publish polish (#25):
    // (a) Banner preflight failure does NOT create an empty output directory on disk.
    editor.transact_named("Add syntax-error script", |ed| {
        ed.symbol_mut()
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::JavaScript,
                    source: "function broken( {".into(),
                },
            });
    });
    let mut banner_prefs = editor.workspace.publish.clone();
    banner_prefs.profile = PublishProfilePreset::WebBannerNative;
    banner_prefs.banner_max_gzip_kb = 0; // force preflight budget failure
    let nonexistent_banner_dir = dir.join("banner-preflight-must-not-exist");
    assert!(!editor.publish_html5_with_settings(
        &nonexistent_banner_dir,
        &Editor::runtime_dir(),
        &banner_prefs,
    ));
    assert!(
        !nonexistent_banner_dir.exists(),
        "failing WebBannerNative preflight must not leave an empty output directory on disk"
    );

    // (b) Standard HTML5 publish & Test Scene on `ExportError::Scripts(_)` open BOTH `Output` and
    // `CompilerErrors` and emit jumpable per-item `OutputSource::Compiler` events.
    let html5_out_dir = dir.join("script-error-html5");
    let std_prefs = editor.workspace.publish.clone();
    assert!(!editor.publish_html5_with_settings(
        &html5_out_dir,
        &Editor::runtime_dir(),
        &std_prefs,
    ));
    assert!(editor.workspace.is_panel_visible(PanelId::Output));
    assert!(editor.workspace.is_panel_visible(PanelId::CompilerErrors));
    assert!(editor.workspace.is_panel_active(PanelId::CompilerErrors));
    assert!(
        editor.output.events().iter().any(|ev| {
            ev.source == OutputSource::Compiler
                && ev.location.as_ref().is_some_and(|loc| loc.is_navigable())
        }),
        "script export failure must emit navigable Compiler events in Output: {:?}",
        editor.output.events()
    );

    // Close all tabs cleanly (discarding dirty prompts via saved_project sync for deterministic headless close)
    // down to 0 open documents (`!editor.has_document()`).
    while editor.has_document() {
        editor.saved_project = Some(editor.project.clone());
        assert!(editor.close_session_tab(&ctx, editor.active_session));
    }
    assert!(!editor.has_document());
    assert_eq!(editor.session_count(), 0);
    assert_eq!(
        recovery_files(&dir),
        0,
        "closing all tabs removes transient recovery files"
    );

    // Every command/shortcut entrypoint must be a safe no-op when 0 tabs are open.
    editor.undo();
    editor.redo();
    assert!(!editor.can_revert());
    editor.revert_to_saved(&ctx);
    assert!(!editor.save(false));
    assert!(!editor.save(true));
    assert!(!editor.save_to_path(dir.join("zero-tab.nflash")));
    editor.copy_selection(&ctx, false);
    editor.copy_selection(&ctx, true);
    editor.export(true);
    editor.export(false);
    editor.test_scene();
    assert!(
        editor
            .run_jsfl_script("noop.jsfl", "fl.trace('hi');")
            .is_err()
    );
    assert!(!editor.has_document());
}

#[test]
fn t322_failed_revert_preserves_an_uncommitted_path_draft_and_history() {
    use crate::{
        stage_mapping::StageMapping,
        vector::{Anchor, PathDraft},
    };

    let (mut editor, ctx, dir) = setup("failed-revert-draft");
    let path = dir.join("draft.nflash");
    assert!(editor.save_to_path(path.clone()));
    editor.transact_named("Dirty document", |ed| ed.project.name = "Unsaved".into());
    let before = editor.project.clone();
    let undo_label = editor.history.undo_label().map(str::to_owned);
    let layer_id = editor.symbol().timeline.layers[editor.layer].id.clone();
    editor.vector.draft = Some(PathDraft {
        anchors: vec![
            Anchor {
                point: egui::pos2(10.0, 20.0),
                tangent: egui::Vec2::ZERO,
            },
            Anchor {
                point: egui::pos2(30.0, 40.0),
                tangent: egui::Vec2::ZERO,
            },
        ],
        pencil: false,
        dragging: false,
        closing: false,
        limit_reached: false,
        symbol: editor.symbol_id().to_owned(),
        frame: editor.frame,
        layer: editor.layer,
        layer_id,
        pointer_origin: egui::Pos2::ZERO,
        pointer_zoom: editor.zoom,
        pointer_mapping: StageMapping::default(),
    });

    std::fs::write(&path, b"not a NeoFlash project").unwrap();
    editor.request_revert();
    assert!(editor.confirm_revert);
    assert_eq!(editor.project, before);
    assert_eq!(editor.history.undo_label(), undo_label.as_deref());
    assert!(editor.vector.draft.is_some());

    assert!(!editor.revert_to_saved(&ctx));
    assert!(!editor.confirm_revert);
    assert_eq!(editor.project, before);
    assert_eq!(editor.history.undo_label(), undo_label.as_deref());
    assert!(editor.vector.draft.is_some());
}

#[test]
fn t412_standalone_as_tab_full_window_layout_save_revert_dirty_and_workspace_preservation() {
    use crate::commands::CommandId;
    use crate::recovery::ExternalFileChange;
    use eframe::App as _;

    let (mut editor, ctx, dir) = setup("t412-standalone-as-tab");
    ctx.enable_accesskit();

    let nflash_path = dir.join("tetris.nflash");
    let mut tetris = Project::blank();
    tetris.name = "Tetris".into();
    std::fs::write(&nflash_path, tetris.to_json().unwrap()).unwrap();

    let figures_src = "// Tetris figures\nvar FIGURES:Array = [\"I\", \"O\", \"T\", \"L\"];\nfunction figureCount():Number {\n    return FIGURES.length;\n}\n";
    let figures_path = dir.join("figures.as");
    std::fs::write(&figures_path, figures_src).unwrap();

    let as3_src = "package game {\n    import flash.display.Sprite;\n    public class Main extends Sprite {\n        public function Main() {}\n    }\n}\n";
    let as3_path = dir.join("Main.as");
    std::fs::write(&as3_path, as3_src).unwrap();

    let sub_dir = dir.join("sub");
    std::fs::create_dir_all(&sub_dir).unwrap();
    let sub_figures_path = sub_dir.join("figures.as");
    std::fs::write(&sub_figures_path, "var SUB_FIG:Number = 7;\n").unwrap();

    // Start from an empty tab bar, open `tetris.nflash` in Tab 0, and capture authoring WorkspacePrefs.
    editor.sessions.clear();
    assert!(editor.try_open_path(nflash_path.clone(), &ctx));
    assert_eq!(editor.active_session, 0);
    assert!(!editor.is_standalone_script_session());
    let workspace_before = editor.workspace.clone();

    let assert_layout_preserved = |ws: &crate::workspace::WorkspacePrefs| {
        assert_eq!(ws.panels, workspace_before.panels);
        assert_eq!(ws.dock, workspace_before.dock);
        assert_eq!(ws.detached, workspace_before.detached);
        assert_eq!(ws.sizes, workspace_before.sizes);
        assert_eq!(ws.active_bottom, workspace_before.active_bottom);
        assert_eq!(ws.stage_toolbar, workspace_before.stage_toolbar);
    };

    // Open `figures.as` in Tab 1 via `try_open_path` -> standalone AS2 script tab!
    assert!(editor.try_open_path(figures_path.clone(), &ctx));
    assert_eq!(editor.session_count(), 2);
    assert_eq!(editor.active_session, 1);
    assert!(editor.is_standalone_script_session());
    assert!(!editor.dirty());
    assert_eq!(
        editor.standalone_script_language(),
        Some(neoflash_core::ScriptLanguage::ActionScript2)
    );
    assert_layout_preserved(&editor.workspace);

    // Dialect switcher changes language cleanly without marking unmodified disk file dirty.
    editor.set_standalone_script_language(neoflash_core::ScriptLanguage::ActionScript1);
    assert_eq!(
        editor.standalone_script_language(),
        Some(neoflash_core::ScriptLanguage::ActionScript1)
    );
    assert!(!editor.dirty());
    editor.set_standalone_script_language(neoflash_core::ScriptLanguage::ActionScript2);

    // Render full 1920x1080 frame on standalone `.as` tab -> pure Script Editor, NO Stage/Timeline/Tools!
    let mut dummy_frame = eframe::Frame::_new_kittest();
    let full_out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0),
            )),
            ..Default::default()
        },
        |ctx| {
            editor.update(ctx, &mut dummy_frame);
        },
    );
    let ak_labels: Vec<String> = full_out
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|u| {
            u.nodes
                .iter()
                .filter_map(|(_, n)| n.label().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    fn collect_painted_texts(shapes: &[egui::epaint::ClippedShape], out: &mut Vec<String>) {
        fn walk_shape(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::epaint::Shape::Vec(v) => {
                    for s in v {
                        walk_shape(s, out);
                    }
                }
                _ => {}
            }
        }
        for cs in shapes {
            walk_shape(&cs.shape, out);
        }
    }
    let mut painted_texts = Vec::new();
    collect_painted_texts(&full_out.shapes, &mut painted_texts);
    assert!(
        ak_labels.iter().any(|l| l.contains("Check syntax")),
        "standalone .as view must render Check syntax tool: {ak_labels:?}"
    );
    assert!(
        painted_texts
            .iter()
            .any(|t| t.contains("ActionScript 2.0 · 6 lines · UTF-8")),
        "status bar must paint standalone script summary: {painted_texts:?}"
    );
    assert!(
        !painted_texts.iter().any(|t| t.starts_with("Stage 550"))
            && !ak_labels.iter().any(|l| l == "Insert layer"),
        "Stage summary and Timeline controls must NOT render on standalone .as tab: {painted_texts:?}"
    );
    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::ControlTestMovie)
            .enabled
    );
    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::FilePublish)
            .enabled
    );
    assert_layout_preserved(&editor.workspace);

    // Open `Main.as` via `open_path_in_session` -> auto-detects ActionScript 3.0!
    assert!(editor.open_path_in_session(&ctx, as3_path.clone()));
    assert_eq!(editor.session_count(), 3);
    assert!(editor.is_standalone_script_session());
    assert_eq!(
        editor.standalone_script_language(),
        Some(neoflash_core::ScriptLanguage::ActionScript3)
    );

    // Drag-and-drop `sub/figures.as` onto window -> opens Tab 3 with folder disambiguation!
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0),
            )),
            dropped_files: vec![egui::DroppedFile {
                path: Some(sub_figures_path.clone()),
                ..Default::default()
            }],
            ..Default::default()
        },
        |ctx| {
            editor.update(ctx, &mut dummy_frame);
        },
    );
    assert_eq!(editor.session_count(), 4);
    assert!(editor.is_standalone_script_session());

    // Switch back to `tetris.nflash` (Tab 0) -> authoring workspace is 100% preserved!
    assert!(editor.switch_session_tab(&ctx, 0));
    assert!(!editor.is_standalone_script_session());
    assert_layout_preserved(&editor.workspace);
    assert!(
        editor
            .command_menu_state(Some(&ctx), CommandId::ControlTestMovie)
            .enabled
    );

    // Switch to `figures.as` (Tab 1), edit, verify dirty close prompt, then Save (`⌘S`) raw UTF-8!
    assert!(editor.switch_session_tab(&ctx, 1));
    let updated_src = format!("{figures_src}var EXTRA_PIECE:String = \"Z\";\n");
    editor.transact_named("Edit script", |ed| {
        ed.project.sources[0].source = updated_src.clone();
    });
    assert!(editor.dirty());
    assert!(editor.close_session_tab(&ctx, 1));
    assert_eq!(editor.confirm, Some(Pending::CloseSession(1)));
    editor.confirm = None;
    editor.close_origin = None;

    assert!(editor.save(false));
    assert!(!editor.dirty());
    let disk_raw = std::fs::read_to_string(&figures_path).unwrap();
    assert_eq!(disk_raw, updated_src);
    assert!(
        !disk_raw.trim_start().starts_with('{'),
        "standalone .as Save must write plain UTF-8 ActionScript, NEVER .nflash JSON!"
    );

    // Refuses overwriting `.nflash` path from a standalone `.as` session; Save As `.as` succeeds!
    assert!(!editor.save_to_path(dir.join("wrong.nflash")));
    let copy_as_path = dir.join("figures_copy.as");
    assert!(editor.save_to_path(copy_as_path.clone()));
    assert_eq!(std::fs::read_to_string(&copy_as_path).unwrap(), updated_src);

    // Edit buffer and `Revert` reloads saved `.as` text from disk.
    editor.transact_named("Edit script", |ed| {
        ed.project.sources[0].source = "// temporary unsaved edit\n".into();
    });
    assert!(editor.dirty());
    assert!(editor.revert_to_saved(&ctx));
    assert!(!editor.dirty());
    assert_eq!(
        editor.standalone_script_source(),
        Some(updated_src.as_str())
    );

    // External modification on disk triggers notification + reloads raw `.as` text cleanly.
    let external_src = "// modified externally\nvar FIGURES:Array = [\"S\", \"Z\"];\n";
    std::fs::write(&copy_as_path, external_src).unwrap();
    let conflicts = editor.check_external_file_changes();
    assert_eq!(conflicts, vec![(1, ExternalFileChange::Modified)]);
    assert!(editor.has_external_change_prompt());
    assert!(
        editor
            .output
            .iter()
            .any(|line| line.contains("External change") && line.contains("figures_copy.as")),
        "expected external-change notification in Output, got: {:?}",
        editor.output.events()
    );
    assert!(editor.reload_external_modification(&ctx));
    assert!(!editor.dirty());
    assert_eq!(editor.standalone_script_source(), Some(external_src));
}
