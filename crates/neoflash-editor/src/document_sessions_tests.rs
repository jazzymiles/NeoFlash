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
