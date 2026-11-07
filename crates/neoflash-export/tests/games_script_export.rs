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

fn node_binary() -> &'static str {
    for candidate in ["/opt/homebrew/bin/node", "/usr/local/bin/node"] {
        if Path::new(candidate).exists() {
            return candidate;
        }
    }
    "node"
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
    let output = Command::new(node_binary())
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

fn build_stress_project(
    name: &str,
    language: neoflash_core::ScriptLanguage,
    code: &str,
) -> Project {
    let mut project = Project::blank();
    project.name = name.to_string();
    let root_id = project.root.clone();
    let root_symbol = project
        .symbols
        .iter_mut()
        .find(|s| s.id == root_id)
        .expect("root symbol");
    root_symbol
        .timeline
        .scripts
        .push(neoflash_core::FrameScript {
            frame: 0,
            script: neoflash_core::Script {
                language,
                source: code.to_string(),
            },
        });
    project
}

#[test]
fn exported_games_and_stress_suites_meet_html5_frame_time_budgets() {
    use neoflash_core::ScriptLanguage;

    let root = workspace_root();
    let temp_dir = temp_test_dir("perf-suite");
    let runtime_dir = temp_dir.join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    let options = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };

    let mut exported_games = Vec::new();
    for rel_path in GAME_FILES {
        let full_path = root.join(rel_path);
        let raw = fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("failed to read {rel_path}: {e}"));
        let project: Project = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("failed to parse {rel_path}: {e}"));
        let slug = Path::new(rel_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("game");
        let out_dir = temp_dir.join(slug);
        export_project_with_options(&project, &out_dir, &runtime_dir, &options)
            .unwrap_or_else(|e| panic!("{rel_path} failed HTML5 export: {e}"));
        exported_games.push((slug.to_string(), out_dir.join("bundle.js")));
    }

    let stress_specs: &[(&str, ScriptLanguage, &str)] = &[
        (
            "stress-particles-as2",
            ScriptLanguage::ActionScript2,
            r#"
stop();
var particles = [];
var tickCount = 0;
var i = 0;
while (i < 60) {
    var mc = this.createEmptyMovieClip("p_" + i, 100 + i);
    mc.beginFill(0x38bdf8, 90);
    mc.moveTo(-4, -4);
    mc.lineTo(4, -4);
    mc.lineTo(4, 4);
    mc.lineTo(-4, 4);
    mc.lineTo(-4, -4);
    mc.endFill();
    mc._x = 40 + (i % 10) * 52;
    mc._y = 40 + Math.floor(i / 10) * 60;
    mc.vx = ((i % 5) - 2) * 1.5 + 0.5;
    mc.vy = ((i % 7) - 3) * 1.2 + 0.4;
    particles.push(mc);
    i = i + 1;
}
this.onEnterFrame = function() {
    tickCount = tickCount + 1;
    var j = 0;
    while (j < 60) {
        var p = particles[j];
        var nx = p._x + p.vx;
        var ny = p._y + p.vy;
        if (nx < 20 || nx > 620) { p.vx = -p.vx; nx = p._x + p.vx; }
        if (ny < 20 || ny > 460) { p.vy = -p.vy; ny = p._y + p.vy; }
        p._x = nx;
        p._y = ny;
        p._rotation = (p._rotation + 3) % 360;
        j = j + 1;
    }
};
"#,
        ),
        (
            "stress-particles-as3",
            ScriptLanguage::ActionScript3,
            r#"
stop();
var particles:Array = [];
var tickCount:int = 0;
for (var i:int = 0; i < 60; i++) {
    var sp:Sprite = new Sprite();
    sp.name = "p_" + i;
    sp.graphics.beginFill(0xa855f7, 0.9);
    sp.graphics.drawRect(-4, -4, 8, 8);
    sp.graphics.endFill();
    sp.x = 40 + (i % 10) * 52;
    sp.y = 40 + Math.floor(i / 10) * 60;
    sp.vx = ((i % 5) - 2) * 1.5 + 0.5;
    sp.vy = ((i % 7) - 3) * 1.2 + 0.4;
    this.addChild(sp);
    particles.push(sp);
}
this.addEventListener("enterFrame", function(ev:Object):void {
    tickCount = tickCount + 1;
    for (var j:int = 0; j < 60; j++) {
        var p:Object = particles[j];
        var nx:Number = p.x + p.vx;
        var ny:Number = p.y + p.vy;
        if (nx < 20 || nx > 620) { p.vx = -p.vx; nx = p.x + p.vx; }
        if (ny < 20 || ny > 460) { p.vy = -p.vy; ny = p.y + p.vy; }
        p.x = nx;
        p.y = ny;
        p.rotation = (p.rotation + 3) % 360;
    }
});
"#,
        ),
        (
            "stress-graphics-as2",
            ScriptLanguage::ActionScript2,
            r#"
stop();
var canvas = this.createEmptyMovieClip("canvas", 10);
var tickCount = 0;
var shapesDrawn = 0;
this.onEnterFrame = function() {
    tickCount = tickCount + 1;
    canvas.clear();
    canvas.lineStyle(1, 0x22d3ee, 100);
    var i = 0;
    while (i < 50) {
        var bx = 30 + (i % 10) * 56 + (tickCount % 7);
        var by = 30 + Math.floor(i / 10) * 80;
        canvas.beginFill(0x0ea5e9 + (i * 1024), 85);
        canvas.moveTo(bx, by);
        canvas.lineTo(bx + 28, by);
        canvas.lineTo(bx + 28, by + 28);
        canvas.lineTo(bx, by + 28);
        canvas.lineTo(bx, by);
        canvas.endFill();
        i = i + 1;
    }
    shapesDrawn = 50;
};
"#,
        ),
        (
            "stress-graphics-as3",
            ScriptLanguage::ActionScript3,
            r#"
stop();
var canvas:Sprite = new Sprite();
canvas.name = "canvas";
this.addChild(canvas);
var tickCount:int = 0;
var shapesDrawn:int = 0;
this.addEventListener("enterFrame", function(ev:Object):void {
    tickCount = tickCount + 1;
    canvas.graphics.clear();
    canvas.graphics.lineStyle(1, 0x22d3ee, 1.0);
    for (var i:int = 0; i < 50; i++) {
        var bx:Number = 30 + (i % 10) * 56 + (tickCount % 7);
        var by:Number = 30 + Math.floor(i / 10) * 80;
        canvas.graphics.beginFill(0x10b981 + (i * 1024), 0.85);
        if ((i & 1) == 0) {
            canvas.graphics.drawRect(bx, by, 28, 28);
        } else {
            canvas.graphics.drawCircle(bx + 14, by + 14, 14);
        }
        canvas.graphics.endFill();
    }
    shapesDrawn = 50;
});
"#,
        ),
        (
            "stress-collision-as2",
            ScriptLanguage::ActionScript2,
            r#"
stop();
var boxes = [];
var tickCount = 0;
var hitCount = 0;
var i = 0;
while (i < 24) {
    var b = this.createEmptyMovieClip("box_" + i, 50 + i);
    b.beginFill(0xf97316, 100);
    b.moveTo(0, 0);
    b.lineTo(24, 0);
    b.lineTo(24, 24);
    b.lineTo(0, 24);
    b.lineTo(0, 0);
    b.endFill();
    b._x = 60 + (i % 6) * 40;
    b._y = 60 + Math.floor(i / 6) * 40;
    boxes.push(b);
    i = i + 1;
}
this.onEnterFrame = function() {
    tickCount = tickCount + 1;
    var probe = boxes[0];
    probe._x = 80 + (tickCount * 7) % 220;
    probe._y = 80 + (tickCount * 5) % 160;
    var j = 1;
    while (j < 24) {
        var target = boxes[j];
        target._x = 60 + ((j * 37 + tickCount * 3) % 240);
        var bounds = target.getBounds(this);
        var pt = { x: 12, y: 12 };
        target.localToGlobal(pt);
        this.globalToLocal(pt);
        if (probe.hitTest(target)) {
            hitCount = hitCount + 1;
        }
        if (bounds.xMax > 0 && pt.x >= 0) {
            hitCount = hitCount + 0;
        }
        j = j + 1;
    }
};
"#,
        ),
        (
            "stress-collision-as3",
            ScriptLanguage::ActionScript3,
            r#"
stop();
var boxes:Array = [];
var tickCount:int = 0;
var hitCount:int = 0;
for (var i:int = 0; i < 24; i++) {
    var b:Sprite = new Sprite();
    b.name = "box_" + i;
    b.graphics.beginFill(0xec4899, 1.0);
    b.graphics.drawRect(0, 0, 24, 24);
    b.graphics.endFill();
    b.x = 60 + (i % 6) * 40;
    b.y = 60 + Math.floor(i / 6) * 40;
    this.addChild(b);
    boxes.push(b);
}
this.addEventListener("enterFrame", function(ev:Object):void {
    tickCount = tickCount + 1;
    var probe:Sprite = boxes[0];
    probe.x = 80 + (tickCount * 7) % 220;
    probe.y = 80 + (tickCount * 5) % 160;
    for (var j:int = 1; j < 24; j++) {
        var target:Sprite = boxes[j];
        target.x = 60 + ((j * 37 + tickCount * 3) % 240);
        var bounds:Object = target.getBounds(this);
        var pt:Point = target.localToGlobal(new Point(12, 12));
        var localPt:Point = this.globalToLocal(pt);
        if (probe.hitTestObject(target) || target.hitTestPoint(probe.x, probe.y, false)) {
            hitCount = hitCount + 1;
        }
        if (bounds.width > 0 && localPt.x >= 0) {
            hitCount = hitCount + 0;
        }
    }
});
"#,
        ),
        (
            "stress-hud-physics-as2",
            ScriptLanguage::ActionScript2,
            r#"
stop();
var bodies = [];
var labels = [];
var tickCount = 0;
var i = 0;
while (i < 36) {
    var b = this.createEmptyMovieClip("body_" + i, 200 + i);
    b.beginFill(0x6366f1, 100);
    b.moveTo(-6, -6);
    b.lineTo(6, -6);
    b.lineTo(6, 6);
    b.lineTo(-6, 6);
    b.lineTo(-6, -6);
    b.endFill();
    b._x = 50 + (i % 9) * 55;
    b._y = 100 + Math.floor(i / 9) * 65;
    b.vy = (i % 5) - 2;
    bodies.push(b);
    i = i + 1;
}
var k = 0;
while (k < 8) {
    var tf = this.createTextField("hud_" + k, 400 + k, 20 + (k % 4) * 150, 12 + Math.floor(k / 4) * 28, 140, 22);
    tf.text = "HUD " + k + ": 0";
    labels.push(tf);
    k = k + 1;
}
this.onEnterFrame = function() {
    tickCount = tickCount + 1;
    var j = 0;
    while (j < 36) {
        var body = bodies[j];
        body.vy = body.vy + 0.25;
        body._y = body._y + body.vy;
        if (body._y > 440) {
            body._y = 440;
            body.vy = -Math.abs(body.vy) * 0.9;
        }
        j = j + 1;
    }
    var m = 0;
    while (m < 8) {
        labels[m].text = "HUD " + m + ": " + (tickCount * (m + 1));
        m = m + 1;
    }
};
"#,
        ),
        (
            "stress-hud-physics-as3",
            ScriptLanguage::ActionScript3,
            r#"
stop();
var bodies:Array = [];
var labels:Array = [];
var tickCount:int = 0;
for (var i:int = 0; i < 36; i++) {
    var b:Sprite = new Sprite();
    b.name = "body_" + i;
    b.graphics.beginFill(0x8b5cf6, 1.0);
    b.graphics.drawRect(-6, -6, 12, 12);
    b.graphics.endFill();
    b.x = 50 + (i % 9) * 55;
    b.y = 100 + Math.floor(i / 9) * 65;
    b.vy = (i % 5) - 2;
    this.addChild(b);
    bodies.push(b);
}
for (var k:int = 0; k < 8; k++) {
    var tf:TextField = new TextField();
    tf.name = "hud_" + k;
    tf.x = 20 + (k % 4) * 150;
    tf.y = 12 + Math.floor(k / 4) * 28;
    tf.width = 140;
    tf.height = 22;
    tf.text = "HUD " + k + ": 0";
    this.addChild(tf);
    labels.push(tf);
}
this.addEventListener("enterFrame", function(ev:Object):void {
    tickCount = tickCount + 1;
    for (var j:int = 0; j < 36; j++) {
        var body:Object = bodies[j];
        body.vy = body.vy + 0.25;
        body.y = body.y + body.vy;
        if (body.y > 440) {
            body.y = 440;
            body.vy = -Math.abs(body.vy) * 0.9;
        }
    }
    for (var m:int = 0; m < 8; m++) {
        labels[m].text = "HUD " + m + ": " + (tickCount * (m + 1));
    }
});
"#,
        ),
    ];

    let mut exported_stress = Vec::new();
    for (slug, lang, source) in stress_specs {
        let project = build_stress_project(slug, *lang, source);
        project
            .validate()
            .unwrap_or_else(|e| panic!("{slug} failed validate: {e}"));
        let out_dir = temp_dir.join(slug);
        export_project_with_options(&project, &out_dir, &runtime_dir, &options)
            .unwrap_or_else(|e| panic!("{slug} failed HTML5 export: {e}"));
        exported_stress.push((slug.to_string(), out_dir.join("bundle.js")));
    }

    let runner_path = temp_dir.join("run-perf-suite.mjs");
    let host_integration_url = root
        .join("crates/neoflash-script/tests/host-integration.mjs")
        .display()
        .to_string();
    let rust_runtime_url = root
        .join("crates/neoflash-script/tests/rust-runtime-host.mjs")
        .display()
        .to_string();
    let web_runtime_url = root.join("web/runtime.js").display().to_string();

    let runner_source = format!(
        r#"import assert from 'node:assert/strict';
import fs from 'node:fs';
import {{ createRequire }} from 'node:module';
import {{ resolve }} from 'node:path';
import {{ createRuntimeHost }} from {host_integration_url:?};
import {{ runtimeWasm }} from {rust_runtime_url:?};
import webRuntime from {web_runtime_url:?};
const {{ createScenePainter }} = webRuntime;
const require = createRequire(import.meta.url);

const root = {root_str:?};
globalThis.Path2D ??= class Path2D {{
  moveTo() {{}}
  lineTo() {{}}
  quadraticCurveTo() {{}}
  bezierCurveTo() {{}}
  closePath() {{}}
}};
const NeoFlashPaths = require(resolve(root, 'web/path.js'));
const NeoFlashAccessibility = require(resolve(root, 'web/accessibility.js'));
const NeoFlashText = require(resolve(root, 'web/text.js'));
const gameBundles = {games_json};
const stressBundles = {stress_json};

function createMockCanvasContext() {{
  const stats = {{ fillCalls: 0, strokeCalls: 0, textCalls: 0 }};
  const noop = () => {{}};
  const gradient = {{ addColorStop: noop }};
  const ctx = {{
    stats,
    canvas: {{ width: 640, height: 480, style: {{}} }},
    save: noop,
    restore: noop,
    setTransform: noop,
    transform: noop,
    translate: noop,
    scale: noop,
    rotate: noop,
    beginPath: noop,
    closePath: noop,
    moveTo: noop,
    lineTo: noop,
    quadraticCurveTo: noop,
    bezierCurveTo: noop,
    arc: noop,
    ellipse: noop,
    rect: noop,
    clip: noop,
    clearRect: noop,
    fillRect: () => {{ stats.fillCalls++; }},
    strokeRect: () => {{ stats.strokeCalls++; }},
    fill: () => {{ stats.fillCalls++; }},
    stroke: () => {{ stats.strokeCalls++; }},
    fillText: () => {{ stats.textCalls++; }},
    strokeText: () => {{ stats.textCalls++; }},
    measureText: (text) => ({{ width: String(text || '').length * 7 }}),
    createLinearGradient: () => gradient,
    createRadialGradient: () => gradient,
    drawImage: noop,
    getImageData: (_x, _y, w, h) => ({{ data: new Uint8ClampedArray(w * h * 4), width: w, height: h }}),
    putImageData: noop,
    setLineDash: noop,
  }};
  return ctx;
}}

function loadBundle(bundlePath) {{
  const text = fs.readFileSync(bundlePath, 'utf8');
  const jsonText = text.replace(/^globalThis\.NeoFlashBundle\s*=\s*/, '').replace(/;\s*$/, '');
  const bundle = JSON.parse(jsonText);
  const rawPlayer = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
  let snapshotCalls = 0;
  const origSnapshot = rawPlayer.runtime_snapshot_json.bind(rawPlayer);
  rawPlayer.runtime_snapshot_json = function(...args) {{
    snapshotCalls++;
    return origSnapshot(...args);
  }};

  const compiled = bundle.scripts.map((s, idx) => ({{
    script: {{ language: s.language, source: s.source }},
    code: s.code,
    location: s.location || `script-${{idx}}`,
  }}));

  const ctx = createMockCanvasContext();
  const env = createRuntimeHost(root, rawPlayer, compiled, {{ bundle, project: bundle.project, context: ctx }});
  env.host.resetProject();
  const bitmapHost = env.host.bitmap();
  const paths = new NeoFlashPaths.PathCache();
  const gradients = new NeoFlashPaths.GradientRenderer({{ createCanvas: () => bitmapHost.createCanvas(1, 1) }});
  const clippings = new NeoFlashPaths.ClipRenderer({{ createCanvas: () => bitmapHost.createCanvas(1, 1) }});
  const painter = createScenePainter({{
    global: {{ NeoFlashPaths, NeoFlashAccessibility, NeoFlashText }},
    paths,
    gradients,
    clippings,
    bitmapHost,
    textHost: env.host.text(),
    filterHost: env.host.filters(),
    videoHost: env.host.video(),
    context: ctx,
  }});
  const paintFrame = () => {{
    const snapshot = env.host.display().snapshot();
    painter.draw(ctx, snapshot, {{ width: 640, height: 480, semantic: false }});
  }};
  return {{
    clip: env.host.proxyFor([]),
    env,
    ctx,
    paintFrame,
    getSnapshotCalls: () => snapshotCalls,
    resetSnapshotCalls: () => {{ snapshotCalls = 0; }},
  }};
}}

const results = [];

// 1. Benchmark all 6 exported games over 240 active gameplay + Canvas 2D render frames.
for (const [slug, bundlePath] of gameBundles) {{
  const {{ clip, env, ctx, paintFrame, getSnapshotCalls, resetSnapshotCalls }} = loadBundle(bundlePath);

  // Warmup 5 frames.
  for (let w = 0; w < 5; w++) {{
    env.host.broadcastFrame();
    paintFrame();
  }}
  resetSnapshotCalls();
  const frames = 240;
  const t0 = performance.now();
  for (let f = 0; f < frames; f++) {{
    if (slug.startsWith('snake-')) {{
      if (clip.gameOver) clip.resetGame();
      if (f % 6 === 0) {{
        const dirs = [[0, -1], [1, 0], [0, 1], [-1, 0]];
        const [dx, dy] = dirs[(f / 6) % 4];
        clip.setDirection(dx, dy);
      }}
    }} else if (slug.startsWith('arkanoid-')) {{
      if (clip.gameOver) clip.resetGame();
      clip.paddleX = Math.max(48, Math.min(592, clip.ballX));
    }} else if (slug.startsWith('tetris-')) {{
      if (clip.gameOver) clip.resetGame();
      if (f % 8 === 0) clip.moveHorizontal((f % 16 === 0) ? -1 : 1);
      if (f % 12 === 0) clip.rotatePiece();
    }}
    env.host.broadcastFrame();
    paintFrame();
  }}
  const elapsedMs = performance.now() - t0;
  const avgMs = elapsedMs / frames;
  const snapshotsPerFrame = getSnapshotCalls() / frames;

  results.push({{
    suite: slug,
    frames,
    totalMs: Number(elapsedMs.toFixed(2)),
    avgMsPerFrame: Number(avgMs.toFixed(3)),
    snapshotsPerFrame: Number(snapshotsPerFrame.toFixed(2)),
    draws: ctx.stats.fillCalls + ctx.stats.strokeCalls + ctx.stats.textCalls,
  }});

  assert.ok(
    snapshotsPerFrame <= 1.05,
    `${{slug}} triggered ${{snapshotsPerFrame.toFixed(2)}} runtime_snapshot_json calls/frame (expected <= 1.05)`
  );
  assert.ok(
    avgMs < 2.5,
    `${{slug}} average frame time ${{avgMs.toFixed(3)}} ms exceeded 2.5 ms/frame budget`
  );
  assert.ok(ctx.stats.fillCalls > frames, `${{slug}} Canvas 2D painter must execute fill calls`);
}}

// 2. Benchmark all 8 AS2/AS3 runtime & rendering stress suites over 120 frames.
for (const [slug, bundlePath] of stressBundles) {{
  const {{ clip, env, ctx, paintFrame, getSnapshotCalls, resetSnapshotCalls }} = loadBundle(bundlePath);

  // Warmup 5 frames.
  for (let w = 0; w < 5; w++) {{
    env.host.broadcastFrame();
    paintFrame();
  }}
  resetSnapshotCalls();
  const frames = 120;
  const t0 = performance.now();
  for (let f = 0; f < frames; f++) {{
    env.host.broadcastFrame();
    paintFrame();
  }}
  const elapsedMs = performance.now() - t0;
  const avgMs = elapsedMs / frames;
  const snapshotsPerFrame = getSnapshotCalls() / frames;

  assert.equal(clip.tickCount, 125, `${{slug}} ran all 125 enterFrame ticks`);
  if (slug.startsWith('stress-graphics-')) {{
    assert.equal(clip.shapesDrawn, 50, `${{slug}} drew 50 procedural shapes per frame`);
  }}

  results.push({{
    suite: slug,
    frames,
    totalMs: Number(elapsedMs.toFixed(2)),
    avgMsPerFrame: Number(avgMs.toFixed(3)),
    snapshotsPerFrame: Number(snapshotsPerFrame.toFixed(2)),
    draws: ctx.stats.fillCalls + ctx.stats.strokeCalls + ctx.stats.textCalls,
  }});

  assert.ok(
    snapshotsPerFrame <= 1.05,
    `${{slug}} triggered ${{snapshotsPerFrame.toFixed(2)}} runtime_snapshot_json calls/frame (expected <= 1.05)`
  );
  assert.ok(
    avgMs < 3.5,
    `${{slug}} average frame time ${{avgMs.toFixed(3)}} ms exceeded 3.5 ms/frame budget`
  );
}}

console.table(results);
"#,
        host_integration_url = host_integration_url,
        rust_runtime_url = rust_runtime_url,
        web_runtime_url = web_runtime_url,
        root_str = root.display().to_string(),
        games_json = serde_json::to_string(&exported_games).unwrap(),
        stress_json = serde_json::to_string(&exported_stress).unwrap(),
    );

    fs::write(&runner_path, runner_source).expect("write node perf runner script");
    let output = Command::new(node_binary())
        .arg(&runner_path)
        .current_dir(&root)
        .output()
        .expect("run node to benchmark exported games and stress suites");

    println!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        panic!(
            "HTML5 performance & stress suite failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let _ = fs::remove_dir_all(temp_dir);
}
