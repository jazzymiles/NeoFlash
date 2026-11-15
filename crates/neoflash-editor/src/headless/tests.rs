//! Unit and end-to-end tests for the NeoFlash Editor headless CLI (`T-302`).
use super::*;
use neoflash_core::{
    Keyframe, Layer, LayerProperties, ObjectEvent, ObjectScript, ResourceUnit, Script, new_id,
};
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
fn project_loaders_reject_oversized_input_before_lenient_validation_fallback() {
    let tmp = TempDirGuard::new("oversized-project");
    let path = tmp.path.join("oversized.nflash");
    let file = fs::File::create(&path).expect("create sparse oversized project");
    file.set_len(MAX_PROJECT_JSON_BYTES as u64 + 1)
        .expect("extend sparse oversized project");
    drop(file);

    for result in [load_project_lenient(&path), load_project_valid(&path)] {
        let error = result.expect_err("oversized project must be rejected before JSON parsing");
        assert_eq!(error.exit_code, 1);
        assert_eq!(error.code, "resource_limit_exceeded");
    }
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
    assert!(caps["batch_schema"]["strict"].is_string());
    assert!(
        caps["batch_schema"]["atomicity"]
            .as_str()
            .unwrap_or_default()
            .contains("not rolled back")
    );
    assert!(caps["batch_schema"]["operations"]["add_element"].is_array());
    assert!(
        caps["batch_schema"]["operations"]["export"]
            .as_array()
            .expect("export schema")
            .iter()
            .any(|field| field == "banner_max_gzip?")
    );
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
    assert!(stderr.is_empty());
    let err: Value = serde_json::from_str(&stdout).expect("json stdout error");
    assert_eq!(err["ok"], false);
    assert_eq!(err["exit_code"], 2);
    assert_eq!(err["error"]["code"], "unknown_subcommand");

    let (code, stdout, stderr) = run_args(&["--headless", "--format", "text", "unknown_cmd"], "");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("Error [unknown_subcommand]"));
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
            layer: None,
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
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "function brokenFrame( {".into(),
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
    assert!(stderr.is_empty());
    let err: Value = serde_json::from_str(&stdout).expect("compile error json");
    assert_eq!(err["ok"], false);
    assert_eq!(err["exit_code"], 1);
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

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "validate",
            "--project",
            broken_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);
    assert!(stderr.is_empty());
    let verr: Value = serde_json::from_str(&stdout).unwrap();
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
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "validate",
            "--project",
            strict_warn_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);
    assert!(stderr.is_empty());
    let m_err: Value = serde_json::from_str(&stdout).unwrap();
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
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "trace(\"boot\");".into(),
            },
        });
        sym.timeline.scripts.push(FrameScript {
            layer: None,
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
    let (code, stdout, stderr) = run_args(
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
    assert!(stderr.is_empty());
    let err: Value = serde_json::from_str(&stdout).unwrap();
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
    assert_eq!(res["traces"], json!(["updated LayerFromFile"]));
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

    // 5. File-backed JSFL is bounded before loading the whole source into memory. The VM already
    // rejects this size; the CLI reader must enforce the same boundary while reading it.
    let oversized_script = tmp.path.join("oversized.jsfl");
    let jsfl_limit = neoflash_core::JsflLimits::default().max_source_bytes;
    fs::write(&oversized_script, vec![b' '; jsfl_limit + 1]).unwrap();
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--script",
            oversized_script.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 1);
    assert!(stderr.is_empty());
    let err: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(err["error"]["code"], "jsfl_error");
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
    assert!(stderr.is_empty());
    let err: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(err["error"]["code"], "compile_failed");
    assert_eq!(err["error"]["details"]["step_index"], 2);
    assert_eq!(fs::read_to_string(&out_proj).unwrap(), snapshot_before);

    // A successful in-place batch must atomically replace an existing project on every platform.
    let overwrite_ops = json!([
        { "op": "set_project", "name": "BatchOverwriteOk" }
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
        &overwrite_ops.to_string(),
    );
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("\"ok\":true"));
    let overwritten = Project::from_json(&fs::read_to_string(&out_proj).unwrap()).unwrap();
    assert_eq!(overwritten.name, "BatchOverwriteOk");

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
    assert!(stderr.is_empty());
    let err: Value = serde_json::from_str(&stdout).unwrap();
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
fn batch_rejects_unknown_or_ambiguous_json_fields() {
    let invalid_batches = [
        json!({ "ops": [], "metadata": "silently ignored before T-303" }),
        json!({ "ops": [], "steps": [] }),
        json!([{ "op": "set_project", "widht": 777 }]),
        json!([{
            "op": "add_element",
            "kind": "rectangle",
            "transform": { "xx": 12.0 }
        }]),
        json!([{
            "op": "add_element",
            "kind": "path",
            "commands": [{ "op": "move_to", "x": 0.0, "y": 0.0, "z": 0.0 }]
        }]),
        json!([{
            "op": "apply_command_steps",
            "elements": [],
            "steps": [{ "op": "translate", "dx": 1.0, "dy": 2.0, "dyy": 2.0 }]
        }]),
        json!([{
            "op": "eval",
            "frames": { "start": 0, "end": 1, "extra": true }
        }]),
        json!([{
            "op": "export",
            "target": "svg",
            "out": "ignored.svg",
            "fram": 0
        }]),
    ];

    for invalid in invalid_batches {
        let (code, stdout, stderr) =
            run_args(&["--headless", "batch", "--stdin"], &invalid.to_string());
        assert_eq!(code, 2, "batch={invalid}, stdout={stdout}, stderr={stderr}");
        assert!(stderr.is_empty());
        let error: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(error["error"]["code"], "invalid_batch_json");
    }

    let ambiguous_jsfl = json!([{
        "op": "jsfl",
        "code": "fl.trace('code');",
        "source": "fl.trace('source');"
    }]);
    let (code, stdout, stderr) = run_args(
        &["--headless", "batch", "--stdin"],
        &ambiguous_jsfl.to_string(),
    );
    assert_eq!(code, 2, "stdout={stdout}, stderr={stderr}");
    assert!(stderr.is_empty());
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "invalid_arguments");
}

#[test]
fn batch_held_span_mutations_share_active_keyframe_and_empty_removal_is_noop() {
    let held_span_ops = json!({
        "ops": [
            { "op": "new", "name": "HeldSpan" },
            { "op": "add_keyframe", "layer": "Layer 1", "frame": 4 },
            {
                "op": "add_element",
                "layer": "Layer 1",
                "frame": 0,
                "id": "held_element",
                "name": "Held",
                "kind": "rectangle"
            },
            {
                "op": "set_script",
                "layer": "Layer 1",
                "frame": 2,
                "element": "held_element",
                "event": "click",
                "language": "as2",
                "source": "trace(\"held\");"
            },
            {
                "op": "apply_command_steps",
                "layer": "Layer 1",
                "frame": 2,
                "elements": ["held_element"],
                "steps": [{ "op": "translate", "dx": 12.0, "dy": 7.0 }]
            },
            { "op": "inspect", "frame": 2, "no_resources": true },
            {
                "op": "remove_element",
                "layer": "Layer 1",
                "frame": 3,
                "element": "held_element"
            },
            { "op": "inspect", "frame": 3, "no_resources": true },
            { "op": "validate" }
        ]
    });
    let (code, stdout, stderr) = run_args(
        &["--headless", "batch", "--stdin"],
        &held_span_ops.to_string(),
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let result: Value = serde_json::from_str(&stdout).unwrap();
    let held_keyframe = &result["results"][5]["result"]["symbols"][0]["layers"][0]["keyframes"][0];
    assert_eq!(held_keyframe["frame"], 0);
    let held = &held_keyframe["elements"][0];
    assert_eq!(held["transform"]["x"], 12.0);
    assert_eq!(held["transform"]["y"], 7.0);
    assert_eq!(held["scripts"][0]["event"], "click");
    assert_eq!(held["scripts"][0]["language"], "actionscript2");
    assert_eq!(
        result["results"][7]["result"]["symbols"][0]["layers"][0]["keyframes"][0]["element_count"],
        0
    );

    let empty_removal_ops = json!([
        { "op": "new", "name": "EmptyRemoval" },
        { "op": "inspect", "no_elements": true, "no_resources": true },
        {
            "op": "set_script",
            "frame": 500,
            "source": "",
            "remove_empty": true
        },
        { "op": "inspect", "no_elements": true, "no_resources": true }
    ]);
    let (code, stdout, stderr) = run_args(
        &["--headless", "batch", "--stdin"],
        &empty_removal_ops.to_string(),
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let result: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        result["results"][3]["result"]["symbols"][0]["frame_count"],
        result["results"][1]["result"]["symbols"][0]["frame_count"]
    );
    assert_eq!(
        result["results"][3]["result"]["symbols"][0]["frame_scripts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // A held key stops being active at the layer's exclusive exposure end. Object mutations must
    // not reach back into that hidden key merely because the symbol timeline itself is longer.
    let tmp = TempDirGuard::new("held-span-end");
    let project_path = tmp.path.join("layer-end.nflash");
    let mut project = Project::blank();
    project.format_version = 5;
    project.symbols[0].timeline.frame_count = 6;
    project.symbols[0].timeline.layers[0].properties.end_frame = Some(2);
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "hidden-after-end".into(),
            name: "Hidden after end".into(),
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 30.0,
                fill: [10, 20, 30, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    fs::write(&project_path, project.to_json().unwrap()).unwrap();
    let hidden_mutations = [
        json!({
            "op": "set_script",
            "frame": 3,
            "element": "hidden-after-end",
            "source": "trace('must not attach');"
        }),
        json!({
            "op": "remove_element",
            "frame": 3,
            "element": "hidden-after-end"
        }),
        json!({
            "op": "apply_command_steps",
            "frame": 3,
            "elements": ["hidden-after-end"],
            "steps": [{ "op": "translate", "dx": 10.0, "dy": 20.0 }]
        }),
    ];
    for mutation in hidden_mutations {
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "batch",
                "--stdin",
                "--project",
                project_path.to_str().unwrap(),
            ],
            &json!([mutation]).to_string(),
        );
        assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
        assert!(stderr.is_empty());
        let error: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(error["error"]["code"], "element_not_found");
        assert_eq!(
            Project::from_json(&fs::read_to_string(&project_path).unwrap()).unwrap(),
            project,
            "failed out-of-exposure mutation must not touch the source project"
        );
    }
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
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            svg_proj.to_str().unwrap(),
            "--target",
            "lottie",
            "--html-preview",
            "--out",
            exp_lottie.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(exp_lottie.is_file());
    let lottie_res: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(lottie_res["format"], "json");
    assert!(lottie_res["notes"].is_array());
    assert!(tmp.path.join("out.lottie.preview.html").is_file());

    let exp_dotlottie = tmp.path.join("out_pkg.lottie");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            svg_proj.to_str().unwrap(),
            "--target",
            "dotlottie",
            "--image-format",
            "webp",
            "--html-preview",
            "--out",
            exp_dotlottie.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(exp_dotlottie.is_file());
    let dotlottie_res: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(dotlottie_res["format"], "dotlottie");
    assert!(tmp.path.join("out_pkg.preview.html").is_file());
    neoflash_export::validate_dotlottie_bytes(&fs::read(&exp_dotlottie).unwrap()).unwrap();

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

        let desktop_out = tmp.path.join("desktop");
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "export",
                "--project",
                svg_proj.to_str().unwrap(),
                "--target",
                "desktop",
                "--scale-mode",
                "no_scale",
                "--stage-align",
                "bottom_right",
                "--allow-fullscreen",
                "false",
                "--out",
                desktop_out.to_str().unwrap(),
            ],
            "",
        );
        assert_eq!(code, 0, "stderr: {stderr}");
        let res: Value = serde_json::from_str(&stdout).unwrap();
        let app_dir = PathBuf::from(res["output_path"].as_str().unwrap());
        let bundle_raw = fs::read_to_string(
            app_dir
                .join("Contents")
                .join("Resources")
                .join("web")
                .join("bundle.js"),
        )
        .unwrap();
        let bundle_json = bundle_raw
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .strip_suffix(";\n")
            .unwrap();
        let bundle: Value = serde_json::from_str(bundle_json).unwrap();
        assert_eq!(bundle["player_config"]["scale_mode"], "no_scale");
        assert_eq!(bundle["player_config"]["stage_align"], "bottom_right");
        assert_eq!(bundle["player_config"]["allow_fullscreen"], false);

        // 6. Export HTML5 DOM publishing mode via CLI --render-mode and strict JSON batch ExportSpec (T-307)
        let dom_cli_out = tmp.path.join("html5_dom_cli");
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "export",
                "--project",
                svg_proj.to_str().unwrap(),
                "--target",
                "html5",
                "--render-mode",
                "dom",
                "--out",
                dom_cli_out.to_str().unwrap(),
            ],
            "",
        );
        assert_eq!(code, 0, "stderr: {stderr}");
        let dom_res: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(dom_res["ok"], true);
        assert_eq!(dom_res["render_mode"], "dom");
        let dom_cli_html = fs::read_to_string(dom_cli_out.join("index.html")).unwrap();
        assert!(dom_cli_html.contains("data-render-mode=\"dom\""));
        assert!(
            dom_cli_html.contains("<neoflash-stage id=\"stage-dom-root\" data-render-mode=\"dom\"")
        );

        let dom_batch_out = tmp.path.join("html5_dom_batch");
        let batch_payload = json!([
            {
                "op": "export",
                "target": "html5",
                "out": dom_batch_out.to_str().unwrap(),
                "render_mode": "dom"
            }
        ]);
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "batch",
                "--stdin",
                "--project",
                svg_proj.to_str().unwrap(),
            ],
            &batch_payload.to_string(),
        );
        assert_eq!(code, 0, "stderr: {stderr}");
        let batch_res: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(batch_res["results"][0]["result"]["render_mode"], "dom");
        let dom_batch_html = fs::read_to_string(dom_batch_out.join("index.html")).unwrap();
        assert!(
            dom_batch_html
                .contains("<neoflash-stage id=\"stage-dom-root\" data-render-mode=\"dom\"")
        );

        // Explicit --render-mode canvas overrides a persisted Dom project setting
        let mut dom_persisted =
            Project::from_json(&fs::read_to_string(&svg_proj).unwrap()).unwrap();
        dom_persisted.set_html_render_mode(neoflash_core::HtmlRenderMode::Dom);
        let dom_proj_path = tmp.path.join("dom_persisted.nflash");
        fs::write(&dom_proj_path, dom_persisted.to_json().unwrap()).unwrap();
        let canvas_override_out = tmp.path.join("html5_canvas_override");
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "export",
                "--project",
                dom_proj_path.to_str().unwrap(),
                "--target",
                "html5",
                "--render-mode",
                "canvas",
                "--out",
                canvas_override_out.to_str().unwrap(),
            ],
            "",
        );
        assert_eq!(code, 0, "stderr: {stderr}");
        let override_res: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(override_res["render_mode"], "canvas");
        let canvas_override_html =
            fs::read_to_string(canvas_override_out.join("index.html")).unwrap();
        assert!(!canvas_override_html.contains("data-render-mode=\"dom\""));
        assert!(!canvas_override_html.contains("<neoflash-stage"));

        // Invalid render_mode values and unknown camelCase fields fail closed with exit code 2
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "export",
                "--project",
                svg_proj.to_str().unwrap(),
                "--target",
                "html5",
                "--render-mode",
                "bogus",
                "--out",
                tmp.path.join("ignored_out").to_str().unwrap(),
            ],
            "",
        );
        assert_eq!(code, 2, "stderr: {stderr}");
        assert!(stderr.is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&stdout).unwrap()["error"]["code"],
            "invalid_argument"
        );

        let bad_camel = json!([{
            "op": "export",
            "target": "html5",
            "out": tmp.path.join("ignored_camel").to_str().unwrap(),
            "renderMode": "dom"
        }]);
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "batch",
                "--stdin",
                "--project",
                svg_proj.to_str().unwrap(),
            ],
            &bad_camel.to_string(),
        );
        assert_eq!(code, 2, "stderr: {stderr}");
        assert!(stderr.is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&stdout).unwrap()["error"]["code"],
            "invalid_batch_json"
        );
    }
}

#[test]
fn t315_headless_new_and_inspect_support_project_script_language() {
    let tmp = TempDirGuard::new("t315-script");
    let default_proj = tmp.path.join("default.nflash");
    let (code, stdout, stderr) = run_args(
        &["--headless", "new", "--out", default_proj.to_str().unwrap()],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let created_default: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(created_default["script_language"], "as3");
    assert_eq!(created_default["also_compile_compatible_scripts"], true);

    let ts_proj = tmp.path.join("ts_app.nflash");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "new",
            "--script",
            "ts",
            "--out",
            ts_proj.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let created_ts: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(created_ts["script_language"], "ts");
    assert_eq!(created_ts["also_compile_compatible_scripts"], true);

    let loaded = Project::from_json(&fs::read_to_string(&ts_proj).unwrap()).unwrap();
    assert_eq!(loaded.script_language, Some(ScriptLanguage::TypeScript));
    assert!(loaded.also_compile_compatible_scripts);
    loaded.validate().unwrap();

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "inspect",
            "--project",
            ts_proj.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let inspected: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(inspected["metadata"]["script_language"], "ts");
    assert_eq!(inspected["metadata"]["effective_script_language"], "ts");
    assert_eq!(
        inspected["metadata"]["also_compile_compatible_scripts"],
        true
    );
}

#[test]
fn t311_capabilities_schema_batch_script_language_and_legacy_none_export() {
    let tmp = TempDirGuard::new("t311-headless");

    // 1. Verify `--headless capabilities` exposes `dotlottie`, `--html-preview`, and batch script_language fields.
    let (code, stdout, stderr) = run_args(&["--headless", "capabilities"], "");
    assert_eq!(code, 0, "stderr: {stderr}");
    let caps: Value = serde_json::from_str(&stdout).unwrap();
    let export_targets = caps["export_targets"].as_array().unwrap();
    assert!(export_targets.iter().any(|v| v == "dotlottie"));
    let export_cmd = caps["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "export")
        .expect("export command in capabilities");
    assert!(
        export_cmd["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "--html-preview")
    );
    let ops_schema = &caps["batch_schema"]["operations"];
    for op_name in ["new", "set_project"] {
        let fields = ops_schema[op_name].as_array().unwrap();
        assert!(fields.iter().any(|f| f == "script_language?"));
        assert!(
            fields
                .iter()
                .any(|f| f == "also_compile_compatible_scripts?")
        );
    }

    // 2. Verify `batch` `new` and `set_project` apply `script_language` and `also_compile_compatible_scripts`.
    let batch_proj = tmp.path.join("batch_script.nflash");
    let batch_req = serde_json::json!({
        "ops": [
            {
                "op": "new",
                "name": "Batch AS2",
                "script_language": "as2",
                "also_compile_compatible_scripts": false
            },
            {
                "op": "save",
                "path": batch_proj.to_str().unwrap()
            }
        ]
    });
    let (code, _, stderr) = run_args(&["--headless", "batch", "--stdin"], &batch_req.to_string());
    assert_eq!(code, 0, "stderr: {stderr}");
    let saved_batch = Project::from_json(&fs::read_to_string(&batch_proj).unwrap()).unwrap();
    assert_eq!(
        saved_batch.script_language,
        Some(ScriptLanguage::ActionScript2)
    );
    assert!(!saved_batch.also_compile_compatible_scripts);

    let set_proj_req = serde_json::json!({
        "ops": [
            {
                "op": "set_project",
                "script_language": "ts",
                "also_compile_compatible_scripts": true
            }
        ]
    });
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--project",
            batch_proj.to_str().unwrap(),
            "--in-place",
            "--stdin",
        ],
        &set_proj_req.to_string(),
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let updated_batch = Project::from_json(&fs::read_to_string(&batch_proj).unwrap()).unwrap();
    assert_eq!(
        updated_batch.script_language,
        Some(ScriptLanguage::TypeScript)
    );
    assert!(updated_batch.also_compile_compatible_scripts);

    // 3. Pre-R-102 (`script_language == None`) mixed-script AS2 project exports cleanly across SWF/FLA/XFL/HTML5.
    let mut legacy_as2 = Project::blank();
    legacy_as2.enable_program();
    legacy_as2.script_language = None;
    legacy_as2.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();".into(),
        },
    });
    legacy_as2.sources.push(neoflash_core::SourceUnit {
        id: "foreign-ts".into(),
        path: "src/Ignored.ts".into(),
        language: ScriptLanguage::TypeScript,
        source: "export const ignored: number = 42;".into(),
    });
    assert_eq!(
        legacy_as2.effective_script_language(),
        ScriptLanguage::ActionScript2
    );
    let legacy_path = tmp.path.join("legacy_as2_mixed.nflash");
    fs::write(&legacy_path, legacy_as2.to_json().unwrap()).unwrap();

    let swf_out = tmp.path.join("legacy_as2.swf");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            legacy_path.to_str().unwrap(),
            "--target",
            "swf",
            "--out",
            swf_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "headless AS2 SWF export failed: {stderr}");
    let swf_bytes = fs::read(&swf_out).unwrap();
    assert_eq!(swf_bytes[3], 8);

    let fla_out = tmp.path.join("legacy_as2.fla");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            legacy_path.to_str().unwrap(),
            "--target",
            "fla",
            "--out",
            fla_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "headless legacy FLA export failed: {stderr}");
    assert!(fla_out.is_file());

    // Pre-R-102 (`script_language == None`) `compile` and `validate` must also resolve R-102
    // disposition and skip foreign TypeScript with a warning rather than failing compilation.
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "compile",
            "--project",
            legacy_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "legacy AS2 compile failed: {stderr}");
    let comp_val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(comp_val["effective_script_language"], "as2");
    assert_eq!(comp_val["compiled_scripts_count"], 1);
    assert_eq!(comp_val["skipped_foreign_scripts_count"], 1);

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "validate",
            "--project",
            legacy_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "legacy AS2 validate failed: {stderr}");
    let val_res: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val_res["effective_script_language"], "as2");
    assert_eq!(val_res["valid"], true);
}

#[test]
fn t321_subcommand_help_strict_json_stdout_errors_and_flfile_copy() {
    for sub in [
        "capabilities",
        "schema",
        "new",
        "inspect",
        "validate",
        "compile",
        "eval",
        "jsfl",
        "batch",
        "import",
        "export",
    ] {
        for flag in ["--help", "-h"] {
            let (code, stdout, stderr) = run_args(&["--headless", sub, flag], "");
            assert_eq!(code, 0, "sub={sub} flag={flag} stderr={stderr}");
            assert!(
                stdout.contains("Usage: neoflash-editor --headless"),
                "sub={sub} stdout={stdout}"
            );
            assert!(stderr.is_empty());
        }
        let (code, stdout, stderr) = run_args(&["--headless", "help", sub], "");
        assert_eq!(code, 0, "help {sub} stderr={stderr}");
        assert!(stdout.contains("Usage: neoflash-editor --headless"));
        assert!(stderr.is_empty());
    }

    // Unknown subcommand with --help or `help bogus` exits 2 with valid JSON on stdout.
    for args in [
        &["--headless", "bogus_cmd", "--help"][..],
        &["--headless", "help", "bogus_cmd"][..],
        &["--headless", "--format=json", "--bogus-global"][..],
        &["--headless", "new", "--version"][..],
    ] {
        let (code, stdout, stderr) = run_args(args, "");
        assert_eq!(code, 2, "args={args:?} stdout={stdout} stderr={stderr}");
        assert!(
            stderr.is_empty(),
            "expected empty stderr for {args:?}: {stderr}"
        );
        let err: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("invalid JSON stdout for {args:?}: {e}: {stdout}"));
        assert_eq!(err["ok"], false);
        assert_eq!(err["exit_code"], 2);
    }

    // Explicit `--format text` writes error to stderr and leaves stdout empty.
    let (code, stdout, stderr) = run_args(&["--headless", "--format=text", "--bogus-global"], "");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("Error [unknown_flag]"));

    // FLfile.copy + fresh --flfile-root creation + copy-on-folder false + text trace capture.
    let tmp = TempDirGuard::new("t321-flfile-copy");
    let proj_path = tmp.path.join("doc.nflash");
    let root_dir = tmp.path.join("brand_new_flroot");
    assert!(!root_dir.exists());
    fs::write(&proj_path, Project::blank().to_json().unwrap()).unwrap();

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--flfile-root",
            root_dir.to_str().unwrap(),
            "--eval",
            r#"
                FLfile.write("file:///src.txt", "copied_payload");
                FLfile.createFolder("file:///sub");
                var ok1 = FLfile.copy("file:///src.txt", "file:///dst.txt");
                var ok2 = FLfile.copy("file:///src.txt", "file:///dst.txt");
                var okDir = FLfile.copy("file:///sub", "file:///sub_copy");
                fl.trace("copy1=" + ok1 + ",copy2=" + ok2 + ",dir=" + okDir + ",val=" + FLfile.read("file:///dst.txt"));
                ok1 && !ok2 && !okDir;
            "#,
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr} stdout={stdout}");
    assert!(root_dir.is_dir());
    let jsfl_json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(jsfl_json["return_value"], true);
    assert_eq!(
        jsfl_json["traces"],
        json!(["copy1=true,copy2=false,dir=false,val=copied_payload"])
    );
    assert_eq!(
        fs::read_to_string(root_dir.join("dst.txt")).unwrap(),
        "copied_payload"
    );
    assert!(!root_dir.join("sub_copy").exists());

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "--format",
            "text",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--eval",
            "fl.trace('hello_text_trace'); 42;",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("hello_text_trace\n"), "stdout={stdout}");

    // Invalid `--folder` paths and invalid export bounds return exit code 2.
    for bad_folder in ["../escape", "a\\b", "a//b", ".", "Icons/.."] {
        let (code, stdout, _) = run_args(
            &[
                "--headless",
                "import",
                "--input",
                proj_path.to_str().unwrap(),
                "--folder",
                bad_folder,
                "--out",
                tmp.path.join("never.nflash").to_str().unwrap(),
            ],
            "",
        );
        assert_eq!(
            code, 2,
            "expected exit 2 for --folder {bad_folder:?}: {stdout}"
        );
        let err: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(err["error"]["code"], "invalid_argument");
    }

    for bad_export in [
        vec![
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "svg",
            "--width",
            "0",
            "--out",
            tmp.path.join("bad_w.svg").to_str().unwrap(),
        ],
        vec![
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "png-sequence",
            "--frame",
            "5",
            "--end-frame",
            "2",
            "--out",
            tmp.path.join("bad_seq").to_str().unwrap(),
        ],
    ] {
        let (code, stdout, _) = run_args(&bad_export, "");
        assert_eq!(code, 2, "expected exit 2 for {bad_export:?}: {stdout}");
        let err: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(err["error"]["code"], "invalid_argument");
    }

    // Capabilities schema advertises boolean `--assets-folder` & `--sprite-sheets` (not `--assets-folder <dir>`).
    let (code, stdout, _) = run_args(&["--headless", "capabilities"], "");
    assert_eq!(code, 0);
    let caps: Value = serde_json::from_str(&stdout).unwrap();
    let export_flags = caps["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "export")
        .unwrap()["flags"]
        .as_array()
        .unwrap();
    assert!(export_flags.contains(&json!("--assets-folder")));
    assert!(export_flags.contains(&json!("--sprite-sheets")));
    assert!(!export_flags.contains(&json!("--assets-folder <dir>")));

    // v1 blank `batch` (no `--project`): `add_symbol` with `folder` (v4) + `add_element` with `skew` (v5)
    // + frame `set_script` + mid-batch `validate` upgrades `format_version` cleanly.
    let v1_batch_out = tmp.path.join("v1_batch_upgraded.nflash");
    let v1_ops = json!([
        { "op": "add_symbol", "name": "CardSym", "kind": "movie_clip", "folder": "/UI/Cards/" },
        { "op": "add_element", "kind": "rect", "x": 10.0, "y": 10.0, "width": 60.0, "height": 40.0, "skew": 12.5 },
        { "op": "set_script", "frame": 0, "language": "as2", "source": "trace('v1_batch_ok');" },
        { "op": "validate" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            v1_batch_out.to_str().unwrap(),
        ],
        &serde_json::to_string(&v1_ops).unwrap(),
    );
    assert_eq!(code, 0, "stderr={stderr} stdout={stdout}");
    let saved_batch = Project::from_json(&fs::read_to_string(&v1_batch_out).unwrap()).unwrap();
    assert_eq!(saved_batch.format_version, 5);
    assert!(saved_batch.library_folders.iter().any(|f| f == "UI/Cards"));
    assert!(
        saved_batch
            .symbols
            .iter()
            .any(|s| s.name == "CardSym" && s.folder == "UI/Cards")
    );

    // `add_symbol` rejects `as_scene: true` with non-empty `folder` and invalid `folder` with exit code 2.
    for bad_ops in [
        json!([{ "op": "add_symbol", "name": "Scene 2", "as_scene": true, "folder": "Scenes" }]),
        json!([{ "op": "add_symbol", "name": "BadSym", "folder": "../escape" }]),
    ] {
        let (code, stdout, _) = run_args(
            &["--headless", "batch", "--stdin"],
            &serde_json::to_string(&bad_ops).unwrap(),
        );
        assert_eq!(code, 2, "expected exit 2 for {bad_ops}: {stdout}");
        let err: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(err["error"]["code"], "invalid_argument");
    }

    // `jsfl` adding a library folder on a v1 `Project::blank()` upgrades to v4 before validation.
    let v1_jsfl_proj = tmp.path.join("v1_blank_for_jsfl.nflash");
    let blank_v1 = Project::blank();
    assert_eq!(blank_v1.format_version, 1);
    fs::write(&v1_jsfl_proj, blank_v1.to_json().unwrap()).unwrap();
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            v1_jsfl_proj.to_str().unwrap(),
            "--in-place",
            "--eval",
            "fl.getDocumentDOM().library.newFolder('Icons');",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr} stdout={stdout}");
    let upgraded_jsfl = Project::from_json(&fs::read_to_string(&v1_jsfl_proj).unwrap()).unwrap();
    assert_eq!(upgraded_jsfl.format_version, 4);
    assert!(upgraded_jsfl.library_folders.iter().any(|f| f == "Icons"));
}

#[test]
fn headless_export_html5_symbol_isolates_requested_scene_or_symbol() {
    if import_export::resolve_web_runtime(None).is_err() {
        return;
    }
    let tmp = TempDirGuard::new("html5_symbol_isolate");
    let mut proj = Project::blank();
    proj.format_version = 6;
    proj.document_class = Some("app.Main".into());
    let scene1_id = proj.root.clone();
    proj.symbols[0].name = "Scene 1".into();
    proj.symbols[0].timeline.frame_count = 12;

    let scene2_id = new_id("scene");
    proj.symbols.push(neoflash_core::Symbol {
        id: scene2_id.clone(),
        name: "Scene 2".into(),
        kind: neoflash_core::SymbolKind::MovieClip,
        folder: String::new(),
        linkage_id: None,
        class_name: None,
        timeline: neoflash_core::Timeline {
            frame_count: 6,
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Keyframe::default()
                }],
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    proj.scenes = vec![scene1_id, scene2_id.clone()];

    let badge_id = new_id("sym");
    proj.symbols.push(neoflash_core::Symbol {
        id: badge_id,
        name: "Badge".into(),
        kind: neoflash_core::SymbolKind::Graphic,
        folder: "UI".into(),
        linkage_id: None,
        class_name: Some("ui.Badge".into()),
        timeline: neoflash_core::Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Keyframe::default()
                }],
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    let button_id = new_id("sym");
    proj.symbols.push(neoflash_core::Symbol {
        id: button_id.clone(),
        name: "Action Button".into(),
        kind: neoflash_core::SymbolKind::Button,
        folder: "UI".into(),
        linkage_id: None,
        class_name: Some("ui.ActionButton".into()),
        timeline: neoflash_core::Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Keyframe::default()
                }],
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    proj.library_folders.push("UI".into());
    proj.script_language = Some(ScriptLanguage::ActionScript3);
    proj.sources = vec![
        neoflash_core::SourceUnit {
            id: new_id("source"),
            path: "app/Main.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package app {
                import flash.display.MovieClip;
                public dynamic class Main extends MovieClip {}
            }"#
            .into(),
        },
        neoflash_core::SourceUnit {
            id: new_id("source"),
            path: "ui/Badge.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package ui {
                import flash.display.MovieClip;
                public dynamic class Badge extends MovieClip {}
            }"#
            .into(),
        },
        neoflash_core::SourceUnit {
            id: new_id("source"),
            path: "ui/ActionButton.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package ui {
                import flash.display.SimpleButton;
                public dynamic class ActionButton extends SimpleButton {}
            }"#
            .into(),
        },
    ];
    proj.validate().unwrap();

    let proj_path = tmp.path.join("multi_scene.nflash");
    fs::write(&proj_path, proj.to_json().unwrap()).unwrap();

    let parse_bundle = |dir: &std::path::Path| -> Value {
        let raw = fs::read_to_string(dir.join("bundle.js")).unwrap();
        let json_str = raw
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .trim_end()
            .strip_suffix(';')
            .unwrap();
        serde_json::from_str(json_str).unwrap()
    };

    // Full export (no --symbol) flattens Scene 1 (12f) + Scene 2 (6f) = 18 frames.
    let full_out = tmp.path.join("out_full");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "html5",
            "--out",
            full_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let full_bundle = parse_bundle(&full_out);
    assert_eq!(full_bundle["load_summary"]["frames_count"], 18);
    assert_eq!(full_bundle["scenes"].as_array().unwrap().len(), 2);
    assert_eq!(full_bundle["project"]["document_class"], "app.Main");

    // Scoped export (--symbol "Scene 2") exports ONLY Scene 2 (6 frames) while keeping library symbols.
    let scene2_out = tmp.path.join("out_scene2");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "html5",
            "--symbol",
            "Scene 2",
            "--out",
            scene2_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let scene2_bundle = parse_bundle(&scene2_out);
    assert_eq!(scene2_bundle["load_summary"]["frames_count"], 6);
    assert_eq!(scene2_bundle["project"]["root"], scene2_id);
    assert!(
        scene2_bundle["project"]["document_class"].is_null(),
        "isolating an unbound secondary scene must not run the original document class"
    );
    assert!(
        scene2_bundle["project"]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "Badge"),
        "library symbols must stay available for Scene 2 instances"
    );
    assert!(
        !scene2_bundle["project"]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "Scene 1"),
        "other scenes must not remain in isolated --symbol HTML5 export"
    );

    // Scoped export (--symbol "Badge") promotes Graphic symbol to 4-frame isolated MovieClip root.
    let badge_out = tmp.path.join("out_badge");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "html5",
            "--symbol",
            "Badge",
            "--out",
            badge_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let badge_bundle = parse_bundle(&badge_out);
    assert_eq!(badge_bundle["load_summary"]["frames_count"], 4);
    assert_eq!(badge_bundle["project"]["document_class"], "ui.Badge");
    assert_ne!(badge_bundle["project"]["document_class"], "app.Main");

    // The explicit desktop and native-banner targets share the same isolated web root contract.
    let desktop_out = tmp.path.join("out_badge_desktop");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "desktop",
            "--symbol",
            "Badge",
            "--out",
            desktop_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr} stdout={stdout}");
    let desktop_report: Value = serde_json::from_str(&stdout).unwrap();
    let desktop_app = std::path::PathBuf::from(
        desktop_report["output_path"]
            .as_str()
            .expect("desktop output_path"),
    );
    let desktop_bundle = parse_bundle(&desktop_app.join("Contents/Resources/web"));
    assert_eq!(desktop_bundle["load_summary"]["frames_count"], 4);
    assert_eq!(
        desktop_bundle["project"]["root"],
        badge_bundle["project"]["root"]
    );
    assert_eq!(desktop_bundle["project"]["document_class"], "ui.Badge");

    // A Button class extends SimpleButton and cannot become the MovieClip document root. Keep the
    // bound Button intact and place one instance in a deterministic one-frame MovieClip wrapper.
    let button_out = tmp.path.join("out_button");
    let (code, _, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "html5",
            "--symbol",
            "Action Button",
            "--out",
            button_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let button_bundle = parse_bundle(&button_out);
    assert_eq!(button_bundle["load_summary"]["frames_count"], 1);
    assert!(button_bundle["project"]["document_class"].is_null());
    let button_symbols = button_bundle["project"]["symbols"].as_array().unwrap();
    let preserved_button = button_symbols
        .iter()
        .find(|symbol| symbol["id"] == button_id)
        .expect("isolated Button symbol");
    assert_eq!(preserved_button["kind"], "button");
    assert_eq!(preserved_button["class_name"], "ui.ActionButton");
    let wrapper_id = button_bundle["project"]["root"].as_str().unwrap();
    let wrapper = button_symbols
        .iter()
        .find(|symbol| symbol["id"] == wrapper_id)
        .expect("Button export wrapper");
    assert_eq!(wrapper["kind"], "movie_clip");
    assert_eq!(
        wrapper["timeline"]["layers"][0]["keyframes"][0]["elements"][0]["kind"]["symbol_id"],
        button_id
    );

    // Native banner export deliberately rejects executable sources, so exercise its symbol route
    // with the same timelines after removing class bindings and their source units.
    let mut banner_project = proj.clone();
    banner_project.document_class = None;
    banner_project.sources.clear();
    for symbol in &mut banner_project.symbols {
        symbol.class_name = None;
    }
    banner_project.validate().unwrap();
    let banner_project_path = tmp.path.join("multi_scene_banner.nflash");
    fs::write(&banner_project_path, banner_project.to_json().unwrap()).unwrap();
    let banner_out = tmp.path.join("badge-banner.html");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            banner_project_path.to_str().unwrap(),
            "--target",
            "banner",
            "--symbol",
            "Badge",
            "--out",
            banner_out.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr} stdout={stdout}");
    let banner_report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(banner_report["frame_count"], 4);
    assert!(banner_out.is_file());

    // Unknown --symbol fails closed instead of silently exporting the whole document.
    let bad_out = tmp.path.join("out_missing");
    let (code, stdout, _) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            proj_path.to_str().unwrap(),
            "--target",
            "html5",
            "--symbol",
            "MissingScene",
            "--out",
            bad_out.to_str().unwrap(),
        ],
        "",
    );
    assert_ne!(code, 0, "unknown --symbol must fail closed: {stdout}");
}

#[test]
fn headless_and_editor_import_font_files_into_library_and_embedding_metadata() {
    let tmp = TempDirGuard::new("font_import");
    let src_ttf = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../neoflash-ui/assets/fonts/Inter-Regular.ttf");
    let ttf_input = tmp.path.join("Inter-Regular.ttf");
    fs::copy(&src_ttf, &ttf_input).unwrap();

    let proj_path = tmp.path.join("fonts_proj.nflash");
    fs::write(&proj_path, Project::blank().to_json().unwrap()).unwrap();

    // 1. Headless CLI import of a real TrueType/OpenType (.ttf) font.
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "import",
            "--input",
            ttf_input.to_str().unwrap(),
            "--into",
            proj_path.to_str().unwrap(),
            "--library-only",
            "--in-place",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr} stdout={stdout}");
    let res_json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(res_json["ok"], true);

    let read_embedding = |proj: &Project| -> Vec<crate::panels::font_embedding::EmbeddedFontEntry> {
        proj.resources
            .iter()
            .find(|r| r.path.eq_ignore_ascii_case("fonts/embedding.json"))
            .and_then(|r| r.decoded_bytes().ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    };

    let proj_after_ttf = Project::from_json(&fs::read_to_string(&proj_path).unwrap()).unwrap();
    proj_after_ttf.validate().unwrap();
    let ttf_res = proj_after_ttf
        .resources
        .iter()
        .find(|r| r.path == "fonts/Inter-Regular.ttf")
        .expect("fonts/Inter-Regular.ttf resource");
    assert_eq!(ttf_res.media_type, "font/ttf");
    let embedded = read_embedding(&proj_after_ttf);
    assert!(
        embedded.iter().any(|f| {
            f.id == ttf_res.id && f.family == "Inter" && f.style.eq_ignore_ascii_case("Regular")
        }),
        "expected Inter Regular in fonts/embedding.json, got: {embedded:?}"
    );

    // 2. Headless CLI import of a WOFF2 font (.woff2) uses filename stem/style fallback cleanly.
    let woff2_input = tmp.path.join("BrandDisplay-Bold.woff2");
    let mut woff2_bytes = b"wOF2".to_vec();
    woff2_bytes.extend_from_slice(&[0u8; 60]);
    fs::write(&woff2_input, &woff2_bytes).unwrap();
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "import",
            "--input",
            woff2_input.to_str().unwrap(),
            "--into",
            proj_path.to_str().unwrap(),
            "--library-only",
            "--in-place",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr={stderr} stdout={stdout}");
    let proj_after_woff2 = Project::from_json(&fs::read_to_string(&proj_path).unwrap()).unwrap();
    proj_after_woff2.validate().unwrap();
    let woff2_res = proj_after_woff2
        .resources
        .iter()
        .find(|r| r.path == "fonts/BrandDisplay-Bold.woff2")
        .expect("fonts/BrandDisplay-Bold.woff2 resource");
    assert_eq!(woff2_res.media_type, "font/woff2");
    assert!(
        read_embedding(&proj_after_woff2)
            .iter()
            .any(|f| { f.id == woff2_res.id && f.family == "BrandDisplay" && f.style == "Bold" }),
        "expected BrandDisplay Bold metadata for WOFF2 import"
    );

    // 3. Corrupt font bytes with .ttf extension are rejected without corrupting project.
    let bad_ttf = tmp.path.join("corrupt.ttf");
    fs::write(&bad_ttf, b"NOT_A_FONT_HEADER").unwrap();
    let (code, stdout, _) = run_args(
        &[
            "--headless",
            "import",
            "--input",
            bad_ttf.to_str().unwrap(),
            "--into",
            proj_path.to_str().unwrap(),
            "--library-only",
            "--in-place",
        ],
        "",
    );
    assert_ne!(code, 0, "corrupt font bytes must fail closed: {stdout}");

    // 4. Authoring Editor::import_graphics_to_library_paths supports font files with full Undo.
    let ctx = eframe::egui::Context::default();
    let mut editor = crate::app::Editor::from_project(&ctx, Project::blank());
    editor
        .import_graphics_to_library_paths(std::slice::from_ref(&ttf_input))
        .unwrap();
    editor.project.validate().unwrap();
    assert!(
        editor
            .project
            .resources
            .iter()
            .any(|r| r.path == "fonts/Inter-Regular.ttf" && r.media_type == "font/ttf")
    );
    assert!(
        read_embedding(&editor.project)
            .iter()
            .any(|f| f.family == "Inter")
    );
    editor.undo();
    editor.project.validate().unwrap();
    assert!(
        !editor
            .project
            .resources
            .iter()
            .any(|r| r.path == "fonts/Inter-Regular.ttf"),
        "Undo must cleanly remove imported font resource and embedding metadata"
    );

    // 5. Adding same-named font files from different folders reserves paths across the batch.
    let first_dir = tmp.path.join("first");
    let second_dir = tmp.path.join("second");
    fs::create_dir_all(&first_dir).unwrap();
    fs::create_dir_all(&second_dir).unwrap();
    let first_font = first_dir.join("Inter-Regular.ttf");
    let second_font = second_dir.join("Inter-Regular.ttf");
    fs::copy(&src_ttf, &first_font).unwrap();
    fs::copy(&src_ttf, &second_font).unwrap();
    let mut editor = crate::app::Editor::from_project(&ctx, Project::blank());
    editor
        .import_resource_paths(&[first_font, second_font])
        .unwrap();
    editor.project.validate().unwrap();
    let imported_paths: std::collections::BTreeSet<_> = editor
        .project
        .resources
        .iter()
        .filter(|resource| resource.media_type == "font/ttf")
        .map(|resource| resource.path.as_str())
        .collect();
    assert_eq!(
        imported_paths,
        ["fonts/Inter-Regular-2.ttf", "fonts/Inter-Regular.ttf"]
            .into_iter()
            .collect(),
        "one Add resource files operation must keep both same-named fonts"
    );
    assert_eq!(read_embedding(&editor.project).len(), 2);

    // 6. A corrupt existing embedding registry blocks an import without overwriting user data.
    let invalid_metadata = ResourceUnit::from_bytes(
        "font_metadata",
        "fonts/embedding.json",
        "application/json",
        b"not valid json",
    )
    .unwrap();
    let mut invalid_project = Project::blank();
    invalid_project.format_version = invalid_project.format_version.max(3);
    invalid_project.resources.push(invalid_metadata);
    invalid_project.validate().unwrap();
    let before = invalid_project.to_json().unwrap();
    let mut editor = crate::app::Editor::from_project(&ctx, invalid_project);
    let error = editor
        .import_resource_paths(std::slice::from_ref(&ttf_input))
        .expect_err("invalid font embedding metadata must fail closed");
    assert!(error.contains("existing metadata is invalid"), "{error}");
    assert_eq!(
        editor.project.to_json().unwrap(),
        before,
        "failed font import must not replace malformed existing metadata"
    );
}
