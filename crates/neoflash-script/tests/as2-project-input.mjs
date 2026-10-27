// One original AS2 family scenario: ordinary publication, real WASM, the shipped
// host and installed Stage input listeners. Imported by project-execute.mjs.
// This is executable integration evidence, not original-player or browser QA.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

class Surface extends EventTarget {
  style = { cursor: 'default' };
  focus() {}
  getBoundingClientRect() { return { left: 0, top: 0, width: 960, height: 540 }; }
  send(type, data = {}) {
    const event = new Event(type, { cancelable: true });
    for (const [name, value] of Object.entries(data)) Object.defineProperty(event, name, { value });
    this.dispatchEvent(event);
  }
}

export async function verifyAs2ProjectInput(root, cargo) {
  const directory = mkdtempSync(resolve(root, 'target/as2-project-input-'));
  let player, host;
  try {
    const exported = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--',
      'examples/as2-project-input.nflash', '--output', directory], { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);
    const sandbox = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(readFileSync(resolve(directory, 'bundle.js'), 'utf8'), sandbox);
    const bundle = vm.runInContext('NeoFlashBundle', sandbox);
    vm.runInContext(readFileSync(resolve(directory, 'neoflash_web.js'), 'utf8'), sandbox);
    sandbox.wasmBytes = Buffer.from(bundle.wasm, 'base64');
    const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', sandbox);
    player = new wasm.WebPlayer(JSON.stringify(bundle.project));
    const canvas = new Surface(), keyboard = new Surface(), windowTarget = new Surface();
    ({ host } = createRuntimeHost(root, player, Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source }, code: item.code, location: item.location,
    })), { bundle, canvas, keyboardTarget: keyboard, keyTarget: windowTarget,
      context: { measureText: line => ({ width: line.length * 8 }) } }));
    host.dispatchScripts(); host.stage().attach();
    const scene = host.proxyFor([]), left = scene.left, right = scene.right;
    const legacy = host.legacy(), Base = host.program().type('app.Base'), Widget = host.program().type('app.Widget');
    const down = (key, code, keyCode) => keyboard.send('keydown', { key, code, keyCode });
    const up = (key, code, keyCode) => keyboard.send('keyup', { key, code, keyCode });
    const press = target => host.stage().handlePointer('pointerdown', { button: 0, buttons: 1, clientX: 170, clientY: 292 }, target);
    assert.equal(host.lastTrace(), 'AS2 ready 2 0 960 540');
    assert.equal(Base.instances, 2); assert.equal(Base.total, 0);
    assert.equal(left.count, 0); assert.equal(right.count, 0);
    assert.equal(left.activate, right.activate, 'AS2 inherited methods are shared, unbound prototype values');
    assert.equal(left.onPress, right.onPress);
    assert.equal(left.caption.text, 'LEFT / 0'); assert.equal(right.caption.text, 'RIGHT / 0');
    press(left);
    assert.equal(scene.mouseEvents, 1); assert.equal(left.count, 1); assert.equal(right.count, 0);
    assert.equal(Base.total, 1); assert.equal(host.lastTrace(), 'AS2 component left 1 1');
    assert.equal(left._x, left.homeX + 8);
    down('ArrowRight', 'ArrowRight', 39);
    assert.equal(scene.keyEvents, 1); assert.equal(right.count, 1); assert.equal(Base.total, 2);
    assert.equal(legacy.Key.isDown(legacy.Key.RIGHT), true);
    up('ArrowRight', 'ArrowRight', 39);
    assert.equal(scene.keyUps, 1); assert.equal(legacy.Key.isDown(39), false);
    assert.equal(host.lastTrace(), 'AS2 key up 39 false');
    down('a', 'KeyA', 65);
    assert.equal(legacy.Key.getAscii(), 97); assert.equal(legacy.Key.getCode(), 65);
    assert.match(scene.statusMessage.text, /Key 65 \/ ASCII 97/);
    windowTarget.send('blur'); assert.equal(legacy.Key.isDown(65), false);
    const keyCount = scene.keyEvents;
    keyboard.send('keydown', { key: 'x', code: 'KeyX', isComposing: true });
    assert.equal(scene.keyEvents, keyCount, 'IME input is not fabricated as AS2 key callbacks');
    down('h', 'KeyH', 72); assert.equal(canvas.style.cursor, 'none'); up('h', 'KeyH', 72);
    down('s', 'KeyS', 83); assert.equal(canvas.style.cursor, 'default'); up('s', 'KeyS', 83);
    down(' ', 'Space', 32); up(' ', 'Space', 32);
    assert.equal(left.count, 0); assert.equal(right.count, 0); assert.equal(Base.total, 0);
    const inherited = left.activate;
    inherited.call(right);
    assert.equal(left.count, 0); assert.equal(right.count, 1); assert.equal(Base.total, 1);
    assert.equal(right.caption.text, 'RIGHT / 1', 'inherited method uses its actual call receiver');
    host.stage().resize(1000, 600);
    assert.equal(scene.resizeEvents, 1); assert.equal(legacy.Stage.width, 1000);
    assert.equal(host.lastTrace(), 'AS2 stage 1000 600');

    // One mutation sequence verifies live prototype lookup and own suppression.
    let replaced = 0;
    Widget.prototype.onPress = function() { replaced++; this.count += 10; };
    press(right); assert.equal(replaced, 1); assert.equal(right.count, 11);
    right.onPress = null;
    press(right); assert.equal(replaced, 1, 'own null suppresses inherited callback');
    press(left); assert.equal(replaced, 2); assert.equal(left.count, 10);
    const order = [];
    const late = { onKeyDown() { assert.equal(this, late); order.push('late'); } };
    const second = { onKeyDown() { assert.equal(this, second); order.push('second'); } };
    const first = { onKeyDown() { assert.equal(this, first); order.push('first'); legacy.Key.removeListener(second); legacy.Key.addListener(late); } };
    assert.equal(legacy.Key.addListener(first), undefined); legacy.Key.addListener(first); legacy.Key.addListener(second);
    down('b', 'KeyB', 66); assert.deepEqual(order, ['first', 'second']);
    down('c', 'KeyC', 67); assert.deepEqual(order, ['first', 'second', 'first', 'late']);

    legacy.Mouse.hide(); host.resetProject();
    assert.equal(host.lastTrace(), 'AS2 ready 2 0 960 540');
    assert.equal(canvas.style.cursor, 'default'); assert.equal(legacy.Key.getCode(), 0);
    const fresh = host.proxyFor([]);
    assert.notEqual(fresh.left, left); assert.equal(fresh.keyEvents, 0);
    assert.equal(host.program().type('app.Base').instances, 2); assert.equal(host.program().type('app.Base').total, 0);
    // A saved retired display proxy must be pruned before callback lookup.
    legacy.Key.addListener(left);
    down('ArrowRight', 'ArrowRight', 39); up('ArrowRight', 'ArrowRight', 39);
    assert.equal(fresh.keyEvents, 1); assert.equal(fresh.keyUps, 1);
    assert.equal(fresh.right.count, 1); assert.equal(fresh.left.count, 0);
    assert.deepEqual(order, ['first', 'second', 'first', 'late'], 'Restart removed old singleton listener objects');
    press(fresh.left); assert.equal(fresh.mouseEvents, 1); assert.equal(fresh.left.count, 1);
    assert.equal(host.lastTrace(), 'AS2 component left 1 2', 'Restart restored the declared prototype callback exactly once');
    host.disposeFamily();
    down('ArrowRight', 'ArrowRight', 39); assert.equal(fresh.keyEvents, 1);
    assert.equal(legacy.Key.isDown(39), false);
  } finally {
    host?.disposeFamily(); player?.free(); rmSync(directory, { recursive: true, force: true });
  }
  console.log('AS2 project/input: inherited linked classes, shared prototype methods/statics, independent instances, Key/Mouse/Stage, listener mutation/lifetime, cursor and Restart passed.');
}
