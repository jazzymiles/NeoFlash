use neoflash_core::Project;
use neoflash_export::{
    AudioExportPolicy, FontExportPolicy, PublishOptions, SizeLevel, export_project_with_options,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const GAME_FILES: &[&str] = &[
    "examples/snake-as2.nflash",
    "examples/snake-as3.nflash",
    "examples/arkanoid-as2.nflash",
    "examples/arkanoid-as3.nflash",
    "examples/tetris-as2.nflash",
    "examples/tetris-as3.nflash",
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
        "neoflash-games-export-{}-{}-{}",
        label,
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

#[test]
fn all_six_games_validate_check_compile_export_and_execute() {
    let root = workspace_root();
    let temp_dir = temp_test_dir("all-six");
    let runtime_dir = temp_dir.join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    let mut exported_bundles = Vec::new();

    for rel_path in GAME_FILES {
        let full_path = root.join(rel_path);
        let raw = fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("failed to read {rel_path}: {e}"));
        let project: Project = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("failed to parse {rel_path} as Project: {e}"));

        // 1. Validate project model invariants.
        if let Err(validation_err) = project.validate() {
            panic!("{rel_path} failed project.validate(): {validation_err}");
        }

        // 2. Check all project scripts (compile + warnings check with zero errors/unsupported features).
        let program = neoflash_script::check_project(&project).unwrap_or_else(|diags| {
            let formatted: Vec<String> = diags.iter().map(ToString::to_string).collect();
            panic!(
                "{rel_path} failed neoflash_script::check_project:\n{}",
                formatted.join("\n")
            );
        });
        assert!(
            program.class_names.is_empty() || !program.code.is_empty(),
            "{rel_path} produced invalid ProjectProgram"
        );

        let warnings = neoflash_script::check_project_warnings(&project);
        let unsupported: Vec<_> = warnings
            .iter()
            .filter(|w| w.code == "unsupported-feature")
            .collect();
        assert!(
            unsupported.is_empty(),
            "{rel_path} reported unsupported-feature warnings: {unsupported:?}"
        );

        // 3. Export HTML5 bundle with export_project_with_options.
        let slug = Path::new(rel_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("game");
        let out_dir = temp_dir.join(slug);
        let options = PublishOptions {
            fonts: FontExportPolicy::Device,
            size: SizeLevel::Full,
            audio: AudioExportPolicy::Original,
        };
        let index_path = export_project_with_options(&project, &out_dir, &runtime_dir, &options)
            .unwrap_or_else(|e| panic!("{rel_path} failed HTML5 export: {e}"));

        assert!(index_path.exists(), "{rel_path} missing index.html");
        assert!(
            out_dir.join("bundle.js").exists(),
            "{rel_path} missing bundle.js"
        );
        assert!(
            out_dir.join("runtime.js").exists(),
            "{rel_path} missing runtime.js"
        );
        assert!(
            out_dir.join("neoflash_web.js").exists(),
            "{rel_path} missing neoflash_web.js"
        );

        let bundle_js =
            fs::read_to_string(out_dir.join("bundle.js")).expect("read exported bundle.js");
        assert!(
            bundle_js.starts_with("globalThis.NeoFlashBundle = "),
            "{rel_path} bundle.js does not assign globalThis.NeoFlashBundle"
        );

        exported_bundles.push((slug.to_string(), out_dir.join("bundle.js")));
    }

    // 4. Execute the exported game bundles headlessly in Node via the NeoFlash runtime host.
    let runner_path = temp_dir.join("run-games-check.mjs");
    let host_integration_url = root
        .join("crates/neoflash-script/tests/host-integration.mjs")
        .display()
        .to_string();
    let rust_runtime_url = root
        .join("crates/neoflash-script/tests/rust-runtime-host.mjs")
        .display()
        .to_string();

    let runner_source = format!(
        r#"import assert from 'node:assert/strict';
import fs from 'node:fs';
import {{ createRuntimeHost }} from {host_integration_url:?};
import {{ runtimeWasm }} from {rust_runtime_url:?};

const root = {root_str:?};
const bundles = {bundles_json};

function loadExportedGame(bundlePath) {{
  const text = fs.readFileSync(bundlePath, 'utf8');
  const jsonText = text.replace(/^globalThis\.NeoFlashBundle\s*=\s*/, '').replace(/;\s*$/, '');
  const bundle = JSON.parse(jsonText);
  assert.ok(Array.isArray(bundle.scripts) && bundle.scripts.length > 0, `No compiled scripts in ${{bundlePath}}`);

  const player = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
  const compiled = bundle.scripts.map((s, idx) => ({{
    script: {{ language: s.language, source: s.source }},
    code: s.code,
    location: s.location || `script-${{idx}}`,
  }}));

  const env = createRuntimeHost(root, player, compiled, {{ bundle, project: bundle.project }});
  env.host.resetProject();
  return {{ clip: env.host.proxyFor([]), env, player }};
}}

for (const [slug, bundlePath] of bundles) {{
  const {{ clip, env }} = loadExportedGame(bundlePath);

  if (slug.startsWith('snake-')) {{
    assert.equal(clip.score, 0, `${{slug}} initial score`);
    assert.equal(clip.snakeX.length, 3, `${{slug}} initial snake length`);
    assert.equal(clip.gameOver, false, `${{slug}} initial gameOver`);
    assert.equal(clip.scoreText.text, 'SCORE: 0', `${{slug}} initial score HUD`);

    // 180-degree reversal must be ignored when moving right (+1, 0).
    clip.setDirection(-1, 0);
    assert.equal(clip.nextDirX, 1, `${{slug}} prevented 180-degree reversal`);

    // Place food directly ahead of the head and step once.
    clip.foodX = clip.snakeX[0] + 1;
    clip.foodY = clip.snakeY[0];
    const stepped = clip.stepSnake();
    assert.equal(stepped, true, `${{slug}} stepSnake succeeded`);
    assert.equal(clip.score, 10, `${{slug}} score incremented after eating food`);
    assert.equal(clip.highScore, 10, `${{slug}} highScore updated`);
    assert.equal(clip.snakeX.length, 4, `${{slug}} snake grew to 4 segments`);
    assert.equal(clip.scoreText.text, 'SCORE: 10', `${{slug}} scoreText updated`);

    // Step until hitting right wall -> game over.
    for (let i = 0; i < 25 && !clip.gameOver; i++) {{
      clip.stepSnake();
    }}
    assert.equal(clip.gameOver, true, `${{slug}} wall collision sets gameOver`);
    assert.match(clip.statusText.text, /GAME OVER/, `${{slug}} statusText shows GAME OVER`);

    // Reset restores initial state.
    clip.resetGame();
    assert.equal(clip.gameOver, false, `${{slug}} reset clears gameOver`);
    assert.equal(clip.score, 0, `${{slug}} reset clears score`);
    assert.equal(clip.snakeX.length, 3, `${{slug}} reset restores length 3`);
  }} else if (slug.startsWith('arkanoid-')) {{
    assert.equal(clip.score, 0, `${{slug}} initial score`);
    assert.equal(clip.lives, 3, `${{slug}} initial lives`);
    assert.equal(clip.bricksRemaining, 24, `${{slug}} initial bricksRemaining`);
    assert.equal(clip.livesText.text, 'LIVES: 3', `${{slug}} initial lives HUD`);

    // Move paddle left and verify clamping/movement.
    const startPaddleX = clip.paddleX;
    clip.movePaddle(-24);
    assert.equal(clip.paddleX, startPaddleX - 24, `${{slug}} movePaddle moved left`);

    // Aim ball directly at brick 0 at (110, 120) and step once.
    clip.ballX = 110;
    clip.ballY = 126;
    clip.ballVX = 0;
    clip.ballVY = -6;
    clip.stepGame();
    assert.equal(clip.bricksAlive[0], false, `${{slug}} brick0 destroyed`);
    assert.equal(clip.bricksRemaining, 23, `${{slug}} bricksRemaining decremented`);
    assert.equal(clip.score, 40, `${{slug}} row 0 brick awarded 40 points`);
    assert.equal(clip.scoreText.text, 'SCORE: 40', `${{slug}} scoreText updated`);

    // Drop ball below bottom boundary -> lose a life.
    clip.ballX = 320;
    clip.ballY = 445;
    clip.ballVY = 6;
    clip.stepGame();
    assert.equal(clip.lives, 2, `${{slug}} life lost on bottom exit`);
    assert.equal(clip.livesText.text, 'LIVES: 2', `${{slug}} livesText updated`);

    // Benchmark 120 full broadcast frames (5 seconds at 24fps) to guard against HTML5 runtime snapshot regressions.
    clip.resetGame();
    const t0 = performance.now();
    for (let f = 0; f < 120; f++) {{
      env.host.broadcastFrame();
    }}
    const elapsedMs = performance.now() - t0;
    assert.ok(elapsedMs < 600, `${{slug}} 120 frames took ${{elapsedMs.toFixed(1)}} ms (must be < 600 ms)`);
  }} else if (slug.startsWith('tetris-')) {{
    assert.equal(clip.score, 0, `${{slug}} initial score`);
    assert.equal(clip.lines, 0, `${{slug}} initial lines`);
    assert.equal(clip.level, 1, `${{slug}} initial level`);
    assert.equal(clip.board.length, 200, `${{slug}} 10x20 board initialized`);

    // Move active piece horizontally and rotate.
    const startX = clip.curX;
    assert.equal(clip.moveHorizontal(-1), true, `${{slug}} moveHorizontal(-1)`);
    assert.equal(clip.curX, startX - 1, `${{slug}} curX moved left`);
    assert.equal(clip.moveHorizontal(1), true, `${{slug}} moveHorizontal(1)`);
    assert.equal(clip.rotatePiece(), true, `${{slug}} rotatePiece()`);

    // Set up bottom row (row 19) with 6 filled cells (cols 0..5) and drop a horizontal I-piece (piece 0, rot 0) at col 6..9.
    clip.resetGame();
    for (let c = 0; c < 6; c++) {{
      clip.board[19 * 10 + c] = 2;
    }}
    clip.curPiece = 0;
    clip.curRot = 0;
    clip.curX = 6;
    clip.curY = 0;
    const dropped = clip.hardDrop();
    assert.ok(dropped > 0, `${{slug}} hardDrop moved piece down`);
    assert.equal(clip.lines, 1, `${{slug}} completed bottom row cleared 1 line`);
    assert.ok(clip.score >= 100, `${{slug}} line clear awarded score`);
    assert.equal(clip.linesText.text, 'LINES: 1', `${{slug}} linesText updated`);
  }}
}}
"#,
        host_integration_url = host_integration_url,
        rust_runtime_url = rust_runtime_url,
        root_str = root.display().to_string(),
        bundles_json = serde_json::to_string(&exported_bundles).unwrap(),
    );

    fs::write(&runner_path, runner_source).expect("write node runner script");
    let output = Command::new("node")
        .arg(&runner_path)
        .current_dir(&root)
        .output()
        .expect("run node to execute exported games");

    if !output.status.success() {
        panic!(
            "Headless Node execution of exported games failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let _ = fs::remove_dir_all(temp_dir);
}
