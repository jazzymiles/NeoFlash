//! `[T-496B]` — Externalized `as-intrinsics.js` & Clean Publish `bundle.js` diet (`FO-10`),
//! AS2 non-scalar `static`/`instance` field initializers (`FO-16`), AS3 class-constant parameter
//! defaults (`Proposal #4`), FP8 SWF round-trip of `fallout-as2`/`doom-as2` & 60-branch `else if`
//! chains (`FO-04`), and multiple AS2 button `keyPress` conditions (`Proposal #5`).

use neoflash_core::{
    Element, ElementKind, FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties,
    ObjectEvent, ObjectScript, Project, Script, ScriptLanguage, SourceUnit, Symbol, SymbolKind,
    Timeline, Transform,
};
use neoflash_export::{
    AudioExportPolicy, FontExportPolicy, HtmlPublishConfig, PublishOptions, SizeLevel,
    SwfPublishOptions, SwfPublishProfile, export_project_with_config,
    export_project_with_config_and_wrapper, publish_swf_bytes,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn write_mock_wasm_runtime(runtime_dir: &Path) {
    fs::create_dir_all(runtime_dir).expect("create mock runtime dir");
    fs::write(runtime_dir.join("neoflash_web.js"), "var NeoFlashWasm;\n")
        .expect("write neoflash_web.js");
    fs::write(runtime_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0")
        .expect("write neoflash_web_bg.wasm");
}

fn node_binary() -> PathBuf {
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    for candidate in ["/opt/homebrew/bin/node", "/usr/local/bin/node"] {
        let path = Path::new(candidate);
        if path.exists() {
            return path.to_path_buf();
        }
    }
    PathBuf::from(exe)
}

fn node_module_url(path: &Path) -> String {
    url::Url::from_file_path(path)
        .unwrap_or_else(|()| panic!("cannot convert {} to a file URL", path.display()))
        .into()
}

#[test]
fn t496b_bundle_diet_field_initializers_const_defaults_swf_roundtrip_and_multi_keypress() {
    let root = workspace_root();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    // -------------------------------------------------------------------------
    // 1. [FO-10] Externalized `as-intrinsics.js` & Clean Publish `bundle.js` diet
    // -------------------------------------------------------------------------
    let mut clean_cfg = HtmlPublishConfig::clean_publish_default();
    clean_cfg.assets_folder = true;
    clean_cfg.sprite_sheets = true;
    clean_cfg.omit_file_fallback = true;
    let compact_opts = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Compact,
        audio: AudioExportPolicy::Original,
    };

    for (slug, rel_path, max_compiled_script_bytes) in [
        ("doom-as2", "examples/doom-as2.nflash", 140_000usize),
        ("snake-as2", "examples/snake-as2.nflash", 14_000usize),
        ("fallout-as2", "examples/fallout-as2.nflash", 285_000usize),
    ] {
        let raw_json = fs::read_to_string(root.join(rel_path)).unwrap();
        let proj = Project::from_json(&raw_json).unwrap();
        let out_dir = temp_dir.path().join(format!("clean-{slug}"));
        export_project_with_config(&proj, &out_dir, &runtime_dir, &compact_opts, &clean_cfg)
            .unwrap();
        assert!(
            out_dir.join("as-intrinsics.js").is_file(),
            "{slug} must emit standalone as-intrinsics.js"
        );
        let bundle_text = fs::read_to_string(out_dir.join("bundle.js")).unwrap();
        assert!(
            !bundle_text.contains("\"as_prelude\":"),
            "{slug} bundle.js must not inline as_prelude"
        );
        let json_slice = bundle_text
            .trim()
            .strip_prefix("globalThis.NeoFlashBundle=")
            .or_else(|| {
                bundle_text
                    .trim()
                    .strip_prefix("globalThis.NeoFlashBundle = ")
            })
            .unwrap()
            .trim_end_matches(';');
        let parsed: serde_json::Value = serde_json::from_str(json_slice).unwrap();
        let compiled_script_bytes = parsed["load_summary"]["scripts_compiled_bytes"]
            .as_u64()
            .unwrap() as usize;
        let embedded_res_b64: usize = parsed["project"]["resources"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|r| r["data_base64"].as_str().map_or(0, str::len))
                    .sum()
            })
            .unwrap_or(0);
        println!(
            "[T-496B FO-10] {slug}: bundle.js = {} B (non-media = {} B, scripts_compiled_bytes = {} B)",
            bundle_text.len(),
            bundle_text.len().saturating_sub(embedded_res_b64),
            compiled_script_bytes
        );
        assert!(
            compiled_script_bytes <= max_compiled_script_bytes,
            "{slug} scripts_compiled_bytes exceeded {max_compiled_script_bytes} B: got {compiled_script_bytes} B"
        );
        // Also verify that without embedded WAV/TTF base64 resources, a 38 KB Fallout-scale AS2 project's
        // entire `bundle.js` is < 160 KB!
        if slug == "doom-as2" {
            let mut code_only = proj.clone();
            code_only.resources.clear();
            for sym in &mut code_only.symbols {
                for layer in &mut sym.timeline.layers {
                    for kf in &mut layer.keyframes {
                        kf.sound = None;
                        kf.bitmap_fills.clear();
                        kf.elements
                            .retain(|e| !matches!(e.kind, ElementKind::Bitmap { .. }));
                    }
                }
            }
            let code_out = temp_dir.path().join("doom-code-only");
            export_project_with_config(
                &code_only,
                &code_out,
                &runtime_dir,
                &compact_opts,
                &clean_cfg,
            )
            .unwrap();
            let code_bundle_len = fs::metadata(code_out.join("bundle.js")).unwrap().len();
            println!("[T-496B FO-10] doom-as2 code-only bundle.js = {code_bundle_len} B");
            assert!(
                code_bundle_len < 160_000,
                "doom-as2 code-only bundle.js must be < 160 KB, got {code_bundle_len} B"
            );
        }
        if slug == "fallout-as2" {
            // Also verify the original 38 KB Fallout v13 multi-class AS2 scale (`VaultData`, `PathLos`, `ArtGen`
            // = 48 KB of `.as` classes!) produces a complete `bundle.js` well under 120 KB!
            let mut classes_only = Project::blank();
            classes_only.script_language = Some(ScriptLanguage::ActionScript2);
            classes_only.sources = proj.sources.clone();
            classes_only.symbols[0].timeline.scripts.push(FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "import fallout.VaultData;\nimport fallout.PathLos;\nimport fallout.ArtGen;\nstop();\n".into(),
                },
            });
            classes_only.upgrade_format_for_features();
            let v13_out = temp_dir.path().join("fallout-classes-38kb");
            export_project_with_config(
                &classes_only,
                &v13_out,
                &runtime_dir,
                &compact_opts,
                &clean_cfg,
            )
            .unwrap();
            let v13_bundle_len = fs::metadata(v13_out.join("bundle.js")).unwrap().len();
            println!(
                "[T-496B FO-10] fallout-as2 3-class (48 KB .as) bundle.js = {v13_bundle_len} B"
            );
            assert!(
                v13_bundle_len < 120_000,
                "48 KB 3-class Fallout AS2 bundle.js must be < 120 KB (was ~808 KB), got {v13_bundle_len} B"
            );
        }
    }

    // Verify pure JS and scriptless projects DO NOT emit `as-intrinsics.js`, while single-file custom
    // HTML wrappers without `$SCRIPT_BUNDLE`/`$RUNTIME_SCRIPT` inline `as_prelude` safely.
    let mut js_proj = Project::blank();
    js_proj.script_language = Some(ScriptLanguage::JavaScript);
    js_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "this.ready = 1;".into(),
        },
    });
    js_proj.upgrade_format_for_features();
    let js_out = temp_dir.path().join("pure-js");
    export_project_with_config(&js_proj, &js_out, &runtime_dir, &compact_opts, &clean_cfg).unwrap();
    assert!(
        !js_out.join("as-intrinsics.js").exists(),
        "pure JS export must omit as-intrinsics.js"
    );
    let scriptless_out = temp_dir.path().join("scriptless");
    export_project_with_config(
        &Project::blank(),
        &scriptless_out,
        &runtime_dir,
        &compact_opts,
        &clean_cfg,
    )
    .unwrap();
    assert!(
        !scriptless_out.join("as-intrinsics.js").exists(),
        "scriptless export must omit as-intrinsics.js"
    );

    // -------------------------------------------------------------------------
    // 2. [FO-16] AS2 non-scalar static & instance field initializers (HTML5 + FP8 SWF)
    // -------------------------------------------------------------------------
    let mut fo16_proj = Project::blank();
    fo16_proj.script_language = Some(ScriptLanguage::ActionScript2);
    fo16_proj.sources.push(SourceUnit {
        id: "base-unit".into(),
        path: "fallout/BaseGrid.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "class fallout.BaseGrid {\n    public var baseTag:String = \"vault\";\n    public function BaseGrid(tag:String) {\n        this.baseTag = tag;\n    }\n}\n".into(),
    });
    fo16_proj.sources.push(SourceUnit {
        id: "hex-unit".into(),
        path: "fallout/HexGrid.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "import fallout.BaseGrid;\nclass fallout.HexGrid extends BaseGrid {\n    public static var EVEN_DX:Array = [0, 1, 0, -1, -1, -1];\n    public static var CFG:Object = { cols: 6, rows: 4 };\n    public static var AREA:Number = 6 * 4;\n    public var items:Array = [10, 20];\n    public var meta:Object = { ok: true };\n    public function HexGrid(extra:Number) {\n        super(\"v13\");\n        this.items.push(extra);\n    }\n}\n".into(),
    });
    fo16_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "import fallout.HexGrid;\nvar a:HexGrid = new HexGrid(30);\nvar b:HexGrid = new HexGrid(40);\na.items.push(99);\nvar summary = HexGrid.EVEN_DX.join(\",\") + \"|\" + HexGrid.CFG.cols + \"|\" + HexGrid.AREA + \"|\" + a.baseTag + \"|\" + a.items.join(\",\") + \"|\" + b.items.join(\",\") + \"|\" + b.meta.ok;\ntrace(summary);\nstop();\n".into(),
        },
    });
    fo16_proj.upgrade_format_for_features();
    fo16_proj.validate().unwrap();
    neoflash_script::check_project(&fo16_proj).expect("FO-16 AS2 class initializers compile");

    let (fo16_swf, fo16_rep) = publish_swf_bytes(
        &fo16_proj,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer8Avm1,
            ..SwfPublishOptions::default()
        },
    )
    .expect("FO-16 FP8 SWF export");
    assert!(!fo16_rep.has_blocking(), "{:?}", fo16_rep.items);
    let fo16_play = neoflash_import::play_swf_bytes(&fo16_swf, 4).expect("play FO-16 SWF");
    assert_eq!(
        fo16_play.trace_output,
        vec!["0,1,0,-1,-1,-1|6|24|v13|10,20,30,99|10,20,40|true"],
        "FO-16 FP8 SWF execution must evaluate static & per-instance non-scalar initializers"
    );

    // Also test single-file custom HTML wrapper inlining `as_prelude` on `fo16_proj`.
    let single_wrapper_out = temp_dir.path().join("fo16-custom-wrapper");
    export_project_with_config_and_wrapper(
        &fo16_proj,
        &single_wrapper_out,
        &runtime_dir,
        &compact_opts,
        &clean_cfg,
        Some("<!doctype html><html><body><canvas id=\"stage\"></canvas><script src=\"bundle.js\"></script><script src=\"runtime.js\"></script></body></html>"),
    )
    .unwrap();
    let wrapper_bundle = fs::read_to_string(single_wrapper_out.join("bundle.js")).unwrap();
    assert!(
        wrapper_bundle.contains("\"as_prelude\":"),
        "custom HTML wrapper without $SCRIPT_BUNDLE must keep inline bundle.as_prelude fallback"
    );

    // -------------------------------------------------------------------------
    // 3. [Proposal #4] AS3 class constants as parameter defaults (HTML5 + AVM2 SWF + diagnostics)
    // -------------------------------------------------------------------------
    let mut prop4_proj = Project::blank();
    prop4_proj.script_language = Some(ScriptLanguage::ActionScript3);
    prop4_proj.sources.push(SourceUnit {
        id: "stepper-as3".into(),
        path: "sim/Stepper.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package sim {\n    public class Stepper {\n        public static const DEFAULT_SPEED:int = 10;\n        private static const ZERO:int = 0;\n        public var initial:int;\n        public function Stepper(seed:int = DEFAULT_SPEED) {\n            this.initial = seed;\n        }\n        public function step(speed:int = DEFAULT_SPEED):int {\n            return speed;\n        }\n        public function reset(val:int = ZERO):int {\n            return val;\n        }\n    }\n}\n".into(),
    });
    prop4_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "import sim.Stepper;\nfunction extStep(speed:int = Stepper.DEFAULT_SPEED):int { return speed; }\nvar sDef:Stepper = new Stepper();\nvar sZero:Stepper = new Stepper(0);\nvar out:String = sDef.initial + \"|\" + sZero.initial + \"|\" + sDef.step() + \"|\" + sDef.step(0) + \"|\" + sDef.reset() + \"|\" + sDef.reset(7) + \"|\" + extStep() + \"|\" + extStep(0);\ntrace(out);\nstop();\n".into(),
        },
    });
    prop4_proj.upgrade_format_for_features();
    prop4_proj.validate().unwrap();
    neoflash_script::check_project(&prop4_proj).expect("Proposal #4 AS3 const defaults compile");

    let (prop4_swf, prop4_rep) = publish_swf_bytes(
        &prop4_proj,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            ..SwfPublishOptions::default()
        },
    )
    .expect("Proposal #4 FP10 SWF export");
    assert!(!prop4_rep.has_blocking(), "{:?}", prop4_rep.items);
    let prop4_play = neoflash_import::play_swf_bytes(&prop4_swf, 4).expect("play Proposal #4 SWF");
    assert_eq!(
        prop4_play.trace_output,
        vec!["10|0|10|0|0|7|10|0"],
        "Proposal #4 AVM2 SWF must preserve omitted default (10) and explicit zero (0)"
    );

    // Unresolved constant default must report exact parameter location in both HTML5 and SWF!
    let mut bad_const_proj = Project::blank();
    bad_const_proj.script_language = Some(ScriptLanguage::ActionScript3);
    bad_const_proj.sources.push(SourceUnit {
        id: "bad-const".into(),
        path: "sim/Bad.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package sim {\n    public class Bad {\n        public function run(ok:int = 1, broken:int = MISSING_SPEED):int {\n            return ok + broken;\n        }\n    }\n}\n".into(),
    });
    bad_const_proj.upgrade_format_for_features();
    let diags = neoflash_script::check_project(&bad_const_proj)
        .expect_err("unresolved parameter default constant must fail check_project");
    assert_eq!(
        diags[0].line, 3,
        "diagnostic must point to line 3: {:?}",
        diags[0]
    );
    assert!(
        diags[0].column >= 40
            && diags[0].message.contains("MISSING_SPEED")
            && diags[0].message.contains("broken"),
        "diagnostic must point to parameter 'broken = MISSING_SPEED' on line 3, got: {:?}",
        diags[0]
    );
    let swf_err = publish_swf_bytes(
        &bad_const_proj,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            ..SwfPublishOptions::default()
        },
    )
    .expect_err("unresolved parameter default constant must fail SWF export");
    let swf_err_msg = swf_err.to_string();
    assert!(
        swf_err_msg.contains("line 3") && swf_err_msg.contains("MISSING_SPEED"),
        "SWF diagnostic must report parameter line 3 and unresolved constant name, got: {swf_err_msg}"
    );

    // -------------------------------------------------------------------------
    // 4. [FO-04] FP8 SWF round-trip of `fallout-as2.nflash`, `doom-as2.nflash`, & 60-branch `else if`
    // -------------------------------------------------------------------------
    for rel_path in ["examples/fallout-as2.nflash", "examples/doom-as2.nflash"] {
        let raw = fs::read_to_string(root.join(rel_path)).unwrap();
        let orig = Project::from_json(&raw).unwrap();
        let (swf_bytes, swf_rep) = publish_swf_bytes(
            &orig,
            &SwfPublishOptions {
                profile: SwfPublishProfile::FlashPlayer8Avm1,
                ..SwfPublishOptions::default()
            },
        )
        .unwrap_or_else(|e| panic!("{rel_path} FP8 SWF export failed: {e}"));
        assert!(
            !swf_rep.has_blocking(),
            "{rel_path} SWF blocking: {:?}",
            swf_rep.items
        );

        let swf_path = temp_dir.path().join(format!(
            "{}.swf",
            Path::new(rel_path).file_stem().unwrap().to_string_lossy()
        ));
        fs::write(&swf_path, &swf_bytes).unwrap();
        let reimported = neoflash_import::import_project(&swf_path)
            .unwrap_or_else(|e| panic!("{rel_path} SWF re-import failed: {e}"));
        reimported
            .validate()
            .unwrap_or_else(|e| panic!("{rel_path} re-imported SWF failed validate(): {e}"));
        if rel_path.contains("fallout-as2") {
            assert!(
                reimported.sources.len() >= 3,
                "fallout-as2 SWF re-import must reconstruct AS2 class SourceUnits, got {}",
                reimported.sources.len()
            );
        }
        neoflash_script::check_project(&reimported).unwrap_or_else(|errs| {
            let formatted: Vec<String> = errs.iter().map(ToString::to_string).collect();
            panic!(
                "{rel_path} re-imported FP8 SWF failed strict check_project:\n{}",
                formatted.join("\n")
            );
        });
    }

    // 60-branch sequential hotkey `if / else if` table must flatten without hitting 48-level parser limit!
    let mut dispatch_branches = String::from("var k = 57;\nvar hit = -1;\n");
    for i in 0..60 {
        let kw = if i == 0 { "if" } else { "else if" };
        dispatch_branches.push_str(&format!("{kw} (k == {i}) {{ hit = {i}; }}\n"));
    }
    dispatch_branches.push_str("else { hit = 999; }\ntrace(hit);\nstop();\n");
    let mut chain_proj = Project::blank();
    chain_proj.script_language = Some(ScriptLanguage::ActionScript2);
    chain_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: dispatch_branches,
        },
    });
    chain_proj.upgrade_format_for_features();
    let (chain_swf, _) = publish_swf_bytes(
        &chain_proj,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer8Avm1,
            ..SwfPublishOptions::default()
        },
    )
    .expect("compile 60-branch if/else-if SWF");
    let chain_swf_path = temp_dir.path().join("chain60.swf");
    fs::write(&chain_swf_path, &chain_swf).unwrap();
    let chain_reimported = neoflash_import::import_project(&chain_swf_path).unwrap();
    let decompiled_chain = &chain_reimported.symbols[0].timeline.scripts[0]
        .script
        .source;
    assert!(
        decompiled_chain.matches("} else if (").count() >= 59,
        "60-branch dispatch table must decompile to flat else-if chain:\n{decompiled_chain}"
    );
    neoflash_script::check_project(&chain_reimported)
        .expect("re-imported 60-branch else-if script must pass check_project with 0 errors");

    // -------------------------------------------------------------------------
    // 5. [Proposal #5] Two AS2 `keyPress` conditions on one DefineButton2 round-trip
    // -------------------------------------------------------------------------
    let mut btn_proj = Project::blank();
    btn_proj.script_language = Some(ScriptLanguage::ActionScript2);
    let btn_sym_id: String = "hotkey-btn".into();
    btn_proj.symbols.push(Symbol {
        id: btn_sym_id.clone(),
        name: "HotkeyButton".into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            guides: vec![],
            scripts: vec![],
            layers: vec![Layer {
                id: "btn-layer".into(),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "btn-hit".into(),
                        kind: ElementKind::Rectangle {
                            width: 80.0,
                            height: 32.0,
                            fill: [40, 180, 90, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    });
    btn_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "var hits = 0;\nstop();\n".into(),
        },
    });
    btn_proj.symbols[0].timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "btn-inst".into(),
        name: "hotkeyBtn".into(),
        transform: Transform::default(),
        kind: ElementKind::Instance {
            symbol_id: btn_sym_id,
            playback: InstancePlayback::Independent,
        },
        scripts: vec![ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "on (keyPress \"a\", keyPress \"b\") {\n    hits = hits + 1;\n    trace(\"hits=\" + hits);\n}\n".into(),
            },
        }],
        ..Element::default()
    }];
    btn_proj.upgrade_format_for_features();
    btn_proj.validate().unwrap();

    let (btn_swf_1, btn_rep_1) = publish_swf_bytes(
        &btn_proj,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer8Avm1,
            ..SwfPublishOptions::default()
        },
    )
    .expect("export multi-keyPress DefineButton2 SWF");
    assert!(!btn_rep_1.has_blocking(), "{:?}", btn_rep_1.items);

    let assert_button_keys_in_swf = |swf_bytes: &[u8], label: &str| {
        let mut player = neoflash_import::SwfBytecodePlayer::from_bytes(swf_bytes).unwrap();
        player.step_frame();
        player.trigger_key_press(b'a');
        assert_eq!(
            player.run_frames(1).trace_output,
            vec!["hits=1"],
            "{label}: key 'a' fires once"
        );
        player.trigger_key_press(b'b');
        assert_eq!(
            player.run_frames(1).trace_output,
            vec!["hits=1", "hits=2"],
            "{label}: key 'b' fires once"
        );
        player.trigger_key_press(b'c');
        assert_eq!(
            player.run_frames(1).trace_output,
            vec!["hits=1", "hits=2"],
            "{label}: third key 'c' must have no effect"
        );
    };
    assert_button_keys_in_swf(&btn_swf_1, "initial exported SWF");

    // Reopen the exported SWF, verify preserved multi-keyPress handler, and re-export + execute!
    let btn_swf_path = temp_dir.path().join("multi_keypress_btn.swf");
    fs::write(&btn_swf_path, &btn_swf_1).unwrap();
    let reopened_btn_proj =
        neoflash_import::import_project(&btn_swf_path).expect("reopen multi-keyPress SWF");
    reopened_btn_proj.validate().unwrap();
    neoflash_script::check_project(&reopened_btn_proj)
        .expect("reopened multi-keyPress SWF passes check_project");
    let reopened_btn_scripts: Vec<&str> = reopened_btn_proj
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .flat_map(|e| &e.scripts)
        .map(|os| os.script.source.as_str())
        .collect();
    assert!(
        reopened_btn_scripts
            .iter()
            .any(|src| src.contains("keyPress \"a\"") && src.contains("keyPress \"b\"")),
        "reopened SWF button script must preserve both keyPress 'a' and keyPress 'b': {reopened_btn_scripts:?}"
    );
    let (btn_swf_2, _) = publish_swf_bytes(
        &reopened_btn_proj,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer8Avm1,
            ..SwfPublishOptions::default()
        },
    )
    .expect("re-export reopened multi-keyPress SWF");
    assert_button_keys_in_swf(&btn_swf_2, "reopened & re-exported SWF");

    // Also execute FO-16 & Proposal #4 HTML5 bundles in the real JS runtime host!
    let fo16_html_out = temp_dir.path().join("fo16-html5");
    export_project_with_config(
        &fo16_proj,
        &fo16_html_out,
        &runtime_dir,
        &compact_opts,
        &clean_cfg,
    )
    .unwrap();
    let prop4_html_out = temp_dir.path().join("prop4-html5");
    export_project_with_config(
        &prop4_proj,
        &prop4_html_out,
        &runtime_dir,
        &compact_opts,
        &clean_cfg,
    )
    .unwrap();

    let node_runner = temp_dir.path().join("verify-t496b-html5.mjs");
    let host_integration_url =
        node_module_url(&root.join("crates/neoflash-script/tests/host-integration.mjs"));
    let rust_runtime_url =
        node_module_url(&root.join("crates/neoflash-script/tests/rust-runtime-host.mjs"));
    let runner_src = format!(
        r#"import assert from 'node:assert/strict';
import fs from 'node:fs';
import {{ createRuntimeHost }} from {host_integration_url:?};
import {{ runtimeWasm }} from {rust_runtime_url:?};

const root = {root_str:?};
function runBundle(bundlePath) {{
  const raw = fs.readFileSync(bundlePath, 'utf8');
  const jsonText = raw.replace(/^globalThis\.NeoFlashBundle\s*=\s*/, '').replace(/;\s*$/, '');
  const bundle = JSON.parse(jsonText);
  const player = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
  const compiled = bundle.scripts.map((s, idx) => ({{
    script: {{ language: s.language, source: s.source }},
    code: s.code,
    location: s.location || `s-${{idx}}`,
  }}));
  const env = createRuntimeHost(root, player, compiled, {{ bundle, project: bundle.project }});
  env.host.resetProject();
  const trace = env.host.lastTrace();
  player.free();
  return trace;
}}

assert.equal(runBundle({fo16_bundle:?}), '0,1,0,-1,-1,-1|6|24|v13|10,20,30,99|10,20,40|true');
assert.equal(runBundle({prop4_bundle:?}), '10|0|10|0|0|7|10|0');
"#,
        host_integration_url = host_integration_url,
        rust_runtime_url = rust_runtime_url,
        root_str = root.display().to_string(),
        fo16_bundle = fo16_html_out.join("bundle.js").display().to_string(),
        prop4_bundle = prop4_html_out.join("bundle.js").display().to_string(),
    );
    fs::write(&node_runner, runner_src).unwrap();
    let out = Command::new(node_binary())
        .arg(&node_runner)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "HTML5 runtime check failed:\nSTDOUT:{}\nSTDERR:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
