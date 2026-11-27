'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { homedir } = require('node:os');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '../..');
const runtimeDir = path.resolve(root, 'target/web-runtime');
const webDir = path.resolve(root, 'web');
const localCargo = path.resolve(homedir(), '.cargo/bin/cargo');
const cargo = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');

function makeFakeElement(tag = 'div', ownerDoc = null) {
  const attrs = new Map();
  const listeners = new Map();
  const children = [];
  const styleProps = new Map();
  const classSet = new Set();
  const style = new Proxy(
    {},
    {
      get(_t, prop) {
        if (prop === 'setProperty') return (k, v) => styleProps.set(k, String(v));
        return styleProps.get(prop) ?? '';
      },
      set(_t, prop, val) {
        styleProps.set(prop, String(val));
        return true;
      },
    }
  );
  const el = {
    tagName: String(tag).toUpperCase(),
    ownerDocument: ownerDoc,
    style,
    dataset: {},
    children,
    parentNode: null,
    get parentElement() { return el.parentNode; },
    textContent: '',
    innerHTML: '',
    value: '',
    clientWidth: 800,
    clientHeight: 600,
    width: 800,
    height: 600,
    classList: {
      add(...names) { for (const n of names) classSet.add(n); },
      remove(...names) { for (const n of names) classSet.delete(n); },
      contains(n) { return classSet.has(n); },
    },
    get className() { return [...classSet].join(' '); },
    set className(val) {
      classSet.clear();
      for (const part of String(val || '').split(/\s+/).filter(Boolean)) classSet.add(part);
    },
    setAttribute(k, v) {
      const key = String(k);
      const str = String(v);
      attrs.set(key, str);
      if (key === 'class') el.className = str;
      if (key.startsWith('data-')) {
        const camel = key.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        el.dataset[camel] = str;
      }
    },
    getAttribute(k) { return attrs.has(String(k)) ? attrs.get(String(k)) : null; },
    removeAttribute(k) { attrs.delete(String(k)); },
    hasAttribute(k) { return attrs.has(String(k)); },
    get firstElementChild() { return children[0] || null; },
    get nextElementSibling() {
      if (!el.parentNode) return null;
      const sibs = el.parentNode.children;
      const idx = sibs.indexOf(el);
      return idx >= 0 && idx + 1 < sibs.length ? sibs[idx + 1] : null;
    },
    getBoundingClientRect() { return { left: 0, top: 0, width: el.width || 800, height: el.height || 600, right: el.width || 800, bottom: el.height || 600 }; },
    contains(other) {
      for (let cur = other; cur; cur = cur.parentNode) {
        if (cur === el) return true;
      }
      return false;
    },
    insertBefore(child, refNode) {
      if (child.parentNode) child.parentNode.removeChild(child);
      child.parentNode = el;
      const idx = refNode ? children.indexOf(refNode) : -1;
      if (idx >= 0) children.splice(idx, 0, child);
      else children.push(child);
      return child;
    },
    appendChild(child) {
      if (child.parentNode) child.parentNode.removeChild(child);
      child.parentNode = el;
      children.push(child);
      return child;
    },
    removeChild(child) {
      const idx = children.indexOf(child);
      if (idx >= 0) {
        children.splice(idx, 1);
        child.parentNode = null;
      }
      return child;
    },
    remove() { if (el.parentNode) el.parentNode.removeChild(el); },
    replaceChildren(...next) {
      for (const c of children) c.parentNode = null;
      children.length = 0;
      for (const c of next) el.appendChild(c);
    },
    addEventListener(type, fn) {
      if (!listeners.has(type)) listeners.set(type, []);
      listeners.get(type).push(fn);
    },
    removeEventListener(type, fn) {
      const arr = listeners.get(type);
      if (!arr) return;
      const idx = arr.indexOf(fn);
      if (idx >= 0) arr.splice(idx, 1);
    },
    dispatchEvent(evt) {
      if (!evt.target) evt.target = el;
      for (let cur = el; cur; cur = cur.parentNode) {
        const arr = cur._listeners?.get(evt.type) || [];
        for (const fn of [...arr]) fn.call(cur, evt);
        if (evt.cancelBubble) break;
      }
      return !evt.defaultPrevented;
    },
    focus() { if (ownerDoc) ownerDoc.activeElement = el; },
    blur() { if (ownerDoc && ownerDoc.activeElement === el) ownerDoc.activeElement = null; },
    getContext() {
      return {
        font: '12px sans-serif',
        fillStyle: '#000',
        strokeStyle: '#000',
        globalAlpha: 1,
        globalCompositeOperation: 'source-over',
        lineWidth: 1,
        lineCap: 'round',
        lineJoin: 'round',
        miterLimit: 3,
        imageSmoothingEnabled: true,
        save() {},
        restore() {},
        setTransform() {},
        transform() {},
        scale() {},
        translate() {},
        rotate() {},
        beginPath() {},
        closePath() {},
        moveTo() {},
        lineTo() {},
        quadraticCurveTo() {},
        bezierCurveTo() {},
        rect() {},
        ellipse() {},
        arc() {},
        fill() {},
        stroke() {},
        clip() {},
        clearRect() {},
        fillRect() {},
        strokeRect() {},
        drawImage() {},
        getImageData: (_x, _y, w, h) => ({ data: new Uint8ClampedArray((w || 1) * (h || 1) * 4), width: w || 1, height: h || 1 }),
        putImageData() {},
        createImageData: (w, h) => ({ data: new Uint8ClampedArray((w || 1) * (h || 1) * 4), width: w || 1, height: h || 1 }),
        createLinearGradient: () => ({ addColorStop() {} }),
        createRadialGradient: () => ({ addColorStop() {} }),
        createPattern: () => ({}),
        measureText: (s) => ({ width: String(s).length * 7 }),
      };
    },
    _listeners: listeners,
  };
  return el;
}

function collectBySelector(rootEl, predicate, out = []) {
  if (predicate(rootEl)) out.push(rootEl);
  for (const child of rootEl.children || []) collectBySelector(child, predicate, out);
  return out;
}

test('FO-01, FO-07, FO-02, FO-06, FO-15: AS2 & AS3 HTML text, format inheritance, timeline swapDepths, descendant cleanup, and >4096 Graphics auto-chunking', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const directory = fs.mkdtempSync(path.resolve(root, 'target/t496-runtime-'));
  let player = null;
  try {
    const project = {
      format_version: 6,
      name: 'T496 Fallout Runtime Probe',
      width: 400,
      height: 300,
      fps: 30,
      background: [16, 24, 20, 255],
      root: 'sym-root',
      script_language: 'action_script2',
      symbols: [
        {
          id: 'sym-root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 3,
            layers: [
              {
                id: 'layer-1',
                name: 'Entities',
                visible: true,
                locked: false,
                keyframes: [
                  {
                    frame: 0,
                    elements: [
                      {
                        id: 'el-placed',
                        name: 'placedInst',
                        transform: { x: 40, y: 50, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'instance', symbol_id: 'sym-unit', playback: 'independent' },
                        scripts: [],
                      },
                      {
                        id: 'el-other',
                        name: 'otherPlaced',
                        transform: { x: 100, y: 120, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'instance', symbol_id: 'sym-unit', playback: 'independent' },
                        scripts: [],
                      },
                    ],
                    tween: null,
                    label: '',
                  },
                  {
                    frame: 1,
                    elements: [
                      {
                        id: 'el-placed',
                        name: 'placedInst',
                        transform: { x: 260, y: 270, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'instance', symbol_id: 'sym-unit', playback: 'independent' },
                        scripts: [],
                      },
                      {
                        id: 'el-other',
                        name: 'otherPlaced',
                        transform: { x: 100, y: 120, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'instance', symbol_id: 'sym-unit', playback: 'independent' },
                        scripts: [],
                      },
                    ],
                    tween: null,
                    label: '',
                  },
                  {
                    frame: 2,
                    elements: [],
                    tween: null,
                    label: '',
                  },
                ],
              },
            ],
            scripts: [
              {
                frame: 0,
                script: {
                  language: 'action_script2',
                  source: `
stop();
// 1. FO-01 + FO-07: permissive malformed HTML + setNewTextFormat & textColor inheritance
this.createTextField("logTf", 10, 8, 8, 360, 120);
var logTf:TextField = this.logTf;
logTf.multiline = true;
logTf.wordWrap = true;
logTf.html = true;
var bodyFmt:TextFormat = new TextFormat();
bodyFmt.font = "Courier New";
bodyFmt.size = 13;
bodyFmt.color = 0xBBF7D0;
bodyFmt.leading = 3;
logTf.setNewTextFormat(bodyFmt);
logTf.htmlText = "Rats & Brood | HP < 10 | <3 | <unknown>inner</unknown> | <b><i>overlap</b></i> | <font color='#FACC15'>Gold</font> Plain2 | Alert <foo";

this.createTextField("colorTf", 11, 8, 130, 360, 40);
var colorTf:TextField = this.colorTf;
colorTf.html = true;
colorTf.textColor = 0x86EFAC;
colorTf.htmlText = "Plain <font color='#FACC15'>Gold</font> Tail";

// 2. FO-02: negative-depth removeMovieClip is no-op, swapDepths promotes timeline clip
var origDepthA:Number = this.placedInst.getDepth();
var origDepthB:Number = this.otherPlaced.getDepth();
this.otherPlaced.removeMovieClip();
var otherStillThere:Boolean = (this.otherPlaced != undefined && this.otherPlaced._x == 100);
this.placedInst._x = 144;
this.placedInst.swapDepths(200);
var promotedDepth:Number = this.placedInst.getDepth();

// 3. FO-06: descendant TextField cleanup on removeMovieClip and depth replacement
var holder1:MovieClip = this.createEmptyMovieClip("flt_0", 210);
holder1.createTextField("tf", 1, 0, 0, 100, 20);
holder1.tf.text = "LOOTED_OLD!";
var holder1Replaced:MovieClip = this.createEmptyMovieClip("flt_0_new", 210);

var holder2:MovieClip = this.createEmptyMovieClip("flt_1", 211);
holder2.createTextField("tf", 1, 0, 0, 100, 20);
holder2.tf.text = "LOOTED!";
holder2.removeMovieClip();

// 4. FO-15: > 4096 procedural drawing ops auto-chunked without crashing
var terrain:MovieClip = this.createEmptyMovieClip("terrainClip", 50);
terrain.lineStyle(1, 0x4ADE80, 90);
var h:Number = 0;
while (h < 900) {
  var hx:Number = (h % 30) * 10;
  var hy:Number = Math.floor(h / 30) * 8;
  terrain.beginFill(0x1E293B, 100);
  terrain.moveTo(hx, hy);
  terrain.lineTo(hx + 8, hy);
  terrain.lineTo(hx + 8, hy + 6);
  terrain.lineTo(hx, hy + 6);
  terrain.endFill();
  h = h + 1;
}

trace("BOOT_OK", origDepthA < 0, origDepthB < 0, otherStillThere, promotedDepth);
`,
                },
              },
            ],
          },
        },
        {
          id: 'sym-unit',
          name: 'UnitSym',
          kind: 'movie_clip',
          linkage_id: 'UnitSym',
          timeline: {
            frame_count: 1,
            layers: [
              {
                id: 'u-layer',
                name: 'Shape',
                visible: true,
                locked: false,
                keyframes: [
                  {
                    frame: 0,
                    elements: [
                      {
                        id: 'u-rect',
                        name: '',
                        transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'rectangle', width: 20, height: 20, fill: [74, 222, 128, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
                        scripts: [],
                      },
                    ],
                    tween: null,
                    label: '',
                  },
                ],
              },
            ],
            scripts: [],
          },
        },
      ],
    };

    const srcPath = path.resolve(directory, 'probe.nflash');
    const outDir = path.resolve(directory, 'html5');
    fs.writeFileSync(srcPath, JSON.stringify(project));
    const exported = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--', srcPath, '--output', outDir], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
    });
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);

    const wasmBytes = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm'));
    const sandbox = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(path.join(outDir, 'bundle.js'), 'utf8'), sandbox);
    const bundle = vm.runInContext('NeoFlashBundle', sandbox);
    vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'), 'utf8'), sandbox);
    sandbox.wasmBytes = wasmBytes;
    const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', sandbox);
    player = new wasm.WebPlayer(JSON.stringify(bundle.project));

    const { host, traceCalls } = createRuntimeHost(
      root,
      player,
      Array.from(bundle.scripts, (item) => ({
        script: { language: item.language, source: item.source },
        code: item.code,
        location: item.location,
      })),
      { bundle, context: { measureText: (line) => ({ width: line.length * 7 }) } }
    );
    host.dispatchScripts();

    assert.deepEqual(traceCalls[0], ['BOOT_OK', true, true, true, 200]);

    // Verify FO-01 & FO-07 on logTf and colorTf
    const snap0 = JSON.parse(player.runtime_snapshot_json());
    const logObj = snap0.objects.find((o) => o.name === 'logTf');
    assert.ok(logObj?.text, 'logTf must exist on Stage');
    assert.equal(
      logObj.text.text,
      'Rats & Brood | HP < 10 | <3 | inner | overlap | Gold Plain2 | Alert <foo'
    );
    const logRuns = logObj.text.format_runs;
    assert.ok(Array.isArray(logRuns) && logRuns.length >= 4);
    assert.equal(logRuns[0].format.color, 0xBBF7D0, 'unwrapped prefix inherits setNewTextFormat color');
    assert.equal(logRuns[0].format.font, 'Courier New', 'unwrapped prefix inherits setNewTextFormat font');
    assert.equal(logRuns[0].format.size, 13, 'unwrapped prefix inherits setNewTextFormat size');
    assert.equal(logRuns[0].format.leading, 3, 'unwrapped prefix inherits setNewTextFormat leading');
    const goldRun = logRuns.find((r) => r.format.color === 0xFACC15);
    assert.ok(goldRun, 'Gold <font> span overrides color to #FACC15');
    assert.equal(goldRun.format.font, 'Courier New', '<font color=...> preserves base font');
    assert.equal( logRuns.at(-1).format.color, 0xBBF7D0, 'unwrapped suffix after </font> returns to base color');

    const colorObj = snap0.objects.find((o) => o.name === 'colorTf');
    assert.ok(colorObj?.text);
    assert.equal(colorObj.text.text, 'Plain Gold Tail');
    assert.equal(colorObj.text.format_runs[0].format.color, 0x86EFAC, 'unwrapped run inherits tf.textColor');
    assert.equal(colorObj.text.format_runs[1].format.color, 0xFACC15);
    assert.equal(colorObj.text.format_runs[2].format.color, 0x86EFAC, 'trailing unwrapped run inherits tf.textColor');

    // Also verify AS3 TextField defaultTextFormat + textColor inheritance on htmlText
    const TextField = host.program().type('flash.text.TextField');
    const TextFormat = host.program().type('flash.text.TextFormat');
    const as3Tf = new TextField();
    as3Tf.defaultTextFormat = new TextFormat('Verdana', 15, 0x38BDF8, true, false, false);
    as3Tf.htmlText = 'Alpha & <font color="#FACC15">Beta</font> Gamma';
    assert.equal(as3Tf.text, 'Alpha & Beta Gamma');
    const as3Fmt0 = as3Tf.getTextFormat(0, 5);
    const as3FmtMid = as3Tf.getTextFormat(8, 12);
    const as3FmtEnd = as3Tf.getTextFormat(13, 18);
    assert.equal(as3Fmt0.color, 0x38BDF8);
    assert.equal(as3Fmt0.font, 'Verdana');
    assert.equal(as3Fmt0.bold, true);
    assert.equal(as3FmtMid.color, 0xFACC15);
    assert.equal(as3FmtEnd.color, 0x38BDF8);

    // Verify FO-06: zero orphaned "LOOTED!" or "LOOTED_OLD!" draws remain on Stage!
    const textDraws = snap0.draws.filter((d) => d.kind?.type === 'text_field');
    assert.equal(
      textDraws.some((d) => String(d.kind.field.text).includes('LOOTED')),
      false,
      'Descendant TextFields of removed/replaced MovieClips must be completely retired from Stage draws'
    );

    // Verify FO-15: 900 hex quads (5,400+ ops > 4,096) rendered into terrainClip without error
    const terrainDraws = snap0.draws.filter((d) => d.name === 'terrainClip');
    assert.ok(terrainDraws.length >= 900, `Expected >= 900 procedural shapes in terrainClip, got ${terrainDraws.length}`);

    // Verify FO-02 across timeline frames: Frame 1 keyframe move does NOT reset placedInst._x or create duplicate!
    const scene = host.proxyFor([]);
    const placed = scene.placedInst;
    scene.gotoAndStop(2);
    host.dispatchScripts();
    assert.equal(placed._x, 144, 'Promoted clip ignores timeline keyframe transform reset on Frame 2');
    assert.equal(player.as2_get_depth(JSON.stringify(placed.toJSON().object_ref)), 200);
    const snapF2 = JSON.parse(player.runtime_snapshot_json());
    assert.equal(
      snapF2.objects.filter((o) => o.name === 'placedInst' && o.on_stage).length,
      1,
      'Timeline frame advance must not duplicate promoted instance'
    );

    // Advance to Frame 3 (blank keyframe): promoted clip at depth 200 survives on Stage, then removeMovieClip removes it!
    scene.gotoAndStop(3);
    host.dispatchScripts();
    assert.equal(placed.parent, scene, 'Promoted positive-depth clip survives across blank timeline keyframe');
    placed.removeMovieClip();
    assert.equal(placed.parent, null, 'Promoted clip at depth >= 0 is removed cleanly by removeMovieClip()');
  } finally {
    player?.free();
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('FO-12: Opt-in AS3 & AS2 Right-Click events and native contextmenu suppression', () => {
  const Events = require('../events.js');
  const StageMod = require('../stage.js');
  const LegacyInput = require('../legacy-input.js');

  const hub = Events.create({
    alive: () => true,
    parentOf: (t) => t?.parent ?? null,
    localPoint: (_t, x, y) => [x, y],
    globalPoint: (_t, x, y) => [x, y],
  });

  const surface = makeFakeElement('div');
  const canvas = makeFakeElement('canvas');
  surface.appendChild(canvas);

  const sprite = { name: 'unitSprite' };
  const rootDisp = { name: 'rootDisp' };
  Object.assign(sprite, hub.methods(sprite));
  Object.assign(rootDisp, hub.methods(rootDisp));
  let legacyRef = null;

  const stageHost = StageMod.create({
    events: hub,
    width: 400,
    height: 300,
    canvas,
    surface,
    alive: () => true,
    parentOf: (t) => (t === sprite ? rootDisp : (t === rootDisp ? stageHost.stage : null)),
    resolveInput: (t) => t,
    hitTest: () => sprite,
    coordinates: (ev) => [ev.clientX || 50, ev.clientY || 60],
    shouldSuppressContextMenu: () => Boolean(legacyRef?.hasRightMouseListeners?.()),
  });
  sprite.parent = rootDisp;
  rootDisp.parent = stageHost.stage;

  legacyRef = LegacyInput.create({
    stage: stageHost.stage,
    onVisibility() {},
  });

  stageHost.attach();

  // 1. Default (no right-click listeners, showDefaultContextMenu === true): native contextmenu is NOT prevented.
  const defaultCtxEvt = { type: 'contextmenu', clientX: 50, clientY: 60, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
  canvas.dispatchEvent(defaultCtxEvt);
  assert.equal(defaultCtxEvt.defaultPrevented, false, 'Native context menu stays enabled when no right-click opt-in exists');

  // 2. AS3 opt-in: registering MouseEvent.RIGHT_CLICK / RIGHT_MOUSE_DOWN / RIGHT_MOUSE_UP dispatches events & suppresses contextmenu.
  const as3Log = [];
  const onAs3Down = (e) => as3Log.push(`down:${e.type}`);
  const onAs3Up = (e) => as3Log.push(`up:${e.type}`);
  const onAs3Click = (e) => as3Log.push(`click:${e.type}`);
  sprite.addEventListener(Events.MouseEvent.RIGHT_MOUSE_DOWN, onAs3Down);
  sprite.addEventListener(Events.MouseEvent.RIGHT_MOUSE_UP, onAs3Up);
  sprite.addEventListener(Events.MouseEvent.RIGHT_CLICK, onAs3Click);

  const as3CtxEvt = { type: 'contextmenu', clientX: 50, clientY: 60, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
  canvas.dispatchEvent(as3CtxEvt);
  assert.equal(as3CtxEvt.defaultPrevented, true, 'AS3 right-click listener suppresses native contextmenu on canvas');

  const surfaceCtxEvt = { type: 'contextmenu', clientX: 50, clientY: 60, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
  surface.dispatchEvent(surfaceCtxEvt);
  assert.equal(surfaceCtxEvt.defaultPrevented, true, 'AS3 right-click listener suppresses native contextmenu on #stage-surface');

  canvas.dispatchEvent({ type: 'pointerdown', button: 2, buttons: 2, isPrimary: true, clientX: 50, clientY: 60 });
  canvas.dispatchEvent({ type: 'pointerup', button: 2, buttons: 0, isPrimary: true, clientX: 50, clientY: 60 });
  // Subsequent browser auxclick for the same click must not duplicate rightClick!
  canvas.dispatchEvent({ type: 'auxclick', button: 2, clientX: 50, clientY: 60 });
  assert.deepEqual(as3Log, ['down:rightMouseDown', 'up:rightMouseUp', 'click:rightClick']);

  // Remove AS3 listeners -> contextmenu is allowed again until AS2 listener opts in.
  sprite.removeEventListener(Events.MouseEvent.RIGHT_MOUSE_DOWN, onAs3Down);
  sprite.removeEventListener(Events.MouseEvent.RIGHT_MOUSE_UP, onAs3Up);
  sprite.removeEventListener(Events.MouseEvent.RIGHT_CLICK, onAs3Click);
  const restoredCtxEvt = { type: 'contextmenu', clientX: 50, clientY: 60, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
  canvas.dispatchEvent(restoredCtxEvt);
  assert.equal(restoredCtxEvt.defaultPrevented, false, 'Removing AS3 right-click listeners restores native contextmenu');

  const stage2Log = [];
  const as2Listener = {
    onRightMouseDown() { stage2Log.push('as2Down'); },
    onRightMouseUp() { stage2Log.push('as2Up'); },
  };
  legacyRef.Mouse.addListener(as2Listener);
  assert.equal(legacyRef.hasRightMouseListeners(), true);

  const as2CtxEvt = { type: 'contextmenu', clientX: 50, clientY: 60, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
  surface.dispatchEvent(as2CtxEvt);
  assert.equal(as2CtxEvt.defaultPrevented, true, 'AS2 Mouse.onRightMouseDown listener suppresses native contextmenu');

  canvas.dispatchEvent({ type: 'pointerdown', button: 2, buttons: 2, isPrimary: true, clientX: 50, clientY: 60 });
  canvas.dispatchEvent({ type: 'pointerup', button: 2, buttons: 0, isPrimary: true, clientX: 50, clientY: 60 });
  assert.deepEqual(stage2Log, ['as2Down', 'as2Up']);

  legacyRef.dispose();
  stageHost.dispose();
});

test('FO-13: DOM mode coalesces consecutive procedural vector paths per MovieClip into 1 shared <svg> and >32 into 1 <canvas> island', async () => {
  const DomLayout = require('../dom-layout.js');
  const fakeDoc = {
    activeElement: null,
    defaultView: globalThis,
    createElement: (tag) => makeFakeElement(tag, fakeDoc),
    createElementNS: (_ns, tag) => makeFakeElement(tag, fakeDoc),
  };
  const surfaceEl = makeFakeElement('div', fakeDoc);
  const paintedIslands = [];
  const domHost = DomLayout.createDomStageHost({
    document: fakeDoc,
    surface: surfaceEl,
    project: { width: 400, height: 300, background: [20, 20, 20, 255] },
    paintCanvasIsland(_canvasEl, draws) {
      paintedIslands.push(draws.length);
      return { left: 0, top: 0, width: 200, height: 150 };
    },
  });

  const rootRef = { id: '1', generation: '1' };
  const entityRef = { id: '2', generation: '1' };
  const terrainRef = { id: '3', generation: '1' };

  // 8 procedural vector paths on entityClip -> must coalesce into 1 shared <svg>!
  const entityDraws = Array.from({ length: 8 }, (_, idx) => ({
    element_id: `runtime-2-${idx}`,
    name: 'entityClip',
    instance_path: [],
    layer_id: '',
    transform: [1, 0, 0, 1, 20 + idx * 4, 30],
    alpha: 1,
    procedural_graphics: true,
    owner_ref: entityRef,
    clip_ids: [],
    filter_id: null,
    kind: {
      type: 'rectangle',
      width: 10,
      height: 10,
      fill: [74, 222, 128, 255],
      stroke: [0, 0, 0, 0],
      stroke_width: 0,
    },
  }));

  // 45 procedural vector paths on terrainClip (> 32 threshold) -> must coalesce into 1 <canvas> island!
  const terrainDraws = Array.from({ length: 45 }, (_, idx) => ({
    element_id: `runtime-3-${idx}`,
    name: 'terrainClip',
    instance_path: [],
    layer_id: '',
    transform: [1, 0, 0, 1, (idx % 9) * 16, Math.floor(idx / 9) * 16],
    alpha: 1,
    procedural_graphics: true,
    owner_ref: terrainRef,
    clip_ids: [],
    filter_id: null,
    kind: {
      type: 'path',
      commands: [
        { op: 'move_to', x: 0, y: 0 },
        { op: 'line_to', x: 12, y: 0 },
        { op: 'line_to', x: 12, y: 12 },
        { op: 'close' },
      ],
      fill_rule: 'evenodd',
      fill: [30, 41, 59, 255],
      stroke: [74, 222, 128, 255],
      stroke_width: 1,
    },
  }));

  domHost.render({
    snapshot: {
      objects: [
        { object_ref: rootRef, parent_ref: null, owner: 'movie_clip', name: 'root', world: [1, 0, 0, 1, 0, 0], on_stage: true },
        { object_ref: entityRef, parent_ref: rootRef, owner: 'movie_clip', name: 'entityClip', world: [1, 0, 0, 1, 20, 30], on_stage: true },
        { object_ref: terrainRef, parent_ref: rootRef, owner: 'movie_clip', name: 'terrainClip', world: [1, 0, 0, 1, 0, 0], on_stage: true },
      ],
      draws: [...terrainDraws, ...entityDraws],
      semantics: [],
      clips: [],
      filter_groups: [],
    },
    semantics: [],
    width: 400,
    height: 300,
    originX: 0,
    originY: 0,
    scale: { x: 1, y: 1 },
  });

  const svgNodes = collectBySelector(surfaceEl, (el) => el.classList?.contains('nf-dom-svg'));
  const islandNodes = collectBySelector(surfaceEl, (el) => el.classList?.contains('nf-dom-canvas-island'));

  assert.equal(svgNodes.length, 1, `8 procedural shapes in entityClip must coalesce into 1 shared <svg>, got ${svgNodes.length}`);
  assert.equal((svgNodes[0].innerHTML.match(/<rect\b/g) || []).length, 8, 'Shared <svg> must contain all 8 <rect> primitives');
  assert.equal(islandNodes.length, 1, `45 procedural shapes in terrainClip must rasterize into 1 <canvas> island, got ${islandNodes.length}`);
  assert.deepEqual(paintedIslands, [45]);

  domHost.dispose();
});

test('FO-08: Structured fatal HTML5 runtime failure telemetry sets dataset.neoflashState, dispatches neoflash-fatal-error, and logs unsuppressed console.error without recursion', async () => {
  const directory = fs.mkdtempSync(path.resolve(root, 'target/t496-fatal-'));
  try {
    const project = {
      format_version: 6,
      name: 'Fatal Telemetry Probe',
      width: 320,
      height: 200,
      fps: 30,
      background: [0, 0, 0, 255],
      root: 'sym-root',
      script_language: 'java_script',
      symbols: [
        {
          id: 'sym-root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [{ id: 'l1', name: 'Layer 1', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], tween: null, label: '' }] }],
            scripts: [
              {
                frame: 0,
                script: {
                  language: 'java_script',
                  source: 'this.stage.addEventListener("mouseDown", () => { throw new Error("Simulated Pip-Boy Fatal Crash"); });',
                },
              },
            ],
          },
        },
      ],
    };
    const srcPath = path.resolve(directory, 'fatal.nflash');
    const outDir = path.resolve(directory, 'html5');
    fs.writeFileSync(srcPath, JSON.stringify(project));
    const exported = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--', srcPath, '--output', outDir], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
    });
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);

    const indexHtml = fs.readFileSync(path.join(outDir, 'index.html'), 'utf8');
    const docEl = makeFakeElement('html');
    const bodyEl = makeFakeElement('body');
    const byId = new Map();
    const fakeDoc = {
      documentElement: docEl,
      body: bodyEl,
      currentScript: null,
      activeElement: null,
      fullscreenEnabled: false,
      createElement: (tag) => makeFakeElement(tag, fakeDoc),
      createElementNS: (_ns, tag) => makeFakeElement(tag, fakeDoc),
      getElementById: (id) => byId.get(id) || null,
      querySelectorAll: () => [],
      addEventListener() {},
      removeEventListener() {},
    };
    docEl.ownerDocument = fakeDoc;
    bodyEl.ownerDocument = fakeDoc;
    docEl.appendChild(bodyEl);
    const registerEl = (id, tag = 'div', parent = bodyEl) => {
      const el = makeFakeElement(tag, fakeDoc);
      el.id = id;
      byId.set(id, el);
      parent.appendChild(el);
      return el;
    };
    const viewport = registerEl('viewport', 'div', bodyEl);
    const space = registerEl('stage-space', 'div', viewport);
    const surface = registerEl('stage-surface', 'div', space);
    const canvas = registerEl('stage', 'canvas', surface);
    registerEl('semantic-target', 'div', surface);
    registerEl('loading', 'div', bodyEl);
    const errorBanner = registerEl('error', 'div', bodyEl);
    registerEl('status', 'div', bodyEl);
    registerEl('project-name', 'div', bodyEl);
    registerEl('load-info', 'div', bodyEl);
    registerEl('trace', 'div', bodyEl);
    for (const m of indexHtml.matchAll(/<([a-zA-Z0-9-]+)[^>]*\bid="([^"]+)"/g)) {
      const [, tag, id] = m;
      if (!byId.has(id)) registerEl(id, tag, bodyEl);
    }

    const windowListeners = new Map();
    const consoleErrors = [];
    let relaySuppressedDuringFatalConsole = null;

    class FakeCustomEvent {
      constructor(type, init = {}) {
        this.type = type;
        this.cancelable = Boolean(init.cancelable);
        this.detail = init.detail ?? null;
        this.defaultPrevented = false;
      }
      preventDefault() {
        if (this.cancelable) this.defaultPrevented = true;
      }
    }

    const ctx = vm.createContext({
      TextEncoder,
      TextDecoder,
      WebAssembly,
      URL,
      Request,
      Response,
      Uint8Array,
      Float32Array,
      ArrayBuffer,
      DataView,
      Promise,
      Map,
      Set,
      WeakMap,
      WeakSet,
      Math,
      Date,
      JSON,
      CustomEvent: FakeCustomEvent,
      Event: FakeCustomEvent,
      btoa: (s) => Buffer.from(s, 'binary').toString('base64'),
      atob: (s) => Buffer.from(s, 'base64').toString('binary'),
      queueMicrotask,
      setTimeout,
      clearTimeout,
      requestAnimationFrame: () => 1,
      cancelAnimationFrame() {},
      performance: { now: () => 0 },
      navigator: {},
      location: { href: 'http://127.0.0.1/index.html', protocol: 'http:', hostname: '127.0.0.1' },
      screen: { width: 1920, height: 1080 },
      devicePixelRatio: 1,
      document: fakeDoc,
      addEventListener(type, fn) {
        if (!windowListeners.has(type)) windowListeners.set(type, []);
        windowListeners.get(type).push(fn);
      },
      removeEventListener() {},
      dispatchEvent(evt) {
        for (const fn of windowListeners.get(evt.type) || []) fn(evt);
        return !evt.defaultPrevented;
      },
      console: {
        log() {},
        info() {},
        warn() {},
        error(...args) {
          if (relaySuppressedDuringFatalConsole === null) {
            relaySuppressedDuringFatalConsole = Boolean(ctx.__neoflashSuppressConsoleRelay);
          }
          consoleErrors.push(args);
        },
      },
    });
    ctx.window = ctx;
    ctx.globalThis = ctx;
    ctx.self = ctx;

    const fatalEvents = [];
    ctx.addEventListener('neoflash-fatal-error', (ev) => {
      fatalEvents.push(ev);
    });

    const scriptFiles = [...indexHtml.matchAll(/<script[^>]+src="([^"]+)"/g)].map((m) => m[1]);
    for (const file of scriptFiles) {
      if (file === 'bundle.js') {
        vm.runInContext(fs.readFileSync(path.join(outDir, 'bundle.js'), 'utf8'), ctx);
        ctx.NeoFlashBundle.wasm = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm')).toString('base64');
        continue;
      }
      const fullPath = file === 'neoflash_web.js'
        ? path.join(runtimeDir, file)
        : (fs.existsSync(path.join(webDir, file)) ? path.join(webDir, file) : path.join(outDir, file));
      vm.runInContext(fs.readFileSync(fullPath, 'utf8'), ctx);
    }

    // Wait for async mountInstance boot promise to settle
    for (let i = 0; i < 30 && !docEl.dataset.neoflashState; i++) {
      await new Promise((r) => setTimeout(r, 15));
    }
    assert.equal(
      docEl.dataset.neoflashState,
      'ready',
      `Clean mount sets documentElement.dataset.neoflashState = "ready" (got ${docEl.dataset.neoflashState}: ${fatalEvents[0]?.detail?.message || errorBanner.textContent})`
    );
    assert.equal(fatalEvents.length, 0);

    // Trigger an input callback failure through stageHost's onError / pointer listener!
    canvas.dispatchEvent({ type: 'pointerdown', button: 0, buttons: 1, isPrimary: true, clientX: 20, clientY: 20 });

    assert.equal(docEl.dataset.neoflashState, 'failed', 'Fatal failure transitions dataset.neoflashState to "failed"');
    assert.equal(errorBanner.style.display, 'block');
    assert.equal(fatalEvents.length, 1, 'Exactly one neoflash-fatal-error CustomEvent must dispatch');
    assert.equal(fatalEvents[0].cancelable, true, 'neoflash-fatal-error CustomEvent must be cancelable');
    assert.match(fatalEvents[0].detail.message, /Simulated Pip-Boy Fatal Crash/);
    assert.equal(fatalEvents[0].detail.error?.message, 'Simulated Pip-Boy Fatal Crash');
    assert.equal(relaySuppressedDuringFatalConsole, true, 'Fatal console.error suppresses unmapped preview relay while still invoking console.error once');
    assert.equal(consoleErrors.length, 1, 'Fatal console.error must fire once without recursive loops');
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
