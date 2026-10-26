// The original editable example runs through the Rust compiler, actual WASM
// Player and actual browser proxy/dispatch code, without a DOM or OS clipboard.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const { resolve } = require('node:path');
const { homedir } = require('node:os');
const { spawnSync } = require('node:child_process');
const vm = require('node:vm');
const root = resolve(__dirname, '../..');

test('AS recovery example keeps instance state, cleanup and independent timelines in the actual host/WASM', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const project = JSON.parse(fs.readFileSync(resolve(root, 'examples/script-recovery.nflash'), 'utf8'));
  const scripts = [];
  for (const symbol of project.symbols) {
    scripts.push(...symbol.timeline.scripts.map(item => item.script));
    for (const layer of symbol.timeline.layers) for (const key of layer.keyframes) {
      for (const element of key.elements) scripts.push(...element.scripts.map(item => item.script));
    }
  }
  const crossHandler = ['action_script2', 'action_script3'].map(language => ({
    root: { language, source: 'function bounce(value) { throw value; } stop();' },
    click: { language, source: 'var run = _root.bounce; var payload = {marker:17}; var seen; try { run(payload); } catch (caught) { seen = caught === payload; } finally { _root.stop(); } trace("Cross handler", seen);' },
  }));
  const failures = ['action_script2', 'action_script3'].flatMap(language => ['null', 'undefined', '"original reason"'].map(value => ({ language, source: `throw ${value};` })));
  scripts.push(...crossHandler.flatMap(pair => [pair.root, pair.click]), ...failures);
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const result = spawnSync(process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo'),
    ['run', '--quiet', '--locked', '-p', 'neoflash-script', '--example', 'compile_batch'],
    { cwd: root, encoding: 'utf8', input: JSON.stringify(scripts), maxBuffer: 8 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const compiled = JSON.parse(result.stdout);
  const compiledScripts = scripts.map((script, index) => {
    assert.ok(Object.hasOwn(compiled[index], 'Ok'), JSON.stringify(compiled[index]));
    return { script, code: compiled[index].Ok, location: `${script.language} recovery fixture` };
  });
  const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
  vm.runInContext(fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web.js'), 'utf8'), context);
  context.wasmBytes = fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web_bg.wasm'));
  const wasm = await vm.runInContext('NeoFlashWasm({ module_or_path: wasmBytes }).then(() => NeoFlashWasm)', context);
  const player = new wasm.WebPlayer(JSON.stringify(project));
  try {
    const { host, traceCalls, hostConsole, scriptConsole } = createRuntimeHost(root, player, compiledScripts);
    host.dispatchScripts();
    assert.equal(player.current_frame(), 0);
    for (let i = 0; i < 12; i++) { player.tick(); host.dispatchScripts(); }
    assert.equal(player.current_frame(), 0, 'the root stop must not stop its nested clips');
    assert.equal(player.clip_frame('["as3-card"]'), 12);
    assert.equal(player.clip_frame('["as2-card"]'), 12);
    function activate(name) {
      const node = JSON.parse(player.accessibility_json()).find(node => node.instance_path[0] === name);
      assert.ok(node);
      assert.equal(player.activate_accessible(JSON.stringify(node.instance_path), node.lifetime_id), true);
      host.dispatchScripts(node.instance_path);
    }
    for (const name of ['as3-card', 'as3-card', 'as2-card', 'as3-card', 'as2-card']) activate(name);
    assert.deepEqual(traceCalls.slice(1), [
      ['AS3', 'Try again', 'cleanup', 1], ['AS3', 'Accepted', 'cleanup', 2],
      ['AS2', 'Try again', 'cleanup', 1], ['AS3', 'Ready', 'cleanup', 3],
      ['AS2', 'Accepted', 'cleanup', 2],
    ]);
    assert.equal(host.proxyFor(['as3-card']).attempts, 3);
    assert.equal(host.proxyFor(['as2-card']).attempts, 2);
    assert.equal(host.lastTrace(), 'AS2 Accepted cleanup 2');
    assert.deepEqual(hostConsole, traceCalls.map(args => ['[NeoFlash]', ...args]));
    assert.deepEqual(scriptConsole, []);
    const before = player.lifetime_id('["as3-card"]');
    player.reset(); host.dispatchScripts();
    assert.notEqual(player.lifetime_id('["as3-card"]'), before);
    activate('as3-card');
    assert.equal(host.lastTrace(), 'AS3 Try again cleanup 1');
  } finally { player.free(); }

  // Separate compiled handlers must share explicit AS2 transport, preserve the
  // payload's identity and let finally control its owning scene.
  for (const pair of crossHandler) {
    const copy = structuredClone(project);
    copy.symbols[0].timeline.scripts[0].script = pair.root;
    copy.symbols[0].timeline.layers[1].keyframes[0].elements[0].scripts[0].script = pair.click;
    const player = new wasm.WebPlayer(JSON.stringify(copy));
    try {
      const { host } = createRuntimeHost(root, player, compiledScripts);
      host.dispatchScripts(); player.click('["as3-card","activate"]'); host.dispatchScripts(['as3-card', 'activate']);
      assert.equal(host.lastTrace(), 'Cross handler true');
      player.tick(); host.dispatchScripts();
      assert.equal(player.current_frame(), 0);
      assert.equal(player.clip_frame('["as3-card"]'), 1);
    } finally { player.free(); }
  }
  for (const script of failures) {
    const copy = structuredClone(project);
    copy.symbols[0].timeline.scripts[0].script = script;
    const player = new wasm.WebPlayer(JSON.stringify(copy));
    try {
      const { host } = createRuntimeHost(root, player, compiledScripts);
      const expected = script.source.includes('null') ? null : script.source.includes('undefined') ? undefined : 'original reason';
      assert.throws(() => host.dispatchScripts(), error => {
        assert.equal(error.message, `${script.language} recovery fixture: ${String(expected)}`);
        assert.equal(error.cause, expected);
        return true;
      });
    } finally { player.free(); }
  }
});
