const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { resolve } = require('node:path');
const vm = require('node:vm');

test('the actual host keeps scheduling after a startup script failure so Restart can recover', async () => {
  const listeners = new Map(), animation = [], errors = [];
  const elements = new Map();
  const context2d = { setTransform() {}, clearRect() {}, fillRect() {} };
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      style: {}, textContent: '', getContext: () => context2d,
      addEventListener: (name, fn) => listeners.set(`${id}:${name}`, fn),
      removeAttribute() {}, setAttribute() {},
    });
    return elements.get(id);
  }
  const script = { language: 'java_script', source: 'throw only on the first attempt' };
  let player;
  class Player {
    constructor() { player = this; this.reset(); }
    reset() { this.frame = 0; this.pending = true; this.playing = true; }
    scripts_json() { const pending = this.pending; this.pending = false; return JSON.stringify(pending ? [{ script, target_path: [], event: 'frame' }] : []); }
    lifetime_id() { return 1; }
    is_dispatch_current() { return true; }
    script_overflowed() { return false; }
    child_path() { return 'null'; }
    scene_json() { return '[]'; }
    accessibility_json() { return '[]'; }
    current_frame() { return this.frame; }
    total_frames() { return 120; }
    is_playing() { return this.playing; }
    set_playing(value) { this.playing = value; }
    tick() { this.frame++; }
  }
  const wasm = Object.assign(async () => {}, { WebPlayer: Player });
  const context = vm.createContext({
    console: { error: value => errors.push(value), warn() {}, log() {} },
    document: { getElementById: element, hidden: false, addEventListener() {} },
    atob: value => Buffer.from(value, 'base64').toString('binary'),
    NeoFlashWasm: wasm,
    NeoFlashBundle: { wasm: '', project: { width: 320, height: 200, fps: 24, background: [255, 255, 255, 255] }, warnings: [],
      scripts: [{ ...script, location: 'Startup / frame 1', code: 'if (!globalThis.hasAttempted) { globalThis.hasAttempted = true; throw null; }' }] },
    NeoFlashPaths: { PathCache: class {} },
    NeoFlashAccessibility: { SemanticLayer: class { update() { return 0; } } },
    NeoFlashPreview: { Preview: class { constructor() { this.view = { scale: 1 }; } resize() {} } },
    ResizeObserver: class { observe() {} },
    addEventListener() {}, requestAnimationFrame: callback => animation.push(callback),
  });
  vm.runInContext(fs.readFileSync(resolve(__dirname, '../runtime.js'), 'utf8'), context);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(errors.length, 1);
  assert.equal(errors[0].cause, null);
  assert.match(element('error').textContent, /Startup \/ frame 1: null/);
  assert.equal(animation.length, 1, 'startup failure must not prevent the animation loop from being installed');
  animation.shift()(0);
  assert.equal(player.frame, 0, 'failed projects do not tick');
  listeners.get('restart:click')();
  assert.equal(element('error').style.display, 'none');
  animation.shift()(100);
  animation.shift()(150);
  assert.equal(player.frame, 1, 'Restart resumes ticking after the original cause is resolved');
  assert.equal(errors.length, 1);
  assert.equal(animation.length, 1, 'recovery does not install a duplicate animation loop');
});
