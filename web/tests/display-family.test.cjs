const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const { resolve } = require('node:path');
const { homedir } = require('node:os');
const { spawnSync } = require('node:child_process');

test('published DisplayObject/MovieClip family works through compiled AS2, actual host and Rust/WASM', async () => {
  const root = resolve(__dirname, '../..');
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
  const directory = fs.mkdtempSync(resolve(root, 'target/display-family-'));
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  let player;
  try {
    const exported = spawnSync(cargo,
      ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--', 'examples/display-family.nflash', '--output', directory],
      { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);
    const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(resolve(directory, 'bundle.js'), 'utf8'), context);
    const bundle = vm.runInContext('NeoFlashBundle', context);
    vm.runInContext(fs.readFileSync(resolve(directory, 'neoflash_web.js'), 'utf8'), context);
    context.wasmBytes = Buffer.from(bundle.wasm, 'base64');
    const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', context);
    player = new wasm.WebPlayer(JSON.stringify(bundle.project));
    const { host } = createRuntimeHost(root, player, Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source }, code: item.code, location: item.location,
    })), { bundle, context: { measureText: line => ({ width: line.length * 8 }) } });
    host.dispatchScripts();
    const scene = host.proxyFor([]), widget = scene.widget, tile = widget.tile;
    const click = name => { player.click(JSON.stringify([name])); host.dispatchScripts([name]); };
    assert.equal(host.lastTrace(), 'family ready');

    // Identity, hierarchy, geometry and coordinate conversion form one contract.
    click('inspect');
    assert.equal(host.lastTrace(), 'family children 2 identity true bounds 100x80 global 80,164');
    assert.equal(widget.parent, scene); assert.equal(scene.parent, host.stage().stage);
    assert.equal(widget.stage, scene.stage); assert.equal(widget.root, scene);
    click('resize');
    assert.equal(host.lastTrace(), 'resize 200 120 2 1.5');
    widget.visible = false;
    assert.equal(widget.width, 200, 'visibility must not erase geometric bounds');
    widget.visible = true;
    click('rotate');
    assert.equal(host.lastTrace(), 'rotate 45 bounds 226 226');
    const point = widget.localToGlobal({ x: 17, y: 13 }), local = widget.globalToLocal(point);
    assert.ok(Math.abs(local.x - 17) < 1e-9 && Math.abs(local.y - 13) < 1e-9);
    click('identity');
    assert.equal(host.lastTrace(), 'rename true true 1 true');
    assert.equal(widget.renamed, tile); assert.equal(widget.getChildByName('tile'), null);
    assert.throws(() => { widget.width = 50; }, /rotated object/);
    widget.rotation = 0; widget.width = 0; widget.width = 100;
    assert.equal(widget.width, 100, 'local geometry allows recovery from zero scale');

    // Public transport calls are void, with independent child clocks preserved.
    assert.equal(widget.stop(), undefined);
    assert.equal(widget.nextFrame(), undefined); assert.equal(widget.currentFrame, 2);
    assert.equal(widget.gotoAndPlay(1), undefined);
    for (let i = 0; i < 3; i++) { host.broadcastFrame(); player.tick(); host.dispatchScripts(); }
    assert.equal(widget.ticks, 3); assert.equal(scene.currentFrame, 1);
    assert.equal(widget.currentFrame, 4); assert.equal(tile.currentFrame, 4);
    host.stage().handlePointer('pointerdown', { button: 0, buttons: 1 }, widget);
    assert.equal(host.lastTrace(), 'AS2 press widget 15');
    click('callbacks');
    const ticks = widget.ticks;
    host.broadcastFrame(); player.tick(); host.dispatchScripts();
    assert.equal(widget.ticks, ticks); assert.equal(widget.currentFrame, 5);

    // Linked class descriptors can coexist with native members and custom fields.
    let amount = 4;
    Object.defineProperty(widget, 'amount', { get: () => amount, set: value => { amount = Number(value) | 0; } });
    widget.amount = 8.9; assert.equal(widget.amount, 8);
    assert.equal(host.program().type('flash.display.MovieClip').__nfIs(widget), true);
    assert.equal(host.program().type('flash.display.DisplayObject').__nfIs(scene.stage), true);
    assert.throws(() => new (host.program().type('flash.display.MovieClip'))(), /Dynamic new/);
    assert.equal(host.program().type('flash.geom.Point').__nfIs(point), true);

    // Plain authored TextFields are live display objects backed by Rust overrides.
    const title = scene.title, originalText = title.text;
    title.text = 'Zoe:45\r\nAda:30';
    assert.equal(title.text, 'Zoe:45\rAda:30');
    assert.equal(title.appendText('\nBob:20'), undefined);
    assert.equal(title.replaceText(0, 3, 'Zoë'), undefined);
    title.textColor = 0x48d8b0;
    assert.equal(title.length, 20); assert.equal(title.numLines, 3);
    assert.equal(title.getLineText(0), 'Zoë:45\r'); assert.equal(title.getLineLength(0), 7);
    assert.equal(title.getLineText(2), 'Bob:20'); assert.equal(title.textColor, 0x48d8b0);
    assert.equal(title.textWidth, 48, 'deterministic Canvas measurement service');
    assert.ok(Math.abs(title.textHeight - 97.2) < 1e-9);
    const liveTitle = JSON.parse(player.scene_json()).find(item => item.name === 'title');
    assert.equal(liveTitle.kind.text, 'Zoë:45\nAda:30\nBob:20');
    assert.deepEqual(liveTitle.kind.color.slice(0, 3), [0x48, 0xd8, 0xb0]);
    assert.notEqual(bundle.project.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind.text, liveTitle.kind.text);
    title.text = 'A😀\nB'; assert.equal(title.length, 5);
    assert.throws(() => title.replaceText(1, 2, 'X'), /Unicode/);
    assert.equal(title.text, 'A😀\rB', 'rejected range must not corrupt text');
    assert.throws(() => { title.htmlText = '<b>pending</b>'; }, /plain-text profile/);
    widget.text = 'custom'; widget.htmlText = 'custom html';
    assert.equal(widget.text, 'custom'); assert.equal(widget.htmlText, 'custom html');
    host.resetProject();
    assert.throws(() => title.text, /removed|replaced/);
    assert.equal(host.proxyFor([]).title.text, originalText);
  } finally {
    if (player) player.free();
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
