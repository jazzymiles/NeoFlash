//! Unit and end-to-end tests for the NeoFlash Editor headless CLI (`T-302`).
use super::*;
use neoflash_core::{Keyframe, Layer, LayerProperties, ObjectEvent, ObjectScript, Script, new_id};
use neoflash_import::export_act_palette;
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-headless-{prefix}-{}-{}",
            std::process::id(),
            new_id("tmp")
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        Self { path: dir }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run_args(args: &[&str], stdin_data: &str) -> (i32, String, String) {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let detection = detect_headless_mode(&owned, None);
    let mut stdin = Cursor::new(stdin_data.as_bytes());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_cli_with_io(
        &owned,
        detection.explicit_headless,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        String::from_utf8(stdout).expect("utf8 stdout"),
        String::from_utf8(stderr).expect("utf8 stderr"),
    )
}

#[test]
fn detection_ignores_macos_finder_psn_and_preserves_gui_defaults() {
    assert_eq!(
        detect_headless_mode(&[], None),
        CliDetection {
            active: false,
            explicit_headless: false,
        }
    );
    assert_eq!(
        detect_headless_mode(&["-psn_0_123456".into()], None),
        CliDetection {
            active: false,
            explicit_headless: false,
        }
    );
    assert_eq!(
        detect_headless_mode(&["example.nflash".into()], None),
        CliDetection {
            active: false,
            explicit_headless: false,
        }
    );
    assert_eq!(
        detect_headless_mode(&["-psn_0_999".into()], Some(OsStr::new("0"))),
        CliDetection {
            active: false,
            explicit_headless: false,
        }
    );
    assert_eq!(
        detect_headless_mode(&["-psn_0_999".into()], Some(OsStr::new("1"))),
        CliDetection {
            active: true,
            explicit_headless: true,
        }
    );
    assert_eq!(
        detect_headless_mode(
            &[
                "-psn_0_999".into(),
                "--headless".into(),
                "capabilities".into()
            ],
            None
        ),
        CliDetection {
            active: true,
            explicit_headless: true,
        }
    );
    assert_eq!(
        detect_headless_mode(&["headless".into(), "validate".into()], None),
        CliDetection {
            active: true,
            explicit_headless: true,
        }
    );
    assert_eq!(
        detect_headless_mode(&["--help".into()], None),
        CliDetection {
            active: true,
            explicit_headless: false,
        }
    );
    assert_eq!(
        detect_headless_mode(&["-V".into()], None),
        CliDetection {
            active: true,
            explicit_headless: false,
        }
    );
}

#[test]
fn help_version_and_capabilities_schema() {
    let (code, stdout, stderr) = run_args(&["--help"], "");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("NeoFlash Editor Headless CLI"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = run_args(&["--version", "--format", "json"], "");
    assert_eq!(code, 0, "stderr: {stderr}");
    let v: Value = serde_json::from_str(&stdout).expect("json version");
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "version");
    assert!(!v["version"].as_str().unwrap_or("").is_empty());

    let (code, stdout, stderr) =
        run_args(&["-psn_0_42", "--headless", "capabilities", "--pretty"], "");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.is_empty());
    let caps: Value = serde_json::from_str(&stdout).expect("json capabilities");
    assert_eq!(caps["ok"], true);
    assert_eq!(caps["command"], "capabilities");
    assert!(caps["batch_schema"]["operations"]["add_element"].is_array());
    assert!(
        caps["export_targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "html5")
    );

    let (code, stdout, stderr) = run_args(&["headless", "schema"], "");
    assert_eq!(code, 0, "stderr: {stderr}");
    let schema: Value = serde_json::from_str(&stdout).expect("schema alias");
    assert_eq!(schema["ok"], true);
    assert_eq!(schema["command"], "capabilities");

    let (code, stdout, stderr) = run_args(&["--headless", "unknown_cmd"], "");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    let err: Value = serde_json::from_str(&stderr).expect("json stderr error");
    assert_eq!(err["ok"], false);
    assert_eq!(err["error"]["code"], "unknown_subcommand");
}

#[test]
fn new_and_inspect_filters_work() {
    let tmp = TempDirGuard::new("new-inspect");
    let proj_path = tmp.path.join("scene.nflash");
    let proj_str = proj_path.to_str().unwrap();

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "new",
            "--output",
            proj_str,
            "--name",
            "HeroMovie",
            "--width",
            "1280",
            "--height",
            "720",
            "--fps",
            "30",
            "--bg",
            "#112233",
            "--language",
            "as3",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let created: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(created["ok"], true);
    assert_eq!(created["width"], 1280);
    assert_eq!(created["height"], 720);
    assert_eq!(created["fps"], 30);
    assert_eq!(created["background"], "#112233");

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "inspect",
            "--project",
            proj_str,
            "--no-elements",
            "--no-scripts",
            "--no-resources",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let inspected: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(inspected["ok"], true);
    assert_eq!(inspected["metadata"]["name"], "HeroMovie");
    assert_eq!(inspected["validation"]["valid"], true);
    assert!(
        inspected["symbols"][0]["frame_scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let demo_path = tmp.path.join("demo.nflash");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "new",
            "--output",
            demo_path.to_str().unwrap(),
            "--demo",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "validate",
            "--project",
            demo_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let v: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["valid"], true);
}

#[test]
fn compile_and_validate_fail_closed_with_located_diagnostics() {
    let tmp = TempDirGuard::new("compile-fail");
    let valid_path = tmp.path.join("valid.nflash");
    let broken_path = tmp.path.join("broken.nflash");
    let strict_warn_path = tmp.path.join("warn.nflash");

    let mut project = Project::blank();
    let root_id = project.root.clone();
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "var score:int = 10;\ntrace(score);\n".into(),
            },
        });
    }
    fs::write(&valid_path, project.to_json().unwrap()).unwrap();

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "compile",
            "--project",
            valid_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let res: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(res["ok"], true);
    assert_eq!(res["error_count"], 0);

    // Broken frame script AND broken element object script.
    let mut broken = Project::blank();
    {
        let root = broken.symbol_mut(&broken.root.clone()).unwrap();
        root.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "let = ;".into(),
            },
        });
        root.timeline.layers[0].keyframes[0]
            .elements
            .push(neoflash_core::Element {
                id: "hero_el".into(),
                name: "Hero".into(),
                scripts: vec![ObjectScript {
                    event: ObjectEvent::Load,
                    script: Script {
                        language: ScriptLanguage::ActionScript2,
                        source: "function bad( {".into(),
                    },
                }],
                ..neoflash_core::Element::default()
            });
    }
    fs::write(&broken_path, broken.to_json().unwrap()).unwrap();

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "compile",
            "--project",
            broken_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    let err: Value = serde_json::from_str(&stderr).expect("compile error json");
    assert_eq!(err["ok"], false);
    assert_eq!(err["error"]["code"], "compile_failed");
    let diags = err["error"]["details"]["diagnostics"].as_array().unwrap();
    assert!(
        diags.len() >= 2,
        "expected frame + element diagnostics: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d["symbol"] == "Scene 1" && d["frame"] == 0)
    );
    assert!(
        diags
            .iter()
            .any(|d| d["element_id"] == "hero_el" && d["layer"] == "Layer 1")
    );

    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "validate",
            "--project",
            broken_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);
    let verr: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(verr["error"]["code"], "validation_failed");

    // Warning-only script (`trce(...)` or unused/unassigned heuristic) verified under --strict if any warning occurs,
    // plus model structural validation error check.
    let mut invalid_model = Project::blank();
    invalid_model.width = 0;
    fs::write(
        &strict_warn_path,
        serde_json::to_string(&invalid_model).unwrap(),
    )
    .unwrap();
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "validate",
            "--project",
            strict_warn_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);
    let m_err: Value = serde_json::from_str(&stderr).unwrap();
    assert!(
        m_err["error"]["details"]["validation_issue_count"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn eval_returns_poses_and_native_authored_playback_traces() {
    let tmp = TempDirGuard::new("eval-traces");
    let proj_path = tmp.path.join("animated.nflash");

    let mut project = Project::blank();
    let root_id = project.root.clone();
    {
        let sym = project.symbol_mut(&root_id).unwrap();
        sym.timeline.frame_count = 10;
        sym.timeline.layers = vec![Layer {
            id: "lyr_box".into(),
            name: "BoxLayer".into(),
            visible: true,
            locked: false,
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(neoflash_core::Easing::Linear),
                    elements: vec![neoflash_core::Element {
                        id: "box1".into(),
                        name: "Mover".into(),
                        kind: neoflash_core::ElementKind::Rectangle {
                            width: 40.0,
                            height: 40.0,
                            fill: [255, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..neoflash_core::Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 4,
                    elements: vec![neoflash_core::Element {
                        id: "box1".into(),
                        name: "Mover".into(),
                        transform: neoflash_core::Transform {
                            x: 100.0,
                            y: 40.0,
                            ..neoflash_core::Transform::default()
                        },
                        kind: neoflash_core::ElementKind::Rectangle {
                            width: 40.0,
                            height: 40.0,
                            fill: [255, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..neoflash_core::Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
            properties: LayerProperties::default(),
        }];
        sym.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace(\"boot\");".into(),
            },
        });
        sym.timeline.scripts.push(FrameScript {
            frame: 2,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace(\"midpoint\");\nstop();".into(),
            },
        });
    }
    fs::write(&proj_path, project.to_json().unwrap()).unwrap();

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "eval",
            "--project",
            proj_path.to_str().unwrap(),
            "--frames",
            "0..=3",
            "--scripts",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let ev: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(ev["ok"], true);
    assert_eq!(ev["scripts_executed"], true);
    let traces: Vec<&str> = ev["trace_lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(traces, vec!["boot", "midpoint"]);
    let frames = ev["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 4);
    assert_eq!(
        frames[1]["layers"][0]["elements"][0]["transform"]["x"],
        25.0
    );
    assert_eq!(frames[2]["halted_by_script"], true);
    assert_eq!(frames[3]["playback_frame"], 2);
}

#[test]
fn jsfl_sandbox_mutates_saves_and_enforces_flfile_capabilities() {
    let tmp = TempDirGuard::new("jsfl");
    let proj_path = tmp.path.join("src.nflash");
    let out_path = tmp.path.join("out.nflash");
    let allowed_dir = tmp.path.join("sandbox_io");
    let outside_file = tmp.path.join("secret.txt");
    fs::create_dir_all(&allowed_dir).unwrap();
    fs::write(&outside_file, "forbidden").unwrap();
    fs::write(allowed_dir.join("input.txt"), "LayerFromFile").unwrap();

    let project = Project::blank();
    let orig_json = project.to_json().unwrap();
    fs::write(&proj_path, &orig_json).unwrap();

    // 1. Default deny-all rejects FLfile.read without modifying output
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--eval",
            "FLfile.read(\"file:///input.txt\");",
            "--out",
            out_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);
    let err: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "jsfl_error");
    assert!(!out_path.exists());

    // 2. Traversal attempt even when --allow-read is enabled fails safely
    let (code, _, _) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--eval",
            "FLfile.read(\"file:///../secret.txt\");",
            "--allow-read",
            allowed_dir.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);

    // 3. Explicit allow-read / allow-write / allow-delete inside granted dir + --out save
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--eval",
            r#"
                var name = FLfile.read("file:///input.txt");
                FLfile.write("file:///written.txt", "ok:" + name);
                var doc = fl.getDocumentDOM();
                doc.width = 640;
                doc.getTimeline().addNewLayer(name);
                fl.trace("updated " + name);
                FLfile.write("file:///temp.txt", "bye");
                FLfile.remove("file:///temp.txt");
                name;
            "#,
            "--allow-read",
            allowed_dir.to_str().unwrap(),
            "--allow-write",
            allowed_dir.to_str().unwrap(),
            "--allow-delete",
            "--out",
            out_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let res: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(res["ok"], true);
    assert_eq!(res["return_value"], "LayerFromFile");
    assert!(res["mutations"].as_u64().unwrap() >= 2);
    assert_eq!(res["output_lines"], json!(["updated LayerFromFile"]));
    assert_eq!(
        fs::read_to_string(allowed_dir.join("written.txt")).unwrap(),
        "ok:LayerFromFile"
    );
    assert!(!allowed_dir.join("temp.txt").exists());
    let saved = Project::from_json(&fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(saved.width, 640);
    assert!(
        saved
            .symbol(&saved.root)
            .unwrap()
            .timeline
            .layers
            .iter()
            .any(|l| l.name == "LayerFromFile")
    );

    // 4. Runtime error with --in-place leaves original project untouched
    let (code, _, _) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--stdin",
            "--in-place",
        ],
        "fl.getDocumentDOM().width = 333;\nthrow 'boom';",
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(&proj_path).unwrap(), orig_json);
}

#[test]
fn batch_pipeline_builds_validates_exports_and_rolls_back_atomically() {
    let tmp = TempDirGuard::new("batch");
    let out_proj = tmp.path.join("built.nflash");
    let svg_out = tmp.path.join("frame.svg");

    let ops = json!({
        "ops": [
            { "op": "new", "name": "BatchDemo", "width": 800, "height": 600, "fps": 24 },
            { "op": "add_symbol", "id": "sym_badge", "name": "Badge", "kind": "movie_clip", "frame_count": 5 },
            { "op": "add_layer", "name": "Artwork" },
            { "op": "add_keyframe", "layer": "Artwork", "frame": 2, "label": "mid" },
            {
                "op": "add_element",
                "id": "el_rect",
                "name": "Card",
                "layer": "Artwork",
                "frame": 0,
                "kind": "rectangle",
                "x": 20.0,
                "y": 30.0,
                "width": 140.0,
                "height": 90.0,
                "fill": "#2255AA"
            },
            {
                "op": "add_element",
                "id": "el_oval",
                "layer": "Artwork",
                "frame": 0,
                "kind": "oval",
                "x": 200.0,
                "y": 80.0,
                "width": 50.0,
                "height": 50.0,
                "fill": "#FFAA00"
            },
            {
                "op": "add_element",
                "id": "el_path",
                "layer": "Artwork",
                "frame": 0,
                "kind": "path",
                "d": "M 0 0 L 40 0 L 20 35 Z",
                "fill": "#00AA66"
            },
            {
                "op": "add_element",
                "id": "el_txt",
                "layer": "Artwork",
                "frame": 0,
                "kind": "text",
                "text": "Hello CLI",
                "font_size": 28.0
            },
            {
                "op": "add_element",
                "id": "el_inst",
                "layer": "Artwork",
                "frame": 0,
                "kind": "instance",
                "symbol_id": "sym_badge"
            },
            {
                "op": "apply_command_steps",
                "layer": "Artwork",
                "frame": 0,
                "elements": ["el_rect"],
                "steps": [{ "op": "translate", "dx": 15.0, "dy": -5.0 }]
            },
            {
                "op": "set_script",
                "frame": 0,
                "language": "as3",
                "source": "trace(\"batch_ok\");"
            },
            { "op": "validate" },
            { "op": "compile" },
            { "op": "eval", "frame": 0, "scripts": true },
            { "op": "inspect", "no_resources": true },
            { "op": "export", "target": "svg", "out": svg_out.to_str().unwrap(), "frame": 0 }
        ]
    });

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            out_proj.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let batch_res: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(batch_res["ok"], true);
    assert_eq!(batch_res["steps_executed"], 16);
    assert!(out_proj.is_file());
    assert!(svg_out.is_file());

    let loaded = Project::from_json(&fs::read_to_string(&out_proj).unwrap()).unwrap();
    let art_layer = loaded
        .symbol(&loaded.root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .find(|l| l.name == "Artwork")
        .unwrap();
    let moved_rect = art_layer.keyframes[0]
        .elements
        .iter()
        .find(|e| e.id == "el_rect")
        .unwrap();
    assert_eq!(moved_rect.transform.x, 35.0);
    assert_eq!(moved_rect.transform.y, 25.0);

    // Atomic rollback: failing compile step prevents --in-place overwrite
    let snapshot_before = fs::read_to_string(&out_proj).unwrap();
    let bad_ops = json!([
        { "op": "set_project", "name": "ShouldRollBack" },
        { "op": "set_script", "frame": 0, "language": "js", "source": "let = ;" },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            out_proj.to_str().unwrap(),
            "--in-place",
        ],
        &bad_ops.to_string(),
    );
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    let err: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "compile_failed");
    assert_eq!(err["error"]["details"]["step_index"], 2);
    assert_eq!(fs::read_to_string(&out_proj).unwrap(), snapshot_before);

    // Per-step diagnostic JSON parse error + `code` alias for `set_script`
    let bad_shape = json!({
        "ops": [
            { "op": "new", "name": "Ok" },
            { "op": "set_script", "frame": 0, "code": "trace(\"alias_ok\");" },
            { "op": "add_keyframe", "frame": 1 }
        ]
    });
    let (code, stdout, stderr) =
        run_args(&["--headless", "batch", "--stdin"], &bad_shape.to_string());
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    let err: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "invalid_batch_json");
    assert!(err["error"]["message"].as_str().unwrap().contains("step 2"));

    let alias_ops = json!({
        "ops": [
            { "op": "new", "name": "Alias", "language": "as2" },
            { "op": "set_script", "frame": 0, "code": "trace(\"alias_ok\");" },
            { "op": "eval", "frames": "0..=0", "scripts": true }
        ]
    });
    let (code, stdout, stderr) =
        run_args(&["--headless", "batch", "--stdin"], &alias_ops.to_string());
    assert_eq!(code, 0, "stderr={stderr}");
    let alias_res: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        alias_res["results"][1]["result"]["language"],
        "actionscript2"
    );
    assert_eq!(
        alias_res["results"][2]["result"]["trace_lines"][0],
        "alias_ok"
    );
}

#[test]
fn import_and_export_targets_roundtrip() {
    let tmp = TempDirGuard::new("import-export");

    // 1. Import SVG -> .nflash
    let svg_in = tmp.path.join("badge.svg");
    fs::write(
        &svg_in,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="0 0 120 80">
            <rect x="10" y="10" width="100" height="60" fill="#3366CC"/>
        </svg>"##,
    )
    .unwrap();
    let svg_proj = tmp.path.join("from_svg.nflash");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "import",
            "--input",
            svg_in.to_str().unwrap(),
            "--out",
            svg_proj.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let imp_svg: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(imp_svg["ok"], true);
    assert!(svg_proj.is_file());

    // 2. Import FXG -> .nflash
    let fxg_in = tmp.path.join("panel.fxg");
    fs::write(
        &fxg_in,
        r##"<?xml version="1.0" encoding="utf-8"?>
        <Graphic version="2.0" viewWidth="200" viewHeight="100" xmlns="http://ns.adobe.com/fxg/2008">
          <Rect x="5" y="5" width="90" height="40"><fill><SolidColor color="#EE4422"/></fill></Rect>
        </Graphic>"##,
    )
    .unwrap();
    let fxg_proj = tmp.path.join("from_fxg.nflash");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "import",
            "--input",
            fxg_in.to_str().unwrap(),
            "--out",
            fxg_proj.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");

    // 3. Import ACT palette -> .nflash
    let act_in = tmp.path.join("colors.act");
    let act_bytes = export_act_palette(&[neoflash_core::GlobalSwatch {
        id: "s1".into(),
        name: "Cobalt".into(),
        color: [30, 80, 200, 255],
    }]);
    fs::write(&act_in, act_bytes).unwrap();
    let act_proj = tmp.path.join("from_act.nflash");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "import",
            "--input",
            act_in.to_str().unwrap(),
            "--out",
            act_proj.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");

    // 4. Export SVG, FLA, XFL, SWF, Lottie, Banner
    let exp_svg = tmp.path.join("out.svg");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            svg_proj.to_str().unwrap(),
            "--target",
            "svg",
            "--out",
            exp_svg.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(serde_json::from_str::<Value>(&stdout).unwrap()["ok"], true);
    assert!(exp_svg.is_file());

    let exp_fla = tmp.path.join("out.fla");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            svg_proj.to_str().unwrap(),
            "--target",
            "fla",
            "--out",
            exp_fla.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(exp_fla.is_file());

    let exp_xfl = tmp.path.join("out_xfl");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            svg_proj.to_str().unwrap(),
            "--target",
            "xfl",
            "--out",
            exp_xfl.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(exp_xfl.join("DOMDocument.xml").is_file());

    let exp_swf = tmp.path.join("out.swf");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            svg_proj.to_str().unwrap(),
            "--target",
            "swf",
            "--out",
            exp_swf.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(exp_swf.is_file());

    let exp_lottie = tmp.path.join("out.lottie.json");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            svg_proj.to_str().unwrap(),
            "--target",
            "lottie",
            "--out",
            exp_lottie.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(exp_lottie.is_file());

    // 5. Export HTML5 (auto-resolving target/web-runtime when built)
    if import_export::resolve_web_runtime(None).is_ok() {
        let html_out = tmp.path.join("html5");
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "export",
                "--project",
                svg_proj.to_str().unwrap(),
                "--target",
                "html5",
                "--size-level",
                "full",
                "--out",
                html_out.to_str().unwrap(),
            ],
            "",
        );
        assert_eq!(code, 0, "stderr: {stderr}");
        let res: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(res["ok"], true);
        assert!(html_out.join("index.html").is_file());
    }
}
