const test = require('node:test');
const assert = require('node:assert/strict');
const { resolve } = require('node:path');

test('actual display proxy preserves AS2 numeric visibility and modern Boolean coercion independently', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const values = new Map([['[]', true], ['["card"]', true]]);
  const lifetimes = new Map([['[]', '1'], ['["card"]', '2']]);
  const writes = [];
  const player = {
    lifetime_id: key => lifetimes.get(key) || '',
    child_path: (key, name) => JSON.stringify(key === '[]' && name === 'card' ? ['card'] : null),
    visible: key => values.get(key),
    set_visible: (key, value) => { assert.equal(typeof value, 'boolean'); writes.push([key, value]); values.set(key, value); return true; },
  };
  const { host } = createRuntimeHost(resolve(__dirname, '../..'), player, []);
  const root = host.proxyFor([]), card = root.card;
  assert.equal(card.visible, true);
  for (const value of [false, 0, -0, '0', '+0', '-0', '0.0', '.0', '-.0']) {
    card.visible = true;
    card._visible = value;
    assert.equal(card.visible, false);
  }
  for (const value of [true, 1, -1, 0.25, Infinity, -Infinity, '1', '-2.5', '+.5', '5.']) {
    card.visible = false;
    card._visible = value;
    assert.equal(card.visible, true);
  }
  for (const initial of [true, false]) for (const value of [null, undefined, NaN, '', 'true', 'false', 'NaN', 'Infinity', '+Infinity', '-Infinity', 'abc']) {
    card.visible = initial;
    const before = writes.length;
    card._visible = value;
    assert.equal(card.visible, initial, 'invalid numeric primitives must preserve either starting state');
    assert.equal(writes.length, before, 'preservation must not overwrite the runtime property');
  }
  for (const value of [false, 0, NaN, '', null, undefined, true, 1, -1, '0', 'false', {}, []]) {
    card.visible = value;
    assert.equal(card._visible, Boolean(value), 'modern visible retains Boolean coercion');
  }
  card.score = 17; card.counter = () => 19;
  root.visible = false;
  assert.equal(root._visible, false);
  assert.equal(card.visible, true, 'local readback is independent of ancestor visibility');
  assert.equal(card.score, 17); assert.equal(card.counter(), 19);
  lifetimes.set('["card"]', '3');
  values.set('["card"]', true);
  assert.throws(() => card.visible, /removed or replaced/);
  assert.throws(() => { card._visible = false; }, /removed or replaced/);
  assert.throws(() => { card._visible = null; }, /removed or replaced/, 'no-op values still require a live receiver');
  const replacement = root.card;
  assert.notEqual(replacement, card);
  assert.equal(replacement.visible, true);
  assert.equal(replacement.score, undefined);
});

test('AS2 visibility rejects unverified coercions without calling user conversion hooks or changing state', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  let visible = false, conversions = 0;
  const { host } = createRuntimeHost(resolve(__dirname, '../..'), {
    lifetime_id: () => '1', visible: () => visible,
    set_visible: (_key, value) => { visible = value; return true; },
  }, []);
  const object = { valueOf() { conversions++; return 1; }, toString() { conversions++; return '1'; } };
  const revoked = Proxy.revocable({}, {}); revoked.revoke();
  for (const value of [object, [], function() {}, new Number(1), new Boolean(true), Symbol('1'), 1n, revoked.proxy,
    '.', '1e', '1e2', '00', '0x0', '0b0', ' 0', '0 ', '1x']) {
    for (const initial of [true, false]) {
      visible = initial;
      assert.throws(() => { host.proxyFor([])._visible = value; }, error => {
        assert.equal(error.name, 'TypeError');
        assert.match(error.message, /AS2 _visible.*(?:supports|not supported).*Pass/);
        return true;
      });
      assert.equal(visible, initial);
    }
  }
  assert.equal(conversions, 0);
});

test('visibility setter cannot silently succeed when runtime rejects a display target', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const { host } = createRuntimeHost(resolve(__dirname, '../..'), {
    lifetime_id: () => '1', set_visible: () => false,
  }, []);
  assert.throws(() => { host.proxyFor([]).visible = false; }, /Cannot change visible/);
  assert.throws(() => { host.proxyFor([])._visible = '0'; }, /Cannot change _visible/);
});

test('published AS2/AS3 fixture filters drawing, click targets and semantics while hidden scripts keep running', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const { homedir } = require('node:os');
  const { spawnSync } = require('node:child_process');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const root = resolve(__dirname, '../..');
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
  const directory = fs.mkdtempSync(resolve(root, 'target/visibility-export-'));
  let player;
  try {
    const exported = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--',
      'examples/visibility.nflash', '--output', directory], { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);
    const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(resolve(directory, 'bundle.js'), 'utf8'), context);
    const bundle = vm.runInContext('NeoFlashBundle', context);
    vm.runInContext(fs.readFileSync(resolve(directory, 'neoflash_web.js'), 'utf8'), context);
    context.wasmBytes = Buffer.from(bundle.wasm, 'base64');
    const wasm = await vm.runInContext('NeoFlashWasm({ module_or_path: wasmBytes }).then(() => NeoFlashWasm)', context);
    player = new wasm.WebPlayer(JSON.stringify(bundle.project));
    const measureContext = { measureText: text => ({ width: Array.from(text).length * 8 }) };
    const { host, traceCalls } = createRuntimeHost(root, player, Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source }, code: item.code, location: item.location,
    })), { context: measureContext });
    const scene = () => JSON.parse(player.scene_json());
    const semantics = () => JSON.parse(player.accessibility_json());
    const activate = name => { player.click(JSON.stringify([name])); host.dispatchScripts([name]); };
    host.dispatchScripts();
    assert.equal(host.lastTrace(), 'ready');
    const legacy = host.proxyFor(['legacy']), modern = host.proxyFor(['modern']);
    const targets = semantics().filter(n => ['legacy', 'modern'].includes(n.instance_path[0]));
    assert.equal(targets.length, 2);
    for (let i = 0; i < 4; i++) { player.tick(); host.dispatchScripts(); }
    activate('legacy'); activate('modern');
    activate('hide-as2'); activate('hide-as3');
    assert.equal(legacy._visible, false); assert.equal(modern.visible, false);
    assert.ok(scene().every(item => !['legacy', 'modern'].includes(item.instance_path[0])));
    assert.ok(semantics().every(item => !['legacy', 'modern'].includes(item.instance_path[0])));
    for (const node of targets) assert.equal(player.activate_accessible(JSON.stringify(node.instance_path), node.lifetime_id), false);
    const beforeClick = traceCalls.length;
    player.click('["legacy","face"]'); host.dispatchScripts(['legacy', 'face']);
    assert.equal(traceCalls.length, beforeClick, 'hidden direct click cannot execute the handler');
    for (let i = 0; i < 7; i++) { player.tick(); host.dispatchScripts(); }
    assert.equal(legacy.ticks, 11); assert.equal(modern.ticks, 11);
    assert.equal(player.clip_frame('["legacy"]'), 11);
    assert.equal(player.clip_frame('["modern"]'), 11);
    activate('create-children');
    assert.equal(legacy.loads, 1); assert.equal(modern.loads, 1);
    assert.deepEqual(traceCalls.slice(-2), [
      ['child Load', false, 'loads', 1, 'ticks', 11],
      ['child Load', false, 'loads', 1, 'ticks', 11],
    ]);
    activate('show-as2'); activate('show-as3');
    for (const old of targets) {
      assert.ok(semantics().some(n => n.lifetime_id === old.lifetime_id && n.instance_path[0] === old.instance_path[0]));
      assert.equal(player.activate_accessible(JSON.stringify(old.instance_path), old.lifetime_id), true);
      host.dispatchScripts(old.instance_path);
    }
    assert.equal(legacy.clicks, 2); assert.equal(modern.clicks, 2);

    // Use the real host hitAt implementation, including reverse painter order.
    const runtime = fs.readFileSync(resolve(root, 'web/runtime.js'), 'utf8');
    const start = runtime.indexOf('    function hitAt(event) {');
    const end = runtime.indexOf("    canvas.addEventListener('click'", start);
    const matrixStart = runtime.indexOf('    function bitmapMatrix(item){');
    const matrixEnd = runtime.indexOf('    function render() {', matrixStart);
    assert.ok(start >= 0 && end > start && matrixStart >= 0 && matrixEnd > matrixStart);
    const { localPoint, hitShape } = require('../runtime.js');
    const hitAt = new Function('player', 'canvas', 'project', 'localPoint', 'hitShape', 'context', 'global', 'paths', 'bitmapHost', 'textHost',
      'let scene; const graphMode = true;\n' + runtime.slice(matrixStart, matrixEnd) + runtime.slice(start, end) + '\nreturn event => { const ref = hitAt(event); if (!ref) return null; return JSON.parse(player.runtime_snapshot_json()).objects.find(node => node.object_ref.id === ref.id && node.object_ref.generation === ref.generation).authored_path; };')(
      player, { width:800, height:480, getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 480 }) }, bundle.project,
      localPoint, hitShape, measureContext, {}, {}, host.bitmap(), host.text());
    modern.x = legacy.x;
    assert.deepEqual(hitAt({ clientX: 100, clientY: 145 }), ['modern', 'face']);
    activate('hide-as3');
    const through = hitAt({ clientX: 100, clientY: 145 });
    assert.deepEqual(through, ['legacy', 'face']);
    player.click(JSON.stringify(through)); host.dispatchScripts(through);
    assert.equal(legacy.clicks, 3); assert.equal(modern.clicks, 2);
    activate('child-toggle'); activate('show-as3');
    assert.equal(modern.face.visible, false);
    assert.ok(!scene().some(item => item.instance_path.join('/') === 'modern/face'));
    activate('show-all');
    assert.ok(scene().some(item => item.instance_path.join('/') === 'modern/face'));
    host.proxyFor([])._visible = false;
    assert.equal(scene().length, 0); assert.equal(semantics().length, 0);
    player.tick(); host.dispatchScripts();
    assert.equal(legacy.ticks, 12);
    host.proxyFor([]).visible = true;
    assert.ok(scene().length > 0);
    assert.equal(host.proxyFor(['legacy']), legacy, 'hide/show must not recreate host custom fields');
    player.reset(); host.dispatchScripts();
    assert.throws(() => legacy.visible, { name: 'ReferenceError', message: /removed.*replaced/ });
    assert.equal(host.proxyFor(['legacy']).visible, true);
    assert.equal(host.proxyFor(['legacy']).clicks, 0);
  } finally {
    if (player) player.free();
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

async function withPublishedCoercionProject(name, verify) {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const { homedir } = require('node:os');
  const { spawnSync } = require('node:child_process');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const root = resolve(__dirname, '../..');
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
  const directory = fs.mkdtempSync(resolve(root, 'target/visibility-coercion-export-'));
  let player;
  try {
    const exported = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--',
      `examples/${name}.nflash`, '--output', directory], { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);
    const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(resolve(directory, 'bundle.js'), 'utf8'), context);
    const bundle = vm.runInContext('NeoFlashBundle', context);
    vm.runInContext(fs.readFileSync(resolve(directory, 'neoflash_web.js'), 'utf8'), context);
    context.wasmBytes = Buffer.from(bundle.wasm, 'base64');
    const wasm = await vm.runInContext('NeoFlashWasm({ module_or_path: wasmBytes }).then(() => NeoFlashWasm)', context);
    player = new wasm.WebPlayer(JSON.stringify(bundle.project));
    const result = createRuntimeHost(root, player, Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source }, code: item.code, location: item.location,
    })));
    const activate = name => { player.click(JSON.stringify([name])); result.host.dispatchScripts([name]); };
    result.host.dispatchScripts();
    verify({ ...result, player, activate });
  } finally {
    if (player) player.free();
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

test('published primitive coercion fixture executes compiled AS2/AS3 and updates the real WASM scene', async () => {
  await withPublishedCoercionProject('visibility-coercion', ({ host, player, activate }) => {
    const shown = name => JSON.parse(player.scene_json()).some(item => item.instance_path[0] === name);
    const accessible = name => JSON.parse(player.accessibility_json()).some(item => item.instance_path[0] === name);
    assert.equal(host.lastTrace(), 'ready');
    assert.equal(shown('card'), true); assert.equal(shown('modern'), true);
    activate('zero');
    assert.equal(host.lastTrace(), 'AS2 zero false');
    assert.equal(shown('card'), false); assert.equal(accessible('card'), false);
    activate('invalid');
    assert.equal(host.lastTrace(), 'AS2 invalid false false false false false');
    assert.equal(shown('card'), false);
    activate('primitives');
    assert.equal(host.lastTrace(), 'AS2 primitives true false true true true true true true');
    assert.equal(shown('card'), true); assert.equal(accessible('card'), true);
    activate('decimals');
    assert.equal(host.lastTrace(), 'AS2 decimals false false true true');
    assert.equal(shown('card'), true);
    activate('modern-check');
    assert.equal(host.lastTrace(), 'AS3 Boolean true true false false false');
    assert.equal(shown('modern'), false); assert.equal(accessible('modern'), false);
  });
});

test('published unsupported coercion reports a host diagnostic, skips AS2 catch and runs finally', async () => {
  await withPublishedCoercionProject('visibility-coercion-unsupported', ({ host, player, activate, traceCalls }) => {
    for (const [name, message] of [
      ['object', 'AS2 _visible object, function, Symbol and BigInt conversions are not supported. Pass a Boolean, number, or supported decimal string explicitly.'],
      ['numeric-text', 'AS2 _visible string conversion currently supports plain signed decimal strings without exponent, radix, extra leading zeroes or whitespace. Pass a number or Boolean for other forms.'],
    ]) {
      player.reset(); host.dispatchScripts();
      assert.equal(host.lastTrace(), 'ready; card hidden');
      const before = traceCalls.length;
      assert.throws(() => activate(name), error => {
        assert.ok(error.message.includes(message));
        assert.equal(error.cause.name, 'TypeError');
        assert.equal(error.cause.message, message);
        assert.equal(Object.getOwnPropertyDescriptor(error.cause, Symbol.for('neoflash.as2.thrown.v1')), undefined,
          'host conversion errors must not become authored AS2 throw envelopes');
        return true;
      });
      assert.deepEqual(traceCalls.slice(before), [['AS2 finally', false]], 'catch and continuation must not execute');
      assert.equal(host.lastTrace(), 'AS2 finally false');
      assert.equal(player.visible('["card"]'), false);
      assert.ok(JSON.parse(player.scene_json()).every(item => item.instance_path[0] !== 'card'));
    }
  });
});
