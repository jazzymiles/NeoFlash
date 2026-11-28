//! Unit and end-to-end tests for the NeoFlash Editor headless CLI (`T-302`).
use super::*;
use neoflash_core::{
    FrameSound, Keyframe, Layer, LayerProperties, ObjectEvent, ObjectScript, ResourceUnit, Script,
    TimelineVideoSpan, new_id,
};
use neoflash_import::export_act_palette;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
enum TreeEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

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

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, TreeEntry> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, TreeEntry>) {
        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let metadata = fs::symlink_metadata(path).unwrap();
        if metadata.file_type().is_symlink() {
            out.insert(relative, TreeEntry::Symlink(fs::read_link(path).unwrap()));
        } else if metadata.is_dir() {
            out.insert(relative, TreeEntry::Directory);
            let mut children = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                visit(root, &child, out);
            }
        } else {
            out.insert(relative, TreeEntry::File(fs::read(path).unwrap()));
        }
    }

    let mut snapshot = BTreeMap::new();
    if root.exists() {
        visit(root, root, &mut snapshot);
    }
    snapshot
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
            .contains("share one rollback journal")
    );
    assert!(caps["batch_schema"]["operations"]["add_element"].is_array());
    assert!(caps["batch_schema"]["operations"]["import"].is_array());
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

    // 4. Runtime error rolls back both the project and every granted FLfile effect.
    fs::write(allowed_dir.join("existing.txt"), "original").unwrap();
    fs::write(allowed_dir.join("delete-me.txt"), "keep").unwrap();
    let files_before_failure = snapshot_tree(&allowed_dir);
    let (code, _, _) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_path.to_str().unwrap(),
            "--stdin",
            "--in-place",
            "--flfile-root",
            allowed_dir.to_str().unwrap(),
            "--allow-delete",
        ],
        r#"
            FLfile.write("file:///existing.txt", "changed");
            FLfile.write("file:///existing.txt", "+appended", "append");
            FLfile.write("file:///created.txt", "created");
            FLfile.createFolder("file:///created-dir");
            FLfile.remove("file:///delete-me.txt");
            fl.getDocumentDOM().width = 333;
            throw "boom";
        "#,
    );
    assert_eq!(code, 1);
    assert_eq!(fs::read_to_string(&proj_path).unwrap(), orig_json);
    assert_eq!(snapshot_tree(&allowed_dir), files_before_failure);

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
fn batch_rolls_back_save_multi_output_export_and_flfile_after_late_failure() {
    let tmp = TempDirGuard::new("batch-effects-rollback");
    let project_path = tmp.path.join("source.nflash");
    let explicit_save = tmp.path.join("explicit-save.nflash");
    let aep_path = tmp.path.join("package.aep");
    let aepx_path = aep_path.with_extension("aepx");
    let jsx_path = aep_path.with_extension("jsx");
    let footage_dir = tmp.path.join("package_Footage");
    let flfile_root = tmp.path.join("flfile");

    let media_dir = tmp.path.join("media");
    fs::create_dir_all(&media_dir).unwrap();
    fs::write(media_dir.join("clip.mp4"), b"external-video").unwrap();
    let mut project = Project::blank();
    project.resources.push(
        ResourceUnit::from_bytes(
            "movie",
            "media/reference.mp4",
            "video/mp4",
            b"resource-placeholder",
        )
        .unwrap(),
    );
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0].video =
        Some(TimelineVideoSpan::external("movie", "media/clip.mp4"));
    upgrade_project_format_versions(&mut project);
    let project_bytes = project.to_json().unwrap().into_bytes();
    fs::write(&project_path, &project_bytes).unwrap();
    fs::write(&explicit_save, b"old explicit save").unwrap();
    fs::write(&aep_path, b"old aep").unwrap();
    fs::write(&aepx_path, b"old aepx").unwrap();
    fs::write(&jsx_path, b"old jsx").unwrap();
    fs::create_dir_all(&footage_dir).unwrap();
    fs::write(footage_dir.join("old.bin"), b"old footage").unwrap();
    fs::create_dir_all(&flfile_root).unwrap();
    fs::write(flfile_root.join("existing.txt"), b"original").unwrap();
    fs::write(flfile_root.join("delete-me.txt"), b"keep").unwrap();

    let save_before = fs::read(&explicit_save).unwrap();
    let aep_before = snapshot_tree(&tmp.path);
    let flfile_before = snapshot_tree(&flfile_root);
    let ops = json!([
        { "op": "set_project", "name": "Effect candidate" },
        { "op": "save", "path": explicit_save },
        { "op": "export", "target": "aep", "out": jsx_path },
        {
            "op": "jsfl",
            "flfile_root": tmp.path,
            "code": "if (FLfile.read('file:///package_Footage/clip.mp4') != 'external-video') { throw 'external footage missing'; }"
        },
        {
            "op": "jsfl",
            "flfile_root": flfile_root,
            "allow_delete": true,
            "code": r#"
                FLfile.write("file:///existing.txt", "changed");
                FLfile.write("file:///existing.txt", "+appended", "append");
                FLfile.write("file:///created.txt", "created");
                FLfile.copy("file:///created.txt", "file:///copied.txt");
                FLfile.createFolder("file:///created-dir");
                FLfile.remove("file:///delete-me.txt");
            "#
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    assert!(stderr.is_empty());
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        error["error"]["code"], "compile_failed",
        "unexpected batch error: {stdout}"
    );
    assert_eq!(error["error"]["details"]["step_index"], 6);
    assert_eq!(fs::read(&project_path).unwrap(), project_bytes);
    assert_eq!(fs::read(&explicit_save).unwrap(), save_before);
    assert_eq!(snapshot_tree(&flfile_root), flfile_before);
    assert_eq!(snapshot_tree(&tmp.path), aep_before);
}

#[test]
fn t434_publish_named_criteo_profile_and_restore_archive_after_failed_batch() {
    let tmp = TempDirGuard::new("criteo-batch");
    let project_path = tmp.path.join("campaign.nflash");
    let mut project = Project::blank();
    project.name = "Night / Sky".into();
    let profile = neoflash_export::PublishProfile {
        id: "campaign-zip".into(),
        name: "Campaign ZIP".into(),
        criteo_max_zip_kib: Some(25),
        ..Default::default()
    };
    neoflash_export::store_profiles_in_project(&mut project, &[profile], Some("campaign-zip"))
        .unwrap();
    fs::write(&project_path, project.to_json().unwrap()).unwrap();
    let output = tmp.path.join("ad");
    let archive = output.join(crate::output_paths::criteo_archive_name(&project));
    let (code, stdout, _) = run_args(
        &[
            "--headless",
            "export",
            "--project",
            project_path.to_str().unwrap(),
            "--target",
            "html5",
            "--out",
            output.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "{stdout}");
    let result: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["profile"], "Campaign ZIP");
    assert_eq!(result["zip_bytes"], fs::metadata(&archive).unwrap().len());
    fs::write(&archive, b"previous creative").unwrap();
    let ops = json!([
        { "op": "export", "target": "html5", "profile": "Criteo", "out": output },
        { "op": "export", "target": "html5", "profile": "Criteo", "out": output, "banner_max_zip": 1 }
    ]);
    let (code, stdout, _) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(code, 1, "{stdout}");
    let failure: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(failure["error"]["details"]["step_index"], 1, "{stdout}");
    assert_eq!(fs::read(archive).unwrap(), b"previous creative");
}

#[test]
fn successful_mixed_batch_commits_read_your_writes_and_step_snapshot_once() {
    let tmp = TempDirGuard::new("batch-effects-success");
    let mid_save = tmp.path.join("mid.nflash");
    let final_save = tmp.path.join("final.nflash");
    let flfile_root = tmp.path.join("flfile");
    fs::create_dir_all(&flfile_root).unwrap();

    let ops = json!([
        { "op": "new", "name": "Initial" },
        { "op": "set_project", "name": "Saved midpoint" },
        { "op": "save", "path": mid_save },
        {
            "op": "jsfl",
            "flfile_root": flfile_root,
            "code": "FLfile.write('file:///first.txt', 'alpha');"
        },
        {
            "op": "jsfl",
            "flfile_root": flfile_root,
            "code": "var v = FLfile.read('file:///first.txt'); FLfile.write('file:///first.txt', '-beta', 'append'); FLfile.copy('file:///first.txt', 'file:///copy.txt'); fl.trace(v);"
        },
        { "op": "set_project", "name": "Final state" },
        { "op": "validate" }
    ]);

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            final_save.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(code, 0, "stdout={stdout}, stderr={stderr}");
    assert!(stderr.is_empty());
    let response: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["results"][4]["result"]["traces"], json!(["alpha"]));
    assert_eq!(
        Project::from_json(&fs::read_to_string(&mid_save).unwrap())
            .unwrap()
            .name,
        "Saved midpoint"
    );
    assert_eq!(
        Project::from_json(&fs::read_to_string(&final_save).unwrap())
            .unwrap()
            .name,
        "Final state"
    );
    assert_eq!(
        fs::read_to_string(flfile_root.join("first.txt")).unwrap(),
        "alpha-beta"
    );
    assert_eq!(
        fs::read_to_string(flfile_root.join("copy.txt")).unwrap(),
        "alpha-beta"
    );
}

#[test]
fn batch_import_mutates_only_the_candidate_and_participates_in_late_rollback() {
    let tmp = TempDirGuard::new("batch-import-rollback");
    let svg = tmp.path.join("asset.svg");
    let saved = tmp.path.join("imported.nflash");
    fs::write(
        &svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="24"><rect width="40" height="24" fill="#3366cc"/></svg>"##,
    )
    .unwrap();
    fs::write(&saved, b"original-project-placeholder").unwrap();
    let baseline = snapshot_tree(&tmp.path);

    let failing = json!([
        { "op": "import", "input": svg, "folder": "Imported/Batch" },
        { "op": "inspect" },
        { "op": "save", "path": saved },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) =
        run_args(&["--headless", "batch", "--stdin"], &failing.to_string());
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    assert!(stderr.is_empty());
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 4);
    assert_eq!(snapshot_tree(&tmp.path), baseline);

    let successful = json!([
        { "op": "import", "inputs": [svg], "folder": "Imported/Batch" },
        { "op": "inspect" }
    ]);
    let (code, stdout, stderr) =
        run_args(&["--headless", "batch", "--stdin"], &successful.to_string());
    assert_eq!(code, 0, "stdout={stdout}, stderr={stderr}");
    let response: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(response["results"][0]["op"], "import");
    assert!(
        response["results"][0]["result"]["project"]["symbols"]
            .as_u64()
            .is_some_and(|count| count > 1)
    );
}

#[test]
fn batch_rolls_back_the_resolved_image_path_when_extension_and_format_differ() {
    let tmp = TempDirGuard::new("batch-image-effect-path");
    let project_path = tmp.path.join("source.nflash");
    let requested = tmp.path.join("render.bmp");
    let actual = tmp.path.join("render.png");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();
    fs::write(&actual, b"original png").unwrap();

    let ops = json!([
        {
            "op": "export",
            "target": "image",
            "out": requested,
            "image_format": "png"
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 2);
    assert_eq!(fs::read(&actual).unwrap(), b"original png");
    assert!(!requested.exists());
}

#[cfg(unix)]
#[test]
fn sequence_rollback_preserves_unrelated_sibling_and_stable_lock_inodes() {
    use std::os::unix::fs::MetadataExt;

    let tmp = TempDirGuard::new("batch-sequence-scope");
    let project_path = tmp.path.join("source.nflash");
    let sequence_path = tmp.path.join("frames.png");
    let unrelated = tmp.path.join("unrelated.bin");
    let lock = tmp.path.join(".neoflash-sequences.lock");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();
    fs::write(&unrelated, b"must keep inode").unwrap();

    let export = json!([{
        "op": "export",
        "target": "png-sequence",
        "out": sequence_path,
        "frame": 0,
        "end_frame": 0
    }]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &export.to_string(),
    );
    assert_eq!(code, 0, "stdout={stdout}, stderr={stderr}");
    let unrelated_inode = fs::metadata(&unrelated).unwrap().ino();
    let lock_inode = fs::metadata(&lock).unwrap().ino();
    let sequence_before = fs::read(&sequence_path).unwrap();
    let tree_before = snapshot_tree(&tmp.path);

    let failing = json!([
        { "op": "set_project", "bg": "#A04020" },
        {
            "op": "export",
            "target": "png-sequence",
            "out": sequence_path,
            "frame": 0,
            "end_frame": 0
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &failing.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 3);
    assert_eq!(fs::read(&sequence_path).unwrap(), sequence_before);
    assert_eq!(fs::read(&unrelated).unwrap(), b"must keep inode");
    assert_eq!(fs::metadata(&unrelated).unwrap().ino(), unrelated_inode);
    assert_eq!(fs::metadata(&lock).unwrap().ino(), lock_inode);
    assert_eq!(snapshot_tree(&tmp.path), tree_before);
}

#[test]
fn repeated_sequence_exports_restore_the_original_absent_family_and_keep_stable_lock() {
    let tmp = TempDirGuard::new("batch-repeated-sequence-rollback");
    let project_path = tmp.path.join("source.nflash");
    let sequence_path = tmp.path.join("frames.png");
    let unrelated = tmp.path.join("unrelated.bin");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();
    fs::write(&unrelated, b"unrelated").unwrap();
    let tree_before = snapshot_tree(&tmp.path);

    let failing = json!([
        {
            "op": "export",
            "target": "png-sequence",
            "out": sequence_path
        },
        { "op": "set_project", "bg": "#2050A0" },
        {
            "op": "export",
            "target": "png-sequence",
            "out": sequence_path
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &failing.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 4);
    let mut expected = tree_before;
    expected.insert(
        PathBuf::from(".neoflash-sequences.lock"),
        TreeEntry::File(Vec::new()),
    );
    assert_eq!(snapshot_tree(&tmp.path), expected);
    assert!(tmp.path.join(".neoflash-sequences.lock").is_file());
    assert!(!tmp.path.join(".neoflash-sequences.json").exists());
}

#[test]
fn legacy_sequence_sidecars_are_restored_after_exact_family_overlap() {
    let tmp = TempDirGuard::new("batch-legacy-sequence-rollback");
    let project_path = tmp.path.join("source.nflash");
    let sequence_path = tmp.path.join("frames.png");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();

    let initial = json!([{
        "op": "export",
        "target": "png-sequence",
        "out": sequence_path
    }]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &initial.to_string(),
    );
    assert_eq!(code, 0, "stdout={stdout}, stderr={stderr}");

    let sidecar_name = "legacy-sidecar.bin";
    let sidecar_bytes = b"legacy owned bytes";
    fs::write(tmp.path.join(sidecar_name), sidecar_bytes).unwrap();
    let (mut manifest, existed) = neoflash_export::read_sequence_manifest(&tmp.path).unwrap();
    assert!(existed);
    let current_key = manifest.sequences.keys().next().unwrap().clone();
    let mut entry = manifest.sequences.remove(&current_key).unwrap();
    entry.family = None;
    entry.files.push(neoflash_export::SequenceOwnedFileRecord {
        name: sidecar_name.to_owned(),
        bytes: sidecar_bytes.len() as u64,
        sha256: neoflash_export::bitmap_encode::sha256_hex(sidecar_bytes),
    });
    manifest.sequences.insert("frames".to_owned(), entry);
    fs::write(
        tmp.path.join(neoflash_export::SEQUENCE_MANIFEST_FILENAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let tree_before = snapshot_tree(&tmp.path);

    let failing = json!([
        { "op": "set_project", "bg": "#C03070" },
        {
            "op": "export",
            "target": "png-sequence",
            "out": sequence_path
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &failing.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 3);
    assert_eq!(snapshot_tree(&tmp.path), tree_before);
}

#[test]
fn legacy_png_video_wav_overlap_restores_non_family_sidecars() {
    fn wav(samples: usize) -> Vec<u8> {
        let data = vec![0_u8; samples * 2];
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + data.len() as u32).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        for value in [16_u32, 1 | (1 << 16), 8_000, 16_000, 2 | (16 << 16)] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(b"data");
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
        bytes
    }

    let tmp = TempDirGuard::new("batch-legacy-video-wav-rollback");
    let project_path = tmp.path.join("source.nflash");
    let video_path = tmp.path.join("movie.png");
    let mut project = Project::blank();
    project.resources.push(
        ResourceUnit::from_bytes("sound", "audio/sound.wav", "audio/wav", &wav(800)).unwrap(),
    );
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0].sound =
        Some(FrameSound::new("sound"));
    project.upgrade_format_for_features();
    project.validate().unwrap();
    fs::write(&project_path, project.to_json().unwrap()).unwrap();

    let initial = json!([{
        "op": "export",
        "target": "video",
        "out": video_path,
        "width": 8,
        "height": 8
    }]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &initial.to_string(),
    );
    assert_eq!(code, 0, "stdout={stdout}, stderr={stderr}");

    let sidecar_name = "legacy-audio-sidecar.bin";
    let sidecar_bytes = b"legacy audio family sidecar";
    fs::write(tmp.path.join(sidecar_name), sidecar_bytes).unwrap();
    let (mut manifest, existed) = neoflash_export::read_sequence_manifest(&tmp.path).unwrap();
    assert!(existed);
    let current_key = manifest.sequences.keys().next().unwrap().clone();
    let mut entry = manifest.sequences.remove(&current_key).unwrap();
    let removed = entry
        .files
        .iter()
        .filter(|owned| !owned.name.ends_with(".wav"))
        .map(|owned| owned.name.clone())
        .collect::<Vec<_>>();
    assert!(entry.files.iter().any(|owned| owned.name.ends_with(".wav")));
    entry.files.retain(|owned| owned.name.ends_with(".wav"));
    for name in removed {
        fs::remove_file(tmp.path.join(name)).unwrap();
    }
    entry.family = None;
    entry.files.push(neoflash_export::SequenceOwnedFileRecord {
        name: sidecar_name.to_owned(),
        bytes: sidecar_bytes.len() as u64,
        sha256: neoflash_export::bitmap_encode::sha256_hex(sidecar_bytes),
    });
    manifest.sequences.insert("movie".to_owned(), entry);
    fs::write(
        tmp.path.join(neoflash_export::SEQUENCE_MANIFEST_FILENAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let tree_before = snapshot_tree(&tmp.path);

    let failing = json!([
        {
            "op": "export",
            "target": "video",
            "out": video_path,
            "width": 8,
            "height": 8
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &failing.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 2);
    assert_eq!(snapshot_tree(&tmp.path), tree_before);
}

#[test]
fn jsfl_removes_a_new_write_root_on_failure_and_keeps_it_on_success() {
    let tmp = TempDirGuard::new("jsfl-new-root");
    let project_path = tmp.path.join("source.nflash");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();

    let failed_root = tmp.path.join("failed/deep/root");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            project_path.to_str().unwrap(),
            "--stdin",
            "--flfile-root",
            failed_root.to_str().unwrap(),
        ],
        "FLfile.write('file:///nested/out.txt', 'temporary'); throw 'late';",
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "jsfl_error");
    assert!(
        error["error"]["details"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("late"))
    );
    assert!(!tmp.path.join("failed").exists());

    let success_root = tmp.path.join("success/deep/root");
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            project_path.to_str().unwrap(),
            "--stdin",
            "--flfile-root",
            success_root.to_str().unwrap(),
        ],
        "FLfile.write('file:///nested/out.txt', 'committed');",
    );
    assert_eq!(code, 0, "stdout={stdout}, stderr={stderr}");
    assert_eq!(
        fs::read_to_string(success_root.join("nested/out.txt")).unwrap(),
        "committed"
    );
}

#[cfg(unix)]
#[test]
fn batch_rolls_back_jsfl_writes_through_a_symlinked_root() {
    use std::os::unix::fs::symlink;

    let tmp = TempDirGuard::new("jsfl-symlink-root");
    let project_path = tmp.path.join("source.nflash");
    let real_root = tmp.path.join("real");
    let alias_root = tmp.path.join("alias");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();
    fs::create_dir_all(&real_root).unwrap();
    fs::write(real_root.join("existing.txt"), b"original").unwrap();
    symlink(&real_root, &alias_root).unwrap();

    let ops = json!([
        {
            "op": "jsfl",
            "flfile_root": alias_root,
            "code": "FLfile.write('file:///existing.txt', 'changed');"
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 2);
    assert_eq!(
        fs::read(real_root.join("existing.txt")).unwrap(),
        b"original"
    );
    assert_eq!(fs::read_link(&alias_root).unwrap(), real_root);
}

#[cfg(unix)]
#[test]
fn batch_restores_an_xfl_output_symlink_after_a_late_failure() {
    use std::os::unix::fs::symlink;

    let tmp = TempDirGuard::new("xfl-output-symlink");
    let project_path = tmp.path.join("source.nflash");
    let referent = tmp.path.join("existing-xfl");
    let output = tmp.path.join("document.xfl");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();
    fs::create_dir_all(&referent).unwrap();
    fs::write(referent.join("keep.bin"), b"referent stays untouched").unwrap();
    symlink(&referent, &output).unwrap();
    let tree_before = snapshot_tree(&tmp.path);

    let ops = json!([
        { "op": "export", "target": "xfl", "out": output },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "compile_failed");
    assert_eq!(error["error"]["details"]["step_index"], 2);
    assert_eq!(snapshot_tree(&tmp.path), tree_before);
    assert_eq!(fs::read_link(&output).unwrap(), referent);
}

#[cfg(unix)]
#[test]
fn jsfl_denies_a_dangling_symlink_escape_before_creating_its_target() {
    use std::os::unix::fs::symlink;

    let tmp = TempDirGuard::new("jsfl-dangling-symlink");
    let project_path = tmp.path.join("source.nflash");
    let allowed = tmp.path.join("allowed");
    let outside = tmp.path.join("outside");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink("../outside/new.txt", allowed.join("escape")).unwrap();

    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            project_path.to_str().unwrap(),
            "--stdin",
            "--flfile-root",
            allowed.to_str().unwrap(),
        ],
        "FLfile.write('file:///escape', 'must not escape');",
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    assert!(!outside.join("new.txt").exists());
    assert_eq!(
        fs::read_link(allowed.join("escape")).unwrap(),
        Path::new("../outside/new.txt")
    );
}

#[cfg(unix)]
#[test]
fn jsfl_hardlink_writes_do_not_mutate_aliases_outside_the_allowed_root() {
    let tmp = TempDirGuard::new("jsfl-hardlink-root");
    let project_path = tmp.path.join("source.nflash");
    let allowed = tmp.path.join("allowed");
    let outside = tmp.path.join("outside");
    fs::write(&project_path, Project::blank().to_json().unwrap()).unwrap();
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let failed_target = allowed.join("failed.txt");
    let failed_alias = outside.join("failed-alias.txt");
    fs::write(&failed_target, b"original").unwrap();
    fs::hard_link(&failed_target, &failed_alias).unwrap();
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            project_path.to_str().unwrap(),
            "--stdin",
            "--flfile-root",
            allowed.to_str().unwrap(),
        ],
        "FLfile.write('file:///failed.txt', 'changed'); throw 'late';",
    );
    assert_eq!(code, 1, "stdout={stdout}, stderr={stderr}");
    let error: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(error["error"]["code"], "jsfl_error");
    assert!(
        error["error"]["details"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("late"))
    );
    assert_eq!(fs::read(&failed_target).unwrap(), b"original");
    assert_eq!(fs::read(&failed_alias).unwrap(), b"original");

    let committed_target = allowed.join("committed.txt");
    let committed_alias = outside.join("committed-alias.txt");
    fs::write(&committed_target, b"before").unwrap();
    fs::hard_link(&committed_target, &committed_alias).unwrap();
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "jsfl",
            "--project",
            project_path.to_str().unwrap(),
            "--stdin",
            "--flfile-root",
            allowed.to_str().unwrap(),
        ],
        "FLfile.write('file:///committed.txt', '+after', 'append');",
    );
    assert_eq!(code, 0, "stdout={stdout}, stderr={stderr}");
    assert_eq!(fs::read(&committed_target).unwrap(), b"before+after");
    assert_eq!(fs::read(&committed_alias).unwrap(), b"before");
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "test writer failed",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn batch_rolls_back_final_output_when_success_response_cannot_be_written() {
    let tmp = TempDirGuard::new("batch-broken-stdout");
    let output = tmp.path.join("output.nflash");
    fs::write(&output, b"original project bytes").unwrap();
    let ops = json!([{ "op": "new", "name": "Candidate" }]).to_string();
    let args = vec![
        "--headless".to_owned(),
        "batch".to_owned(),
        "--stdin".to_owned(),
        "--out".to_owned(),
        output.display().to_string(),
    ];
    let mut stdin = Cursor::new(ops.as_bytes());
    let mut stdout = FailingWriter;
    let mut stderr = Vec::new();
    let code = run_cli_with_io(&args, true, &mut stdin, &mut stdout, &mut stderr);
    assert_eq!(code, 1);
    assert_eq!(fs::read(&output).unwrap(), b"original project bytes");
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
        let mut profiled = Project::from_json(&fs::read_to_string(&svg_proj).unwrap()).unwrap();
        let profile = neoflash_export::PublishProfile {
            html_template: Some(
                "<title>Custom host</title>$RUNTIME_SCRIPT<canvas id='stage'></canvas>".into(),
            ),
            ..Default::default()
        };
        neoflash_export::store_profiles_in_project(
            &mut profiled,
            std::slice::from_ref(&profile),
            Some(&profile.id),
        )
        .unwrap();
        let profiled_path = tmp.path.join("with_wrapper.nflash");
        fs::write(&profiled_path, profiled.to_json().unwrap()).unwrap();
        let html_out = tmp.path.join("html5");
        let (code, stdout, stderr) = run_args(
            &[
                "--headless",
                "export",
                "--project",
                profiled_path.to_str().unwrap(),
                "--target",
                "html5",
                "--size-level",
                "full",
                "--assets-folder",
                "--out",
                html_out.to_str().unwrap(),
            ],
            "",
        );
        assert_eq!(code, 0, "stderr: {stderr}");
        let res: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(res["ok"], true);
        assert!(
            fs::read_to_string(html_out.join("index.html"))
                .unwrap()
                .contains("<title>Custom host</title>")
        );

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
                profiled_path.to_str().unwrap(),
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
        assert!(!dom_cli_html.contains("<title>Custom host</title>"));
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

        // Explicit Canvas overrides both the saved DOM renderer and the profile's host.
        profiled.set_html_render_mode(neoflash_core::HtmlRenderMode::Dom);
        let dom_proj_path = tmp.path.join("dom_persisted.nflash");
        fs::write(&dom_proj_path, profiled.to_json().unwrap()).unwrap();
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
        assert!(canvas_override_html.contains("<canvas id=\"stage\""));
        assert!(!canvas_override_html.contains("<title>Custom host</title>"));
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

#[test]
fn t412_headless_cli_hydrates_disk_includes_across_validate_compile_eval_and_export() {
    let tmp = TempDirGuard::new("t412-headless-includes");
    let run_json = |args: &[&str]| -> (i32, Value, String) {
        let (code, out, err) = run_args(args, "");
        let parsed: Value = serde_json::from_str(out.trim())
            .unwrap_or_else(|e| panic!("invalid JSON stdout={out:?} stderr={err:?}: {e}"));
        (code, parsed, err)
    };
    fs::create_dir_all(tmp.path.join("sub")).unwrap();
    fs::write(
        tmp.path.join("sub").join("rotate.as"),
        "var ROT_STEP:Number = 90;\n",
    )
    .unwrap();
    fs::write(
        tmp.path.join("figures.as"),
        "#include \"sub/rotate.as\"\nvar PIECE_NAME:String = \"L\";\n",
    )
    .unwrap();

    let mut project = Project::blank();
    project.name = "HeadlessTetris".into();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    let root_id = project.root.clone();
    project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .scripts
        .push(neoflash_core::FrameScript {
            layer: None,
            frame: 0,
            script: neoflash_core::Script {
                language: ScriptLanguage::ActionScript2,
                source: "#include \"figures.as\"\ntrace(PIECE_NAME + \":\" + ROT_STEP);\nstop();\n"
                    .into(),
            },
        });
    crate::timeline_commands::upgrade_all_format_versions(&mut project);
    let proj_path = tmp.path.join("game.nflash");
    fs::write(&proj_path, project.to_json().unwrap()).unwrap();
    let proj_str = proj_path.to_string_lossy().into_owned();

    // 1. validate
    let (code, val, err) = run_json(&["--headless", "validate", "--project", &proj_str]);
    assert_eq!(code, 0, "validate failed: {val} stderr={err}");
    assert_eq!(val["script_error_count"], 0);

    // 2. compile
    let (code, val, err) = run_json(&["--headless", "compile", "--project", &proj_str]);
    assert_eq!(code, 0, "compile failed: {val} stderr={err}");
    assert_eq!(val["error_count"], 0);

    // 3. eval --scripts
    let (code, val, err) = run_json(&["--headless", "eval", "--project", &proj_str, "--scripts"]);
    assert_eq!(code, 0, "eval failed: {val} stderr={err}");
    assert_eq!(val["trace_lines"], serde_json::json!(["L:90"]));

    // 4. export --target swf
    let swf_out = tmp.path.join("game.swf");
    let swf_str = swf_out.to_string_lossy().into_owned();
    let (code, val, err) = run_json(&[
        "--headless",
        "export",
        "--project",
        &proj_str,
        "--target",
        "swf",
        "--out",
        &swf_str,
    ]);
    assert_eq!(code, 0, "export swf failed: {val} stderr={err}");
    assert!(swf_out.is_file());
    assert_eq!(
        val["compatibility"]["profile"],
        "Flash Player 8 (SWF v8, AVM1)"
    );
}

#[test]
fn t488_headless_as2_default_lang_text_box_jsfl_media_and_swf_vm_eval() {
    let tmp = TempDirGuard::new("t488_headless_doom_fixes");
    let wav_a = tmp.path.join("pistol.wav");
    let wav_b = tmp.path.join("shotgun.wav");
    let pcm_wav = b"RIFF,\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00D\xac\x00\x00\x88X\x01\x00\x02\x00\x10\x00data\x08\x00\x00\x00\x00\x00\x10\x00\x00\x00\xf0\xff";
    fs::write(&wav_a, pcm_wav).unwrap();
    fs::write(&wav_b, pcm_wav).unwrap();

    let proj_path = tmp.path.join("doom_probe.nflash");
    let ops = json!([
        { "op": "new", "name": "DoomProbe", "width": 320, "height": 200, "fps": 30, "script_language": "as2" },
        {
            "op": "import",
            "inputs": [wav_a.to_str().unwrap(), wav_b.to_str().unwrap()],
            "library_only": true
        },
        {
            "op": "add_element",
            "kind": "text_box",
            "name": "txtHealth",
            "x": 10.0,
            "y": 160.0,
            "width": 80.0,
            "height": 24.0,
            "text": "100%",
            "font_family": "Courier New",
            "font_size": 18.0,
            "color": "#CC2222",
            "align": "center",
            "bold": true,
            "field_type": "dynamic",
            "selectable": false
        },
        {
            "op": "set_script",
            "path": "doom/Cfg.as",
            "source": "class doom.Cfg { public static var SAFE_SPAWN:Boolean = true; public static var COLS:Number = 320; }\n"
        },
        {
            "op": "set_script",
            "frame": 0,
            "source": "import doom.Cfg;\nstop();\nvar snd:Sound = new Sound();\nsnd.attachSound('shotgun');\ntrace('vm:' + Cfg.SAFE_SPAWN + ':' + Cfg.COLS);\n"
        },
        { "op": "validate", "strict": true },
        { "op": "compile", "strict": true }
    ]);
    let (code, stdout, stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            proj_path.to_str().unwrap(),
        ],
        &serde_json::to_string(&ops).unwrap(),
    );
    assert_eq!(code, 0, "batch failed: stdout={stdout} stderr={stderr}");
    let batch_json: Value = serde_json::from_str(&stdout).unwrap();
    let import_step = &batch_json["results"][1]["result"]["report"];
    assert_eq!(import_step["format"], "wav");
    assert_eq!(import_step["sources"].as_array().unwrap().len(), 2);

    let run_json = |args: &[&str]| -> (i32, Value, String) {
        let (code, stdout, stderr) = run_args(args, "");
        let parsed: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        (code, parsed, stderr)
    };
    // JSFL sees media resources in doc.library.items and text attributes on Element
    let (code, jsfl_val, err) = run_json(&[
        "--headless",
        "jsfl",
        "--project",
        proj_path.to_str().unwrap(),
        "--eval",
        "var doc = fl.getDocumentDOM(); var items = doc.library.items; var sounds = 0; for (var i = 0; i < items.length; i++) { if (items[i].itemType == 'sound') sounds++; } var el = doc.getTimeline().layers[0].frames[0].elements[0]; fl.trace(sounds + ':' + el.align + ':' + el.bold + ':' + el.textType + ':' + el.selectable);",
    ]);
    assert_eq!(code, 0, "jsfl failed: {jsfl_val} stderr={err}");
    assert_eq!(jsfl_val["traces"], json!(["2:center:true:dynamic:false"]));

    // Preview --strict fails cleanly without GUI 'Cmd+Return' copy when AS2 class import exceeds preview subset
    let (code, strict_out, _) = run_args(
        &[
            "--headless",
            "eval",
            "--project",
            proj_path.to_str().unwrap(),
            "--scripts",
            "--strict",
        ],
        "",
    );
    assert_eq!(
        code, 1,
        "eval --scripts --strict should exit 1: {strict_out}"
    );
    assert!(!strict_out.contains("Cmd+Return"));

    // --vm swf compiles & runs real AVM1 bytecode in headless mode!
    let (code, swf_eval, err) = run_json(&[
        "--headless",
        "eval",
        "--project",
        proj_path.to_str().unwrap(),
        "--vm",
        "swf",
        "--strict",
    ]);
    assert_eq!(code, 0, "eval --vm swf failed: {swf_eval} stderr={err}");
    assert_eq!(swf_eval["vm"], "swf");
    assert_eq!(swf_eval["trace_lines"], json!(["vm:true:320"]));

    // --target image accepts 0-based --frame-index 0
    let png_out = tmp.path.join("stage0.png");
    let (code, img_val, err) = run_json(&[
        "--headless",
        "export",
        "--project",
        proj_path.to_str().unwrap(),
        "--target",
        "image",
        "--frame-index",
        "0",
        "--out",
        png_out.to_str().unwrap(),
    ]);
    assert_eq!(
        code, 0,
        "image export --frame-index 0 failed: {img_val} stderr={err}"
    );
    assert_eq!(img_val["frame_index"], 0);
    assert_eq!(img_val["frame"], 1);
}

#[test]
fn t496c_headless_frames_convert_image_counts_export_profile_protection_and_sequence_collision() {
    let tmp = TempDirGuard::new("t496c_headless_slice");
    let run_json = |args: &[&str]| -> (i32, Value, String) {
        let (code, stdout, stderr) = run_args(args, "");
        let parsed: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        (code, parsed, stderr)
    };

    // 1. [FO-09]: --headless new --frames 1 & batch new / set_project frame_count
    let single_frame_proj = tmp.path.join("vault_single_frame.nflash");
    let (code, new_val, err) = run_json(&[
        "--headless",
        "new",
        "--output",
        single_frame_proj.to_str().unwrap(),
        "--name",
        "VaultSingle",
        "--frames",
        "1",
        "--script",
        "as2",
    ]);
    assert_eq!(code, 0, "new --frames 1 failed: {new_val} stderr={err}");
    assert_eq!(new_val["frame_count"], 1);
    let loaded_single = Project::from_nflash_bytes(&fs::read(&single_frame_proj).unwrap()).unwrap();
    assert_eq!(
        loaded_single
            .symbol(&loaded_single.root)
            .unwrap()
            .timeline
            .frame_count,
        1
    );

    let (bad_frames_code, _, _) = run_json(&[
        "--headless",
        "new",
        "--output",
        tmp.path.join("zero.nflash").to_str().unwrap(),
        "--frames",
        "0",
    ]);
    assert_eq!(
        bad_frames_code, 2,
        "--frames 0 must be rejected with usage exit code 2"
    );

    // Batch `new` (frame_count: 3) + `set_project` (frame_count: 1) + `convert_script` (to: "as3")
    let batch_out = tmp.path.join("batch_converted.nflash");
    let batch_ops = json!([
        {
            "op": "new",
            "name": "BatchConvertScene",
            "width": 640,
            "height": 480,
            "fps": 30,
            "frame_count": 3,
            "script_language": "as2"
        },
        {
            "op": "set_project",
            "frame_count": 1
        },
        {
            "op": "add_layer",
            "name": "Overlay"
        },
        {
            "op": "add_element",
            "layer": "Layer 1",
            "kind": "rectangle",
            "width": 120.0,
            "height": 60.0,
            "fill": "#22AA66"
        },
        {
            "op": "add_element",
            "layer": "Overlay",
            "kind": "text",
            "text": "VAULT 13",
            "font_size": 20.0,
            "color": "#FFEE66"
        },
        {
            "op": "set_script",
            "frame": 0,
            "source": "stop();\nvar vaultOpen:Boolean = true;\ntrace('vault:' + vaultOpen);\n"
        },
        {
            "op": "convert_script",
            "to": "as3",
            "strict": true
        },
        {
            "op": "validate",
            "strict": true
        }
    ]);
    let (code, batch_stdout, batch_stderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            batch_out.to_str().unwrap(),
        ],
        &serde_json::to_string(&batch_ops).unwrap(),
    );
    assert_eq!(
        code, 0,
        "batch frame_count + convert_script failed: stdout={batch_stdout} stderr={batch_stderr}"
    );
    let batch_val: Value = serde_json::from_str(&batch_stdout).unwrap();
    assert_eq!(batch_val["results"][0]["result"]["frame_count"], 3);
    assert_eq!(batch_val["results"][1]["result"]["frame_count"], 1);
    assert_eq!(batch_val["results"][6]["result"]["ok"], true);
    assert_eq!(batch_val["results"][6]["result"]["from_language"], "as2");
    assert_eq!(batch_val["results"][6]["result"]["to_language"], "as3");
    assert_eq!(batch_val["results"][6]["result"]["converted_scripts"], 1);
    assert_eq!(batch_val["results"][6]["result"]["error_count"], 0);

    // 2. [FO-14]: --headless export --target image reports real visible layer_count, path_count, and element_count
    let stage_png = tmp.path.join("vault_stage.png");
    let (code, img_val, err) = run_json(&[
        "--headless",
        "export",
        "--project",
        batch_out.to_str().unwrap(),
        "--target",
        "image",
        "--out",
        stage_png.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "target image failed: {img_val} stderr={err}");
    assert_eq!(img_val["layer_count"], 2);
    assert_eq!(img_val["path_count"], 2);
    assert_eq!(img_val["element_count"], 2);

    // Also verify [FO-14] on examples/games/fallout-as2.nflash if present in repo
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap();
    let fallout_example = repo_root.join("examples/games/fallout-as2.nflash");
    if fallout_example.is_file() {
        let fallout_png = tmp.path.join("fallout_stage.png");
        let (fc, fval, ferr) = run_json(&[
            "--headless",
            "export",
            "--project",
            fallout_example.to_str().unwrap(),
            "--target",
            "image",
            "--out",
            fallout_png.to_str().unwrap(),
        ]);
        assert_eq!(fc, 0, "fallout image export failed: {fval} stderr={ferr}");
        assert!(
            fval["layer_count"].as_u64().unwrap_or(0) >= 5,
            "expected >= 5 visible layers in fallout-as2, got {fval}"
        );
        assert!(
            fval["path_count"].as_u64().unwrap_or(0) > 0,
            "expected > 0 vector paths in fallout-as2, got {fval}"
        );
        assert!(
            fval["element_count"].as_u64().unwrap_or(0) >= 20,
            "expected >= 20 elements in fallout-as2, got {fval}"
        );
    }

    // 3. [FO-11]: --headless capabilities lists "convert", and --headless convert converts
    // examples/games/tetris-as2.nflash -> as3 and js while refusing identical/aliased --out!
    let (cap_code, cap_val, _) = run_json(&["--headless", "capabilities"]);
    assert_eq!(cap_code, 0);
    assert!(
        cap_val["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "convert"),
        "capabilities must advertise 'convert' subcommand"
    );
    assert!(
        cap_val["batch_schema"]["operations"]
            .as_object()
            .unwrap()
            .contains_key("convert_script")
    );

    let tetris_as2 = repo_root.join("examples/games/tetris-as2.nflash");
    let tetris_copy = tmp.path.join("tetris-source.nflash");
    fs::copy(&tetris_as2, &tetris_copy).unwrap();
    let tetris_orig_bytes = fs::read(&tetris_copy).unwrap();

    // Identical --project and --out refused with source byte-identical
    let (same_code, same_val, _) = run_json(&[
        "--headless",
        "convert",
        "--project",
        tetris_copy.to_str().unwrap(),
        "--to",
        "as3",
        "--out",
        tetris_copy.to_str().unwrap(),
    ]);
    assert_eq!(same_code, 1);
    assert_eq!(same_val["error"]["code"], "source_overwrite_refused");
    assert_eq!(fs::read(&tetris_copy).unwrap(), tetris_orig_bytes);

    let tetris_as3_out = tmp.path.join("tetris-converted-as3.nflash");
    let (c_as3, v_as3, e_as3) = run_json(&[
        "--headless",
        "convert",
        "--project",
        tetris_copy.to_str().unwrap(),
        "--to",
        "as3",
        "--out",
        tetris_as3_out.to_str().unwrap(),
    ]);
    assert_eq!(
        c_as3, 0,
        "convert tetris -> as3 failed: {v_as3} stderr={e_as3}"
    );
    assert_eq!(v_as3["ok"], true);
    assert_eq!(v_as3["from_language"], "as2");
    assert_eq!(v_as3["to_language"], "as3");
    assert!(v_as3["converted_scripts"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(v_as3["error_count"], 0);

    let tetris_js_out = tmp.path.join("tetris-converted-js.nflash");
    let (c_js, v_js, e_js) = run_json(&[
        "--headless",
        "convert",
        "--project",
        tetris_copy.to_str().unwrap(),
        "--to",
        "js",
        "--out",
        tetris_js_out.to_str().unwrap(),
    ]);
    assert_eq!(c_js, 0, "convert tetris -> js failed: {v_js} stderr={e_js}");
    assert_eq!(v_js["ok"], true);
    assert_eq!(v_js["from_language"], "as2");
    assert_eq!(v_js["to_language"], "js");
    assert_eq!(v_js["error_count"], 0);

    // 4. Proposal #1: --export-profile protects source project (identical, hardlink, symlink)
    // and preserves previous profile JSON on failed write!
    let orig_single_bytes = fs::read(&single_frame_proj).unwrap();
    let hardlink_alias = tmp.path.join("vault_hardlink.json");
    fs::hard_link(&single_frame_proj, &hardlink_alias).unwrap();
    #[cfg(unix)]
    let symlink_alias = {
        let link = tmp.path.join("vault_symlink.json");
        std::os::unix::fs::symlink(&single_frame_proj, &link).unwrap();
        Some(link)
    };
    #[cfg(not(unix))]
    let symlink_alias: Option<std::path::PathBuf> = None;

    for bad_out in std::iter::once(&single_frame_proj)
        .chain(std::iter::once(&hardlink_alias))
        .chain(symlink_alias.iter())
    {
        let (p_code, p_val, _) = run_json(&[
            "--headless",
            "export",
            "--project",
            single_frame_proj.to_str().unwrap(),
            "--export-profile",
            bad_out.to_str().unwrap(),
        ]);
        assert_eq!(
            p_code,
            1,
            "expected --export-profile alias {} to be rejected: {p_val}",
            bad_out.display()
        );
        assert_eq!(p_val["error"]["code"], "source_overwrite_refused");
        assert_eq!(
            fs::read(&single_frame_proj).unwrap(),
            orig_single_bytes,
            "source project must remain byte-identical after rejected --export-profile"
        );
    }

    // Separate --export-profile JSON succeeds
    let profile_json_path = tmp.path.join("exported-profile.json");
    let (ok_prof_code, ok_prof_val, ok_prof_err) = run_json(&[
        "--headless",
        "export",
        "--project",
        single_frame_proj.to_str().unwrap(),
        "--profile",
        "Compact",
        "--export-profile",
        profile_json_path.to_str().unwrap(),
    ]);
    assert_eq!(
        ok_prof_code, 0,
        "valid --export-profile failed: {ok_prof_val} stderr={ok_prof_err}"
    );
    let saved_profile_bytes = fs::read(&profile_json_path).unwrap();
    assert!(!saved_profile_bytes.is_empty());

    // Failed write (e.g. parent path is a regular file, or read-only directory on Unix) preserves existing profile!
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let locked_dir = tmp.path.join("locked_profiles");
        fs::create_dir_all(&locked_dir).unwrap();
        let protected_prev = locked_dir.join("current-profile.json");
        fs::write(&protected_prev, &saved_profile_bytes).unwrap();
        fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let (fail_code, _, _) = run_json(&[
            "--headless",
            "export",
            "--project",
            single_frame_proj.to_str().unwrap(),
            "--profile",
            "Full",
            "--export-profile",
            protected_prev.to_str().unwrap(),
        ]);
        fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            fail_code, 1,
            "expected write into read-only directory to fail"
        );
        assert_eq!(
            fs::read(&protected_prev).unwrap(),
            saved_profile_bytes,
            "previous profile JSON must be preserved byte-identical when atomic write fails"
        );
    }

    // 5. Proposal #2: Repeated image-sequence frame numbers (`walk_1.png`, `walk_01.png`, `walk_2.png`)
    // rejected with actionable diagnostic naming frame 1 and BOTH conflicting paths before changing project!
    let seq_dir = tmp.path.join("seq_collision");
    fs::create_dir_all(&seq_dir).unwrap();
    // Minimal valid 1x1 PNG
    let mut tiny_png = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([255, 0, 0, 255]),
    ))
    .write_to(
        &mut std::io::Cursor::new(&mut tiny_png),
        image::ImageFormat::Png,
    )
    .unwrap();
    let walk_1 = seq_dir.join("walk_1.png");
    let walk_01 = seq_dir.join("walk_01.png");
    let walk_2 = seq_dir.join("walk_2.png");
    fs::write(&walk_1, &tiny_png).unwrap();
    fs::write(&walk_01, &tiny_png).unwrap();
    fs::write(&walk_2, &tiny_png).unwrap();

    // CLI `--headless import --into ... --in-place` leaves destination project byte-identical
    let (seq_code, seq_val, _) = run_json(&[
        "--headless",
        "import",
        "--input",
        walk_1.to_str().unwrap(),
        walk_01.to_str().unwrap(),
        walk_2.to_str().unwrap(),
        "--into",
        single_frame_proj.to_str().unwrap(),
        "--in-place",
    ]);
    assert_eq!(
        seq_code, 1,
        "expected duplicate sequence frame error: {seq_val}"
    );
    assert_eq!(seq_val["error"]["code"], "duplicate_sequence_frame");
    let err_msg = seq_val["error"]["message"].as_str().unwrap_or("");
    assert!(
        err_msg.contains("frame number 1")
            && err_msg.contains("walk_1.png")
            && err_msg.contains("walk_01.png"),
        "expected diagnostic naming frame 1 and both paths, got: {err_msg}"
    );
    assert_eq!(
        fs::read(&single_frame_proj).unwrap(),
        orig_single_bytes,
        "destination project must stay byte-identical when sequence collision is rejected"
    );

    // GUI Editor `import_graphics_paths` also rejects colliding sequence before mutating project
    let ctx = eframe::egui::Context::default();
    let mut editor = crate::app::Editor::from_project(&ctx, Project::blank());
    let before_json = editor.project.to_json().unwrap();
    let gui_err = editor
        .import_graphics_paths(&[walk_1.clone(), walk_01.clone(), walk_2.clone()], None)
        .expect_err("GUI sequence import must reject colliding frame number 1");
    assert!(
        gui_err.contains("frame number 1")
            && gui_err.contains("walk_1.png")
            && gui_err.contains("walk_01.png"),
        "got GUI error: {gui_err}"
    );
    assert_eq!(editor.project.to_json().unwrap(), before_json);

    // Removing the duplicate (`walk_01.png`) allows valid numeric sequence (`walk_1.png`, `walk_2.png`, `walk_10.png`)
    // to preserve numeric order and gap-hold timing!
    fs::remove_file(&walk_01).unwrap();
    let walk_10 = seq_dir.join("walk_10.png");
    fs::write(&walk_10, tiny_png).unwrap();
    let valid_seq_out = tmp.path.join("valid_walk_seq.nflash");
    let (v_code, v_seq, v_err) = run_json(&[
        "--headless",
        "import",
        "--input",
        walk_10.to_str().unwrap(),
        walk_1.to_str().unwrap(),
        walk_2.to_str().unwrap(),
        "--out",
        valid_seq_out.to_str().unwrap(),
    ]);
    assert_eq!(
        v_code, 0,
        "valid sequence import failed: {v_seq} stderr={v_err}"
    );
    let seq_proj = Project::from_nflash_bytes(&fs::read(&valid_seq_out).unwrap()).unwrap();
    let seq_root = seq_proj.symbol(&seq_proj.root).unwrap();
    assert_eq!(
        seq_root.timeline.frame_count, 10,
        "frames 1, 2, 10 with default gap holds must span 10 timeline frames (0..=9)"
    );
    let kf_frames: Vec<u32> = seq_root.timeline.layers[0]
        .keyframes
        .iter()
        .map(|k| k.frame)
        .collect();
    assert_eq!(kf_frames, vec![0, 1, 9]);
}

#[test]
fn headless_export_xcode_preserves_app_shell_by_default_and_rolls_back_in_batch() {
    let run_json = |args: &[&str]| -> (i32, Value, String) {
        let (code, stdout, stderr) = run_args(args, "");
        let parsed = serde_json::from_str(&stdout).unwrap_or(Value::Null);
        (code, parsed, stderr)
    };
    let (cap_code, cap_val, cap_err) = run_json(&["--headless", "capabilities"]);
    assert_eq!(
        cap_code, 0,
        "capabilities failed: {cap_val} stderr={cap_err}"
    );
    let export_targets = cap_val["export_targets"]
        .as_array()
        .expect("export_targets array");
    assert!(
        export_targets.iter().any(|v| v.as_str() == Some("xcode")),
        "expected 'xcode' in capabilities export_targets: {export_targets:?}"
    );
    let batch_export_fields = cap_val["batch_schema"]["operations"]["export"]
        .as_array()
        .expect("batch_schema.operations.export array");
    assert!(
        batch_export_fields
            .iter()
            .any(|v| v.as_str() == Some("bundle_id_prefix?")),
        "expected bundle_id_prefix? in batch_operations.export: {batch_export_fields:?}"
    );

    let tmp = TempDirGuard::new("headless-xcode-export");
    let project_path = tmp.path.join("demo.nflash");
    fs::write(&project_path, Project::demo().to_json().unwrap()).unwrap();
    let xcode_out = tmp.path.join("DemoXcode");

    let (code, val, err) = run_json(&[
        "--headless",
        "export",
        "--project",
        project_path.to_str().unwrap(),
        "--target",
        "xcode",
        "--out",
        xcode_out.to_str().unwrap(),
        "--bundle-id-prefix",
        "cc.redinger.t499",
    ]);
    assert_eq!(code, 0, "xcode export failed: {val} stderr={err}");
    assert_eq!(val["target"], "xcode");
    assert_eq!(val["experimental"], true);
    assert_eq!(val["bundle_id_prefix"], "cc.redinger.t499");
    assert!(
        xcode_out
            .join("Generated/NeoFlashKit/NFData.swift")
            .is_file()
    );
    assert!(
        xcode_out
            .join("Generated/Scenes/GeneratedScenes.swift")
            .is_file()
    );
    let scheme = val["scheme_name"].as_str().expect("scheme_name");
    let app_swift = xcode_out.join(format!("App/{scheme}App.swift"));
    assert!(app_swift.is_file());

    // User customization in App/ must be preserved unless --overwrite-app-shell is set
    let custom_shell = "// custom user shell marker\n";
    let mut customized = fs::read_to_string(&app_swift).unwrap();
    customized.push_str(custom_shell);
    fs::write(&app_swift, &customized).unwrap();

    let (code2, val2, err2) = run_json(&[
        "--headless",
        "export",
        "--project",
        project_path.to_str().unwrap(),
        "--target",
        "xcode",
        "--out",
        xcode_out.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0, "re-export failed: {val2} stderr={err2}");
    assert!(
        fs::read_to_string(&app_swift)
            .unwrap()
            .contains(custom_shell),
        "default xcode re-export must preserve App/ shell customizations"
    );

    let (code3, val3, err3) = run_json(&[
        "--headless",
        "export",
        "--project",
        project_path.to_str().unwrap(),
        "--target",
        "xcode",
        "--out",
        xcode_out.to_str().unwrap(),
        "--overwrite-app-shell",
    ]);
    assert_eq!(code3, 0, "overwrite export failed: {val3} stderr={err3}");
    assert!(
        !fs::read_to_string(&app_swift)
            .unwrap()
            .contains(custom_shell),
        "--overwrite-app-shell must regenerate App/NeoFlashApp.swift"
    );

    // Batch rollback must restore absent directory when a subsequent step fails
    let batch_xcode_out = tmp.path.join("RolledBackXcode");
    let ops = json!([
        {
            "op": "export",
            "target": "xcode",
            "out": batch_xcode_out,
            "bundle_id_prefix": "cc.redinger.rollback"
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "js",
            "source": "let = ;"
        },
        { "op": "compile" }
    ]);
    let (bcode, bstdout, bstderr) = run_args(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            project_path.to_str().unwrap(),
        ],
        &ops.to_string(),
    );
    assert_eq!(
        bcode, 1,
        "expected batch failure: stdout={bstdout} stderr={bstderr}"
    );
    assert!(
        !batch_xcode_out.exists(),
        "batch rollback must remove newly exported xcode project directory"
    );
}
