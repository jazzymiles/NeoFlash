use crate::{
    app::Editor,
    commands::CommandId,
    macos_menu::{NativeMenuItemSpec, NativeMenuSpec},
    save_as_panel::{SaveAsFormat, available_script_options, update_save_as_filename},
};
use eframe::egui;
use neoflash_core::{
    Element, ElementKind, FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties,
    ObjectEvent, ObjectScript, Project, Script, ScriptLanguage, Symbol, SymbolKind, Timeline,
    Transform,
};
use neoflash_script::ConversionTarget;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct TempTestDir(PathBuf);

impl TempTestDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("neoflash-t455-{tag}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn make_test_symbol(id: &str, name: &str, kind: SymbolKind) -> Symbol {
    Symbol {
        id: id.into(),
        name: name.into(),
        folder: String::new(),
        kind,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: format!("layer-{id}"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn build_as2_interactive_project() -> Project {
    let mut proj = Project::blank();
    proj.name = "RetroArcade".into();
    proj.script_language = Some(ScriptLanguage::ActionScript2);

    let btn_sym = make_test_symbol("sym_btn", "PlayBtn", SymbolKind::Button);
    let btn_id = btn_sym.id.clone();

    let hero_sym = make_test_symbol("sym_hero", "HeroShip", SymbolKind::MovieClip);
    let hero_id = hero_sym.id.clone();

    proj.symbols.push(btn_sym);
    proj.symbols.push(hero_sym);

    let root = proj.root.clone();
    let root_sym = proj.symbol_mut(&root).expect("root symbol");
    root_sym.timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "el_hero".into(),
            name: "hero".into(),
            kind: ElementKind::Instance {
                symbol_id: hero_id,
                playback: InstancePlayback::Loop,
            },
            transform: Transform::default(),
            ..Element::default()
        });
    root_sym.timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "el_btn".into(),
            name: String::new(), // Unnamed button to exercise deterministic auto-naming!
            kind: ElementKind::Instance {
                symbol_id: btn_id,
                playback: InstancePlayback::Loop,
            },
            transform: Transform::default(),
            scripts: vec![ObjectScript {
                event: ObjectEvent::Click,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "on (release) {\n    score = score + 10;\n    hero._alpha = 50;\n}"
                        .into(),
                },
            }],
            ..Element::default()
        });
    root_sym.timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();\nvar score:Number = 0;\nhero._xscale = 100;".into(),
        },
    });
    proj.upgrade_format_for_features();
    proj
}

#[test]
fn t455_save_as_converted_preserves_dirty_source_tab_and_opens_converted_as3_js_as2_tabs() {
    let ctx = egui::Context::default();
    let dir = TempTestDir::new("save-converted");

    // Verify Illustrator-style dropdown policy & filename transitions
    let as2_opts = available_script_options(ScriptLanguage::ActionScript2);
    assert_eq!(as2_opts.len(), 3);
    let js_opts = available_script_options(ScriptLanguage::JavaScript);
    assert_eq!(js_opts.len(), 1, "JS/TS -> AS* must remain skipped");
    assert_eq!(
        update_save_as_filename("RetroArcade.nflash", SaveAsFormat::NeoFlash, true),
        "RetroArcade (converted).nflash"
    );

    let orig_path = dir.path().join("RetroArcade.nflash");
    let proj = build_as2_interactive_project();
    std::fs::write(&orig_path, proj.to_json().expect("json")).expect("save initial project");

    let mut editor = Editor::from_project(&ctx, Project::blank());
    while editor.has_document() {
        assert!(editor.close_session_tab(&ctx, 0));
    }
    editor.open_path(orig_path.clone(), &ctx);
    assert_eq!(editor.session_count(), 1);
    assert!(!editor.dirty());

    // Make an unsaved edit on the AS2 source tab before invoking Save As (converted)
    editor.transact(|ed| {
        let root_id = ed.project.root.clone();
        let root = ed.project.symbol_mut(&root_id).unwrap();
        root.timeline.scripts[0].script.source =
            "stop();\nvar score:Number = 5;\nhero._xscale = 150;".into();
    });
    assert!(editor.dirty(), "source AS2 tab should be dirty");

    // 1. Convert AS2 -> AS3 via Save As (converted)
    let as3_path = dir.path().join("RetroArcade (converted).nflash");
    assert!(editor.save_as_converted_with_ctx(
        as3_path.clone(),
        ConversionTarget::ActionScript3,
        Some(&ctx),
    ));

    assert_eq!(
        editor.session_count(),
        2,
        "converted copy opens in a new tab"
    );
    assert_eq!(editor.active_session, 1, "converted tab becomes active");
    assert!(!editor.dirty(), "newly saved converted AS3 tab is clean");
    assert_eq!(
        editor.project.script_language,
        Some(ScriptLanguage::ActionScript3)
    );
    neoflash_script::check_project(&editor.project)
        .expect("converted AS3 project must compile cleanly");
    let root_id = editor.project.root.clone();
    let as3_src = &editor.project.symbol(&root_id).unwrap().timeline.scripts[0]
        .script
        .source;
    assert!(as3_src.contains("hero.scaleX = 1.5"), "got AS3:\n{as3_src}");
    assert!(as3_src.contains("hero.alpha = 0.5"), "got AS3:\n{as3_src}");
    assert!(
        as3_src.contains("addEventListener(MouseEvent.CLICK"),
        "button Release script should lift to parent timeline MouseEvent.CLICK:\n{as3_src}"
    );
    let canon_as3 = std::fs::canonicalize(&as3_path).unwrap_or_else(|_| as3_path.clone());
    assert!(
        editor
            .workspace
            .general
            .recent_files
            .iter()
            .any(|p| Path::new(p) == canon_as3 || Path::new(p) == as3_path),
        "converted file should be added to Open Recent"
    );

    // 2. Convert AS3 -> AS2 from tab 1 into a third tab
    let roundtrip_as2_path = dir.path().join("RetroArcade AS2 (converted).nflash");
    assert!(editor.save_as_converted_with_ctx(
        roundtrip_as2_path,
        ConversionTarget::ActionScript2,
        Some(&ctx),
    ));
    assert_eq!(editor.session_count(), 3);
    assert_eq!(
        editor.project.script_language,
        Some(ScriptLanguage::ActionScript2)
    );
    neoflash_script::check_project(&editor.project)
        .expect("AS3 -> AS2 converted project must compile cleanly");

    // 3. Convert AS2 -> JavaScript into a fourth tab
    let js_path = dir.path().join("RetroArcade JS (converted).nflash");
    assert!(editor.save_as_converted_with_ctx(js_path, ConversionTarget::JavaScript, Some(&ctx),));
    assert_eq!(editor.session_count(), 4);
    assert_eq!(
        editor.project.script_language,
        Some(ScriptLanguage::JavaScript)
    );
    neoflash_script::check_project(&editor.project)
        .expect("converted JS project must compile cleanly");

    // 4. Switch back to original tab 0 and verify its dirty AS2 edits were NEVER lost or overwritten!
    assert!(editor.switch_session_tab(&ctx, 0));
    assert!(
        editor.dirty(),
        "original AS2 tab must retain its unsaved dirty state"
    );
    assert_eq!(
        editor.project.script_language,
        Some(ScriptLanguage::ActionScript2)
    );
    let orig_root = editor.project.root.clone();
    let orig_src = &editor.project.symbol(&orig_root).unwrap().timeline.scripts[0]
        .script
        .source;
    assert!(
        orig_src.contains("hero._xscale = 150;"),
        "original AS2 script must remain untouched: {orig_src}"
    );
}

#[test]
fn t455_new_xcode_swift_scene_live_guardrails_non_destructive_export_and_menus() {
    let ctx = egui::Context::default();
    let dir = TempTestDir::new("xcode-swift");

    // Start from zero-document state to prove FileNewXcodeSwiftScene works with no open document
    let mut editor = Editor::from_project(&ctx, Project::blank());
    while editor.has_document() {
        assert!(editor.close_session_tab(&ctx, 0));
    }
    assert!(!editor.has_document());

    let zero_doc_spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let file_items = &zero_doc_spec.menu("File").expect("File menu").items;
    assert!(
        file_items.iter().any(|item| matches!(
            item,
            NativeMenuItemSpec::Action {
                label,
                command_id: Some(CommandId::FileNewXcodeSwiftScene),
                enabled: true,
                ..
            } if label.contains("Experimental")
        )),
        "File ▸ New Xcode / Swift Scene (Experimental) must be enabled and labeled Experimental in zero-document mode"
    );
    let zero_doc_search =
        crate::panels::function_search::results_for_editor(&editor, Some(&ctx), "experimental");
    assert!(zero_doc_search.contains(&CommandId::FileNewXcodeSwiftScene));

    // Execute File ▸ New Xcode / Swift Scene
    editor.execute_command(&ctx, CommandId::FileNewXcodeSwiftScene);
    assert!(editor.has_document());
    assert!(
        editor.project.publish_settings().xcode_swift_target,
        "starter scene must enable xcode_swift_target"
    );
    assert!(
        editor
            .project
            .sources
            .iter()
            .any(|u| u.path == "SwiftHost.as"),
        "starter scene must scaffold SwiftHost.as"
    );

    // Starter template must pass both compilation AND live Swift target guardrails with 0 warnings!
    neoflash_script::check_project(&editor.project).expect("starter scene must compile cleanly");
    let initial_warnings = neoflash_script::check_project_warnings(&editor.project);
    assert!(
        initial_warnings.is_empty(),
        "starter Xcode/Swift scene must have zero guardrail warnings, got: {initial_warnings:?}"
    );

    // Verify File ▸ Export ▸ Export Xcode Project (Swift)… is enabled in NativeMenuSpec & F1 search
    let doc_spec = NativeMenuSpec::build(&editor, Some(&ctx));
    let doc_file_items = &doc_spec.menu("File").expect("File menu").items;
    let export_items = doc_file_items
        .iter()
        .find_map(|it| match it {
            NativeMenuItemSpec::Submenu { label, items, .. } if label == "Export" => Some(items),
            _ => None,
        })
        .expect("Export submenu");
    assert!(
        export_items.iter().any(|it| matches!(
            it,
            NativeMenuItemSpec::Action {
                label,
                command_id: Some(CommandId::FileExportXcodeSwift),
                enabled: true,
                ..
            } if label.contains("Experimental")
        )),
        "File ▸ Export ▸ Export Xcode Project (Swift, Experimental)… must be enabled and labeled Experimental"
    );
    let doc_exp_search =
        crate::panels::function_search::results_for_editor(&editor, Some(&ctx), "experimental");
    assert!(doc_exp_search.contains(&CommandId::FileExportXcodeSwift));

    // Export Xcode project for the first time
    let xcode_out = dir.path().join("ExportedSwiftApp");
    assert!(editor.export_xcode_dialog(Some(xcode_out.clone())));
    let app_host_path = xcode_out.join("App").join("AppSwiftHost.swift");
    let contract_path = xcode_out
        .join("Generated")
        .join("Scenes")
        .join("SwiftHostContract.swift");
    assert!(app_host_path.is_file());
    assert!(contract_path.is_file());

    // Simulate user writing native StoreKit/UIKit code inside App/AppSwiftHost.swift
    let mut custom_host = std::fs::read_to_string(&app_host_path).expect("read AppSwiftHost.swift");
    let insert_pos = custom_host.rfind('}').expect("closing brace");
    custom_host.insert_str(
        insert_pos,
        "\n    // USER CUSTOM SWIFT CODE — DO NOT OVERWRITE\n    private let customToken = \"storekit-live\"\n",
    );
    std::fs::write(&app_host_path, &custom_host).expect("write customized AppSwiftHost.swift");

    // Add a new portable method to SwiftHost.as AND an ad-hoc proxy call on a Stage button without declaring it in SwiftHost.as!
    editor.transact(|ed| {
        let unit = ed
            .project
            .sources
            .iter_mut()
            .find(|u| u.path == "SwiftHost.as")
            .expect("SwiftHost.as");
        unit.source = unit.source.replace(
            "public static function hapticImpact(",
            "public static function unlockAchievement(badgeId:String, points:int):Boolean {\n            trace(\"[SwiftHost mock] unlockAchievement\", badgeId, points);\n            return true;\n        }\n\n        public static function hapticImpact(",
        );
        let btn_sym = make_test_symbol("sym_bonus_btn", "BonusBtn", SymbolKind::Button);
        let btn_id = btn_sym.id.clone();
        ed.project.symbols.push(btn_sym);
        let root_id = ed.project.root.clone();
        let root = ed.project.symbol_mut(&root_id).expect("root");
        root.timeline.layers[0].keyframes[0].elements.push(Element {
            id: "el_bonus_btn".into(),
            name: String::new(),
            kind: ElementKind::Instance {
                symbol_id: btn_id,
                playback: InstancePlayback::Loop,
            },
            transform: Transform::default(),
            scripts: vec![ObjectScript {
                event: ObjectEvent::Click,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "on (release) {\n    SwiftHost.claimDailyBonus(\"gold\");\n}".into(),
                },
            }],
            ..Element::default()
        });
    });
    assert!(neoflash_script::check_project_warnings(&editor.project).is_empty());

    // Re-export to the same folder -> App/ custom code preserved + both explicit & ad-hoc empty stubs spliced in!
    assert!(editor.export_xcode_dialog(Some(xcode_out)));
    let updated_host =
        std::fs::read_to_string(&app_host_path).expect("read updated AppSwiftHost.swift");
    assert!(
        updated_host.contains("USER CUSTOM SWIFT CODE — DO NOT OVERWRITE")
            && updated_host.contains("storekit-live"),
        "re-export must never clobber user Swift code in App/AppSwiftHost.swift"
    );
    assert!(
        updated_host.contains("func unlockAchievement(badgeId: String, points: Int) -> Bool"),
        "re-export must append stub for newly added SwiftHost.as method:\n{updated_host}"
    );
    assert!(
        updated_host.contains("    func claimDailyBonus(arg0: String) {\n    }"),
        "re-export must append pure empty stub for ad-hoc button proxy SwiftHost.claimDailyBonus:\n{updated_host}"
    );

    // Finally, verify live guardrail warning fires when a non-portable Flash type is put in SwiftHost.as
    editor.transact(|ed| {
        let unit = ed
            .project
            .sources
            .iter_mut()
            .find(|u| u.path == "SwiftHost.as")
            .expect("SwiftHost.as");
        unit.source = unit.source.replace(
            "public class SwiftHost {",
            "import flash.display.MovieClip;\n    public class SwiftHost {\n        public static function badRef(clip:MovieClip):void {}",
        );
    });
    let guardrail_warnings = neoflash_script::check_project_warnings(&editor.project);
    assert!(
        guardrail_warnings
            .iter()
            .any(|w| w.message.contains("MovieClip") && w.message.contains("SwiftHost")),
        "expected live SwiftHost portability warning for MovieClip parameter, got: {guardrail_warnings:?}"
    );
}
