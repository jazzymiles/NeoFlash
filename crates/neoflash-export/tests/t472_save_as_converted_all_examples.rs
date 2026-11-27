//! `[T-472]` Track D — End-to-end `Save As (converted)` hardening across all 18
//! `examples/as2/*` + `examples/as3/*` reference projects and 6 arcade games (`48` conversions).

use neoflash_core::{AuthoredPlayback, Project};
use neoflash_export::{
    AudioExportPolicy, FontExportPolicy, PublishOptions, SizeLevel, export_project_with_options,
};
use neoflash_script::{
    ConversionIssueKind, ConversionTarget, check_project, check_project_warnings, compile_project,
    convert_project_scripts,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const AS2_PROJECTS: &[&str] = &[
    "examples/as2/00-child-module.nflash",
    "examples/as2/01-math-geometry.nflash",
    "examples/as2/02-oop-xml-collections.nflash",
    "examples/as2/03-movieclips-nested-timelines.nflash",
    "examples/as2/04-events-stage-align-drag.nflash",
    "examples/as2/05-drawing-api-filters-bitmapdata.nflash",
    "examples/as2/06-dynamic-input-html-text.nflash",
    "examples/as2/07-sound-video-loadpublish.nflash",
    "examples/as2/08-interactive-arcade-miniapp.nflash",
    "examples/games/snake-as2.nflash",
    "examples/games/arkanoid-as2.nflash",
    "examples/games/tetris-as2.nflash",
];

const AS3_PROJECTS: &[&str] = &[
    "examples/as3/00-child-module.nflash",
    "examples/as3/01-math-geometry.nflash",
    "examples/as3/02-oop-xml-collections.nflash",
    "examples/as3/03-movieclips-nested-timelines.nflash",
    "examples/as3/04-events-stage-align-drag.nflash",
    "examples/as3/05-drawing-api-filters-bitmapdata.nflash",
    "examples/as3/06-dynamic-input-html-text.nflash",
    "examples/as3/07-sound-video-loadpublish.nflash",
    "examples/as3/08-interactive-arcade-miniapp.nflash",
    "examples/games/snake-as3.nflash",
    "examples/games/arkanoid-as3.nflash",
    "examples/games/tetris-as3.nflash",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn temp_test_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let dir = std::env::temp_dir().join(format!(
        "neoflash-t472-convert-{label}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

fn write_mock_wasm_runtime(runtime_dir: &Path) {
    fs::create_dir_all(runtime_dir).expect("create mock runtime dir");
    fs::write(runtime_dir.join("neoflash_web.js"), "var NeoFlashWasm;\n")
        .expect("write neoflash_web.js");
    fs::write(runtime_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0")
        .expect("write neoflash_web_bg.wasm");
}

fn ensure_web_runtime_present(root: &Path) {
    let dst = root.join("target/web-runtime");
    if dst.join("neoflash_web.js").is_file() && dst.join("neoflash_web_bg.wasm").is_file() {
        return;
    }
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output()
        && out.status.success()
    {
        let common_dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if let Some(main_root) = common_dir.parent() {
            let src = main_root.join("target/web-runtime");
            if src.join("neoflash_web.js").is_file() && src.join("neoflash_web_bg.wasm").is_file() {
                fs::create_dir_all(&dst).expect("create target/web-runtime");
                for name in ["neoflash_web.js", "neoflash_web_bg.wasm"] {
                    fs::copy(src.join(name), dst.join(name)).expect("copy web-runtime artifact");
                }
            }
        }
    }
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

#[derive(Serialize)]
struct ExportedConversionSpec {
    idx: usize,
    rel_path: String,
    target: String,
    bundle_path: String,
    has_manual_attention: bool,
}

#[test]
fn t472_all_48_example_and_arcade_conversions_validate_compile_export_and_execute() {
    let root = workspace_root();
    ensure_web_runtime_present(&root);
    let temp_dir = temp_test_dir("all-48");
    let runtime_dir = temp_dir.join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    let publish_options = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };

    let mut cases: Vec<(&str, ConversionTarget)> = Vec::with_capacity(48);
    for &p in AS2_PROJECTS {
        cases.push((p, ConversionTarget::ActionScript3));
        cases.push((p, ConversionTarget::JavaScript));
    }
    for &p in AS3_PROJECTS {
        cases.push((p, ConversionTarget::ActionScript2));
        cases.push((p, ConversionTarget::JavaScript));
    }
    assert_eq!(cases.len(), 48);

    let mut failures = Vec::new();
    let mut exported_specs = Vec::with_capacity(48);

    for (idx, (rel_path, target)) in cases.iter().enumerate() {
        let label = format!("{rel_path} -> {target:?}");
        let full_path = root.join(rel_path);
        let raw_bytes = fs::read(&full_path).unwrap_or_else(|e| panic!("read {rel_path}: {e}"));
        let source_project = Project::from_nflash_bytes(&raw_bytes)
            .unwrap_or_else(|e| panic!("parse {rel_path}: {e}"));

        let (converted, report) = match convert_project_scripts(&source_project, *target) {
            Ok(pair) => pair,
            Err(err) => {
                failures.push(format!(
                    "[{idx}] {label}: convert_project_scripts failed: {err}"
                ));
                continue;
            }
        };

        if let Err(err) = converted.validate() {
            failures.push(format!("[{idx}] {label}: validate() failed: {err}"));
            continue;
        }
        assert_eq!(converted.script_language, Some(target.script_language()));

        if let Err(diags) = check_project(&converted) {
            let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
            failures.push(format!(
                "[{idx}] {label}: check_project failed:\n  {}\n--- SOURCES ---\n{}",
                msgs.join("\n  "),
                dump_scripts(&converted)
            ));
            continue;
        }

        if let Err(diags) = compile_project(&converted) {
            let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
            failures.push(format!(
                "[{idx}] {label}: compile_project failed:\n  {}",
                msgs.join("\n  ")
            ));
            continue;
        }

        let warnings = check_project_warnings(&converted);
        let unsupported: Vec<_> = warnings
            .iter()
            .filter(|w| w.code == "unsupported-feature")
            .collect();
        if !unsupported.is_empty() {
            failures.push(format!(
                "[{idx}] {label}: unsupported-feature warnings: {unsupported:?}\n--- SOURCES ---\n{}",
                dump_scripts(&converted)
            ));
            continue;
        }

        let out_dir = temp_dir.join(format!("bundle-{idx}"));
        if let Err(err) =
            export_project_with_options(&converted, &out_dir, &runtime_dir, &publish_options)
        {
            failures.push(format!(
                "[{idx}] {label}: export_project_with_options failed: {err}"
            ));
            continue;
        }
        assert!(out_dir.join("index.html").is_file());
        assert!(out_dir.join("bundle.js").is_file());
        assert!(out_dir.join("module.mjs").is_file());

        // Multi-frame AuthoredPlayback stage evaluation check
        let mut playback =
            AuthoredPlayback::with_live_scripts(&converted, &converted.root, 0, true);
        for _ in 0..6 {
            let scene = playback.scene(&converted, false, true);
            let _ = scene.draws.len();
            playback.tick(&converted);
        }

        let has_manual_attention = report
            .issues
            .iter()
            .any(|i| i.kind == ConversionIssueKind::ManualAttention);

        exported_specs.push(ExportedConversionSpec {
            idx,
            rel_path: (*rel_path).to_owned(),
            target: format!("{target:?}"),
            bundle_path: out_dir.join("bundle.js").display().to_string(),
            has_manual_attention,
        });
    }

    assert!(
        failures.is_empty(),
        "{}/48 conversions failed static/export checks:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
    assert_eq!(exported_specs.len(), 48);

    // Execute ALL 48 exported bundles headlessly in Node + WebPlayer WASM runtime!
    let runner_path = temp_dir.join("run-all-48-converted.mjs");
    let host_integration_url =
        node_module_url(&root.join("crates/neoflash-script/tests/host-integration.mjs"));
    let rust_runtime_url =
        node_module_url(&root.join("crates/neoflash-script/tests/rust-runtime-host.mjs"));

    let runner_source = format!(
        r#"import assert from 'node:assert/strict';
import fs from 'node:fs';
import {{ createRuntimeHost }} from {host_integration_url:?};
import {{ runtimeWasm }} from {rust_runtime_url:?};

const root = {root_str:?};
const specs = {specs_json};

function makeMemoryStorage() {{
  const map = new Map();
  return {{
    getItem(k) {{ return map.has(k) ? map.get(k) : null; }},
    setItem(k, v) {{ map.set(String(k), String(v)); }},
    removeItem(k) {{ map.delete(String(k)); }},
    get length() {{ return map.size; }},
    key(i) {{ return Array.from(map.keys())[i] ?? null; }},
  }};
}}

function makeAudioContext() {{
  const ctx = {{
    state: 'running',
    sampleRate: 22050,
    currentTime: 0,
    destination: {{ kind: 'destination' }},
    resume() {{ this.state = 'running'; return Promise.resolve(); }},
    close() {{ this.state = 'closed'; return Promise.resolve(); }},
    decodeAudioData() {{ return Promise.resolve({{ numberOfChannels: 1, sampleRate: 22050, length: 2205, duration: 0.1 }}); }},
    createGain() {{ return {{ kind: 'gain', gain: {{ value: 1 }}, connect() {{}}, disconnect() {{}} }}; }},
    createStereoPanner() {{ return {{ kind: 'pan', pan: {{ value: 0 }}, connect() {{}}, disconnect() {{}} }}; }},
    createBufferSource() {{
      return {{ buffer: null, loop: false, loopStart: 0, loopEnd: 0, onended: null, connect() {{}}, disconnect() {{}}, start() {{}}, stop() {{}} }};
    }},
  }};
  return ctx;
}}

function loadBundle(bundlePath) {{
  const text = fs.readFileSync(bundlePath, 'utf8');
  const jsonText = text.replace(/^globalThis\.NeoFlashBundle\s*=\s*/, '').replace(/;\s*$/, '');
  const bundle = JSON.parse(jsonText);
  const player = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
  const compiled = bundle.scripts.map((s, idx) => ({{
    script: {{ language: s.language, source: s.source }},
    code: s.code,
    location: s.location || `script-${{idx}}`,
  }}));
  const env = createRuntimeHost(root, player, compiled, {{
    bundle,
    project: bundle.project,
    context: {{
      font: '16px sans-serif',
      measureText: line => ({{ width: String(line).length * 8, actualBoundingBoxAscent: 12, actualBoundingBoxDescent: 4 }}),
    }},
    storage: {{
      url: 'https://neoflash.test/app/index.html',
      storage: makeMemoryStorage(),
    }},
    audio: {{
      createContext: makeAudioContext,
      isUserGesture: () => true,
    }},
    net: {{
      baseURL: 'https://neoflash.test/app/index.html',
      fetch: () => new Promise(() => {{}}),
    }},
    video: {{
      createVideo: () => ({{
        pause() {{}},
        load() {{}},
        play() {{ return Promise.resolve(); }},
        addEventListener() {{}},
        removeEventListener() {{}},
        removeAttribute() {{}},
        videoWidth: 320,
        videoHeight: 180,
        readyState: 0,
        currentTime: 0,
        duration: 0,
        style: {{}},
      }}),
    }},
  }});
  env.host.resetProject();
  return {{ clip: env.host.proxyFor([]), env, player, bundle }};
}}

const runtimeFailures = [];

for (const spec of specs) {{
  const tag = `[${{spec.idx}}] ${{spec.rel_path}} -> ${{spec.target}}`;
  let activeEnv = null;
  let activePlayer = null;
  try {{
    const loaded = loadBundle(spec.bundle_path);
    const {{ clip, env }} = loaded;
    activeEnv = env;
    activePlayer = loaded.player;
    const traces = () => env.traceCalls.map(args => args.map(String).join(' '));

    // Advance 16 frames across every single converted project to exercise enterFrame / timeline ticks.
    for (let f = 0; f < 16; f++) {{
      env.host.broadcastFrame();
    }}

    if (spec.rel_path.includes('snake-')) {{
      clip.resetGame();
      assert.equal(clip.scoreText.text, 'SCORE: 0', `${{tag}} initial score HUD`);
      clip.setDirection(-1, 0);
      for (let i = 0; i < 30; i++) {{
        clip.stepSnake();
      }}
      assert.match(String(clip.statusText.text), /GAME OVER/, `${{tag}} wall collision GAME OVER`);
      clip.resetGame();
      assert.equal(clip.scoreText.text, 'SCORE: 0', `${{tag}} reset score HUD`);
    }} else if (spec.rel_path.includes('arkanoid-')) {{
      clip.resetGame();
      assert.equal(clip.scoreText.text, 'SCORE: 0', `${{tag}} initial score HUD`);
      assert.equal(clip.livesText.text, 'LIVES: 3', `${{tag}} initial lives HUD`);
      clip.movePaddle(-24);
      clip.stepGame();
      clip.resetGame();
      assert.equal(clip.livesText.text, 'LIVES: 3', `${{tag}} reset lives HUD`);
    }} else if (spec.rel_path.includes('tetris-')) {{
      clip.resetGame();
      assert.equal(clip.scoreText.text, 'SCORE: 0', `${{tag}} initial score HUD`);
      assert.equal(clip.linesText.text, 'LINES: 0', `${{tag}} initial lines HUD`);
      assert.equal(clip.moveHorizontal(-1), true, `${{tag}} moveHorizontal(-1)`);
      assert.equal(clip.moveHorizontal(1), true, `${{tag}} moveHorizontal(1)`);
      assert.equal(clip.rotatePiece(), true, `${{tag}} rotatePiece()`);
      assert.ok(clip.hardDrop() > 0, `${{tag}} hardDrop()`);
      clip.resetGame();
      assert.equal(clip.scoreText.text, 'SCORE: 0', `${{tag}} reset score HUD`);
    }} else if (spec.rel_path.includes('/00-child-module')) {{
      assert.match(String(clip.badgeLabel.text), /MODULE LIVE/, `${{tag}} child module badgeLabel`);
      assert.ok(traces().some(t => t.includes('[PASS]')), `${{tag}} expected [PASS] trace, got ${{JSON.stringify(traces())}}`);
    }} else if (!spec.rel_path.includes('/07-sound-video-loadpublish') && !spec.has_manual_attention) {{
      assert.ok(
        traces().some(t => t.includes('[PASS]')),
        `${{tag}} expected [PASS] trace, statusText=${{clip.statusText?.text}}, detailText=${{clip.detailText?.text}}, traces=${{JSON.stringify(traces())}}`
      );
    }}
  }} catch (err) {{
    runtimeFailures.push(`${{tag}}: ${{err && err.stack ? err.stack : err}}`);
  }} finally {{
    try {{ activeEnv?.host?.disposeFamily?.(); }} catch (_) {{}}
    try {{ activePlayer?.free?.(); }} catch (_) {{}}
  }}
}}

if (runtimeFailures.length > 0) {{
  console.error(`${{runtimeFailures.length}}/48 runtime executions failed:\n\n` + runtimeFailures.join('\n\n'));
  process.exit(1);
}}
console.log(`ALL 48 CONVERTED BUNDLES EXECUTED CLEANLY IN WASM+JS HOST`);
"#,
        host_integration_url = host_integration_url,
        rust_runtime_url = rust_runtime_url,
        root_str = root.display().to_string(),
        specs_json = serde_json::to_string(&exported_specs).unwrap(),
    );

    fs::write(&runner_path, runner_source).expect("write node runner");
    let output = Command::new(node_binary())
        .arg(&runner_path)
        .current_dir(&root)
        .output()
        .expect("run node on 48 converted bundles");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&temp_dir);

    assert!(
        output.status.success(),
        "Live WASM+Node runtime execution failed on converted bundles!\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
    );
    assert!(stdout.contains("ALL 48 CONVERTED BUNDLES EXECUTED CLEANLY IN WASM+JS HOST"));
}

fn dump_scripts(project: &Project) -> String {
    let mut out = String::new();
    for s in &project.sources {
        out.push_str(&format!(
            "// [source {} ({:?})]\n{}\n",
            s.path, s.language, s.source
        ));
    }
    for sym in &project.symbols {
        for fs in &sym.timeline.scripts {
            out.push_str(&format!(
                "// [{} frame {} ({:?})]\n{}\n",
                sym.name, fs.frame, fs.script.language, fs.script.source
            ));
        }
    }
    out
}

#[test]
fn t472_a03_as3_document_class_to_as2_and_roundtrips_execute_in_wasm_host() {
    let root = workspace_root();
    ensure_web_runtime_present(&root);
    let temp_dir = temp_test_dir("a03-doc-class");

    let mut as3_proj = Project::blank();
    as3_proj.name = "DocClassRoundtrip".to_owned();
    as3_proj.script_language = Some(neoflash_core::ScriptLanguage::ActionScript3);
    as3_proj.document_class = Some("app.Main".to_owned());
    as3_proj.sources = vec![neoflash_core::SourceUnit {
        id: "src_main".to_owned(),
        path: "app/Main.as".to_owned(),
        language: neoflash_core::ScriptLanguage::ActionScript3,
        source: concat!(
            "package app {\n",
            "    import flash.display.MovieClip;\n",
            "    public class Main extends MovieClip {\n",
            "        public var score:Number = 0;\n",
            "        public function Main() {\n",
            "            super();\n",
            "            this.score = 42;\n",
            "            trace(\"[PASS] Main constructed score=\" + this.score);\n",
            "        }\n",
            "        public function ping():String {\n",
            "            return \"pong:\" + this.score;\n",
            "        }\n",
            "    }\n",
            "}\n"
        )
        .to_owned(),
    }];
    as3_proj.upgrade_format_for_features();
    as3_proj
        .validate()
        .expect("valid AS3 document_class project");

    // 1. AS3 -> AS2
    let (p_as2, _) =
        convert_project_scripts(&as3_proj, ConversionTarget::ActionScript2).expect("AS3 -> AS2");
    assert_eq!(p_as2.document_class.as_deref(), Some("app.Main"));
    let root_as2 = p_as2.symbol(&p_as2.root).unwrap();
    assert!(root_as2.class_name.is_none());
    assert!(
        root_as2.timeline.scripts.is_empty(),
        "unused _documentApp alias must not be injected on Frame 0"
    );
    check_project(&p_as2).expect("check AS2 converted project");

    // 2. AS3 -> AS2 -> JS
    let (p_js, _) =
        convert_project_scripts(&p_as2, ConversionTarget::JavaScript).expect("AS2 -> JS");
    assert!(p_js.document_class.is_none());
    check_project(&p_js).expect("check AS3->AS2->JS converted project");

    // 3. AS3 -> AS2 -> AS3
    let (p_as3_rt, _) =
        convert_project_scripts(&p_as2, ConversionTarget::ActionScript3).expect("AS2 -> AS3");
    assert_eq!(p_as3_rt.document_class.as_deref(), Some("app.Main"));
    check_project(&p_as3_rt).expect("check AS3->AS2->AS3 converted project");

    let runtime_dir = temp_dir.join("runtime");
    write_mock_wasm_runtime(&runtime_dir);
    let publish_opts = PublishOptions::default();

    let mut bundle_paths = Vec::new();
    for (label, proj) in [("as2", &p_as2), ("js", &p_js), ("as3_rt", &p_as3_rt)] {
        let out_dir = temp_dir.join(format!("pub-{label}"));
        export_project_with_options(proj, &out_dir, &runtime_dir, &publish_opts)
            .unwrap_or_else(|e| panic!("export {label} failed: {e}"));
        bundle_paths.push((label.to_owned(), out_dir.join("bundle.js")));
    }

    let host_integration_url =
        node_module_url(&root.join("crates/neoflash-script/tests/host-integration.mjs"));
    let rust_runtime_url =
        node_module_url(&root.join("crates/neoflash-script/tests/rust-runtime-host.mjs"));
    let runner_path = temp_dir.join("run-a03.mjs");
    let specs_json = serde_json::to_string(&bundle_paths).unwrap();
    let runner_src = format!(
        r#"import assert from 'node:assert/strict';
import fs from 'node:fs';
import {{ createRuntimeHost }} from {host_integration_url:?};
import {{ runtimeWasm }} from {rust_runtime_url:?};

const root = {root_str:?};
const specs = {specs_json};

for (const [label, bundlePath] of specs) {{
  const text = fs.readFileSync(bundlePath, 'utf8');
  const jsonText = text.replace(/^globalThis\.NeoFlashBundle\s*=\s*/, '').replace(/;\s*$/, '');
  const bundle = JSON.parse(jsonText);
  const player = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
  const compiled = bundle.scripts.map((s, idx) => ({{
    script: {{ language: s.language, source: s.source }},
    code: s.code,
    location: s.location || `script-${{idx}}`,
  }}));
  const env = createRuntimeHost(root, player, compiled, {{ bundle, project: bundle.project }});
  env.host.resetProject();
  const clip = env.host.proxyFor([]);
  const traces = env.traceCalls.map(args => args.map(String).join(' '));
  assert.ok(
    traces.some(t => t.includes('[PASS] Main constructed score=42')),
    `[${{label}}] expected [PASS] trace, got ${{JSON.stringify(traces)}}`
  );
  assert.equal(clip.ping(), 'pong:42', `[${{label}}] ping() method on constructed document_class`);
}}
console.log('A03_DOC_CLASS_ROUNDTRIP_OK');
"#,
        host_integration_url = host_integration_url,
        rust_runtime_url = rust_runtime_url,
        root_str = root.display().to_string(),
        specs_json = specs_json,
    );
    fs::write(&runner_path, runner_src).expect("write a03 runner");
    let output = Command::new(node_binary())
        .arg(&runner_path)
        .current_dir(&root)
        .output()
        .expect("run node a03");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&temp_dir);
    assert!(
        output.status.success() && stdout.contains("A03_DOC_CLASS_ROUNDTRIP_OK"),
        "A-03 WASM+Node roundtrip failed!\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
    );
}
