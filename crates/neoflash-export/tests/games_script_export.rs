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

#[test]
fn high_vertex_tween_and_shape_tween_stress_with_measured_fps() {
    use neoflash_core::{
        Easing, Element, ElementKind, FillRule, InstancePlayback, Keyframe, Layer, LayerProperties,
        PathCommand, Script, ScriptLanguage, ShapeHint, Symbol, SymbolKind, Timeline, Transform,
        TweenKind,
    };
    use std::f64::consts::TAU;

    let root = workspace_root();
    let temp_dir = temp_test_dir("high-vertex-tween-fps");
    let runtime_dir = temp_dir.join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    // Build a 256-vertex star-rose contour and a 256-vertex gear-mandala contour
    let vertex_count = 256usize;
    let mut star_rose_cmds = Vec::with_capacity(vertex_count + 2);
    let mut gear_mandala_cmds = Vec::with_capacity(vertex_count + 2);
    for i in 0..vertex_count {
        let angle = (i as f64 / vertex_count as f64) * TAU;
        let r_rose = 120.0 + 48.0 * (angle * 8.0).cos() + 18.0 * (angle * 24.0).sin();
        let x1 = 320.0 + r_rose * angle.cos();
        let y1 = 240.0 + r_rose * angle.sin();
        if i == 0 {
            star_rose_cmds.push(PathCommand::MoveTo { x: x1, y: y1 });
        } else {
            star_rose_cmds.push(PathCommand::LineTo { x: x1, y: y1 });
        }

        let tooth = if (i / 4) % 2 == 0 { 150.0 } else { 95.0 };
        let ripple = 12.0 * (angle * 16.0).sin();
        let r_gear = tooth + ripple;
        let x2 = 320.0 + r_gear * angle.cos();
        let y2 = 240.0 + r_gear * angle.sin();
        if i == 0 {
            gear_mandala_cmds.push(PathCommand::MoveTo { x: x2, y: y2 });
        } else {
            gear_mandala_cmds.push(PathCommand::LineTo { x: x2, y: y2 });
        }
    }
    star_rose_cmds.push(PathCommand::Close);
    gear_mandala_cmds.push(PathCommand::Close);

    let mut project = Project::blank();
    project.format_version = 9;
    project.name = "High-Vertex Shape & Classic Tween Stress (Measured FPS)".into();
    project.width = 640;
    project.height = 480;
    project.fps = 24;

    // Satellite Graphic symbol for classic tweening
    let sat_sym = Symbol {
        id: "sym-orbiter".into(),
        name: "Orbiter Satellite".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "sat-layer".into(),
                name: "Body".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "sat-dot".into(),
                        name: "Dot".into(),
                        kind: ElementKind::Ellipse {
                            width: 14.0,
                            height: 14.0,
                            fill: [56, 189, 248, 235],
                            stroke: [255, 255, 255, 255],
                            stroke_width: 1.5,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };
    project.symbols.push(sat_sym);

    let mut start_satellites = Vec::with_capacity(32);
    let mut end_satellites = Vec::with_capacity(32);
    for i in 0..32 {
        let a0 = (i as f64 / 32.0) * TAU;
        let a1 = a0 + std::f64::consts::PI;
        start_satellites.push(Element {
            id: format!("sat-{i}"),
            name: format!("sat_{i}"),
            transform: Transform {
                x: 320.0 + 180.0 * a0.cos(),
                y: 240.0 + 180.0 * a0.sin(),
                scale_x: 0.8,
                scale_y: 0.8,
                rotation: 0.0,
                alpha: 0.75,
                ..Transform::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym-orbiter".into(),
                playback: InstancePlayback::Loop,
            },
            ..Element::default()
        });
        end_satellites.push(Element {
            id: format!("sat-{i}"),
            name: format!("sat_{i}"),
            transform: Transform {
                x: 320.0 + 210.0 * a1.cos(),
                y: 240.0 + 210.0 * a1.sin(),
                scale_x: 1.25,
                scale_y: 1.25,
                rotation: 180.0,
                alpha: 1.0,
                ..Transform::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym-orbiter".into(),
                playback: InstancePlayback::Loop,
            },
            ..Element::default()
        });
    }

    let root_id = project.root.clone();
    {
        let root_sym = project.symbol_mut(&root_id).expect("root symbol");
        root_sym.timeline.frame_count = 60;
        // Real measured FPS counter script alongside target FPS
        root_sym.timeline.scripts = vec![neoflash_core::FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: r#"
if (this.fpsInitialized == undefined) {
    this.fpsInitialized = true;
    this.targetFps = 24;
    this.measuredFps = 24;
    this.frameCount = 0;
    this.lastStampMs = getTimer();
    this.fpsHud = this.createTextField("fpsHud", 500, 16, 14, 480, 26);
    this.fpsHud.text = "TARGET: 24 FPS | MEASURED: 24.0 FPS";
    this.onEnterFrame = function() {
        this.frameCount = this.frameCount + 1;
        var nowMs = getTimer();
        var dt = nowMs - this.lastStampMs;
        if (dt <= 0) {
            dt = 1;
        }
        this.lastStampMs = nowMs;
        var instFps = 1000 / dt;
        this.measuredFps = Math.round((this.measuredFps * 0.8 + instFps * 0.2) * 10) / 10;
        this.fpsHud.text = "TARGET: " + this.targetFps + " FPS | MEASURED: " + this.measuredFps + " FPS | FRAME: " + this._currentframe;
    };
}
"#
                .into(),
            },
        }];
        root_sym.timeline.layers = vec![
            Layer {
                id: "high-vertex-shape-tween".into(),
                name: "256-Vertex Shape Tween".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Shape,
                        shape_hints: vec![
                            ShapeHint {
                                label: 'a',
                                start_uv: [0.0, 0.5],
                                end_uv: [0.0, 0.5],
                            },
                            ShapeHint {
                                label: 'b',
                                start_uv: [0.5, 0.0],
                                end_uv: [0.5, 0.0],
                            },
                        ],
                        elements: vec![Element {
                            id: "poly-256".into(),
                            name: "StarRose256".into(),
                            kind: ElementKind::Path {
                                commands: star_rose_cmds,
                                fill_rule: FillRule::NonZero,
                                fill: [168, 85, 247, 230],
                                stroke: [240, 171, 252, 255],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 59,
                        elements: vec![Element {
                            id: "poly-256".into(),
                            name: "GearMandala256".into(),
                            kind: ElementKind::Path {
                                commands: gear_mandala_cmds,
                                fill_rule: FillRule::NonZero,
                                fill: [16, 185, 129, 230],
                                stroke: [110, 231, 183, 255],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            Layer {
                id: "multi-classic-tween".into(),
                name: "32 Classic Tween Satellites".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Classic,
                        elements: start_satellites,
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 59,
                        elements: end_satellites,
                        ..Keyframe::default()
                    },
                ],
            },
        ];
    }

    project
        .validate()
        .expect("high-vertex stress project validates");
    let out_dir = temp_dir.join("high-vertex-stress");
    let options = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };
    export_project_with_options(&project, &out_dir, &runtime_dir, &options)
        .expect("export high-vertex stress bundle");

    let runner_path = temp_dir.join("run-high-vertex.mjs");
    let host_integration_url = root
        .join("crates/neoflash-script/tests/host-integration.mjs")
        .display()
        .to_string();
    let rust_runtime_url = root
        .join("crates/neoflash-script/tests/rust-runtime-host.mjs")
        .display()
        .to_string();
    let web_runtime_url = root.join("web/runtime.js").display().to_string();
    let bundle_path = out_dir.join("bundle.js").display().to_string();

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

const text = fs.readFileSync({bundle_path:?}, 'utf8');
const jsonText = text.replace(/^globalThis\.NeoFlashBundle\s*=\s*/, '').replace(/;\s*$/, '');
const bundle = JSON.parse(jsonText);
const player = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
const compiled = bundle.scripts.map((s, idx) => ({{
  script: {{ language: s.language, source: s.source }},
  code: s.code,
  location: s.location || `script-${{idx}}`,
}}));

let fillCalls = 0;
let strokeCalls = 0;
const noop = () => {{}};
const gradient = {{ addColorStop: noop }};
const ctx = {{
  canvas: {{ width: 640, height: 480, style: {{}} }},
  save: noop, restore: noop, setTransform: noop, transform: noop,
  translate: noop, scale: noop, rotate: noop, beginPath: noop, closePath: noop,
  moveTo: noop, lineTo: noop, quadraticCurveTo: noop, bezierCurveTo: noop,
  arc: noop, ellipse: noop, rect: noop, clip: noop, clearRect: noop,
  fillRect: () => {{ fillCalls++; }},
  strokeRect: () => {{ strokeCalls++; }},
  fill: () => {{ fillCalls++; }},
  stroke: () => {{ strokeCalls++; }},
  fillText: noop, strokeText: noop,
  measureText: (t) => ({{ width: String(t || '').length * 7 }}),
  createLinearGradient: () => gradient,
  createRadialGradient: () => gradient,
  drawImage: noop,
  getImageData: (_x, _y, w, h) => ({{ data: new Uint8ClampedArray(w * h * 4), width: w, height: h }}),
  putImageData: noop,
  setLineDash: noop,
}};

const env = createRuntimeHost(root, player, compiled, {{ bundle, project: bundle.project, context: ctx }});
env.host.resetProject();
const bitmapHost = env.host.bitmap();
const paths = new NeoFlashPaths.PathCache();
const gradients = new NeoFlashPaths.GradientRenderer({{ createCanvas: () => bitmapHost.createCanvas(1, 1) }});
const clippings = new NeoFlashPaths.ClipRenderer({{ createCanvas: () => bitmapHost.createCanvas(1, 1) }});
const painter = createScenePainter({{
  global: {{ NeoFlashPaths, NeoFlashAccessibility, NeoFlashText }},
  paths, gradients, clippings, bitmapHost,
  textHost: env.host.text(), filterHost: env.host.filters(), videoHost: env.host.video(),
  context: ctx,
}});

const clip = env.host.proxyFor([]);
const frameDeltas = [];
let lastNow = performance.now();
for (let f = 0; f < 60; f++) {{
  env.host.broadcastFrame();
  player.tick();
  env.host.dispatchScripts();
  const snapshot = env.host.display().snapshot();
  // Verify the 256-vertex morphed shape + 32 satellites are present on every frame
  assert.ok(snapshot.draws.length >= 33, `frame ${{f}} expected >= 33 draw items, got ${{snapshot.draws.length}}`);
  painter.draw(ctx, snapshot, {{ width: 640, height: 480, semantic: false }});
  const now = performance.now();
  frameDeltas.push(Math.max(0.01, now - lastNow));
  lastNow = now;
}}

const avgDeltaMs = frameDeltas.reduce((a, b) => a + b, 0) / frameDeltas.length;
const hostMeasuredFps = 1000 / avgDeltaMs;
assert.equal(clip.targetFps, 24, 'targetFps is 24');
assert.ok(clip.measuredFps > 0, `script measuredFps must be positive, got ${{clip.measuredFps}}`);
assert.match(clip.fpsHud.text, /TARGET: 24 FPS \| MEASURED: [0-9.]+ FPS/, 'HUD text includes both target and real measured FPS');
assert.ok(hostMeasuredFps > 60, `host measured FPS (${{hostMeasuredFps.toFixed(1)}}) must exceed 60 FPS on 256-vertex shape tween + 32 classic tweens`);
assert.ok(fillCalls >= 60 * 33, `expected >= 1980 fill calls across 60 frames, got ${{fillCalls}}`);
"#,
        host_integration_url = host_integration_url,
        rust_runtime_url = rust_runtime_url,
        web_runtime_url = web_runtime_url,
        root_str = root.display().to_string(),
        bundle_path = bundle_path,
    );

    fs::write(&runner_path, runner_source).expect("write high-vertex runner");
    let output = Command::new(node_binary())
        .arg(&runner_path)
        .current_dir(&root)
        .output()
        .expect("run node high-vertex stress test");

    if !output.status.success() {
        panic!(
            "High-vertex tween & measured FPS test failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn behavioral_verification_of_eight_imported_fla_archetypes_in_html5() {
    use neoflash_core::{
        Easing, Element, ElementKind, FrameScript, FrameSound, InstancePlayback, Keyframe, Layer,
        LayerProperties, LayerRole, ObjectEvent, ObjectScript, PathCommand, ResourceUnit, Script,
        ScriptLanguage, SoundEffect, SoundSync, Symbol, SymbolKind, TextFormat, Timeline,
        Transform,
    };

    let root = workspace_root();
    let temp_dir = temp_test_dir("eight-imported-flas");
    let runtime_dir = temp_dir.join("runtime");
    write_mock_wasm_runtime(&runtime_dir);

    let mut archetypes: Vec<(&str, Project)> = Vec::new();

    // 1. Archetype 1: Interactive Button (`on(release)` + frame jump)
    {
        let mut p = Project::blank();
        p.name = "FLA 1 Interactive Button".into();
        let btn_sym = Symbol {
            id: "sym-btn".into(),
            name: "ActionBtn".into(),
            folder: String::new(),
            kind: SymbolKind::Button,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 4,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "btn-layer".into(),
                    name: "States".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: (0..4)
                        .map(|f| Keyframe {
                            frame: f,
                            elements: vec![Element {
                                id: format!("btn-rect-{f}"),
                                name: "hit".into(),
                                kind: ElementKind::Rectangle {
                                    width: 120.0,
                                    height: 44.0,
                                    fill: [37, 99, 235, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        })
                        .collect(),
                }],
            },
        };
        p.symbols.push(btn_sym);
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        root_sym.timeline.frame_count = 2;
        root_sym.timeline.scripts = vec![
            FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "stop(); var clicked = false;".into(),
                },
            },
            FrameScript {
                frame: 1,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "stop(); clicked = true;".into(),
                },
            },
        ];
        root_sym.timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "btn-inst".into(),
                    name: "goBtn".into(),
                    transform: Transform {
                        x: 100.0,
                        y: 100.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-btn".into(),
                        playback: InstancePlayback::SingleFrame(0),
                    },
                    scripts: vec![ObjectScript {
                        event: ObjectEvent::Click,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: "gotoAndStop(2);".into(),
                        },
                    }],
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 1,
                elements: vec![],
                ..Keyframe::default()
            },
        ];
        archetypes.push(("fla-1-button", p));
    }

    // 2. Archetype 2: Nested MovieClips with independent timelines
    {
        let mut p = Project::blank();
        p.name = "FLA 2 Nested Clips".into();
        let gear_sym = Symbol {
            id: "sym-gear".into(),
            name: "NestedGear".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 4,
                scripts: vec![FrameScript {
                    frame: 0,
                    script: Script {
                        language: ScriptLanguage::ActionScript2,
                        source: "this.ticks = (this.ticks || 0) + 1;".into(),
                    },
                }],
                guides: vec![],
                layers: vec![Layer {
                    id: "gear-layer".into(),
                    name: "Gear".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "gear-shape".into(),
                            name: "box".into(),
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 40.0,
                                fill: [245, 158, 11, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                }],
            },
        };
        p.symbols.push(gear_sym);
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        root_sym.timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "gear-inst".into(),
                name: "nestedClip".into(),
                transform: Transform {
                    x: 150.0,
                    y: 120.0,
                    ..Transform::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: "sym-gear".into(),
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            });
        archetypes.push(("fla-2-nested-clips", p));
    }

    // 3. Archetype 3: Dynamic TextField with variable & formatting
    {
        let mut p = Project::blank();
        p.name = "FLA 3 TextFields".into();
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        let fmt = TextFormat {
            font: "Helvetica".into(),
            size: 20.0,
            ..TextFormat::default()
        };
        root_sym.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "stop(); this.banner.text = 'IMPORTED FLA TEXT OK';".into(),
            },
        });
        root_sym.timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "tf-banner".into(),
                name: "banner".into(),
                transform: Transform {
                    x: 40.0,
                    y: 60.0,
                    ..Transform::default()
                },
                kind: ElementKind::TextBox {
                    text: "Initial".into(),
                    width: 320.0,
                    height: 40.0,
                    format: fmt,
                    word_wrap: true,
                    multiline: false,
                },
                ..Element::default()
            });
        archetypes.push(("fla-3-textfield", p));
    }

    // 4. Archetype 4: Frame labels & timeline navigation (`gotoAndStop("target_label")`)
    {
        let mut p = Project::blank();
        p.name = "FLA 4 Frame Navigation".into();
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        root_sym.timeline.frame_count = 5;
        root_sym.timeline.scripts = vec![
            FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "var navState = 'start'; gotoAndStop('victory_frame');".into(),
                },
            },
            FrameScript {
                frame: 3,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "stop(); navState = 'arrived_victory';".into(),
                },
            },
        ];
        root_sym.timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                label: "intro".into(),
                ..Keyframe::default()
            },
            Keyframe {
                frame: 3,
                label: "victory_frame".into(),
                ..Keyframe::default()
            },
        ];
        archetypes.push(("fla-4-frame-nav", p));
    }

    // 5. Archetype 5: Data loading (`LoadVars.decode` + `XML` parsing)
    {
        let mut p = Project::blank();
        p.name = "FLA 5 Data Loading".into();
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        root_sym.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: r#"
stop();
var lv = new LoadVars();
lv.decode("player=Miles&score=9000");
var xml = new XML("<config><mode>arcade</mode></config>");
this.parsedPlayer = lv.player;
this.parsedScore = Number(lv.score);
this.xmlRootName = xml.firstChild.nodeName;
"#
                .into(),
            },
        });
        archetypes.push(("fla-5-data-loading", p));
    }

    // 6. Archetype 6: Timeline Sound (PCM WAV + Stream sync)
    {
        let mut p = Project::blank();
        p.format_version = 6;
        p.name = "FLA 6 Timeline Sound".into();
        let sample_rate = 8000u32;
        let num_samples = 800u32;
        let data_bytes = num_samples * 2;
        let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        for _ in 0..num_samples {
            wav.extend_from_slice(&1024i16.to_le_bytes());
        }
        p.resources.push(
            ResourceUnit::from_bytes("res-beep", "audio/beep.wav", "audio/wav", &wav).unwrap(),
        );
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        root_sym.timeline.layers[0].keyframes[0].sound = Some(FrameSound {
            resource: "res-beep".into(),
            sync: SoundSync::Start,
            loops: 1,
            loop_forever: false,
            effect: SoundEffect::None,
            in_point: None,
            out_point: None,
            envelope: None,
        });
        archetypes.push(("fla-6-sound", p));
    }

    // 7. Archetype 7: Guided Classic Tween (`orient_to_path: true`)
    {
        let mut p = Project::blank();
        p.format_version = 6;
        p.name = "FLA 7 Guided Tween".into();
        let pod_sym = Symbol {
            id: "sym-pod".into(),
            name: "Pod".into(),
            folder: String::new(),
            kind: SymbolKind::Graphic,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 1,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "pod-l".into(),
                    name: "Pod".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "pod-r".into(),
                            name: "PodRect".into(),
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 16.0,
                                fill: [16, 185, 129, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                }],
            },
        };
        p.symbols.push(pod_sym);
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        root_sym.timeline.frame_count = 10;
        root_sym.timeline.layers = vec![
            Layer {
                id: "guide-l".into(),
                name: "Guide".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    role: LayerRole::Guide,
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "guide-path".into(),
                        name: "Path".into(),
                        kind: ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 40.0, y: 200.0 },
                                PathCommand::LineTo { x: 440.0, y: 200.0 },
                            ],
                            fill_rule: neoflash_core::FillRule::NonZero,
                            fill: [0, 0, 0, 0],
                            stroke: [100, 100, 255, 255],
                            stroke_width: 1.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            Layer {
                id: "guided-l".into(),
                name: "Guided".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    motion_guide: Some("guide-l".into()),
                    ..LayerProperties::default()
                },
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::Linear),
                        orient_to_path: true,
                        elements: vec![Element {
                            id: "pod-i".into(),
                            name: "pod".into(),
                            transform: Transform {
                                x: 40.0,
                                y: 200.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Instance {
                                symbol_id: "sym-pod".into(),
                                playback: InstancePlayback::Loop,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 9,
                        elements: vec![Element {
                            id: "pod-i".into(),
                            name: "pod".into(),
                            transform: Transform {
                                x: 440.0,
                                y: 200.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Instance {
                                symbol_id: "sym-pod".into(),
                                playback: InstancePlayback::Loop,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
        ];
        archetypes.push(("fla-7-guided-tween", p));
    }

    // 8. Archetype 8: AS3 Interactive Sprite + Graphics + Event Dispatch
    {
        let mut p = Project::blank();
        p.name = "FLA 8 AS3 Interactive".into();
        let root_id = p.root.clone();
        let root_sym = p.symbol_mut(&root_id).unwrap();
        root_sym.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: r#"
stop();
var badge:Sprite = new Sprite();
badge.name = "as3Badge";
badge.graphics.beginFill(0xec4899, 1.0);
badge.graphics.drawCircle(120, 120, 32);
badge.graphics.endFill();
this.addChild(badge);
this.as3Ready = (this.getChildByName("as3Badge") != null);
"#
                .into(),
            },
        });
        archetypes.push(("fla-8-as3-interactive", p));
    }

    let options = PublishOptions {
        fonts: FontExportPolicy::Device,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };

    let mut exported_bundles = Vec::new();
    for (slug, authored_project) in &archetypes {
        authored_project
            .validate()
            .unwrap_or_else(|e| panic!("{slug} authored validate failed: {e}"));
        let fla_path = temp_dir.join(format!("{slug}.fla"));
        neoflash_import::export_fla(authored_project, &fla_path)
            .unwrap_or_else(|e| panic!("{slug} export_fla failed: {e}"));

        let imported = neoflash_import::import_project(&fla_path)
            .unwrap_or_else(|e| panic!("{slug} import_project from .fla failed: {e}"));
        imported
            .validate()
            .unwrap_or_else(|e| panic!("{slug} imported validate failed: {e}"));

        let out_dir = temp_dir.join(slug);
        export_project_with_options(&imported, &out_dir, &runtime_dir, &options)
            .unwrap_or_else(|e| panic!("{slug} HTML5 export failed: {e}"));
        exported_bundles.push((slug.to_string(), out_dir.join("bundle.js")));
    }

    let runner_path = temp_dir.join("run-eight-flas.mjs");
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

function createMockAudioContext() {{
  return {{
    state: 'running',
    sampleRate: 8000,
    currentTime: 0,
    destination: {{ kind: 'destination' }},
    resume() {{ this.state = 'running'; return Promise.resolve(); }},
    close() {{ this.state = 'closed'; return Promise.resolve(); }},
    decodeAudioData() {{
      return Promise.resolve({{ numberOfChannels: 1, sampleRate: 8000, length: 800, duration: 0.1 }});
    }},
    createGain() {{
      return {{ kind: 'gain', gain: {{ value: 1, setValueAtTime() {{}}, linearRampToValueAtTime() {{}} }}, connect() {{}}, disconnect() {{}} }};
    }},
    createStereoPanner() {{
      return {{ kind: 'pan', pan: {{ value: 0 }}, connect() {{}}, disconnect() {{}} }};
    }},
    createBufferSource() {{
      return {{ buffer: null, loop: false, loopStart: 0, loopEnd: 0, onended: null, connect() {{}}, disconnect() {{}}, start() {{}}, stop() {{}} }};
    }},
  }};
}}

for (const [slug, bundlePath] of bundles) {{
  const text = fs.readFileSync(bundlePath, 'utf8');
  const jsonText = text.replace(/^globalThis\.NeoFlashBundle\s*=\s*/, '').replace(/;\s*$/, '');
  const bundle = JSON.parse(jsonText);
  const player = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
  const compiled = (bundle.scripts || []).map((s, idx) => ({{
    script: {{ language: s.language, source: s.source }},
    code: s.code,
    location: s.location || `script-${{idx}}`,
  }}));
  const env = createRuntimeHost(root, player, compiled, {{
    bundle,
    project: bundle.project,
    audio: {{ createContext: createMockAudioContext, isUserGesture: () => true }},
  }});
  env.host.resetProject();
  const clip = env.host.proxyFor([]);

  if (slug === 'fla-1-button') {{
    assert.equal(clip.clicked, false, 'button initially not clicked');
    assert.equal(player.current_frame(), 0, 'starts at frame 0');
  }} else if (slug === 'fla-2-nested-clips') {{
    assert.ok(clip.nestedClip, 'nestedClip instance exists on stage');
    assert.equal(clip.nestedClip.ticks, 1, 'nestedClip ran frame 0 script');
  }} else if (slug === 'fla-3-textfield') {{
    assert.equal(clip.banner.text, 'IMPORTED FLA TEXT OK', 'dynamic TextField updated after FLA roundtrip');
  }} else if (slug === 'fla-4-frame-nav') {{
    assert.equal(clip.navState, 'arrived_victory', 'gotoAndStop(label) navigated to frame 4');
    assert.equal(player.current_frame(), 3, 'player is at 0-based frame 3');
  }} else if (slug === 'fla-5-data-loading') {{
    assert.equal(clip.parsedPlayer, 'Miles', 'LoadVars decoded player');
    assert.equal(clip.parsedScore, 9000, 'LoadVars decoded score');
    assert.equal(clip.xmlRootName, 'config', 'XML parsed root node');
  }} else if (slug === 'fla-6-sound') {{
    const soundResources = JSON.parse(player.timeline_sound_resources_json());
    assert.equal(soundResources.length, 1, 'imported FLA preserved timeline sound resource');
  }} else if (slug === 'fla-7-guided-tween') {{
    const snap0 = env.host.display().snapshot();
    player.tick();
    player.tick();
    const snap2 = env.host.display().snapshot();
    assert.ok(snap0.draws.length >= 1 && snap2.draws.length >= 1, 'guided tween draws across frames');
  }} else if (slug === 'fla-8-as3-interactive') {{
    assert.equal(clip.as3Ready, true, 'AS3 child Sprite created and attached');
  }}
}}
"#,
        host_integration_url = host_integration_url,
        rust_runtime_url = rust_runtime_url,
        root_str = root.display().to_string(),
        bundles_json = serde_json::to_string(&exported_bundles).unwrap(),
    );

    fs::write(&runner_path, runner_source).expect("write eight-flas runner");
    let output = Command::new(node_binary())
        .arg(&runner_path)
        .current_dir(&root)
        .output()
        .expect("run node eight-flas behavioral test");

    if !output.status.success() {
        panic!(
            "Eight imported FLA behavioral HTML5 test failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let _ = fs::remove_dir_all(temp_dir);
}
