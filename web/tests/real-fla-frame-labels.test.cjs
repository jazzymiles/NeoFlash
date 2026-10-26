// The unchanged public FLA is imported, compiled, and run against the actual
// WASM timeline and browser proxy. Upstream output.txt is the independent oracle.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const { resolve } = require('node:path');
const { homedir } = require('node:os');
const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const vm = require('node:vm');
const root = resolve(__dirname, '../..');

test('unchanged Ruffle frame-label FLA reproduces its reference trace in the real host/WASM', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const source = resolve(root, 'examples/fla-corpus/upstream/ruffle/avm1-frame-labels/test.fla');
  const before = createHash('sha256').update(fs.readFileSync(source)).digest('hex');
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
  const directory = fs.mkdtempSync(resolve(root, 'target/real-fla-frame-labels-'));
  try {
    const destination = resolve(directory, 'imported.nflash');
    const imported = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-import', '--', 'import', source, destination], { cwd: root, encoding: 'utf8' });
    assert.equal(imported.status, 0, imported.stderr);
    const project = JSON.parse(fs.readFileSync(destination, 'utf8'));
    assert.deepEqual([project.width, project.height, project.fps], [550, 400, 24]);
    assert.equal(project.symbols.length, 2);
    const rootSymbol = project.symbols.find(symbol => symbol.id === project.root);
    assert.equal(rootSymbol.timeline.frame_count, 7);
    const scripts = project.symbols.flatMap(symbol => symbol.timeline.scripts.map(item => item.script));
    assert.equal(scripts.length, 9, 'all root and nested frame scripts survive import');
    assert.ok(scripts.every(script => script.language === 'action_script2'));
    const result = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-script', '--example', 'compile_batch'],
      { cwd: root, encoding: 'utf8', input: JSON.stringify(scripts), maxBuffer: 8 * 1024 * 1024 });
    assert.equal(result.status, 0, result.stderr);
    const outputs = JSON.parse(result.stdout);
    const compiled = scripts.map((script, index) => {
      assert.ok(outputs[index].Ok, JSON.stringify(outputs[index]));
      return { script, code: outputs[index].Ok, location: `Ruffle source script ${index}` };
    });
    const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web.js'), 'utf8'), context);
    context.wasmBytes = fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web_bg.wasm'));
    const wasm = await vm.runInContext('NeoFlashWasm({ module_or_path: wasmBytes }).then(() => NeoFlashWasm)', context);
    const player = new wasm.WebPlayer(JSON.stringify(project));
    try {
      const { host, traceCalls, hostConsole, scriptConsole } = createRuntimeHost(root, player, compiled);
      host.dispatchScripts();
      for (let tick = 0; tick < 3; tick++) { player.tick(); host.dispatchScripts(); }
      const expected = fs.readFileSync(resolve(source, '../output.txt'), 'utf8').trimEnd().split('\n');
      assert.deepEqual(traceCalls.map(values => values.join(' ')), expected);
      assert.equal(player.current_frame(), 2, 'duplicate SomeFrame resolves to the first frame 3');
      for (let tick = 0; tick < 8; tick++) { player.tick(); host.dispatchScripts(); }
      assert.equal(player.current_frame(), 2, 'the final gotoAndStop keeps the root stopped');
      assert.deepEqual(traceCalls.map(values => values.join(' ')), expected, 'no extra frame 6/7 scripts run');
      assert.equal(player.clip_frame(player.child_path('[]', 'clip')), 0, 'nested clip loops to its own stop frame');
      const oldLifetime = player.lifetime_id(player.child_path('[]', 'clip'));
      player.reset(); host.dispatchScripts();
      assert.notEqual(player.lifetime_id(player.child_path('[]', 'clip')), oldLifetime);
      for (let tick = 0; tick < 3; tick++) { player.tick(); host.dispatchScripts(); }
      assert.deepEqual(traceCalls.map(values => values.join(' ')), [...expected, ...expected], 'restart reproduces the same trace with fresh instance state');
      assert.equal(player.current_frame(), 2);
      assert.deepEqual(hostConsole, traceCalls.map(values => ['[NeoFlash]', ...values]));
      assert.deepEqual(scriptConsole, []);
      assert.equal(JSON.parse(player.scene_json()).length, 1, 'the authored square remains present');
    } finally { player.free(); }
    assert.equal(createHash('sha256').update(fs.readFileSync(source)).digest('hex'), before);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
