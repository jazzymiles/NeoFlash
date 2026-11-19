//! Subprocess end-to-end integration tests for `neoflash-editor --headless` (`T-313` / `T-321`).
//!
//! Spawns the real `neoflash-editor` executable with display environment variables cleared so
//! every CLI dispatch mode (`--headless`, `headless`, `NEOFLASH_HEADLESS=1`, `-psn_*` filtering,
//! `--help`/`-h`, `--version`/`-V`), subcommand help, strict exit codes (`0`, `1`, `2`), pure JSON
//! `stdout` contract on both success and error, atomic `--in-place` writes/rollbacks, JSFL
//! `fl.trace()` + `fl.getDocumentDOM()` + `FLfile.*` sandboxing, `R-102` script-language policy,
//! standalone + `--into` `import` (`SVG`, `PNG`, `WAV`, `SWC`, `XFL` dir & `.xfl` file, `Figma`,
//! `PSD`, `PDF`), and full `export` (`html5` Canvas/DOM/profiles, `swf` `fp8`/`fp9`/`fp10`,
//! `image` `png`/`webp`/`psd`/`pdf`/`jpeg`/`ai`, `lottie`, `dotlottie`, `svg`, `png-sequence`,
//! `atlas`, `aep`, `fla`, `xfl`) are verified across real OS process boundaries.

use image::{ImageFormat, Rgba, RgbaImage};
use neoflash_core::{Project, ScriptLanguage, Symbol, SymbolKind, Timeline};
use serde_json::{Value, json};
use std::fs;
use std::io::{Cursor, Write};
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
        assert!(
            self.stderr.is_empty(),
            "expected empty stderr in JSON mode (exit={}), got stderr:\n{}\n--- stdout ---\n{}",
            self.code,
            self.stderr,
            self.stdout
        );
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout was not a single valid JSON object (exit={}): {e}\n--- stdout ---\n{}\n--- stderr ---\n{}",
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
    fs::write(
        mock.join("neoflash_web.js"),
        "let NeoFlashWasm = function() {};\n",
    )
    .expect("write neoflash_web.js");
    fs::write(mock.join("neoflash_web_bg.wasm"), b"\0asm\x01\x00\x00\x00")
        .expect("write neoflash_web_bg.wasm");
    mock
}

fn make_solid_png_bytes(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let img = RgbaImage::from_pixel(width, height, Rgba(rgba));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("encode synthetic PNG");
    buf
}

fn make_pcm_wav_bytes(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_bytes_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2;
    let riff_size = 36 + data_bytes_len;
    let mut wav = Vec::with_capacity((44 + data_bytes_len) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes_len.to_le_bytes());
    for &sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn build_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32_ieee(data);
        let size = data.len() as u32;
        let local_offset = out.len() as u32;

        // Local file header
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20_u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0_u16.to_le_bytes()); // flags
        out.extend_from_slice(&0_u16.to_le_bytes()); // method 0 = stored
        out.extend_from_slice(&0_u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0_u16.to_le_bytes()); // mod date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes()); // extra len
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        // Central directory header
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20_u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20_u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0_u16.to_le_bytes()); // flags
        central.extend_from_slice(&0_u16.to_le_bytes()); // method 0 = stored
        central.extend_from_slice(&0_u16.to_le_bytes()); // mod time
        central.extend_from_slice(&0_u16.to_le_bytes()); // mod date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes()); // extra len
        central.extend_from_slice(&0_u16.to_le_bytes()); // comment len
        central.extend_from_slice(&0_u16.to_le_bytes()); // disk start
        central.extend_from_slice(&0_u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0_u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    // End of central directory
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0_u16.to_le_bytes()); // disk
    out.extend_from_slice(&0_u16.to_le_bytes()); // cd start disk
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes()); // comment len
    out
}

fn build_synthetic_swc_bytes(dir: &Path) -> Vec<u8> {
    // Export a clean SWF10 library with a class-linked symbol via `--headless export --target swf`.
    let mut lib_project = Project::blank();
    lib_project.script_language = Some(ScriptLanguage::ActionScript3);
    lib_project.enable_program();
    lib_project.symbols.push(Symbol {
        id: "sym_hero_badge".into(),
        name: "HeroBadge".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: Some("com.example.HeroBadge".into()),
        linkage_id: Some("com.example.HeroBadge".into()),
        timeline: Timeline {
            frame_count: 1,
            layers: vec![neoflash_core::Layer {
                id: "lyr_badge".into(),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![neoflash_core::Keyframe {
                    frame: 0,
                    elements: vec![neoflash_core::Element {
                        id: "el_badge".into(),
                        name: "BadgeShape".into(),
                        kind: neoflash_core::ElementKind::Rectangle {
                            width: 48.0,
                            height: 32.0,
                            fill: [40, 120, 220, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..neoflash_core::Element::default()
                    }],
                    ..neoflash_core::Keyframe::default()
                }],
                properties: neoflash_core::LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    lib_project.sources.push(neoflash_core::SourceUnit {
        id: "src_hero_badge".into(),
        path: "com/example/HeroBadge.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: "package com.example { import flash.display.MovieClip; public class HeroBadge extends MovieClip { public function HeroBadge() {} } }\n".into(),
    });
    lib_project.upgrade_format_for_features();
    let lib_proj_path = dir.join("_swc_src.nflash");
    let lib_swf_path = dir.join("_swc_library.swf");
    fs::write(&lib_proj_path, lib_project.to_json().unwrap()).unwrap();
    let exp = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            lib_proj_path.to_str().unwrap(),
            "--target",
            "swf",
            "--swf-target",
            "fp10",
            "--out",
            lib_swf_path.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(
        exp.code, 0,
        "build library.swf for SWC test: {}",
        exp.stdout
    );
    let lib_swf = fs::read(&lib_swf_path).unwrap();
    let icon_png = make_solid_png_bytes(8, 8, [20, 180, 240, 255]);
    let catalog_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<swc xmlns="http://www.adobe.com/flash/swccatalog/9">
  <versions>
    <swc version="1.2"/>
  </versions>
  <features>
    <feature-script-deps/>
    <feature-components/>
    <feature-files/>
  </features>
  <components>
    <component className="com.example:HeroBadge" name="HeroBadge" uri="http://example.com/ui" icon="assets/icon.png"/>
  </components>
  <libraries>
    <library path="library.swf">
      <script name="com/example/HeroBadge" mod="1700000200">
        <def id="com.example:HeroBadge"/>
        <dep id="flash.display:MovieClip" type="i"/>
      </script>
      <script name="com/example/HeroService" mod="1700000200">
        <def id="com.example:HeroService"/>
      </script>
    </library>
  </libraries>
  <files>
    <file path="assets/icon.png" mod="1700000100"/>
  </files>
</swc>"#;
    build_stored_zip(&[
        ("catalog.xml", catalog_xml.as_bytes()),
        ("library.swf", &lib_swf),
        ("assets/icon.png", &icon_png),
    ])
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
fn subprocess_dispatch_modes_help_exit_codes_and_pure_json_stdout_contract() {
    // 1. `--help` / `-h` and `--version` / `-V` run display-free without Dock/GUI initialization.
    for flag in ["--help", "-h"] {
        let help = run_bin(&[flag], &[], None);
        assert_eq!(help.code, 0, "stderr: {}", help.stderr);
        assert!(help.stdout.contains("NeoFlash Editor Headless CLI"));
        assert!(help.stderr.is_empty());
    }

    for sub in [
        "capabilities",
        "new",
        "inspect",
        "validate",
        "compile",
        "eval",
        "batch",
        "jsfl",
        "import",
        "export",
    ] {
        for flag in ["--help", "-h"] {
            let sub_help = run_bin(&["--headless", sub, flag], &[], None);
            assert_eq!(
                sub_help.code, 0,
                "subcommand {sub} {flag} failed: {}",
                sub_help.stderr
            );
            assert!(
                sub_help
                    .stdout
                    .contains("Usage: neoflash-editor --headless"),
                "unexpected help output for {sub}: {}",
                sub_help.stdout
            );
            assert!(sub_help.stderr.is_empty());
        }
    }

    // 2. `--version --format json` and `-V` emit pure JSON / text on stdout.
    let ver = run_bin(&["--version", "--format", "json"], &[], None);
    assert_eq!(ver.code, 0, "stderr: {}", ver.stderr);
    let ver_json = ver.parse_stdout_json();
    assert_eq!(ver_json["ok"], true);
    assert_eq!(ver_json["command"], "version");
    assert!(
        ver_json["version"]
            .as_str()
            .unwrap_or_default()
            .starts_with("0.1.0")
    );

    let short_ver = run_bin(&["-V"], &[], None);
    assert_eq!(short_ver.code, 0);
    assert!(short_ver.stdout.contains("neoflash-editor 0.1.0"));

    // 3. `-psn_*` Finder launch argument is ignored alongside `--headless capabilities --pretty`.
    let caps = run_bin(
        &["-psn_0_424242", "--headless", "capabilities", "--pretty"],
        &[],
        None,
    );
    assert_eq!(caps.code, 0, "stderr: {}", caps.stderr);
    let caps_json = caps.parse_stdout_json();
    assert_eq!(caps_json["ok"], true);
    assert_eq!(caps_json["command"], "capabilities");
    for expected_target in [
        "html5",
        "swf",
        "image",
        "png-sequence",
        "aep",
        "fla",
        "xfl",
        "svg",
        "lottie",
        "dotlottie",
        "atlas",
        "video",
        "audio",
    ] {
        assert!(
            caps_json["export_targets"]
                .as_array()
                .expect("export_targets array")
                .iter()
                .any(|v| v == expected_target),
            "missing export target {expected_target} in capabilities"
        );
    }
    for expected_import in [
        "nflash", "fla", "xfl", "swf", "swc", "fig", "svg", "psd", "pdf", "ai", "fxg", "png",
        "wav", "mp4",
    ] {
        assert!(
            caps_json["import_formats"]
                .as_array()
                .expect("import_formats array")
                .iter()
                .any(|v| v == expected_import),
            "missing import format {expected_import} in capabilities"
        );
    }
    assert_eq!(caps_json["exit_codes"]["0"], "success");

    let caps_text = run_bin(
        &["--headless", "--format", "text", "capabilities"],
        &[],
        None,
    );
    assert_eq!(caps_text.code, 0);
    assert!(caps_text.stdout.contains("export_targets="));
    assert!(caps_text.stdout.contains("import_formats="));

    // 4. `headless schema` positional mode and `NEOFLASH_HEADLESS=1` env mode both emit pure JSON.
    let schema = run_bin(&["headless", "schema"], &[], None);
    assert_eq!(schema.code, 0, "stderr: {}", schema.stderr);
    assert_eq!(schema.parse_stdout_json()["command"], "capabilities");

    let env_ver = run_bin(&["version"], &[("NEOFLASH_HEADLESS", "1")], None);
    assert_eq!(env_ver.code, 0, "stderr: {}", env_ver.stderr);
    assert_eq!(env_ver.parse_stdout_json()["command"], "version");

    // 5. Usage errors across subcommands return exit code 2 with valid JSON on stdout and empty stderr.
    for usage_args in [
        &["--headless", "nonexistent_subcommand"][..],
        &["--headless", "--unknown-global-flag", "capabilities"][..],
        &["--headless", "new"][..],
        &["--headless", "import", "--out", "out.nflash"][..],
        &["--headless", "export", "--project", "x.nflash"][..],
        &["--headless", "jsfl", "--project", "x.nflash"][..],
    ] {
        let bad = run_bin(usage_args, &[], None);
        assert_eq!(bad.code, 2, "args={usage_args:?}");
        let bad_err = bad.parse_stdout_json();
        assert_eq!(bad_err["ok"], false);
        assert_eq!(bad_err["exit_code"], 2);
        assert!(bad_err["error"]["code"].is_string());
    }

    // 6. Runtime missing-file error returns exit code 1 with valid JSON error on stdout.
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
    let missing_err = missing.parse_stdout_json();
    assert_eq!(missing_err["ok"], false);
    assert_eq!(missing_err["exit_code"], 1);

    // 7. Explicit `--format text` writes errors to stderr and leaves stdout empty.
    let text_err = run_bin(
        &["--headless", "--format", "text", "nonexistent_subcommand"],
        &[],
        None,
    );
    assert_eq!(text_err.code, 2);
    assert!(text_err.stdout.is_empty());
    assert!(text_err.stderr.contains("Error [unknown_subcommand]"));
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
    let rollback_err = rollback.parse_stdout_json();
    assert_eq!(rollback_err["ok"], false);
    assert_eq!(rollback_err["exit_code"], 1);
    assert_eq!(rollback_err["error"]["code"], "compile_failed");
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
    let rejected_err = rejected.parse_stdout_json();
    assert_eq!(rejected_err["exit_code"], 2);
    assert_eq!(rejected_err["error"]["code"], "invalid_batch_json");
    assert_eq!(fs::read(&proj_path).unwrap(), saved_bytes);

    // 5. Foreign AS3 script with syntax error is skipped cleanly under TypeScript project lock (`R-102`).
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
        compile_with_foreign.stdout
    );
    let compile_json = compile_with_foreign.parse_stdout_json();
    assert_eq!(compile_json["ok"], true);
    assert_eq!(compile_json["effective_script_language"], "ts");
    assert_eq!(compile_json["compiled_scripts_count"], 2);
    assert_eq!(compile_json["skipped_foreign_scripts_count"], 1);
    assert!(
        compile_json["warning_count"].as_u64().unwrap_or(0) >= 2,
        "expected warnings for compatible JS + skipped foreign AS3: {compile_json}"
    );

    // `--strict` upgrades those warnings into exit code 1 with valid JSON on stdout.
    let strict_compile = run_bin(
        &["--headless", "compile", "--project", proj_str, "--strict"],
        &[],
        None,
    );
    assert_eq!(strict_compile.code, 1);
    let strict_err = strict_compile.parse_stdout_json();
    assert_eq!(strict_err["ok"], false);
    assert_eq!(strict_err["exit_code"], 1);
    assert_eq!(strict_err["error"]["code"], "compile_failed");
}

#[test]
fn subprocess_jsfl_traces_dom_flfile_and_in_place_rollback() {
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
    let orig_bytes = fs::read(&proj_path).unwrap();

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
    assert_eq!(denied.parse_stdout_json()["error"]["code"], "jsfl_error");
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
    assert_eq!(traversal.parse_stdout_json()["exit_code"], 1);

    // 3. `--flfile-root` + `--allow-delete` via `--stdin` captures `traces`, mutates DOM, and copies/removes files.
    let script = br#"
        var label = FLfile.read("file:///layer_name.txt");
        FLfile.write("file:///audit.txt", "created:" + label);
        var copied = FLfile.copy("file:///audit.txt", "file:///audit_copy.txt");
        FLfile.write("file:///ephemeral.txt", "temp");
        var removed = FLfile.remove("file:///ephemeral.txt");
        var dom = fl.getDocumentDOM();
        dom.width = 1024;
        dom.getTimeline().addNewLayer(label);
        fl.trace("jsfl_done:" + label);
        fl.trace("copied=" + copied + ",removed=" + removed);
        label;
    "#;
    let allowed = run_bin(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_str,
            "--stdin",
            "--flfile-root",
            sandbox_str,
            "--allow-delete",
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
        allowed_json["traces"],
        json!(["jsfl_done:FromSandboxedJsfl", "copied=true,removed=true"])
    );
    assert_eq!(
        fs::read_to_string(sandbox_dir.join("audit_copy.txt")).unwrap(),
        "created:FromSandboxedJsfl"
    );
    assert!(!sandbox_dir.join("ephemeral.txt").exists());

    // Text mode emits `fl.trace()` lines cleanly on stdout.
    let text_trace = run_bin(
        &[
            "--headless",
            "--format",
            "text",
            "jsfl",
            "--project",
            out_str,
            "--eval",
            "fl.trace('trace_line_alpha:' + fl.getDocumentDOM().width);",
        ],
        &[],
        None,
    );
    assert_eq!(text_trace.code, 0);
    assert!(text_trace.stdout.contains("trace_line_alpha:1024\n"));
    assert!(text_trace.stdout.contains("JSFL ok:"));

    // 4. Uncaught JSFL error with `--in-place` exits 1, emits JSON on stdout, and leaves `.nflash` untouched.
    let failed_inplace = run_bin(
        &[
            "--headless",
            "jsfl",
            "--project",
            proj_str,
            "--in-place",
            "--eval",
            "var d = fl.getDocumentDOM(); d.width = 1920; d.getTimeline().addNewLayer('NeverSaved'); throw new Error('abort_mutation');",
        ],
        &[],
        None,
    );
    assert_eq!(failed_inplace.code, 1);
    let fail_json = failed_inplace.parse_stdout_json();
    assert_eq!(fail_json["ok"], false);
    assert_eq!(fail_json["exit_code"], 1);
    assert_eq!(fail_json["error"]["code"], "jsfl_error");
    assert_eq!(
        fs::read(&proj_path).unwrap(),
        orig_bytes,
        "uncaught JSFL error must not overwrite --in-place project"
    );

    let val = run_bin(&["--headless", "validate", "--project", out_str], &[], None);
    assert_eq!(val.code, 0, "stderr: {}", val.stderr);
}

#[test]
fn subprocess_import_standalone_and_into_across_svg_png_wav_swc_and_xfl() {
    let tmp = TempTestDir::new("import-matrix");

    // 1. Standalone & `--into` SVG import.
    let svg_path = tmp.path.join("icon.svg");
    fs::write(
        &svg_path,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="0 0 120 80">
            <rect x="8" y="8" width="104" height="64" fill="#2266DD"/>
        </svg>"##,
    )
    .unwrap();
    let svg_standalone = tmp.path.join("standalone_svg.nflash");
    let imp_svg = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            svg_path.to_str().unwrap(),
            "--out",
            svg_standalone.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_svg.code, 0);
    let imp_svg_json = imp_svg.parse_stdout_json();
    assert_eq!(imp_svg_json["ok"], true);
    assert!(imp_svg_json["into"].is_null());

    // 2. Standalone & `--into` PNG raster import.
    let png_path = tmp.path.join("sprite.png");
    fs::write(&png_path, make_solid_png_bytes(24, 16, [240, 90, 40, 255])).unwrap();
    let png_standalone = tmp.path.join("standalone_png.nflash");
    let imp_png = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            png_path.to_str().unwrap(),
            "--out",
            png_standalone.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_png.code, 0);
    let imp_png_json = imp_png.parse_stdout_json();
    assert_eq!(imp_png_json["ok"], true);
    assert!(imp_png_json["project"]["resources"].as_u64().unwrap_or(0) >= 1);

    // 3. Standalone & `--into` WAV audio import (verifying FrameSound + audio resource).
    let wav_path = tmp.path.join("chime.wav");
    let pcm_samples: Vec<i16> = (0..441)
        .map(|i| ((i as f32 * 0.25).sin() * 12000.0) as i16)
        .collect();
    fs::write(&wav_path, make_pcm_wav_bytes(22_050, &pcm_samples)).unwrap();
    let wav_standalone = tmp.path.join("standalone_wav.nflash");
    let imp_wav = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            wav_path.to_str().unwrap(),
            "--out",
            wav_standalone.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_wav.code, 0);
    let wav_proj = Project::from_json(&fs::read_to_string(&wav_standalone).unwrap()).unwrap();
    assert!(wav_proj.resources.iter().any(|r| r.is_audio()));
    assert_eq!(wav_proj.symbols[0].timeline.frame_count, 1);
    assert!(
        wav_proj.symbols[0]
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .any(|kf| kf
                .sound
                .as_ref()
                .is_some_and(|sound| sound.sync == neoflash_core::SoundSync::Stream)),
        "standalone WAV playback should follow the timeline even after a user gesture or seek"
    );

    // 4. Standalone & `--into` SWC library import (`link_swc_into_project` + conflict rollback).
    let swc_path = tmp.path.join("hero_ui.swc");
    fs::write(&swc_path, build_synthetic_swc_bytes(&tmp.path)).unwrap();
    let swc_standalone = tmp.path.join("standalone_swc.nflash");
    let imp_swc = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            swc_path.to_str().unwrap(),
            "--out",
            swc_standalone.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_swc.code, 0);
    let imp_swc_json = imp_swc.parse_stdout_json();
    assert!(
        imp_swc_json["swc_link"]["linked_symbols"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );
    assert!(
        imp_swc_json["swc_link"]["linked_sources"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );
    let val_swc = run_bin(
        &[
            "--headless",
            "validate",
            "--project",
            swc_standalone.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(
        val_swc.code, 0,
        "validate standalone SWC: {}",
        val_swc.stdout
    );

    // 5. Export an XFL directory from `svg_standalone`, then test BOTH XFL directory AND `.xfl` marker file!
    let xfl_dir = tmp.path.join("exported_xfl");
    let exp_xfl = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            svg_standalone.to_str().unwrap(),
            "--target",
            "xfl",
            "--out",
            xfl_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_xfl.code, 0);
    let xfl_marker_file = fs::read_dir(&xfl_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("xfl"))
        .expect("exported XFL directory contains <name>.xfl marker file");

    let from_xfl_dir = tmp.path.join("from_xfl_dir.nflash");
    let imp_xfl_dir = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            xfl_dir.to_str().unwrap(),
            "--out",
            from_xfl_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_xfl_dir.code, 0);
    assert_eq!(imp_xfl_dir.parse_stdout_json()["ok"], true);

    let from_xfl_file = tmp.path.join("from_xfl_file.nflash");
    let imp_xfl_file = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            xfl_marker_file.to_str().unwrap(),
            "--out",
            from_xfl_file.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_xfl_file.code, 0);
    assert_eq!(imp_xfl_file.parse_stdout_json()["ok"], true);

    // 6. Merge SVG, PNG, WAV, SWC, and `.xfl` file `--into` an existing project (`--out` and `--in-place`).
    let host_proj = tmp.path.join("composite_host.nflash");
    let mk_host = run_bin(
        &[
            "--headless",
            "new",
            "--name",
            "CompositeHost",
            "--out",
            host_proj.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(mk_host.code, 0);

    let merged_out = tmp.path.join("composite_merged.nflash");
    let merge_first = run_bin(
        &[
            "--headless",
            "import",
            "--into",
            host_proj.to_str().unwrap(),
            "--input",
            svg_path.to_str().unwrap(),
            "--input",
            png_path.to_str().unwrap(),
            "--input",
            wav_path.to_str().unwrap(),
            "--out",
            merged_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(merge_first.code, 0, "stdout: {}", merge_first.stdout);
    let merge_first_json = merge_first.parse_stdout_json();
    assert_eq!(merge_first_json["into"], host_proj.to_str().unwrap());

    let merge_second = run_bin(
        &[
            "--headless",
            "import",
            "--into",
            merged_out.to_str().unwrap(),
            "--in-place",
            "--input",
            swc_path.to_str().unwrap(),
            "--input",
            xfl_marker_file.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(merge_second.code, 0, "stdout: {}", merge_second.stdout);
    let merged_proj = Project::from_json(&fs::read_to_string(&merged_out).unwrap()).unwrap();
    merged_proj.validate().expect("merged project validates");
    assert!(
        merged_proj
            .symbols
            .iter()
            .any(|s| s.class_name.as_deref() == Some("com.example.HeroBadge")),
        "merged project should contain linked SWC HeroBadge symbol"
    );
    assert!(merged_proj.resources.iter().any(|r| r.is_audio()));

    // 7. Atomic rollback on failed `--into --in-place` (both `--swc-conflict error` and corrupt input).
    let snapshot_merged = fs::read(&merged_out).unwrap();
    let swc_conflict = run_bin(
        &[
            "--headless",
            "import",
            "--into",
            merged_out.to_str().unwrap(),
            "--in-place",
            "--swc-conflict",
            "error",
            "--input",
            swc_path.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(swc_conflict.code, 1);
    assert_eq!(swc_conflict.parse_stdout_json()["exit_code"], 1);
    assert_eq!(
        fs::read(&merged_out).unwrap(),
        snapshot_merged,
        "failed SWC conflict import must leave --in-place project byte-identical"
    );
}

#[test]
fn subprocess_export_full_matrix_html5_swf_image_lottie_svg_atlas_aep_fla_xfl() {
    let tmp = TempTestDir::new("export-matrix");
    let runtime_dir = ensure_mock_web_runtime(&tmp.path);
    let runtime_str = runtime_dir.to_str().unwrap();

    // 1. Import synthetic `.figma.json` REST document and validate resulting `.nflash`.
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

    // 2. HTML5 Canvas, DOM (`--render-mode dom`), `--profile "Web App · DOM"`, `--profile Compact`,
    // and `--profile "Web Banner · Native"` (zero-WASM native banner).
    let canvas_dir = tmp.path.join("export_canvas");
    let exp_canvas = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "html5",
            "--profile",
            "Compact",
            "--runtime",
            runtime_str,
            "--out",
            canvas_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_canvas.code, 0, "stdout: {}", exp_canvas.stdout);
    let canvas_json = exp_canvas.parse_stdout_json();
    assert_eq!(canvas_json["render_mode"], "canvas");
    assert_eq!(canvas_json["size_level"], "compact");
    let canvas_html = fs::read_to_string(canvas_dir.join("index.html")).unwrap();
    assert!(!canvas_html.contains("<neoflash-stage"));

    let dom_dir = tmp.path.join("export_dom_profile");
    let exp_dom = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "html5",
            "--profile",
            "Web App · DOM",
            "--runtime",
            runtime_str,
            "--out",
            dom_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_dom.code, 0, "stdout: {}", exp_dom.stdout);
    assert_eq!(exp_dom.parse_stdout_json()["render_mode"], "dom");
    let dom_html = fs::read_to_string(dom_dir.join("index.html")).unwrap();
    assert!(dom_html.contains("<neoflash-stage id=\"stage-dom-root\" data-render-mode=\"dom\""));

    // Build a clean 3-frame animated vector project with a symbol for SWF, Image, Atlas, AEP, and Banner.
    let anim_proj = tmp.path.join("anim_scene.nflash");
    let anim_batch = json!([
        { "op": "new", "name": "AnimScene", "width": 320, "height": 240, "fps": 24, "script_language": "as2" },
        { "op": "add_symbol", "id": "sym_star", "name": "StarBadge", "kind": "movie_clip", "frame_count": 2 },
        {
            "op": "add_element",
            "symbol": "sym_star",
            "id": "star_rect",
            "name": "StarCore",
            "kind": "rectangle",
            "x": 4.0,
            "y": 4.0,
            "width": 40.0,
            "height": 40.0,
            "fill": "#FFAA00"
        },
        { "op": "add_keyframe", "layer": "Layer 1", "frame": 0, "tween": "linear", "tween_kind": "classic" },
        {
            "op": "add_element",
            "id": "card_bg",
            "name": "CardBg",
            "kind": "rectangle",
            "x": 20.0,
            "y": 20.0,
            "width": 120.0,
            "height": 80.0,
            "fill": "#2266DD"
        },
        {
            "op": "add_element",
            "id": "star_inst",
            "name": "StarInstance",
            "kind": "instance",
            "symbol_id": "sym_star",
            "x": 160.0,
            "y": 60.0
        },
        { "op": "add_keyframe", "layer": "Layer 1", "frame": 2, "copy_previous": true },
        {
            "op": "apply_command_steps",
            "layer": "Layer 1",
            "frame": 2,
            "elements": ["card_bg"],
            "steps": [{ "op": "translate", "dx": 40.0, "dy": 20.0 }]
        },
        { "op": "set_script", "frame": 0, "language": "as2", "source": "stop();" }
    ]);
    let mk_anim = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            anim_proj.to_str().unwrap(),
        ],
        &[],
        Some(anim_batch.to_string().as_bytes()),
    );
    assert_eq!(mk_anim.code, 0, "stdout: {}", mk_anim.stdout);

    let native_banner_dir = tmp.path.join("export_native_banner");
    let exp_banner = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            imported_nflash.to_str().unwrap(),
            "--target",
            "html5",
            "--profile",
            "Web Banner · Native",
            "--out",
            native_banner_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_banner.code, 0, "stdout: {}", exp_banner.stdout);
    assert_eq!(
        exp_banner.parse_stdout_json()["profile"],
        "Web Banner · Native"
    );
    assert!(native_banner_dir.join("index.html").is_file());

    let blocked_banner_dir = tmp.path.join("blocked_native_banner");
    let blocked_banner = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "html5",
            "--profile",
            "Web Banner · Native",
            "--out",
            blocked_banner_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(blocked_banner.code, 1);
    assert_eq!(blocked_banner.parse_stdout_json()["exit_code"], 1);
    assert!(!blocked_banner_dir.exists());

    // 3. SWF targets: default for AS2 (`fp8` -> v8), explicit `fp8`, `fp9` (scriptless override on AS2
    //    or AS3 project -> v9), and `fp10-scriptless` (v10).
    let swf_default_as2 = tmp.path.join("default_as2.swf");
    let exp_swf_def = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "swf",
            "--out",
            swf_default_as2.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(exp_swf_def.code, 0, "stdout: {}", exp_swf_def.stdout);
    assert_eq!(fs::read(&swf_default_as2).unwrap()[3], 8);

    let as3_swf_proj = tmp.path.join("as3_swf_src.nflash");
    let as3_batch = json!([
        { "op": "new", "name": "SwfAs3", "width": 320, "height": 240, "fps": 24, "script_language": "as3" },
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
        { "op": "set_script", "frame": 0, "language": "as3", "source": "stop();" }
    ]);
    assert_eq!(
        run_bin(
            &[
                "--headless",
                "batch",
                "--stdin",
                "--out",
                as3_swf_proj.to_str().unwrap(),
            ],
            &[],
            Some(as3_batch.to_string().as_bytes()),
        )
        .code,
        0
    );

    for (proj, swf_target, expected_ver) in [
        (&anim_proj, "fp8", 8_u8),
        (&as3_swf_proj, "fp9", 9_u8),
        (&as3_swf_proj, "fp10", 10_u8),
        (&anim_proj, "fp10-scriptless", 10_u8),
    ] {
        let out_swf = tmp.path.join(format!("target_{swf_target}.swf"));
        let ex = run_bin(
            &[
                "--headless",
                "export",
                "--project",
                proj.to_str().unwrap(),
                "--target",
                "swf",
                "--swf-target",
                swf_target,
                "--out",
                out_swf.to_str().unwrap(),
            ],
            &[],
            None,
        );
        assert_eq!(ex.code, 0, "swf {swf_target} failed: {}", ex.stdout);
        let bytes = fs::read(&out_swf).unwrap();
        assert_eq!(bytes[3], expected_ver, "swf_target={swf_target}");
    }

    // 4. Single-frame / document `image` export (`png`, `webp`, `psd`, `pdf`, `jpeg`, `ai`) + `--frame` + `--scale` + `--symbol`.
    // `--frame 0` must fail with exit code 2 (`--frame` is 1-based for `image`).
    let bad_frame0 = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "image",
            "--frame",
            "0",
            "--out",
            tmp.path.join("bad_frame0.png").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(bad_frame0.code, 2);
    assert_eq!(bad_frame0.parse_stdout_json()["exit_code"], 2);

    let img_png = tmp.path.join("frame2_2x.png");
    let ex_png = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "image",
            "--frame",
            "2",
            "--scale",
            "2.0",
            "--out",
            img_png.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_png.code, 0, "stdout: {}", ex_png.stdout);
    let png_json = ex_png.parse_stdout_json();
    assert_eq!(png_json["frame"], 2);
    assert_eq!(png_json["width"], 640);
    assert_eq!(png_json["height"], 480);

    let sym_webp = tmp.path.join("symbol_star.webp");
    let ex_webp = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "image",
            "--symbol",
            "StarBadge",
            "--out",
            sym_webp.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_webp.code, 0, "stdout: {}", ex_webp.stdout);
    assert_eq!(&fs::read(&sym_webp).unwrap()[..4], b"RIFF");

    let img_psd = tmp.path.join("scene.psd");
    let ex_psd = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "image",
            "--out",
            img_psd.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_psd.code, 0, "stdout: {}", ex_psd.stdout);
    assert_eq!(&fs::read(&img_psd).unwrap()[..4], b"8BPS");

    let img_pdf = tmp.path.join("scene.pdf");
    let ex_pdf = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "image",
            "--out",
            img_pdf.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_pdf.code, 0, "stdout: {}", ex_pdf.stdout);
    assert_eq!(&fs::read(&img_pdf).unwrap()[..5], b"%PDF-");

    let img_jpeg = tmp.path.join("scene.jpeg");
    let sibling_jpg = tmp.path.join("scene.jpg");
    fs::write(&sibling_jpg, b"unrelated existing JPG").unwrap();
    fs::write(&img_jpeg, b"replace this existing JPEG atomically").unwrap();
    let ex_jpeg = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "image",
            "--jpeg-quality",
            "85",
            "--out",
            img_jpeg.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_jpeg.code, 0, "stdout: {}", ex_jpeg.stdout);
    assert!(img_jpeg.is_file(), "exact .jpeg extension preserved");
    assert_eq!(
        fs::read(&sibling_jpg).unwrap(),
        b"unrelated existing JPG",
        "requesting scene.jpeg must not overwrite sibling scene.jpg"
    );
    assert!(
        fs::read(&img_jpeg)
            .unwrap()
            .starts_with(&[0xFF, 0xD8, 0xFF])
    );
    assert!(
        fs::read_dir(&tmp.path).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".neoflash-jpeg-stage-")),
        "temporary JPEG staging directories must be removed"
    );

    // Re-import exported PSD AND import synthetic vector PDF/AI via `--headless import`.
    let reimp_psd_out = tmp.path.join("from_psd.nflash");
    let reimp_psd = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            img_psd.to_str().unwrap(),
            "--out",
            reimp_psd_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(
        reimp_psd.code, 0,
        "reimport psd failed: {}",
        reimp_psd.stdout
    );

    let vector_pdf_bytes = b"%PDF-1.4\n\
        1 0 obj << /Type /Page /MediaBox [0 0 320 240] >> endobj\n\
        2 0 obj << /Length 140 >>\n\
        stream\n\
        q\n\
        0.2 0.4 0.8 rg\n\
        10 10 m 90 10 l 90 70 l 10 70 l h f\n\
        BT /F1 18 Tf 15 30 Td (Vector PDF) Tj ET\n\
        Q\n\
        endstream\n\
        endobj\n";
    for ext in ["pdf", "ai"] {
        let vec_doc = tmp.path.join(format!("vector_art.{ext}"));
        fs::write(&vec_doc, vector_pdf_bytes).unwrap();
        let reimp_out = tmp.path.join(format!("from_{ext}.nflash"));
        let reimp = run_bin(
            &[
                "--headless",
                "import",
                "--input",
                vec_doc.to_str().unwrap(),
                "--out",
                reimp_out.to_str().unwrap(),
            ],
            &[],
            None,
        );
        assert_eq!(reimp.code, 0, "import {ext} failed: {}", reimp.stdout);
    }

    // 5. Export PNG sequence (directory `--out`), Texture Atlas, and After Effects (`aep`).
    let seq_dir = tmp.path.join("seq_frames");
    let ex_seq = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "png-sequence",
            "--frame",
            "0",
            "--end-frame",
            "2",
            "--out",
            seq_dir.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_seq.code, 0, "stdout: {}", ex_seq.stdout);
    assert_eq!(ex_seq.parse_stdout_json()["frame_count"], 3);

    let atlas_json_path = tmp.path.join("sprites.json");
    let ex_atlas = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "atlas",
            "--symbol",
            "StarBadge",
            "--out",
            atlas_json_path.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_atlas.code, 0, "stdout: {}", ex_atlas.stdout);
    assert!(atlas_json_path.is_file());
    assert!(tmp.path.join("sprites.png").is_file());

    let aep_out = tmp.path.join("anim_scene.jsx");
    let ex_aep = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            anim_proj.to_str().unwrap(),
            "--target",
            "aep",
            "--out",
            aep_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(ex_aep.code, 0, "stdout: {}", ex_aep.stdout);
    let aep_json = ex_aep.parse_stdout_json();
    assert!(aep_json["composition_count"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(aep_json["output_path"].as_str(), aep_out.to_str());
    assert!(aep_json["aep_path"].is_null() && aep_json["aepx_path"].is_null());
    assert!(
        fs::read_to_string(&aep_out)
            .unwrap()
            .contains("app.project.save")
    );

    // 6. Export Lottie JSON + HTML preview (`</script>` + `\u2028` escaped), `.lottie` ZIP, FLA, XFL, SVG.
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
    assert_eq!(exp_lottie.code, 0, "stdout: {}", exp_lottie.stdout);
    let preview_html = fs::read_to_string(tmp.path.join("anim.lottie.preview.html"))
        .or_else(|_| fs::read_to_string(tmp.path.join("anim.preview.html")))
        .expect("lottie html preview written");
    assert!(!preview_html.contains("Launch </script>"));
    assert!(!preview_html.contains('\u{2028}'));

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
    assert_eq!(exp_dotlottie.code, 0, "stdout: {}", exp_dotlottie.stdout);
    assert_eq!(&fs::read(&dotlottie_out).unwrap()[..4], b"PK\x03\x04");

    // SWF preflight failure on unmappable U+2028 text returns exit code 1 + valid JSON on stdout.
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
        swf_blocked.parse_stdout_json()["error"]["code"],
        "export_failed"
    );

    let xfl_out = tmp.path.join("roundtrip_xfl");
    let fla_out = tmp.path.join("roundtrip.fla");
    let swf_out = tmp.path.join("roundtrip.swf");
    let svg_out = tmp.path.join("roundtrip.svg");

    for (target, src_p, out_p) in [
        ("xfl", &imported_nflash, &xfl_out),
        ("fla", &imported_nflash, &fla_out),
        ("swf", &as3_swf_proj, &swf_out),
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
        assert_eq!(ex.code, 0, "export {target} failed: {}", ex.stdout);

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
        assert_eq!(reimp.code, 0, "reimport {target} failed: {}", reimp.stdout);

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
            reval.stdout
        );
    }
}

#[test]
fn subprocess_review_regressions_folder_swc_symbol_shadowing_and_bounds() {
    let tmp = TempTestDir::new("review-regressions");
    let mock_runtime = ensure_mock_web_runtime(&tmp.path);

    // 1. Standalone PNG & SVG with `--folder` upgrade `format_version >= 4`, normalize slashes, and validate.
    let png_path = tmp.path.join("sprite.png");
    fs::write(&png_path, make_solid_png_bytes(16, 16, [40, 180, 120, 255])).unwrap();
    let png_folder_out = tmp.path.join("png_folder.nflash");
    let imp_png_folder = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            png_path.to_str().unwrap(),
            "--folder",
            "/Icons/Sprites/",
            "--out",
            png_folder_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_png_folder.code, 0, "stdout: {}", imp_png_folder.stdout);
    let png_proj = Project::from_json(&fs::read_to_string(&png_folder_out).unwrap()).unwrap();
    assert!(png_proj.format_version >= 4);
    assert!(
        png_proj
            .symbols
            .iter()
            .any(|s| s.id != png_proj.root && s.folder == "Icons/Sprites")
    );

    let svg_path = tmp.path.join("hero_icon.svg");
    fs::write(
        &svg_path,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48">
            <g id="inner"><circle cx="24" cy="24" r="18" fill="#EE5533"/></g>
        </svg>"##,
    )
    .unwrap();
    let svg_folder_out = tmp.path.join("svg_folder.nflash");
    let imp_svg_folder = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            svg_path.to_str().unwrap(),
            "--folder",
            "Art/Vectors",
            "--out",
            svg_folder_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(imp_svg_folder.code, 0, "stdout: {}", imp_svg_folder.stdout);
    let val_svg_folder = run_bin(
        &[
            "--headless",
            "validate",
            "--project",
            svg_folder_out.to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(val_svg_folder.code, 0, "stdout: {}", val_svg_folder.stdout);

    // Invalid `--folder` rejected with exit code 2.
    let bad_folder = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            png_path.to_str().unwrap(),
            "--folder",
            "../invalid",
            "--out",
            tmp.path.join("bad_folder.nflash").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(bad_folder.code, 2);
    assert_eq!(bad_folder.parse_stdout_json()["exit_code"], 2);

    // 2. Build a host project with root-level symbol `PreExistingCard` (folder = "") plus symbols
    //    `sprite` and `hero_icon` matching incoming file stems to test `--symbol` shadowing & SWC `--folder`.
    let host_path = tmp.path.join("host.nflash");
    let batch_ops = json!([
        { "op": "new", "name": "HostDoc", "language": "as3" },
        { "op": "add_symbol", "name": "PreExistingCard", "kind": "movie_clip" },
        { "op": "add_symbol", "name": "sprite", "kind": "movie_clip" },
        { "op": "add_symbol", "name": "hero_icon", "kind": "movie_clip" }
    ]);
    let mk_host = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            host_path.to_str().unwrap(),
        ],
        &[],
        Some(serde_json::to_string(&batch_ops).unwrap().as_bytes()),
    );
    assert_eq!(mk_host.code, 0, "stdout: {}", mk_host.stdout);

    // Non-existent `--symbol` matching an incoming file stem on a blank project MUST fail with
    // `symbol_not_found` (exit 1) instead of creating a self-referencing symbol cycle!
    let blank_path = tmp.path.join("blank.nflash");
    assert_eq!(
        run_bin(
            &["--headless", "new", "--out", blank_path.to_str().unwrap()],
            &[],
            None
        )
        .code,
        0
    );
    let self_cycle_guard = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            png_path.to_str().unwrap(),
            "--into",
            blank_path.to_str().unwrap(),
            "--symbol",
            "sprite",
            "--out",
            tmp.path.join("no_self_cycle.nflash").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(self_cycle_guard.code, 1);
    assert_eq!(
        self_cycle_guard.parse_stdout_json()["error"]["code"],
        "symbol_not_found"
    );

    // Importing `sprite.png --symbol sprite` and `hero_icon.svg --symbol hero_icon` into `host`
    // resolves target symbols BEFORE pushing imported symbols (no `ambiguous_symbol` or cycle).
    let imp_into_sprite = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            png_path.to_str().unwrap(),
            "--into",
            host_path.to_str().unwrap(),
            "--symbol",
            "sprite",
            "--in-place",
        ],
        &[],
        None,
    );
    assert_eq!(
        imp_into_sprite.code, 0,
        "stdout: {}",
        imp_into_sprite.stdout
    );

    let imp_into_hero = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            svg_path.to_str().unwrap(),
            "--into",
            host_path.to_str().unwrap(),
            "--symbol",
            "hero_icon",
            "--in-place",
        ],
        &[],
        None,
    );
    assert_eq!(imp_into_hero.code, 0, "stdout: {}", imp_into_hero.stdout);

    // 3. SWC `--into` with `--folder Vendor` preserves `PreExistingCard.folder == ""` while scoping SWC symbols under `Vendor`.
    let swc_path = tmp.path.join("hero_ui.swc");
    fs::write(&swc_path, build_synthetic_swc_bytes(&tmp.path)).unwrap();
    let imp_swc_vendor = run_bin(
        &[
            "--headless",
            "import",
            "--input",
            swc_path.to_str().unwrap(),
            "--into",
            host_path.to_str().unwrap(),
            "--folder",
            "Vendor",
            "--in-place",
        ],
        &[],
        None,
    );
    assert_eq!(imp_swc_vendor.code, 0, "stdout: {}", imp_swc_vendor.stdout);

    let final_host = Project::from_json(&fs::read_to_string(&host_path).unwrap()).unwrap();
    let pre_card = final_host
        .symbols
        .iter()
        .find(|s| s.name == "PreExistingCard")
        .expect("PreExistingCard present");
    assert_eq!(
        pre_card.folder, "",
        "SWC --folder must not move pre-existing host symbols"
    );
    assert!(
        final_host
            .symbols
            .iter()
            .any(|s| s.folder == "Vendor" || s.folder.starts_with("Vendor/")),
        "newly linked SWC symbols should be placed under Vendor"
    );

    // 4. `--audio-policy strip` and `--audio-policy wav16` work on HTML5 export; zero dimensions & inverted frame range exit 2.
    for policy in ["strip", "wav16"] {
        let html_dir = tmp.path.join(format!("html_{policy}"));
        let exp = run_bin(
            &[
                "--headless",
                "export",
                "--project",
                host_path.to_str().unwrap(),
                "--target",
                "html5",
                "--runtime",
                mock_runtime.to_str().unwrap(),
                "--audio-policy",
                policy,
                "--out",
                html_dir.to_str().unwrap(),
            ],
            &[],
            None,
        );
        assert_eq!(exp.code, 0, "audio-policy {policy}: {}", exp.stdout);
    }

    let zero_w = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            host_path.to_str().unwrap(),
            "--target",
            "image",
            "--width",
            "0",
            "--out",
            tmp.path.join("zero.png").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(zero_w.code, 2);

    let inv_range = run_bin(
        &[
            "--headless",
            "export",
            "--project",
            host_path.to_str().unwrap(),
            "--target",
            "lottie",
            "--frame",
            "4",
            "--end-frame",
            "2",
            "--out",
            tmp.path.join("inv.json").to_str().unwrap(),
        ],
        &[],
        None,
    );
    assert_eq!(inv_range.code, 2);

    // 5. `--flfile-root` on a brand-new nonexistent directory + `FLfile.copy` on a folder returns `false`.
    let fresh_flroot = tmp.path.join("fresh_flroot_dir");
    assert!(!fresh_flroot.exists());
    let jsfl_res = run_bin(
        &[
            "--headless",
            "jsfl",
            "--project",
            host_path.to_str().unwrap(),
            "--flfile-root",
            fresh_flroot.to_str().unwrap(),
            "--eval",
            "FLfile.createFolder('file:///sub'); FLfile.copy('file:///sub', 'file:///sub2');",
        ],
        &[],
        None,
    );
    assert_eq!(jsfl_res.code, 0, "stdout: {}", jsfl_res.stdout);
    assert_eq!(jsfl_res.parse_stdout_json()["return_value"], false);
    assert!(fresh_flroot.join("sub").is_dir());

    // 6. v1 blank `batch` (`add_symbol` with `folder` + `add_element` with `skew` + frame `set_script` + `validate`)
    //    and `jsfl` adding a library folder on a v1 `Project::blank()` both upgrade `format_version` cleanly.
    let v1_batch_out = tmp.path.join("v1_batch.nflash");
    let v1_batch_ops = json!([
        { "op": "add_symbol", "name": "BadgeCard", "kind": "movie_clip", "folder": "/Library/Badges/" },
        { "op": "add_element", "kind": "rect", "x": 8.0, "y": 8.0, "width": 32.0, "height": 32.0, "skew": 10.0 },
        { "op": "set_script", "frame": 0, "language": "as2", "source": "trace('batch_v1_to_v5');" },
        { "op": "validate" }
    ]);
    let v1_batch_res = run_bin(
        &[
            "--headless",
            "batch",
            "--stdin",
            "--out",
            v1_batch_out.to_str().unwrap(),
        ],
        &[],
        Some(serde_json::to_string(&v1_batch_ops).unwrap().as_bytes()),
    );
    assert_eq!(v1_batch_res.code, 0, "stdout: {}", v1_batch_res.stdout);
    let v1_batch_proj = Project::from_json(&fs::read_to_string(&v1_batch_out).unwrap()).unwrap();
    assert_eq!(v1_batch_proj.format_version, 5);

    let scene_folder_err = run_bin(
        &["--headless", "batch", "--stdin"],
        &[],
        Some(
            serde_json::to_string(&json!([
                { "op": "add_symbol", "name": "Scene 2", "as_scene": true, "folder": "FolderNotAllowed" }
            ]))
            .unwrap()
            .as_bytes(),
        ),
    );
    assert_eq!(scene_folder_err.code, 2);

    let v1_jsfl_path = tmp.path.join("v1_blank_jsfl.nflash");
    fs::write(&v1_jsfl_path, Project::blank().to_json().unwrap()).unwrap();
    let v1_jsfl_res = run_bin(
        &[
            "--headless",
            "jsfl",
            "--project",
            v1_jsfl_path.to_str().unwrap(),
            "--in-place",
            "--eval",
            "fl.getDocumentDOM().library.newFolder('Icons');",
        ],
        &[],
        None,
    );
    assert_eq!(v1_jsfl_res.code, 0, "stdout: {}", v1_jsfl_res.stdout);
    let v1_jsfl_proj = Project::from_json(&fs::read_to_string(&v1_jsfl_path).unwrap()).unwrap();
    assert_eq!(v1_jsfl_proj.format_version, 4);
}
