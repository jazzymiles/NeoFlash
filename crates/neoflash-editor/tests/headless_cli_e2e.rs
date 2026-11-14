//! Subprocess end-to-end integration tests for `neoflash-editor --headless` (`T-313`).
//!
//! Spawns the real `neoflash-editor` executable with display environment variables cleared so
//! every CLI dispatch mode (`--headless`, `headless`, `NEOFLASH_HEADLESS=1`, `-psn_*` filtering),
//! exit code (`0`, `1`, `2`), pure JSON `stdout` / `stderr` separation, atomic `batch` rollback,
//! `deny_unknown_fields`, JSFL `FLfile` capability sandboxing, `R-102` unified project script
//! language policy (`T-315`), Figma/FLA/XFL/SWF/SVG `import`, and HTML5 Canvas/DOM (`T-307`) +
//! Lottie/dotLottie (`T-308`) `export` are verified across process boundaries.

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

struct TempTestDir {
    path: PathBuf,
}

impl TempTestDir {
    fn new(prefix: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-cli-e2e-{prefix}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        Self { path: dir }
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct CliRun {
    code: i32,
    stdout: String,
    stderr: String,
}

impl CliRun {
    fn parse_stdout_json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout was not pure JSON (exit={}): {e}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                self.code, self.stdout, self.stderr
            )
        })
    }

    fn parse_stderr_json(&self) -> Value {
        serde_json::from_str(&self.stderr).unwrap_or_else(|e| {
            panic!(
                "stderr was not pure JSON (exit={}): {e}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                self.code, self.stdout, self.stderr
            )
        })
    }
}

fn ensure_mock_web_runtime(dir: &Path) -> PathBuf {
    let real_rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/web-runtime");
    if real_rt.join("neoflash_web.js").is_file() && real_rt.join("neoflash_web_bg.wasm").is_file() {
        return real_rt;
    }
    let mock = dir.join("mock_web_runtime");
    fs::create_dir_all(&mock).expect("create mock web runtime dir");
    fs::write(mock.join("neoflash_web.js"), "// mock runtime\n").expect("write neoflash_web.js");
    fs::write(mock.join("neoflash_web_bg.wasm"), b"\0asm\x01\x00\x00\x00")
        .expect("write neoflash_web_bg.wasm");
    mock
}

fn run_bin(args: &[&str], envs: &[(&str, &str)], stdin_payload: Option<&[u8]>) -> CliRun {
    let bin = env!("CARGO_BIN_EXE_neoflash-editor");
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("DISPLAY", "")
        .env("WAYLAND_DISPLAY", "")
        .env_remove("NEOFLASH_HEADLESS")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (k, v) in envs {
        cmd.env(k, v);
    }

    if stdin_payload.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    let mut child = cmd.spawn().expect("spawn neoflash-editor binary");
    if let Some(payload) = stdin_payload {
        let mut child_stdin = child.stdin.take().expect("take child stdin");
        let bytes = payload.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = child_stdin.write_all(&bytes);
        });
        let output = child.wait_with_output().expect("wait for neoflash-editor");
        let _ = writer.join();
        return CliRun {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
    }

    let output = child.wait_with_output().expect("wait for neoflash-editor");
    CliRun {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn subprocess_dispatch_modes_exit_codes_and_pure_json_streams() {
    // 1. `--help` runs display-free in Text mode by default and exits 0.
    let help = run_bin(&["--help"], &[], None);
    assert_eq!(help.code, 0, "stderr: {}", help.stderr);
    assert!(help.stdout.contains("NeoFlash Editor Headless CLI"));
    assert!(help.stderr.is_empty());

    // 2. `--version --format json` emits pure JSON on stdout.
    let ver = run_bin(&["--version", "--format", "json"], &[], None);
    assert_eq!(ver.code, 0, "stderr: {}", ver.stderr);
    assert!(ver.stderr.is_empty());
    let ver_json = ver.parse_stdout_json();
    assert_eq!(ver_json["ok"], true);
    assert_eq!(ver_json["command"], "version");
    assert!(
        ver_json["version"]
            .as_str()
            .unwrap_or_default()
            .starts_with("0.1.0")
    );

    // 3. `-psn_*` Finder launch argument is ignored alongside `--headless capabilities --pretty`.
    let caps = run_bin(
        &["-psn_0_424242", "--headless", "capabilities", "--pretty"],
        &[],
        None,
    );
    assert_eq!(caps.code, 0, "stderr: {}", caps.stderr);
    assert!(caps.stderr.is_empty());
    let caps_json = caps.parse_stdout_json();
    assert_eq!(caps_json["ok"], true);
    assert_eq!(caps_json["command"], "capabilities");
    assert!(
        caps_json["export_targets"]
            .as_array()
            .expect("export_targets array")
            .iter()
            .any(|v| v == "html5")
    );

    // 4. `headless schema` positional mode and `NEOFLASH_HEADLESS=1` env mode both emit pure JSON.
    let schema = run_bin(&["headless", "schema"], &[], None);
    assert_eq!(schema.code, 0, "stderr: {}", schema.stderr);
    assert_eq!(schema.parse_stdout_json()["command"], "capabilities");

    let env_ver = run_bin(&["version"], &[("NEOFLASH_HEADLESS", "1")], None);
    assert_eq!(env_ver.code, 0, "stderr: {}", env_ver.stderr);
    assert_eq!(env_ver.parse_stdout_json()["command"], "version");

    // 5. Usage error returns exit code 2 with empty stdout and JSON error on stderr.
    let bad_cmd = run_bin(&["--headless", "nonexistent_subcommand"], &[], None);
    assert_eq!(bad_cmd.code, 2);
    assert!(bad_cmd.stdout.is_empty());
    let bad_err = bad_cmd.parse_stderr_json();
    assert_eq!(bad_err["ok"], false);
    assert_eq!(bad_err["error"]["code"], "unknown_subcommand");

    // 6. Runtime missing-file error returns exit code 1 with JSON error on stderr.
    let missing = run_bin(
        &[
            "--headless",
            "validate",
            "--project",
            "/nonexistent/path/missing.nflash",
        ],
        &[],
        None,
    );
    assert_eq!(missing.code, 1);
    assert!(missing.stdout.is_empty());
    assert_eq!(missing.parse_stderr_json()["ok"], false);
}

#[test]
fn subprocess_new_script_policy_stdin_batch_rollback_and_eval_pipeline() {
    let tmp = TempTestDir::new("batch-script");
    let proj_path = tmp.path.join("ts_project.nflash");
    let proj_str = proj_path.to_str().unwrap();

    // 1. Create a TypeScript project via `--headless new --script ts` (`T-315` / `R-102`).
    let created = run_bin(
        &[
            "--headless",
            "new",
            "--script",
            "ts",
            "--width",
            "960",
            "--height",
            "540",
            "--fps",
            "30",
            "--out",
            proj_str,
        ],
        &[],
        None,
    );
    assert_eq!(created.code, 0, "stderr: {}", created.stderr);
    let created_json = created.parse_stdout_json();
    assert_eq!(created_json["script_language"], "ts");
    assert_eq!(created_json["also_compile_compatible_scripts"], true);

    // 2. Populate artwork, tween, primary TypeScript script, and compatible JavaScript script via stdin `batch`.
    let batch_ops = json!({
        "ops": [
            { "op": "add_layer", "name": "UI" },
            { "op": "add_keyframe", "layer": "UI", "frame": 0, "tween": "linear", "tween_kind": "classic" },
            {
                "op": "add_element",
                "id": "hero_card",
                "name": "HeroCard",
                "layer": "UI",
                "frame": 0,
                "kind": "rectangle",
                "x": 40.0,
                "y": 50.0,
                "width": 200.0,
                "height": 100.0,
                "fill": "#3366CC"
            },
            {
                "op": "add_element",
                "id": "hostile_txt",
                "name": "Headline",
                "layer": "UI",
                "frame": 0,
                "kind": "text",
                "text": "Safe </script>\u{2028}Title",
                "font_size": 24.0
            },
            { "op": "add_keyframe", "layer": "UI", "frame": 2, "label": "settled", "copy_previous": true },
            {
                "op": "set_script",
                "frame": 0,
                "language": "ts",
                "source": "const bootTag: string = \"ts_boot\";\ntrace(bootTag);"
            },
            {
                "op": "set_script",
                "frame": 2,
                "language": "js",
                "source": "trace(\"compat_js_frame2\");"
            },
            { "op": "validate" },
            { "op": "compile" }
        ]
    });
    let built = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            proj_str,
            "--in-place",
        ],
        &[],
        Some(batch_ops.to_string().as_bytes()),
    );
    assert_eq!(built.code, 0, "stderr: {}", built.stderr);
    let built_json = built.parse_stdout_json();
    assert_eq!(built_json["ok"], true);
    assert_eq!(built_json["steps_executed"], 9);

    // 3. Standalone `inspect`, `validate`, `compile`, and `eval --scripts` subprocess checks.
    let inspected = run_bin(&["--headless", "inspect", "--project", proj_str], &[], None);
    assert_eq!(inspected.code, 0, "stderr: {}", inspected.stderr);
    let inspected_json = inspected.parse_stdout_json();
    assert_eq!(inspected_json["metadata"]["script_language"], "ts");
    assert_eq!(
        inspected_json["metadata"]["effective_script_language"],
        "ts"
    );

    let eval_run = run_bin(
        &[
            "--headless",
            "eval",
            "--project",
            proj_str,
            "--frames",
            "0..=2",
            "--scripts",
        ],
        &[],
        None,
    );
    assert_eq!(eval_run.code, 0, "stderr: {}", eval_run.stderr);
    let eval_json = eval_run.parse_stdout_json();
    assert_eq!(eval_json["ok"], true);
    let traces: Vec<&str> = eval_json["trace_lines"]
        .as_array()
        .expect("trace_lines")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(traces, vec!["ts_boot", "compat_js_frame2"]);

    // 4. Atomic rollback on runtime/compile failure + strict `deny_unknown_fields` rejection.
    let saved_bytes = fs::read(&proj_path).expect("read saved project bytes");

    let failing_compile_batch = json!([
        { "op": "set_project", "name": "MutatedBeforeFail" },
        { "op": "set_script", "frame": 0, "language": "ts", "source": "const broken: number = ;" },
        { "op": "compile" }
    ]);
    let rollback = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            proj_str,
            "--in-place",
        ],
        &[],
        Some(failing_compile_batch.to_string().as_bytes()),
    );
    assert_eq!(rollback.code, 1);
    assert!(rollback.stdout.is_empty());
    assert_eq!(
        rollback.parse_stderr_json()["error"]["code"],
        "compile_failed"
    );
    assert_eq!(
        fs::read(&proj_path).unwrap(),
        saved_bytes,
        "failed --in-place batch must leave project file byte-identical"
    );

    let unknown_field_batch = json!([
        { "op": "set_project", "widht": 1280 }
    ]);
    let rejected = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            proj_str,
            "--in-place",
        ],
        &[],
        Some(unknown_field_batch.to_string().as_bytes()),
    );
    assert_eq!(rejected.code, 2);
    assert!(rejected.stdout.is_empty());
    assert_eq!(
        rejected.parse_stderr_json()["error"]["code"],
        "invalid_batch_json"
    );
    assert_eq!(fs::read(&proj_path).unwrap(), saved_bytes);

    // 5. Foreign AS3 script with syntax error is skipped cleanly under TypeScript project lock (`T-315`).
    let add_foreign = json!([
        {
            "op": "set_script",
            "frame": 1,
            "language": "as3",
            "source": "package { public class IgnoredForeign { if ((( } }"
        }
    ]);
    let foreign_res = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--project",
            proj_str,
            "--in-place",
        ],
        &[],
        Some(add_foreign.to_string().as_bytes()),
    );
    assert_eq!(foreign_res.code, 0, "stderr: {}", foreign_res.stderr);

    let compile_with_foreign =
        run_bin(&["--headless", "compile", "--project", proj_str], &[], None);
    assert_eq!(
        compile_with_foreign.code, 0,
        "skipped foreign AS3 script must not fail compile: {}",
        compile_with_foreign.stderr
    );
    let compile_json = compile_with_foreign.parse_stdout_json();
    assert_eq!(compile_json["ok"], true);
    assert!(
        compile_json["warning_count"].as_u64().unwrap_or(0) >= 2,
        "expected warnings for compatible JS + skipped foreign AS3: {compile_json}"
    );
}

#[test]
fn subprocess_jsfl_sandbox_enforces_capabilities_and_path_traversal() {
    let tmp = TempTestDir::new("jsfl-sandbox");
    let proj_path = tmp.path.join("base.nflash");
    let out_path = tmp.path.join("mutated.nflash");
    let sandbox_dir = tmp.path.join("allowed_io");
    let outside_secret = tmp.path.join("outside_secret.txt");
    fs::create_dir_all(&sandbox_dir).unwrap();
    fs::write(&outside_secret, "do_not_read").unwrap();
    fs::write(sandbox_dir.join("layer_name.txt"), "FromSandboxedJsfl").unwrap();

    let proj_str = proj_path.to_str().unwrap();
    let out_str = out_path.to_str().unwrap();
    let sandbox_str = sandbox_dir.to_str().unwrap();

    let new_res = run_bin(&["--headless", "new", "--out", proj_str], &[], None);
    assert_eq!(new_res.code, 0);

    // 1. Default deny-all blocks FLfile.read and leaves `--out` uncreated.
    let denied = run_bin(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_str,
            "--eval",
            "FLfile.read(\"file:///layer_name.txt\");",
            "--out",
            out_str,
        ],
        &[],
        None,
    );
    assert_eq!(denied.code, 1);
    assert_eq!(denied.parse_stderr_json()["error"]["code"], "jsfl_error");
    assert!(!out_path.exists());

    // 2. `..` traversal fails even when `--allow-read` is granted.
    let traversal = run_bin(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_str,
            "--eval",
            "FLfile.read(\"file:///../outside_secret.txt\");",
            "--allow-read",
            sandbox_str,
        ],
        &[],
        None,
    );
    assert_eq!(traversal.code, 1);

    // 3. Explicit `--allow-read` + `--allow-write` via `--stdin` mutates document and writes file safely.
    let script = br#"
        var label = FLfile.read("file:///layer_name.txt");
        FLfile.write("file:///audit.txt", "created:" + label);
        var dom = fl.getDocumentDOM();
        dom.width = 1024;
        dom.getTimeline().addNewLayer(label);
        fl.trace("jsfl_done:" + label);
        label;
    "#;
    let allowed = run_bin(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_str,
            "--stdin",
            "--allow-read",
            sandbox_str,
            "--allow-write",
            sandbox_str,
            "--out",
            out_str,
        ],
        &[],
        Some(script),
    );
    assert_eq!(allowed.code, 0, "stderr: {}", allowed.stderr);
    let allowed_json = allowed.parse_stdout_json();
    assert_eq!(allowed_json["ok"], true);
    assert_eq!(allowed_json["return_value"], "FromSandboxedJsfl");
    assert_eq!(
        fs::read_to_string(sandbox_dir.join("audit.txt")).unwrap(),
        "created:FromSandboxedJsfl"
    );

    let val = run_bin(&["--headless", "validate", "--project", out_str], &[], None);
    assert_eq!(val.code, 0, "stderr: {}", val.stderr);
}

#[test]
fn subprocess_import_and_export_roundtrips_figma_interchange_dom_and_lottie() {
    let tmp = TempTestDir::new("import-export");
    let runtime_dir = ensure_mock_web_runtime(&tmp.path);
    let runtime_str = runtime_dir.to_str().unwrap();

    // 1. Import `.figma.json` REST document via subprocess and validate resulting `.nflash`.
    let figma_json_path = tmp.path.join("sample.figma.json");
    let figma_doc = json!({
        "name": "CLI Figma Sample",
        "document": {
            "id": "0:0",
            "name": "Document",
            "type": "DOCUMENT",
            "children": [{
                "id": "0:1",
                "name": "Page 1",
                "type": "CANVAS",
                "backgroundColor": { "r": 0.96, "g": 0.97, "b": 0.99, "a": 1.0 },
                "children": [{
                    "id": "1:2",
                    "name": "Main Frame",
                    "type": "FRAME",
                    "absoluteBoundingBox": { "x": 0.0, "y": 0.0, "width": 640.0, "height": 360.0 },
                    "fills": [{ "type": "SOLID", "color": { "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 } }],
                    "children": [
                        {
                            "id": "1:3",
                            "name": "CTA",
                            "type": "RECTANGLE",
                            "absoluteBoundingBox": { "x": 40.0, "y": 40.0, "width": 180.0, "height": 52.0 },
                            "cornerRadius": 8.0,
                            "fills": [{ "type": "SOLID", "color": { "r": 0.15, "g": 0.45, "b": 0.95, "a": 1.0 } }]
                        },
                        {
                            "id": "1:4",
                            "name": "Label",
                            "type": "TEXT",
                            "absoluteBoundingBox": { "x": 64.0, "y": 54.0, "width": 132.0, "height": 24.0 },
                            "characters": "Launch </script>\u{2028}App",
                            "fills": [{ "type": "SOLID", "color": { "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 } }],
                            "style": { "fontFamily": "Arial", "fontSize": 18.0, "fontWeight": 400 }
                        }
                    ]
                }]
            }]
        }
    });
    fs::write(&figma_json_path, figma_doc.to_string()).unwrap();

    let imported_nflash = tmp.path.join("from_figma.nflash");
    let imp = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            figma_json_path.to_str().unwrap(),
            "--out",
            imported_nflash.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp.code, 0, "stderr: {}", imp.stderr);
    assert_eq!(imp.parse_stdout_json()["ok"], true);

    let val = run_bin(
        &[
            "--headless",
            "validate",
            "--project",
            imported_nflash.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(val.code, 0, "stderr: {}", val.stderr);

    // Optional local Zstd `.fig` corpus smoke when `target/figma-corpus` is present on disk (`T-312`).
    let local_corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/figma-corpus");
    if let Ok(entries) = fs::read_dir(&local_corpus_dir) {
        let mut smallest_fig: Option<(u64, PathBuf)> = None;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("fig")
                && let Ok(meta) = entry.metadata()
            {
                let len = meta.len();
                if smallest_fig.as_ref().is_none_or(|(best, _)| len < *best) {
                    smallest_fig = Some((len, p));
                }
            }
        }
        if let Some((_, fig_path)) = smallest_fig {
            let fig_out = tmp.path.join("from_zstd_fig.nflash");
            let fig_imp = run_bin(
                &[
                    "--headless",
                    "import",
                    "--input",
                    fig_path.to_str().unwrap(),
                    "--out",
                    fig_out.to_str().unwrap(),
                ],
                &[],
                None,
            );
            assert_eq!(fig_imp.code, 0, "Zstd .fig import failed cleanly");
            let fig_val = run_bin(
                &[
                    "--headless",
                    "validate",
                    "--project",
                    fig_out.to_str().unwrap(),
                ],
                &[],
                None,
            );
            assert_eq!(fig_val.code, 0, "Zstd .fig output failed validate");
        }
    }

    // 2. Export HTML5 default Canvas vs `--render-mode dom` (`T-307`).
    let canvas_dir = tmp.path.join("export_canvas");
    let exp_canvas = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "html5",
            "--runtime",
            runtime_str,
            "--out",
            canvas_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_canvas.code, 0, "stderr: {}", exp_canvas.stderr);
    assert_eq!(exp_canvas.parse_stdout_json()["render_mode"], "canvas");
    let canvas_html = fs::read_to_string(canvas_dir.join("index.html")).unwrap();
    assert!(!canvas_html.contains("<neoflash-stage"));

    let dom_dir = tmp.path.join("export_dom");
    let exp_dom = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "html5",
            "--render-mode",
            "dom",
            "--runtime",
            runtime_str,
            "--out",
            dom_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_dom.code, 0, "stderr: {}", exp_dom.stderr);
    assert_eq!(exp_dom.parse_stdout_json()["render_mode"], "dom");
    let dom_html = fs::read_to_string(dom_dir.join("index.html")).unwrap();
    assert!(dom_html.contains("<neoflash-stage id=\"stage-dom-root\" data-render-mode=\"dom\""));

    // 3. Export Lottie JSON + HTML preview (`</script>` + `\u2028` escaped) and `.lottie` ZIP (`T-308`).
    let lottie_json_out = tmp.path.join("anim.lottie.json");
    let exp_lottie = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "lottie",
            "--html-preview",
            "--out",
            lottie_json_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_lottie.code, 0, "stderr: {}", exp_lottie.stderr);
    let lottie_parsed: Value =
        serde_json::from_str(&fs::read_to_string(&lottie_json_out).unwrap()).unwrap();
    assert!(lottie_parsed["layers"].is_array());
    let preview_html = fs::read_to_string(tmp.path.join("anim.lottie.preview.html"))
        .or_else(|_| fs::read_to_string(tmp.path.join("anim.preview.html")))
        .expect("lottie html preview written");
    assert!(
        !preview_html.contains("Launch </script>"),
        "preview HTML must neutralize raw </script> inside inline JSON"
    );
    assert!(
        !preview_html.contains('\u{2028}'),
        "preview HTML must escape U+2028 line separator"
    );

    let dotlottie_out = tmp.path.join("anim.lottie");
    let exp_dotlottie = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "dotlottie",
            "--out",
            dotlottie_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_dotlottie.code, 0, "stderr: {}", exp_dotlottie.stderr);
    let dotlottie_bytes = fs::read(&dotlottie_out).unwrap();
    assert_eq!(&dotlottie_bytes[..4], b"PK\x03\x04");

    // 4. Export XFL, binary FLA, and SVG from the Figma-imported project, plus SWF from a clean
    // vector/script project (verifying both fail-closed SWF preflight on unmappable U+2028 text
    // and clean SWF export + reimport roundtrip).
    let swf_blocked = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "swf",
            "--out",
            tmp.path.join("blocked.swf").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(swf_blocked.code, 1);
    assert_eq!(
        swf_blocked.parse_stderr_json()["error"]["code"],
        "export_failed"
    );

    let clean_swf_proj = tmp.path.join("clean_swf_src.nflash");
    let swf_batch = json!([
        { "op": "new", "name": "SwfRoundtrip", "width": 640, "height": 360, "fps": 24 },
        {
            "op": "add_element",
            "id": "badge",
            "name": "Badge",
            "kind": "rectangle",
            "x": 24.0,
            "y": 24.0,
            "width": 160.0,
            "height": 80.0,
            "fill": "#2266DD"
        },
        {
            "op": "set_script",
            "frame": 0,
            "language": "as3",
            "source": "stop();"
        }
    ]);
    let mk_swf_proj = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            clean_swf_proj.to_str().unwrap(),
        ],
        &[],
        Some(swf_batch.to_string().as_bytes()),
    );
    assert_eq!(mk_swf_proj.code, 0, "stderr: {}", mk_swf_proj.stderr);

    let xfl_out = tmp.path.join("roundtrip_xfl");
    let fla_out = tmp.path.join("roundtrip.fla");
    let swf_out = tmp.path.join("roundtrip.swf");
    let svg_out = tmp.path.join("roundtrip.svg");

    for (target, src_p, out_p) in [
        ("xfl", &imported_nflash, &xfl_out),
        ("fla", &imported_nflash, &fla_out),
        ("swf", &clean_swf_proj, &swf_out),
        ("svg", &imported_nflash, &svg_out),
    ] {
        let ex = run_bin(
            &[
                "--headless",
                "export",
                "--project",
                src_p.to_str().unwrap(),
                "--target",
                target,
                "--out",
                out_p.to_str().unwrap(),
            ],
            &[],
            None,
        );
        assert_eq!(ex.code, 0, "export {target} failed: {}", ex.stderr);

        let reimported = tmp.path.join(format!("reimported_{target}.nflash"));
        let reimp = run_bin(
            &[
                "--headless",
                "import",
                "--input",
                out_p.to_str().unwrap(),
                "--out",
                reimported.to_str().unwrap(),
            ],
            &[],
            None,
        );
        assert_eq!(reimp.code, 0, "reimport {target} failed: {}", reimp.stderr);

        let reval = run_bin(
            &[
                "--headless",
                "validate",
                "--project",
                reimported.to_str().unwrap(),
            ],
            &[],
            None,
        );
        assert_eq!(
            reval.code, 0,
            "validate reimported {target} failed: {}",
            reval.stderr
        );
    }
}
