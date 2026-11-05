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
