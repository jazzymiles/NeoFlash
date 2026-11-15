//! End-to-end Editor UI, Undo, migration, and Compiler errors tests for `R-102` (`T-315`):
//! unified project script language lock and foreign-script policy across `Document settings`,
//! `Properties — Document ▸ Publish`, `Actions`, `Project files`, and `Compiler errors`.
use crate::{
    app::{Editor, Pending, ScriptTarget},
    panels::{actions_sidebar::ScriptLocation, compiler_errors::collect_compiler_diagnostics},
    workspace::PanelId,
};
use eframe::egui::{self, Pos2, Rect, Vec2};
use neoflash_core::*;

fn collect_shape_texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for child in shapes {
                collect_shape_texts(child, out);
            }
        }
        _ => {}
    }
}

fn painted_texts(output: &egui::FullOutput) -> Vec<String> {
    let mut texts = Vec::new();
    for clipped in &output.shapes {
        collect_shape_texts(&clipped.shape, &mut texts);
    }
    texts
}

fn render_panel(
    ctx: &egui::Context,
    size: Vec2,
    mut f: impl FnMut(&egui::Context, &mut egui::Ui),
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| f(ctx, ui));
        },
    )
}

#[test]
fn t315_pre_r102_migration_preserves_untouched_project_and_renders_publish_counts() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    // Build a pre-R-102 document (`script_language: None`) where TypeScript has the majority
    // (2 TS scripts), plus 1 compatible JS script and 1 skipped AS2 script.
    let mut legacy = Project::blank();
    assert!(legacy.format_version < 6);
    assert_eq!(legacy.script_language, None);
    legacy.sources.push(SourceUnit {
        id: "ts-1".into(),
        path: "main.ts".into(),
        language: ScriptLanguage::TypeScript,
        source: "const a: number = 1;".into(),
    });
    legacy.symbols[0].timeline.frame_count = 3;
    legacy.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::TypeScript,
            source: "const b: number = 2;".into(),
        },
    });
    legacy.symbols[0].timeline.scripts.push(FrameScript {
        frame: 1,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "console.log('js');".into(),
        },
    });
    legacy.symbols[0].timeline.scripts.push(FrameScript {
        frame: 2,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();".into(),
        },
    });
    let orig = legacy.clone();

    let mut editor = Editor::from_project(&ctx, legacy);
    // `Editor::from_project` must NOT mutate `project` before an actual edit (`Undo` baseline intact).
    assert_eq!(editor.project, orig);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
    assert_eq!(
        editor.project.effective_script_language(),
        ScriptLanguage::TypeScript
    );

    // Render `Properties — Document` and verify `Publish` paints the R-102 controls & live foreign count.
    let out = render_panel(&ctx, Vec2::new(420.0, 900.0), |ctx, ui| {
        editor.properties(ui, ctx);
    });
    let texts = painted_texts(&out);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("HTML5 · Rust / WebAssembly")),
        "Properties Publish must show HTML5 badge: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "TypeScript"),
        "Properties Publish must show inferred TypeScript select: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "Also compile JavaScript"),
        "Properties Publish must show compatible language checkbox: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| {
            t == "2 scripts are in other languages and stay in the document: 1 compiles with a warning, 1 is skipped when publishing."
        }),
        "Properties Publish must show exact R-102 item 3 foreign count message: {texts:?}"
    );
    assert_eq!(editor.project, orig);
}

#[test]
fn t315_document_settings_make_default_and_properties_publish_roundtrip_with_single_undo() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let mut project = Project::blank();
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "console.log('compat');".into(),
        },
    });
    let baseline = project.clone();
    let mut editor = Editor::from_project(&ctx, project);

    // Open `Document settings` (`⌘J`): inferred language is JavaScript because of the sole JS script.
    editor.open_document_settings();
    assert_eq!(
        editor.document_settings.as_ref().unwrap().script_language,
        ScriptLanguage::JavaScript
    );

    // Change draft to TypeScript (keeping `also_compile_compatible_scripts == true`) BEFORE clicking OK:
    // live `InlineMessage` in the modal must immediately report 1 compatible JS script (`Info`), while
    // `editor.project` remains untouched! (`egui::Modal` sizes on pass 1 and paints on pass 2.)
    let run_modal = |editor: &mut Editor| {
        for _ in 0..2 {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                    ..Default::default()
                },
                |ctx| {
                    editor.document_settings_dialog(ctx);
                },
            );
            let texts = painted_texts(&out);
            if !texts.is_empty() {
                return texts;
            }
        }
        Vec::new()
    };

    editor.document_settings.as_mut().unwrap().script_language = ScriptLanguage::TypeScript;
    let modal_texts = run_modal(&mut editor);
    assert!(
        modal_texts
            .iter()
            .any(|t| t == "1 script is in JavaScript and compiles with a warning."),
        "Document settings draft must show live Info count before OK: {modal_texts:?}"
    );
    assert_eq!(editor.project, baseline);

    // Uncheck `also_compile_compatible_scripts` in draft -> live message flips to skipped Warning,
    // and switching draft to ActionScript 3.0 renders disabled `"Also compile compatible languages"`
    // WITHOUT overwriting the stored `false` choice!
    editor
        .document_settings
        .as_mut()
        .unwrap()
        .also_compile_compatible_scripts = false;
    editor.document_settings.as_mut().unwrap().script_language = ScriptLanguage::ActionScript3;
    let as3_texts = run_modal(&mut editor);
    assert!(
        as3_texts
            .iter()
            .any(|t| t == "Also compile compatible languages"),
        "AS3 Document settings must render disabled compatible checkbox: {as3_texts:?}"
    );
    assert!(
        !editor
            .document_settings
            .as_ref()
            .unwrap()
            .also_compile_compatible_scripts,
        "switching draft through AS3 must preserve stored false bool"
    );

    // Switch draft back to TypeScript, save as `DocumentDefaults`, and commit in 1 Undo step.
    let draft = editor.document_settings.take().unwrap();
    let mut final_draft = draft;
    final_draft.script_language = ScriptLanguage::TypeScript;
    assert!(!final_draft.also_compile_compatible_scripts);
    editor.workspace.document_defaults = Some(crate::workspace::DocumentDefaults {
        width: 800,
        height: 600,
        fps: 24,
        background: editor.project.background,
        units: editor.stage_view.prefs.units,
        script_language: final_draft.script_language,
        also_compile_compatible_scripts: final_draft.also_compile_compatible_scripts,
    });
    editor.commit_document_settings(final_draft);
    assert_eq!(
        editor.project.script_language,
        Some(ScriptLanguage::TypeScript)
    );
    assert!(!editor.project.also_compile_compatible_scripts);
    assert_eq!(editor.history.undo_label(), Some("Document settings"));
    editor.project.validate().unwrap();

    // Single Undo restores exact pre-R-102 baseline.
    editor.undo();
    assert_eq!(editor.project, baseline);
    editor.redo();
    assert_eq!(
        editor.project.script_language,
        Some(ScriptLanguage::TypeScript)
    );
    assert!(!editor.project.also_compile_compatible_scripts);

    // `Properties — Document ▸ Publish` edits also preserve `also_compile_compatible_scripts` across AS3
    // and each commit in 1 Undo step.
    editor.set_project_script_language(ScriptLanguage::ActionScript3);
    assert_eq!(editor.history.undo_label(), Some("Script language"));
    assert!(
        !editor.project.also_compile_compatible_scripts,
        "switching to AS3 in Properties must preserve stored false bool"
    );
    editor.set_project_script_language(ScriptLanguage::ActionScript2);
    assert!(!editor.project.also_compile_compatible_scripts);
    editor.set_also_compile_compatible_scripts(true);
    assert_eq!(
        editor.history.undo_label(),
        Some("Compatible script policy")
    );
    assert!(editor.project.also_compile_compatible_scripts);
    editor.project.validate().unwrap();

    // `File ▸ New` (`Pending::New`) applies `DocumentDefaults` (`TypeScript` + `false`).
    editor.perform(Pending::New, &ctx);
    assert_eq!(
        editor.project.script_language,
        Some(ScriptLanguage::TypeScript)
    );
    assert!(!editor.project.also_compile_compatible_scripts);
    editor.project.validate().unwrap();
}

#[test]
fn t315_actions_and_project_files_badges_notice_rows_new_class_gating_and_mark_as_undo() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let mut project = Project::blank();
    project.format_version = 6;
    project.script_language = Some(ScriptLanguage::TypeScript);
    project.also_compile_compatible_scripts = true;
    project.symbols[0].timeline.frame_count = 3;

    // Frame 1 (index 0): AS2 script -> Skipped in TypeScript project.
    let verbatim_as2_code = "// keep-exact-text\nstop();\n";
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: verbatim_as2_code.into(),
        },
    });
    // Frame 2 (index 1): JS script -> Compatible in TypeScript project.
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 1,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "console.log('js-frame');".into(),
        },
    });
    // Add an editable stage element so `Selected object` stays enabled even in TypeScript/AS3 (`R-102` item 8).
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "hero-box".into(),
            name: "heroBox".into(),
            kind: ElementKind::Rectangle {
                width: 60.0,
                height: 40.0,
                fill: [40, 140, 220, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    // Source files: `helper.js` (extension-fixed -> notice WITHOUT `Mark as`) and `Legacy.as` (notice WITHOUT `Mark as` in TS, WITH `Mark as` in AS3).
    project.sources.push(SourceUnit {
        id: "src-js".into(),
        path: "helper.js".into(),
        language: ScriptLanguage::JavaScript,
        source: "export const x = 1;\n".into(),
    });
    project.sources.push(SourceUnit {
        id: "src-as2".into(),
        path: "Legacy.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "class Legacy {}\n".into(),
    });

    let mut editor = Editor::from_project(&ctx, project);
    editor.show_panel(PanelId::Actions);
    let hero_target = editor.target_for_id("hero-box").expect("hero-box target");
    editor.selection = crate::selection::Selection {
        symbol: editor.symbol_id().to_owned(),
        targets: vec![hero_target],
    };
    assert!(
        editor.selected_editable(),
        "Selected object target stays enabled regardless of project script language (R-102 item 8)"
    );

    // 1. Frame 1 (Skipped AS2): renders `ActionScript 2.0` badge + Skipped Warning notice + `Mark as TypeScript`.
    editor.frame = 0;
    editor.script_target = ScriptTarget::Frame;
    let out_skipped = render_panel(&ctx, Vec2::new(1200.0, 460.0), |_, ui| {
        editor.actions_contents(ui);
    });
    let skipped_texts = painted_texts(&out_skipped);
    assert!(skipped_texts.iter().any(|t| t == "ActionScript 2.0"));
    assert!(
        skipped_texts.iter().any(|t| {
            t == "This script is ActionScript 2.0. The project uses TypeScript, so the script is skipped when publishing."
        }),
        "missing skipped notice row: {skipped_texts:?}"
    );
    assert!(
        skipped_texts.iter().any(|t| t == "Mark as TypeScript"),
        "frame script must offer Mark as TypeScript button: {skipped_texts:?}"
    );

    // Retagging via `mark_current_script_as_project_language` changes ONLY `language`, preserves `source`
    // verbatim, commits in 1 `"Mark script language"` Undo step, and hides the notice row immediately!
    assert!(editor.mark_current_script_as_project_language());
    assert_eq!(editor.history.undo_label(), Some("Mark script language"));
    let retagged_frame = &editor.symbol().timeline.scripts[0].script;
    assert_eq!(retagged_frame.language, ScriptLanguage::TypeScript);
    assert_eq!(retagged_frame.source, verbatim_as2_code);
    editor.project.validate().unwrap();

    let out_after_mark = render_panel(&ctx, Vec2::new(1200.0, 460.0), |_, ui| {
        editor.actions_contents(ui);
    });
    let after_mark_texts = painted_texts(&out_after_mark);
    assert!(
        !after_mark_texts
            .iter()
            .any(|t| t.contains("This script is")),
        "notice row must disappear immediately after Mark as: {after_mark_texts:?}"
    );
    editor.undo();
    assert_eq!(
        editor.symbol().timeline.scripts[0].script.language,
        ScriptLanguage::ActionScript2
    );
    assert_eq!(
        editor.symbol().timeline.scripts[0].script.source,
        verbatim_as2_code
    );

    // 2. Frame 2 (Compatible JS): renders `JavaScript` badge + Info notice + `Mark as TypeScript`.
    editor.frame = 1;
    let out_compat = render_panel(&ctx, Vec2::new(1200.0, 460.0), |_, ui| {
        editor.actions_contents(ui);
    });
    let compat_texts = painted_texts(&out_compat);
    assert!(
        compat_texts.iter().any(|t| {
            t == "This script is JavaScript. The project uses TypeScript; the script compiles with a warning."
        }),
        "missing compatible notice row: {compat_texts:?}"
    );
    assert!(compat_texts.iter().any(|t| t == "Mark as TypeScript"));

    // 3. Empty Frame 3 (index 2): new scripts are created in `TypeScript` (`effective_script_language`).
    editor.frame = 2;
    editor.actions_ui.pending = Some(crate::code_tools::CodeCommand::Insert {
        text: "const fresh: number = 7;".into(),
        caret: 24,
        import: None,
    });
    let _ = render_panel(&ctx, Vec2::new(1200.0, 460.0), |_, ui| {
        editor.actions_contents(ui);
    });
    let created = editor
        .symbol()
        .timeline
        .scripts
        .iter()
        .find(|s| s.frame == 2)
        .expect("new script created on Frame 3");
    assert_eq!(created.script.language, ScriptLanguage::TypeScript);

    // 4. `Project files` (`ScriptTarget::Source`):
    //    - `helper.js` gets the compatible notice WITHOUT `Mark as TypeScript` (extension fixes JS).
    //    - `Legacy.as` in TypeScript omits `Mark as TypeScript`, but in ActionScript 3.0 offers `Mark as ActionScript 3.0`!
    editor.script_target = ScriptTarget::Source;
    editor.selected_source = Some("src-js".into());
    let out_src_js = render_panel(&ctx, Vec2::new(1200.0, 520.0), |_, ui| {
        editor.actions_contents(ui);
    });
    let src_js_texts = painted_texts(&out_src_js);
    assert!(
        src_js_texts.iter().any(|t| {
            t == "This script is JavaScript. The project uses TypeScript; the script compiles with a warning."
        }),
        "helper.js must show compatible notice row: {src_js_texts:?}"
    );
    assert!(
        !src_js_texts.iter().any(|t| t == "Mark as TypeScript"),
        "extension-derived .js source file must omit Mark as button: {src_js_texts:?}"
    );
    assert!(!Editor::can_retag_source_unit_to(
        "helper.js",
        ScriptLanguage::TypeScript
    ));
    assert!(!Editor::can_retag_source_unit_to(
        "Legacy.as",
        ScriptLanguage::TypeScript
    ));
    assert!(Editor::can_retag_source_unit_to(
        "Legacy.as",
        ScriptLanguage::ActionScript3
    ));

    // Switch project to ActionScript 3.0 and select `Legacy.as` (`ActionScript 2.0`):
    // now `Mark as ActionScript 3.0` IS offered and retags `Legacy.as` in 1 Undo step!
    editor.set_project_script_language(ScriptLanguage::ActionScript3);
    editor.selected_source = Some("src-as2".into());
    let out_src_as = render_panel(&ctx, Vec2::new(1200.0, 520.0), |_, ui| {
        editor.actions_contents(ui);
    });
    let src_as_texts = painted_texts(&out_src_as);
    assert!(
        src_as_texts.iter().any(|t| t == "Mark as ActionScript 3.0"),
        ".as file in AS3 project must offer Mark as ActionScript 3.0: {src_as_texts:?}"
    );
    assert!(editor.mark_current_script_as_project_language());
    assert_eq!(
        editor
            .project
            .sources
            .iter()
            .find(|s| s.id == "src-as2")
            .unwrap()
            .language,
        ScriptLanguage::ActionScript3
    );
    assert_eq!(
        editor
            .project
            .sources
            .iter()
            .find(|s| s.id == "src-as2")
            .unwrap()
            .source,
        "class Legacy {}\n"
    );

    // 5. `No project classes` EmptyState gating for AS3 (`New AS3 class`), AS2 (`New AS2 class`), and TS (no class button).
    editor.project.sources.clear();
    editor.selected_source = None;
    editor.set_project_script_language(ScriptLanguage::ActionScript3);
    let empty_as3 = painted_texts(&render_panel(&ctx, Vec2::new(1200.0, 520.0), |_, ui| {
        editor.actions_contents(ui);
    }));
    assert!(empty_as3.iter().any(|t| t == "New AS3 class"));

    editor.set_project_script_language(ScriptLanguage::ActionScript2);
    let empty_as2 = painted_texts(&render_panel(&ctx, Vec2::new(1200.0, 520.0), |_, ui| {
        editor.actions_contents(ui);
    }));
    assert!(empty_as2.iter().any(|t| t == "New AS2 class"));

    editor.set_project_script_language(ScriptLanguage::ActionScript1);
    let empty_as1 = painted_texts(&render_panel(&ctx, Vec2::new(1200.0, 520.0), |_, ui| {
        editor.actions_contents(ui);
    }));
    assert!(
        !empty_as1
            .iter()
            .any(|t| t == "New AS3 class" || t == "New AS2 class"),
        "AS1 must not offer New AS2/AS3 class action: {empty_as1:?}"
    );
}

#[test]
fn t315_compiler_errors_emits_located_foreign_warnings_and_jumps_to_actions_notice() {
    let ctx = egui::Context::default();

    let mut project = Project::blank();
    project.format_version = 6;
    project.script_language = Some(ScriptLanguage::TypeScript);
    project.also_compile_compatible_scripts = true;
    project.symbols[0].timeline.frame_count = 2;

    // Frame 1 (index 0): compatible JS script -> 1 located warning (not an error).
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "console.log('ok');".into(),
        },
    });
    // Frame 2 (index 1): skipped AS3 script with BROKEN syntax -> 1 located warning at 1:1, ZERO syntax errors!
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 1,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "if ((( broken as3 syntax".into(),
        },
    });
    let root_id = project.root.clone();

    let items = collect_compiler_diagnostics(&project);
    assert!(
        items.iter().all(|item| !item.is_error),
        "skipped broken AS3 script must not emit syntax errors in Compiler errors: {items:?}"
    );
    let compat_idx = items
        .iter()
        .position(|item| {
            item.script
                == Some(ScriptLocation::Frame {
                    symbol: root_id.clone(),
                    frame: 0,
                })
                && item.message.contains("Compiled as compatible language")
                && item.line == 1
                && item.column == 1
        })
        .expect("located compatible warning on Frame 1");
    let skipped_idx = items
        .iter()
        .position(|item| {
            item.script
                == Some(ScriptLocation::Frame {
                    symbol: root_id.clone(),
                    frame: 1,
                })
                && item.message.contains("Skipped when publishing")
                && item.line == 1
                && item.column == 1
        })
        .expect("located skipped warning on Frame 2");
    assert_ne!(compat_idx, skipped_idx);
    // Ensure foreign-script warnings are NOT duplicated as unlocated HTML5 publish rows.
    assert_eq!(
        items
            .iter()
            .filter(|i| i.message.contains("Skipped when publishing"))
            .count(),
        1
    );

    // Jumping from `Compiler errors` opens Frame 2 in `Actions`, where the skipped notice row is shown.
    let mut editor = Editor::from_project(&ctx, project);
    editor.refresh_compiler_diagnostics();
    assert!(editor.jump_to_compiler_diagnostic(&ctx, skipped_idx));
    assert!(editor.script_target == ScriptTarget::Frame);
    assert_eq!(editor.frame, 1);

    let out = render_panel(&ctx, Vec2::new(1200.0, 460.0), |_, ui| {
        editor.actions_contents(ui);
    });
    let texts = painted_texts(&out);
    assert!(
        texts.iter().any(|t| {
            t == "This script is ActionScript 3.0. The project uses TypeScript, so the script is skipped when publishing."
        }),
        "jumping from Compiler errors must open the foreign script with its R-102 notice row: {texts:?}"
    );
}

#[test]
fn t311_project_new_file_language_policy_and_legacy_none_fla_save() {
    let ctx = egui::Context::default();

    // 1. Project panel New file language gating + tooltip for TypeScript project.
    let mut ts_project = Project::blank();
    ts_project.script_language = Some(ScriptLanguage::TypeScript);
    ts_project.also_compile_compatible_scripts = true;
    ts_project.sources.push(SourceUnit {
        id: "foreign-as3".into(),
        path: "src/Legacy.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package {}".into(),
    });
    let mut ts_editor = Editor::from_project(&ctx, ts_project);
    assert_eq!(
        ts_editor.project.effective_script_language(),
        ScriptLanguage::TypeScript
    );
    assert_eq!(
        ts_editor.project_new_file_language_allowed(ScriptLanguage::TypeScript),
        (true, None)
    );
    assert_eq!(
        ts_editor.project_new_file_language_allowed(ScriptLanguage::JavaScript),
        (true, None)
    );
    assert_eq!(
        ts_editor.project_new_file_language_allowed(ScriptLanguage::ActionScript3),
        (
            false,
            Some("The project script language is TypeScript".into())
        )
    );

    ts_editor.project.also_compile_compatible_scripts = false;
    assert_eq!(
        ts_editor.project_new_file_language_allowed(ScriptLanguage::JavaScript),
        (
            false,
            Some("The project script language is TypeScript".into())
        )
    );

    // 2. ActionScript 1.0 New file emits prototype function constructor, not AS2 `class`.
    let mut as1_project = Project::blank();
    as1_project.script_language = Some(ScriptLanguage::ActionScript1);
    let mut as1_editor = Editor::from_project(&ctx, as1_project);
    as1_editor.project_new_source_file(as1_editor.project.effective_script_language());
    let created = as1_editor.project.sources.last().expect("created AS1 file");
    assert_eq!(created.language, ScriptLanguage::ActionScript1);
    assert!(created.source.contains("function Class1()"));
    assert!(!created.source.contains("class Class1"));

    // 3. Pre-R-102 (`script_language == None`) mixed-script project saves cleanly to `.fla`.
    let mut legacy_mixed = Project::blank();
    legacy_mixed.script_language = None;
    legacy_mixed.sources.push(SourceUnit {
        id: "src-ts".into(),
        path: "src/Main.ts".into(),
        language: ScriptLanguage::TypeScript,
        source: "export const ready: number = 1;".into(),
    });
    legacy_mixed.symbols[0].timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();".into(),
        },
    });
    assert_eq!(
        legacy_mixed.effective_script_language(),
        ScriptLanguage::ActionScript2
    );
    let mut legacy_editor = Editor::from_project(&ctx, legacy_mixed);
    let temp = std::env::temp_dir().join(format!(
        "neoflash-t311-legacy-fla-save-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();
    let fla_out = temp.join("legacy-mixed.fla");
    assert!(
        legacy_editor.save_to_path(fla_out.clone()),
        "saving pre-R-102 mixed-script project to .fla must resolve script_language"
    );
    assert!(fla_out.is_file());
    let _ = std::fs::remove_dir_all(&temp);
}
