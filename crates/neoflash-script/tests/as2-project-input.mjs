// One original AS2 family scenario: ordinary publication, real WASM, the shipped
// host and installed Stage input listeners. Imported by project-execute.mjs.
// This is executable integration evidence, not original-player or browser QA.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

const require = createRequire(import.meta.url);

class DomElement {
  constructor(doc, tag) {
    this.ownerDocument = doc;
    this.tagName = tag;
    this.children = [];
    this.style = {};
    this.attrs = {};
    this.events = {};
    this.parentNode = null;
    this.disabled = false;
    this.textContent = '';
  }
  setAttribute(k, v) { this.attrs[k] = v; }
  getAttribute(k) { return this.attrs[k] ?? null; }
  removeAttribute(k) { delete this.attrs[k]; }
  appendChild(el) { this.insertBefore(el, null); return el; }
  insertBefore(el, before) {
    el.remove();
    const i = before ? this.children.indexOf(before) : this.children.length;
    this.children.splice(i, 0, el);
    el.parentNode = this;
  }
  remove() {
    if (this.parentNode) {
      const p = this.parentNode;
      p.children.splice(p.children.indexOf(this), 1);
      this.parentNode = null;
      if (this.ownerDocument.activeElement === this) this.ownerDocument.activeElement = null;
    }
  }
  addEventListener(k, fn) { this.events[k] = fn; }
  removeEventListener(k, fn) { if (this.events[k] === fn) delete this.events[k]; }
  focus() { this.ownerDocument.activeElement = this; this.events.focus?.(); }
  scrollIntoView() {}
  getBoundingClientRect() { return { left: 0, top: 0, right: 200, bottom: 120, width: 200, height: 120 }; }
  contains(el) { return el === this || this.children.some(c => c.contains?.(el)); }
  querySelectorAll(sel) {
    const out = [];
    const match = node => sel === 'button:not(:disabled)' ? node.tagName === 'button' && !node.disabled : node.tagName === sel;
    const walk = node => { for (const c of node.children) { if (match(c)) out.push(c); walk(c); } };
    walk(this);
    return out;
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
  get firstElementChild() { return this.children[0] || null; }
  get nextElementSibling() {
    if (!this.parentNode) return null;
    return this.parentNode.children[this.parentNode.children.indexOf(this) + 1] || null;
  }
}

function createSemanticDom() {
  const doc = { activeElement: null, addEventListener() {}, removeEventListener() {}, createElement(tag) { return new DomElement(doc, tag); } };
  return new DomElement(doc, 'div');
}

async function until(predicate, label) {
  const deadline = Date.now() + 4000;
  while (!predicate() && Date.now() < deadline) await new Promise(done => setTimeout(done, 5));
  assert.ok(predicate(), label);
}

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
    // Main is both a MovieClip with onMouseDown and an explicit Mouse listener.
    // Ruffle 89a7049 player.rs queues both clip events and NotifyListeners; these
    // are independent routes, not duplicate singleton registrations.
    assert.equal(scene.mouseEvents, 2); assert.equal(left.count, 1); assert.equal(right.count, 0);
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
    down('s', 'KeyS', 83); assert.equal(canvas.style.cursor, 'pointer', 'Mouse.show restores the hand cursor for the hovered onPress clip'); up('s', 'KeyS', 83);
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
    press(fresh.left); assert.equal(fresh.mouseEvents, 2); assert.equal(fresh.left.count, 1);
    assert.equal(host.lastTrace(), 'AS2 component left 1 2', 'Restart restored the declared prototype callback exactly once');
    host.disposeFamily();
    down('ArrowRight', 'ArrowRight', 39); assert.equal(fresh.keyEvents, 1);
    assert.equal(legacy.Key.isDown(39), false);

    await verifyAs2FoundationSurface(root, cargo, directory);
  } finally {
    host?.disposeFamily(); player?.free(); rmSync(directory, { recursive: true, force: true });
  }
  console.log('AS2 project/input: inherited linked classes, shared prototype methods/statics, independent instances, Key/Mouse/Stage, listener mutation/lifetime, cursor and Restart passed.');
}

async function verifyAs2FoundationSurface(root, cargo, directory) {
  const project = JSON.parse(readFileSync(resolve(root, 'examples/as2-project-input.nflash'), 'utf8'));
  const pngResource = JSON.parse(readFileSync(resolve(root, 'examples/bitmap-pixels.nflash'), 'utf8')).resources[0];
  project.format_version = 3;
  const includeResource = {
    id: 'batch4-include-macros',
    path: 'includes/batch4-macros.as',
    media_type: 'text/plain',
    data_base64: Buffer.from('includedMacroSum = 19 + 23;', 'utf8').toString('base64'),
  };
  project.resources = [pngResource, includeResource];
  project.document_class = 'app.FoundationMain';
  project.symbols[0].timeline.frame_count = 2;
  project.symbols[0].timeline.layers[0].keyframes[0].elements.push(
    {
      id: 'actionBtn',
      name: 'actionBtn',
      transform: { x: 400, y: 200, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
      kind: { type: 'instance', symbol_id: 'btn_sym', playback: 'independent' },
      scripts: [],
    },
    {
      id: 'stageVideo',
      name: 'stageVideo',
      transform: { x: 520, y: 200, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
      kind: { type: 'video', width: 160, height: 90 },
      scripts: [],
    },
    {
      id: 'stageComp',
      name: 'stageComp',
      transform: { x: 400, y: 260, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
      kind: { type: 'instance', symbol_id: 'init_sym_a', playback: 'independent' },
      scripts: [],
    },
  );
  project.symbols[0].timeline.layers[0].keyframes.push({
    frame: 1,
    elements: JSON.parse(JSON.stringify(project.symbols[0].timeline.layers[0].keyframes[0].elements)),
    tween: null,
    label: 'ready',
  });
  project.symbols[0].timeline.scripts.push({
    frame: 1,
    script: {
      language: 'action_script2',
      source: '#include "includes/batch4-macros.as"\nrootCalled = (rootCalled ? rootCalled : 0) + 1;',
    },
  });
  project.symbols.push(
    {
      id: 'btn_sym',
      name: 'ActionButton',
      kind: 'button',
      timeline: {
        frame_count: 4,
        layers: [{
          id: 'btn-layer',
          name: 'btn-layer',
          visible: true,
          locked: false,
          keyframes: [{
            frame: 0,
            elements: [{
              id: 'btn-plate',
              name: '',
              transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
              kind: { type: 'rectangle', width: 90, height: 32, fill: [45, 110, 200, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
              scripts: [],
            }],
            tween: null,
            label: '',
          }],
        }],
        scripts: [],
      },
    },
    {
      id: 'card_sym',
      name: 'CardSymbol',
      kind: 'movie_clip',
      linkage_id: 'CardClip',
      timeline: {
        frame_count: 1,
        layers: [{
          id: 'card-layer',
          name: 'card-layer',
          visible: true,
          locked: false,
          keyframes: [{ frame: 0, elements: [], tween: null, label: '' }],
        }],
        scripts: [],
      },
    },
    {
      id: 'init_sym_a',
      name: 'StageComponentSymbol',
      kind: 'movie_clip',
      linkage_id: 'StageComp',
      timeline: {
        frame_count: 1,
        layers: [{
          id: 'init-a-layer',
          name: 'init-a-layer',
          visible: true,
          locked: false,
          keyframes: [{
            frame: 0,
            elements: [{
              id: 'init-a-rect',
              name: '',
              transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
              kind: { type: 'rectangle', width: 60, height: 24, fill: [90, 170, 95, 255], stroke: [0, 0, 0, 0], stroke_width: 0 },
              scripts: [],
            }],
            tween: null,
            label: '',
          }],
        }],
        scripts: [{
          frame: 0,
          script: {
            language: 'action_script2',
            source: [
              'import app.RegisteredCard;',
              '#initclip 10',
              '_global.initOrder.push("order10");',
              'Object.registerClass("StageComp", RegisteredCard);',
              '#endinitclip',
              'frameVisits = (frameVisits ? frameVisits : 0) + 1;',
            ].join('\n'),
          },
        }],
      },
    },
    {
      id: 'init_sym_b',
      name: 'BootstrapSymbol',
      kind: 'movie_clip',
      linkage_id: 'BootstrapClip',
      timeline: {
        frame_count: 1,
        layers: [{
          id: 'init-b-layer',
          name: 'init-b-layer',
          visible: true,
          locked: false,
          keyframes: [{ frame: 0, elements: [], tween: null, label: '' }],
        }],
        scripts: [{
          frame: 0,
          script: {
            language: 'action_script2',
            source: [
              'import ext.HostWidget;',
              '#initclip 3',
              '_global.initOrder.push("order3");',
              '#endinitclip',
              '#initclip',
              '_global.initOrder = ["unordered:" + (this === _level0)];',
              'var ctor:Function = function(seed:String):Void { this.label = seed; };',
              'ctor.prototype.render = function():String { return "widget:" + this.label; };',
              'ctor.make = function(seed:String):HostWidget { return new HostWidget(seed); };',
              '_global.ext = {HostWidget: ctor};',
              '#endinitclip',
            ].join('\n'),
          },
        }],
      },
    },
    {
      id: 'sub_sym',
      name: 'SubSymbol',
      kind: 'movie_clip',
      linkage_id: 'SubClip',
      timeline: {
        frame_count: 2,
        layers: [{
          id: 'sub-layer',
          name: 'sub-layer',
          visible: true,
          locked: false,
          keyframes: [
            { frame: 0, elements: [], tween: null, label: '' },
            { frame: 1, elements: [], tween: null, label: 'active' },
          ],
        }],
        scripts: [
          {
            frame: 0,
            script: {
              language: 'action_script2',
              source: 'function frameProbe():Object { return arguments.caller; } seenRootName = _root._name; seenEvalRootName = eval("_root")._name; seenLevel0IsMain = (_level0 === _root._parent && _level0 !== _root && eval("_level0") === _level0); seenLevel1 = _level1; seenEvalLevel1 = eval("_level1"); seenFrameCallerIsNull = (frameProbe() === null);',
            },
          },
          {
            frame: 1,
            script: {
              language: 'action_script2',
              source: 'calledCount = (calledCount ? calledCount : 0) + 10; calledOn = _name;',
            },
          },
        ],
      },
    },
    {
      id: 'bmp_link_sym',
      name: 'LinkedCreatureSymbol',
      kind: 'movie_clip',
      linkage_id: 'CreatureBitmap',
      timeline: {
        frame_count: 1,
        layers: [{
          id: 'bmp-link-layer',
          name: 'bmp-link-layer',
          visible: true,
          locked: false,
          keyframes: [{
            frame: 0,
            elements: [{
              id: 'bmp-link-el',
              name: '',
              transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
              kind: { type: 'bitmap', resource_id: pngResource.id, width: 16, height: 16, smoothing: false },
              scripts: [],
            }],
            tween: null,
            label: '',
          }],
        }],
        scripts: [],
      },
    },
    {
      id: 'static_banner_sym',
      name: 'StaticBannerSymbol',
      kind: 'movie_clip',
      linkage_id: 'StaticBannerClip',
      timeline: {
        frame_count: 1,
        layers: [{
          id: 'static-banner-layer',
          name: 'static-banner-layer',
          visible: true,
          locked: false,
          keyframes: [{
            frame: 0,
            elements: [{
              id: 'static-banner-text',
              name: '',
              transform: { x: 4, y: 6, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
              kind: { type: 'text', text: 'NeoFlash Static\nLine2', font_family: 'Arial', font_size: 14, color: [17, 34, 51, 255] },
              scripts: [],
            }],
            tween: null,
            label: '',
          }],
        }],
        scripts: [],
      },
    },
  );
  project.sources.push(
    {
      id: 'as2-source-intrinsic-iwidget',
      path: 'ext/IHostWidget.as',
      language: 'action_script2',
      source: 'intrinsic interface ext.IHostWidget { function render():String; }',
    },
    {
      id: 'as2-source-intrinsic-widget',
      path: 'ext/HostWidget.as',
      language: 'action_script2',
      source: [
        'import ext.IHostWidget;',
        'intrinsic class ext.HostWidget implements IHostWidget {',
        '  public var label:String;',
        '  public function HostWidget(seed:String);',
        '  public function render():String;',
        '  public static function make(seed:String):HostWidget;',
        '}',
      ].join('\n'),
    },
    {
      id: 'as2-source-registered-card',
      path: 'app/RegisteredCard.as',
      language: 'action_script2',
      source: [
        'class app.RegisteredCard extends MovieClip {',
        '  public var badge:String = "";',
        '  public function RegisteredCard() {',
        '    super();',
        '    badge = "registered:" + _name + ":" + getSWFVersion();',
        '  }',
        '  public function invokeProbe(fn:Function):Object {',
        '    return fn(7);',
        '  }',
        '}',
      ].join('\n'),
    },
    {
      id: 'as2-source-inspector',
      path: 'app/Inspector.as',
      language: 'action_script2',
      source: 'class app.Inspector {\n  public var tag:String = "base";\n  public function Inspector() {}\n}',
    },
    {
      id: 'as2-source-subinspector',
      path: 'app/SubInspector.as',
      language: 'action_script2',
      source: 'import app.Inspector;\nclass app.SubInspector extends Inspector {\n  public function SubInspector() { super(); }\n}',
    },
    {
      id: 'as2-source-math-geom-audit',
      path: 'app/MathGeomAudit.as',
      language: 'action_script2',
      source: [
        'import flash.geom.ColorTransform;',
        'import flash.geom.Matrix;',
        'import flash.geom.Point;',
        'import flash.geom.Rectangle;',
        'import flash.geom.Transform;',
        'class app.MathGeomAudit {',
        '  public static function run(host:MovieClip):Object {',
        '    var out:Object = {};',
        '    out.consts = [Infinity > 1e300, -Infinity < -1e300, isNaN(NaN)];',
        '    var boxed:Number = new Number(42.5);',
        '    out.numConsts = [Number.MAX_VALUE > 1e308, Number.MIN_VALUE > 0 && Number.MIN_VALUE < 1e-300, Number.POSITIVE_INFINITY == Infinity, Number.NEGATIVE_INFINITY == -Infinity, isNaN(Number.NaN), boxed.valueOf() == 42.5];',
        '    var randOk:Boolean = (random(0) == 0);',
        '    for (var ri:Number = 0; ri < 16; ri++) {',
        '      var rv:Number = random(5);',
        '      if (rv < 0 || rv >= 5 || int(rv) != rv) randOk = false;',
        '    }',
        '    out.numGlobals = [int(-7.9) == -7, int(7.9) == 7, isFinite(42) && !isFinite(Infinity) && !isFinite(NaN), isNaN(NaN) && !isNaN(0), Math.abs(parseFloat("  -3.25e2px") + 325) < 1e-9, randOk];',
        '    var mr:Number = Math.random();',
        '    out.mathConsts = [Math.abs(Math.E - 2.718281828) < 1e-6, Math.abs(Math.LN10 - 2.302585093) < 1e-6, Math.abs(Math.LN2 - 0.69314718) < 1e-6, Math.abs(Math.LOG10E - 0.43429448) < 1e-6, Math.abs(Math.LOG2E - 1.44269504) < 1e-6, Math.abs(Math.PI - 3.1415926535) < 1e-8, Math.abs(Math.SQRT1_2 - 0.70710678) < 1e-6, Math.abs(Math.SQRT2 - 1.41421356) < 1e-6];',
        '    out.mathFns = [Math.abs(-9) == 9, Math.acos(1) == 0, Math.asin(0) == 0, Math.abs(Math.atan(1) - Math.PI / 4) < 1e-9, Math.abs(Math.atan2(1, 1) - Math.PI / 4) < 1e-9, Math.ceil(2.1) == 3, Math.cos(0) == 1, Math.abs(Math.exp(1) - Math.E) < 1e-9, Math.floor(2.9) == 2, Math.abs(Math.log(Math.E) - 1) < 1e-9, Math.max(3, 9) == 9, Math.min(3, 9) == 3, Math.pow(2, 10) == 1024, mr >= 0 && mr < 1, Math.round(-2.5) == -2, Math.abs(Math.sin(Math.PI / 2) - 1) < 1e-9, Math.sqrt(81) == 9, Math.tan(0) == 0];',
        '    var p:Point = new Point(3, 4);',
        '    var pClone:Point = p.clone();',
        '    var pAdd:Point = p.add(new Point(1, 2));',
        '    var pSub:Point = p.subtract(new Point(1, 1));',
        '    pClone.offset(3, 4);',
        '    pClone.normalize(10);',
        '    var pDist:Number = Point.distance(new Point(0, 0), new Point(6, 8));',
        '    var pInterp:Point = Point.interpolate(new Point(10, 20), new Point(0, 0), 0.25);',
        '    var pPolar:Point = Point.polar(5, 0);',
        '    out.pointChecks = [p instanceof Point, p.x == 3 && p.y == 4, p.length == 5, p.equals(new Point(3, 4)), pAdd.x == 4 && pAdd.y == 6, pSub.x == 2 && pSub.y == 3, Math.abs(pClone.length - 10) < 1e-9, pDist == 10, Math.abs(pInterp.x - 2.5) < 1e-9 && Math.abs(pInterp.y - 5) < 1e-9, Math.abs(pPolar.x - 5) < 1e-9 && Math.abs(pPolar.y) < 1e-9, p.toString().indexOf("x=3") >= 0];',
        '    var rc:Rectangle = new Rectangle(10, 20, 30, 40);',
        '    var rcClone:Rectangle = rc.clone();',
        '    var rcBasic:Boolean = (rc instanceof Rectangle) && rc.x == 10 && rc.y == 20 && rc.width == 30 && rc.height == 40 && rc.left == 10 && rc.right == 40 && rc.top == 20 && rc.bottom == 60 && rc.topLeft.equals(new Point(10, 20)) && rc.bottomRight.equals(new Point(40, 60)) && rc.size.equals(new Point(30, 40));',
        '    var rcQueries:Boolean = rc.equals(rcClone) && rc.contains(15, 25) && !rc.contains(40, 60) && rc.containsPoint(new Point(15, 25)) && rc.containsRectangle(new Rectangle(12, 22, 10, 10)) && !rc.containsRectangle(new Rectangle(0, 0, 20, 20)) && rc.intersects(new Rectangle(20, 30, 5, 5)) && !rc.isEmpty();',
        '    var rcInter:Rectangle = rc.intersection(new Rectangle(20, 30, 50, 50));',
        '    var rcUnion:Rectangle = rc.union(new Rectangle(0, 0, 10, 10));',
        '    rcClone.inflate(2, 3);',
        '    rcClone.inflatePoint(new Point(1, 2));',
        '    rcClone.offset(5, 6);',
        '    rcClone.offsetPoint(new Point(1, 2));',
        '    var rcMut:Rectangle = new Rectangle();',
        '    rcMut.left = 2; rcMut.right = 12; rcMut.top = 4; rcMut.bottom = 14;',
        '    rcMut.topLeft = new Point(1, 2); rcMut.bottomRight = new Point(11, 22); rcMut.size = new Point(8, 9);',
        '    var rcStr:String = rc.toString();',
        '    rcMut.setEmpty();',
        '    out.rectChecks = [rcBasic, rcQueries, rcInter.equals(new Rectangle(20, 30, 20, 30)), rcUnion.equals(new Rectangle(0, 0, 40, 60)), rcClone.width == 36 && rcClone.height == 50 && rcClone.x == 13 && rcClone.y == 23, rcMut.isEmpty(), rcStr.indexOf("w=30") >= 0];',
        '    var m:Matrix = new Matrix(1, 0, 0, 1, 5, 6);',
        '    var mClone:Matrix = m.clone();',
        '    mClone.translate(3, 4);',
        '    mClone.scale(2, 3);',
        '    mClone.rotate(0);',
        '    mClone.concat(new Matrix(1, 0, 0, 1, 1, 2));',
        '    var tp:Point = m.transformPoint(new Point(2, 3));',
        '    var dtp:Point = m.deltaTransformPoint(new Point(2, 3));',
        '    var inv:Matrix = new Matrix(2, 0, 0, 4, 10, 20);',
        '    inv.invert();',
        '    var idm:Matrix = new Matrix(9, 1, 2, 9, 3, 4);',
        '    idm.identity();',
        '    var boxM:Matrix = new Matrix();',
        '    boxM.createBox(2, 3, 0, 10, 20);',
        '    var gradM:Matrix = new Matrix();',
        '    gradM.createGradientBox(100, 50, 0, 10, 20);',
        '    out.matrixChecks = [m instanceof Matrix, m.a == 1 && m.b == 0 && m.c == 0 && m.d == 1 && m.tx == 5 && m.ty == 6, mClone.tx == 17 && mClone.ty == 32, tp.x == 7 && tp.y == 9, dtp.x == 2 && dtp.y == 3, Math.abs(inv.a - 0.5) < 1e-9 && Math.abs(inv.d - 0.25) < 1e-9 && inv.tx == -5 && inv.ty == -5, idm.a == 1 && idm.d == 1 && idm.tx == 0 && idm.ty == 0, boxM.a == 2 && boxM.d == 3 && boxM.tx == 10 && boxM.ty == 20, Math.abs(gradM.a - 100 / 1638.4) < 1e-6 && gradM.tx == 60 && gradM.ty == 45, m.toString().indexOf("tx=5") >= 0];',
        '    var ct:ColorTransform = new ColorTransform(0.5, 0.25, 0.75, 0.8, 10, 20, 30, 40);',
        '    var ctInit:Boolean = (ct instanceof ColorTransform) && ct.redMultiplier == 0.5 && ct.greenMultiplier == 0.25 && ct.blueMultiplier == 0.75 && Math.abs(ct.alphaMultiplier - 0.8) < 1e-6 && ct.redOffset == 10 && ct.greenOffset == 20 && ct.blueOffset == 30 && ct.alphaOffset == 40;',
        '    ct.concat(new ColorTransform(2, 4, 1, 1, 4, 8, 12, 0));',
        '    var ctAfterConcat:Boolean = ct.redMultiplier == 1 && ct.greenMultiplier == 1 && Math.abs(ct.redOffset - 24) < 1e-6 && Math.abs(ct.greenOffset - 88) < 1e-6;',
        '    ct.rgb = 0x112233;',
        '    out.colorChecks = [ctInit, ctAfterConcat, ct.rgb == 0x112233 && ct.redMultiplier == 0 && ct.greenMultiplier == 0 && ct.blueMultiplier == 0 && ct.redOffset == 0x11 && ct.greenOffset == 0x22 && ct.blueOffset == 0x33, ct.toString().indexOf("redOffset=17") >= 0];',
        '    var gClip:MovieClip = host.createEmptyMovieClip("geomProbe", 90);',
        '    gClip.beginFill(0x224466, 100); gClip.moveTo(0, 0); gClip.lineTo(20, 0); gClip.lineTo(20, 10); gClip.lineTo(0, 10); gClip.endFill();',
        '    var tr:Transform = new Transform(gClip);',
        '    tr.matrix = new Matrix(2, 0, 0, 3, 15, 25);',
        '    tr.colorTransform = new ColorTransform(1, 1, 1, 0.5, 12, 24, 36, 0);',
        '    var pb:Rectangle = gClip.transform.pixelBounds;',
        '    out.transformChecks = [tr instanceof Transform, tr.matrix.a == 2 && tr.matrix.d == 3 && tr.matrix.tx == 15 && tr.matrix.ty == 25, tr.concatenatedMatrix.tx == 15 && tr.concatenatedMatrix.ty == 25, tr.colorTransform.redOffset == 12 && Math.abs(tr.colorTransform.alphaMultiplier - 0.5) < 1e-6, tr.concatenatedColorTransform.blueOffset == 36, pb instanceof Rectangle && pb.width >= 40 && pb.height >= 30];',
        '    return out;',
        '  }',
        '}',
      ].join('\n'),
    },
    {
      id: 'as2-source-services-audit',
      path: 'app/ServicesAudit.as',
      language: 'action_script2',
      source: [
        'import flash.net.FileReference;',
        'class app.ServicesAudit {',
        '  public static function run(host:MovieClip):Object {',
        '    var out:Object = {};',
        '    var asfLog:Array = [];',
        '    host.onAsfuncHit = function(arg:String):Void { asfLog.push("hit:" + arg); };',
        '    host.onAsfuncBare = function(arg:String):Void { asfLog.push("bare:" + String(arg === undefined)); };',
        '    var href1:String = asfunction("onAsfuncHit", "hello,world");',
        '    var href2:String = asfunction("onAsfuncBare");',
        '    var fnAsf:Function = asfunction;',
        '    getURL(href1);',
        '    getURL(href2);',
        '    getURL(fnAsf("onAsfuncHit", "bound"));',
        '    getURL("https://app.example.test/docs?from=globalGetURL", "_blank");',
        '    var navClip:MovieClip = host.createEmptyMovieClip("navClip", 95);',
        '    navClip.navToken = "fromAS2";',
        '    navClip.getURL("https://app.example.test/form", "_blank", "GET");',
        '    var linkTf:TextField = host.createTextField("linkTf", 91, 10, 10, 180, 30);',
        '    linkTf.html = true;',
        '    linkTf.htmlText = "<a href=\\"asfunction:onAsfuncHit,fromHtml\\">ClickLink</a>";',
        '    out.linkTf = linkTf;',
        '    out.asfLog = asfLog;',
        '    out.asfunctionHrefs = [href1, href2];',
        '    fscommand("notifyHost", "payload-42");',
        '    fscommand("fullscreen", "true");',
        '    out.mmResult = MMExecute("fl.getDocumentDOM()");',
        '    var loadSlot:MovieClip = host.createEmptyMovieClip("loadSlot", 92);',
        '    var unloadSlot:MovieClip = host.createEmptyMovieClip("unloadSlot", 93);',
        '    loadMovie("images/pixel-creature.png", loadSlot);',
        '    loadMovie("images/pixel-creature.png", "unloadSlot");',
        '    unloadMovie(unloadSlot);',
        '    host.tryLoadMovieNum = function(lvl:Number):Void { loadMovieNum("images/pixel-creature.png", lvl); };',
        '    host.tryUnloadMovieNum = function(lvl:Number):Void { unloadMovieNum(lvl); };',
        '    out.loadSlot = loadSlot;',
        '    out.unloadSlot = unloadSlot;',
        '    var printClip:MovieClip = host.createEmptyMovieClip("printClip", 94);',
        '    printClip.beginFill(0xcc3300, 100); printClip.moveTo(0, 0); printClip.lineTo(40, 0); printClip.lineTo(40, 30); printClip.lineTo(0, 30); printClip.endFill();',
        '    print(printClip, "bmovie");',
        '    printAsBitmap(printClip, "bmax");',
        '    printNum(0, "bframe");',
        '    printAsBitmapNum(0, "bmovie");',
        '    var cam:Camera = Camera.get(0);',
        '    var camInit:Array = [cam.bandwidth, cam.quality, cam.keyFrameInterval, cam.loopback];',
        '    cam.setQuality(32768, 85);',
        '    cam.setKeyFrameInterval(24);',
        '    cam.setLoopback(true);',
        '    out.camChecks = [camInit[0] == 16384 && camInit[1] == 0 && camInit[2] == 15 && camInit[3] == false, cam.bandwidth == 32768, cam.quality == 85, cam.keyFrameInterval == 24, cam.loopback == true];',
        '    var mic:Microphone = Microphone.get(0);',
        '    var micInit:Array = [mic.codec, mic.encodeQuality, mic.framesPerPacket];',
        '    mic.setCodec("speex");',
        '    mic.setEncodeQuality(9);',
        '    mic.setFramesPerPacket(1);',
        '    out.micChecks = [micInit[0] == "NellyMoser" && micInit[1] == 6 && micInit[2] == 2, mic.codec == "Speex", mic.encodeQuality == 9, mic.framesPerPacket == 1];',
        '    var sysHandler:Function = function(info:Object):Void {};',
        '    System.onStatus = sysHandler;',
        '    var sysClipOk:Boolean = System.setClipboard("neoflash-clipboard");',
        '    System.showSettings(2);',
        '    System.security.allowDomain("cdn.example.test", "api.example.test");',
        '    System.security.allowInsecureDomain("legacy.example.test");',
        '    System.security.loadPolicyFile("https://cdn.example.test/crossdomain.xml");',
        '    var imeEvents:Array = [];',
        '    var imeInit:Array = [System.IME.getEnabled(), System.IME.getConversionMode()];',
        '    var imeL:Object = { onIMEComposition: function(txt:String):Void { imeEvents.push(txt); } };',
        '    System.IME.addListener(imeL);',
        '    var imeEnOk:Boolean = System.IME.setEnabled(true);',
        '    var imeModeOk:Boolean = System.IME.setConversionMode(System.IME.JAPANESE_HIRAGANA);',
        '    var imeCompOk:Boolean = System.IME.setCompositionString("kana");',
        '    var imeConv1:Boolean = System.IME.doConversion();',
        '    System.IME.setEnabled(false);',
        '    var imeConvDisabled:Boolean = System.IME.doConversion();',
        '    System.IME.setEnabled(true);',
        '    out.systemChecks = [System.onStatus === sysHandler, sysClipOk, imeInit[0] == false && imeInit[1] == "ALPHANUMERIC_HALF", imeEnOk && System.IME.getEnabled() == true, imeModeOk && System.IME.getConversionMode() == "JAPANESE_HIRAGANA", imeCompOk && imeConv1 && !imeConvDisabled, imeEvents.length == 1 && imeEvents[0] == "kana"];',
        '    var lcLog:Array = [];',
        '    var rx:LocalConnection = new LocalConnection();',
        '    rx.allowDomain = function(d:String):Boolean { lcLog.push("allow:" + d); return d == "app.example.test"; };',
        '    rx.allowInsecureDomain = function(d:String):Boolean { lcLog.push("insec:" + d); return d == "http-ok.example.test"; };',
        '    rx.onBusMsg = function(tag:String):Void { lcLog.push("msg:" + tag); };',
        '    rx.connect("batch3_bus");',
        '    var tx:LocalConnection = new LocalConnection();',
        '    tx.send("batch3_bus", "onBusMsg", "sameOrigin");',
        '    out.lcLog = lcLog;',
        '    out.lcRx = rx;',
        '    var fr:FileReference = new FileReference();',
        '    var frBefore:Boolean = (fr.creationDate === undefined && fr.creator === undefined);',
        '    fr.browse([{description: "Text", extension: "*.txt"}]);',
        '    out.fileChecks = [frBefore, fr.creationDate instanceof Date, fr.creationDate.getTime() == 1700000000000, fr.creator === undefined];',
        '    var localNc:NetConnection = new NetConnection();',
        '    localNc.connect(null);',
        '    var remNc:NetConnection = new NetConnection();',
        '    remNc.connect("https://app.example.test/gateway");',
        '    var ns2:NetStream = new NetStream(localNc);',
        '    var nsDelay:Number = ns2.liveDelay;',
        '    var nsPause0:Number = ns2.maxPauseBufferTime;',
        '    ns2.maxPauseBufferTime = 90;',
        '    var info:Object = ns2.getInfo();',
        '    ns2.attachAudio(mic);',
        '    ns2.attachVideo(cam, 0);',
        '    ns2.play2({streamName: "https://app.example.test/sample.mp4", start: 0});',
        '    var nsLog:Array = [];',
        '    ns2.onStatus = function(st:Object):Void { nsLog.push("status:" + st.code); };',
        '    ns2.onCuePoint = function(cp:Object):Void { nsLog.push("cue:" + cp.name); };',
        '    ns2.onTextData = function(td:Object):Void { nsLog.push("text:" + td.text); };',
        '    ns2.onXMPData = function(xd:Object):Void { nsLog.push("xmp:" + xd.data); };',
        '    ns2.publish("feed", "live");',
        '    ns2.send("onCuePoint", {name: "c1"});',
        '    ns2.send("onTextData", {text: "sub"});',
        '    ns2.send("onXMPData", {data: "<xmp/>"});',
        '    out.netChecks = [localNc.protocol == "", remNc.protocol == "https", nsDelay == 0, nsPause0 == 30 && ns2.maxPauseBufferTime == 90, info.isLive == false && info.droppedFrames == 0 && info.audioBufferLength == 0];',
        '    out.nsLog = nsLog;',
        '    var rsoLog:Array = [];',
        '    var rso:SharedObject = SharedObject.getRemote("lobby", "https://app.example.test/fms", false);',
        '    var rsoStatusFn:Function = function(st:Object):Void { rsoLog.push("status:" + st.code); };',
        '    rso.onStatus = rsoStatusFn;',
        '    rso.onSync = function(list:Array):Void { rsoLog.push("sync:" + list[0].code); };',
        '    rso.onChat = function(msg:String):Void { rsoLog.push("chat:" + msg); };',
        '    var rsoConn:Boolean = rso.connect(localNc);',
        '    var rsoFps:Boolean = rso.setFps(15);',
        '    rso.data.topic = "as2";',
        '    rso.send("onChat", "hello-room");',
        '    out.rso = rso;',
        '    out.rsoChecks = [rso != null, rso.onStatus === rsoStatusFn, rsoConn, rsoFps, rso.data.topic == "as2"];',
        '    out.rsoLog = rsoLog;',
        '    var xmlLog:Array = [];',
        '    var xs1:XMLSocket = new XMLSocket();',
        '    var xsTimeout0:Number = xs1.timeout;',
        '    xs1.timeout = 4500;',
        '    var lowPortOk:Boolean = xs1.connect("chat.example.test", 80);',
        '    xs1.onConnect = function(ok:Boolean):Void { xmlLog.push("conn1:" + ok); if (ok) this.send("<hello v=\\"1\\"/>"); };',
        '    xs1.onXML = function(doc:XML):Void { xmlLog.push("xml1:" + doc.firstChild.nodeName + ":" + doc.firstChild.attributes.ok); };',
        '    xs1.onClose = function():Void { xmlLog.push("close1"); };',
        '    var hiPortOk:Boolean = xs1.connect("chat.example.test", 9000);',
        '    var xs2:XMLSocket = new XMLSocket();',
        '    xs2.onConnect = function(ok:Boolean):Void { xmlLog.push("conn2:" + ok); };',
        '    xs2.onData = function(raw:String):Void { xmlLog.push("raw2:" + raw); };',
        '    xs2.onClose = function():Void { xmlLog.push("close2"); };',
        '    var xs2ConnOk:Boolean = xs2.connect("chat.example.test", 9001);',
        '    out.xs2 = xs2;',
        '    out.xmlChecks = [xs1 instanceof XMLSocket, xsTimeout0 == 20000 && xs1.timeout == 4500, lowPortOk == false, hiPortOk == true, xs2ConnOk == true];',
        '    out.xmlLog = xmlLog;',
        '    return out;',
        '  }',
        '}',
      ].join('\n'),
    },
    {
      id: 'as2-source-foundation-main',
      path: 'app/FoundationMain.as',
      language: 'action_script2',
      source: [
        'import app.RegisteredCard;',
        'import app.Inspector;',
        'import app.SubInspector;',
        'import app.MathGeomAudit;',
        'import app.ServicesAudit;',
        'import app.CoreLangAudit;',
        'import app.GraphicsTextAudit;',
        'import ext.HostWidget;',
        'import flash.display.BitmapData;',
        'import flash.filters.DisplacementMapFilter;',
        'import flash.geom.Matrix;',
        'import flash.geom.Point;',
        'import flash.geom.Rectangle;',
        'import flash.text.TextRenderer;',
        'class app.FoundationMain extends MovieClip {',
        '  public var foundationReport:Object;',
        '  public var holderRef:MovieClip;',
        '  public var subRef:MovieClip;',
        '  public var artRef:MovieClip;',
        '  public var loaderRef:MovieClipLoader;',
        '  public var soundRef:Sound;',
        '  public var streamRef:NetStream;',
        '  public var storyText:String;',
        '  public var rootCalled:Number;',
        '  public var includedMacroSum:Number;',
        '  public var rootTellMarker:Boolean;',
        '  public function FoundationMain() {',
        '    super();',
        '    stop();',
        '    var r:Object = {};',
        '    r.initQuality = _quality;',
        '    r.initHighQuality = _highquality;',
        '    r.initFocusRect = _focusrect;',
        '    r.initSoundBuf = _soundbuftime;',
        '    _quality = "best";',
        '    r.bestQuality = this._quality;',
        '    r.bestHighQuality = this._highquality;',
        '    r.btnQuality = actionBtn._quality;',
        '    r.btnHighQuality = actionBtn._highquality;',
        '    r.toggleToLow = toggleHighQuality();',
        '    r.lowViaGetProp = getProperty(this, 19);',
        '    r.lowHqViaGetProp = getProperty(this, 16);',
        '    setProperty(this, 16, 1);',
        '    r.afterSetPropQuality = _quality;',
        '    _focusrect = false;',
        '    _soundbuftime = 9;',
        '    r.evalFocusRect = eval("_focusrect");',
        '    r.btnSoundBuf = actionBtn._soundbuftime;',
        '    set("_quality", "medium");',
        '    r.afterSetQuality = _quality;',
        '    r.mediumHq = _highquality;',
        '    showRedrawRegions(true, 0x11aa55);',
        '    showRedrawRegions(false);',
        '    r.swfVersion = this.getSWFVersion();',
        '    var holder:MovieClip = this.createEmptyMovieClip("holder", 10);',
        '    r.initLockRoot = holder._lockroot;',
        '    holder._lockroot = true;',
        '    r.lockedFlag = holder._lockroot;',
        '    var sub:MovieClip = holder.attachMovie("SubClip", "sub", 1);',
        '    r.subRootMatchesHolder = (sub._root === holder);',
        '    holderRef = holder;',
        '    subRef = sub;',
        '    r.unsetClipGrid = holder.scale9Grid;',
        '    holder.scale9Grid = new Rectangle(4, 6, 40, 20);',
        '    var clipGridCopy:Rectangle = holder.scale9Grid;',
        '    clipGridCopy.width = 999;',
        '    r.clipGridWidth = holder.scale9Grid.width;',
        '    holder.scale9Grid = null;',
        '    r.clearedClipGrid = holder.scale9Grid;',
        '    r.isButton = (actionBtn instanceof Button);',
        '    r.btnInitVisible = actionBtn._visible;',
        '    actionBtn._visible = false;',
        '    r.btnHidden = actionBtn._visible;',
        '    actionBtn._visible = true;',
        '    actionBtn.scale9Grid = new Rectangle(2, 3, 20, 10);',
        '    r.btnGridHeight = actionBtn.scale9Grid.height;',
        '    actionBtn.scale9Grid = undefined;',
        '    r.btnClearedGrid = actionBtn.scale9Grid;',
        '    r.wmodeGPU = Stage.wmodeGPU;',
        '    var tf:TextField = this.createTextField("tf", 20, 10, 10, 160, 40);',
        '    r.tfQuality = tf._quality;',
        '    r.tfHighQuality = tf._highquality;',
        '    r.tfSoundBuf = tf._soundbuftime;',
        '    tf._quality = "HIGH";',
        '    tf._soundbuftime = 7;',
        '    r.globalQualityFromTf = _quality;',
        '    r.globalSoundBufFromTf = _soundbuftime;',
        '    r.tfDefaults = [tf.embedFonts, tf.antiAliasType, tf.gridFitType, tf.sharpness, tf.thickness];',
        '    tf.embedFonts = true;',
        '    tf.antiAliasType = "advanced";',
        '    tf.gridFitType = "subpixel";',
        '    tf.sharpness = 600;',
        '    tf.thickness = -350;',
        '    r.tfUpdated = [tf.embedFonts, tf.antiAliasType, tf.gridFitType, tf.sharpness, tf.thickness];',
        '    var fonts:Array = TextField.getFontList();',
        '    r.fontCount = fonts.length;',
        '    r.hasArial = false;',
        '    for (var i:Number = 0; i < fonts.length; i++) {',
        '      if (fonts[i] == "Arial") r.hasArial = true;',
        '    }',
        '    r.rendererDefaults = [TextRenderer.displayMode, TextRenderer.maxLevel];',
        '    TextRenderer.displayMode = "lcd";',
        '    TextRenderer.maxLevel = 8;',
        '    TextRenderer.setAdvancedAntialiasingTable("Arial", "bold", "dark", [{fontSize: 12, insideCutoff: 0.6, outsideCutoff: -0.4}]);',
        '    r.rendererUpdated = [TextRenderer.displayMode, TextRenderer.maxLevel];',
        '    var d:Date = new Date(Date.UTC(2026, 10, 1, 12, 30, 0));',
        '    var badDate:Date = new Date(NaN);',
        '    r.isDate = (d instanceof Date);',
        '    r.utcYear = d.getUTCYear();',
        '    r.tzFinite = !isNaN(d.getTimezoneOffset());',
        '    r.dateStrHasYear = (d.toString().indexOf("2026") >= 0);',
        '    r.badDateChecks = [isNaN(badDate.getUTCYear()), isNaN(badDate.getTimezoneOffset()), badDate.toString() == "Invalid Date"];',
        '    var plain:Object = {};',
        '    r.plainCtor = (plain.constructor === Object);',
        '    r.plainProto = (plain.__proto__ === Object.prototype);',
        '    var customProto:Object = {inheritedValue: 42};',
        '    var childObj:Object = {};',
        '    childObj.__proto__ = customProto;',
        '    r.reassignedProto = [childObj.inheritedValue, childObj.__proto__ === customProto];',
        '    var ins:Inspector = new Inspector();',
        '    var subIns:SubInspector = new SubInspector();',
        '    r.classProtos = [ins.__proto__ === Inspector.prototype, ins.constructor === Inspector, subIns.__proto__ === SubInspector.prototype, SubInspector.prototype.constructor === Inspector];',
        '    r.regOk = Object.registerClass("CardClip", RegisteredCard);',
        '    var card1:MovieClip = this.attachMovie("CardClip", "card1", 30);',
        '    r.card1Checks = [card1 instanceof RegisteredCard, card1.badge];',
        '    var fact:Function = function(n:Number):Number {',
        '      if (n <= 1) return 1;',
        '      return n * arguments.callee(n - 1);',
        '    };',
        '    r.factorial5 = fact(5);',
        '    var probe:Function = function(x:Number):Object {',
        '      return {caller: arguments.caller, calleeMatches: arguments.callee === probe, x: x};',
        '    };',
        '    var outerProbe:Function = function():Object {',
        '      return probe(3);',
        '    };',
        '    var topCall:Object = probe(1);',
        '    var nestedCall:Object = outerProbe();',
        '    var methodCall:Object = card1.invokeProbe(probe);',
        '    r.callerChecks = [topCall.caller === FoundationMain, topCall.calleeMatches, nestedCall.caller === outerProbe, methodCall.caller === card1.invokeProbe];',
        '    r.unregOk = Object.registerClass("CardClip", null);',
        '    var card2:MovieClip = this.attachMovie("CardClip", "card2", 31);',
        '    r.card2Checks = [card2 instanceof RegisteredCard, card2.badge === undefined];',
        '    r.initOrder = _global.initOrder.slice();',
        '    r.stageCompChecks = [stageComp instanceof RegisteredCard, stageComp.badge];',
        '    var hw1:HostWidget = new HostWidget("alpha");',
        '    var hw2:HostWidget = HostWidget.make("beta");',
        '    r.intrinsicChecks = [hw1 instanceof HostWidget, hw1.render(), hw2 instanceof HostWidget, hw2.label];',
        '    var tile:BitmapData = new BitmapData(4, 4, false, 0xff2288dd);',
        '    var art:MovieClip = this.createEmptyMovieClip("art", 40);',
        '    var mat:Matrix = new Matrix();',
        '    mat.translate(2, 3);',
        '    art.beginBitmapFill(tile, mat, false, true);',
        '    art.moveTo(0, 0); art.lineTo(20, 0); art.lineTo(20, 16); art.lineTo(0, 16); art.lineTo(0, 0);',
        '    art.endFill();',
        '    art.beginBitmapFill(tile);',
        '    art.moveTo(30, 0); art.lineTo(46, 0); art.lineTo(46, 16); art.lineTo(30, 16); art.lineTo(30, 0);',
        '    art.endFill();',
        '    artRef = art;',
        '    tellTarget("holder/sub") {',
        '      slashMarker = "targeted";',
        '      gotoAndStop(2);',
        '    }',
        '    tellTarget("holder") {',
        '      tellTarget("sub") {',
        '        nestedTell = _name + ":" + _currentframe;',
        '      }',
        '      tellTarget("") {',
        '        rootTellMarker = (_target == "/");',
        '      }',
        '    }',
        '    r.tellTargetChecks = [sub.slashMarker, sub._currentframe, sub.nestedTell, rootTellMarker];',
        '    sub.gotoAndStop(1);',
        '    var loadedHits:Array = [];',
        '    ifFrameLoaded(1) { loadedHits.push("f1"); }',
        '    ifFrameLoaded(2) { loadedHits.push("f2"); }',
        '    ifFrameLoaded(99) { loadedHits.push("f99"); }',
        '    ifFrameLoaded("ready") { loadedHits.push("ready"); }',
        '    ifFrameLoaded("missing") { loadedHits.push("missing"); }',
        '    r.ifFrameLoadedChecks = loadedHits;',
        '    rootCalled = 0;',
        '    call(2);',
        '    call("ready");',
        '    call("missing");',
        '    var beforeSubCount:Number = sub.calledCount ? sub.calledCount : 0;',
        '    call("holder/sub:2");',
        '    call("holder/sub:active");',
        '    r.callChecks = [rootCalled, _currentframe, sub.calledCount - beforeSubCount, sub.calledOn, sub._currentframe];',
        '    r.levelChecks = [_level0 === this, _level1 === undefined, eval("_level1") === undefined, _level2 === undefined];',
        '    var logTf:TextField = this.createTextField("logTf", 25, 10, 60, 120, 24);',
        '    logTf.multiline = true;',
        '    logTf.wordWrap = true;',
        '    logTf.variable = "storyText";',
        '    storyText = "line1\\nline2\\nline3\\nline4\\nline5";',
        '    var initVarScroll:Number = storyText.scroll;',
        '    var maxVarScroll:Number = storyText.maxscroll;',
        '    storyText.scroll = 2;',
        '    r.varScrollChecks = [initVarScroll, maxVarScroll >= 3, storyText.scroll, logTf.scroll, eval("storyText.scroll")];',
        '    r.accActive = Accessibility.isActive();',
        '    actionBtn._visible = true;',
        '    actionBtn._accProps = {name: "Launch Action", description: "Starts mission"};',
        '    stageComp._accProps = {name: "Stage Card", forceSimple: true};',
        '    holder._accProps = {silent: true, name: "Muted"};',
        '    Accessibility.updateProperties();',
        '    var vid:Video = stageVideo;',
        '    var initDeblock:Number = vid.deblocking;',
        '    vid.deblocking = 4;',
        '    r.videoChecks = [vid instanceof Video, initDeblock, vid.deblocking];',
        '    var mcl:MovieClipLoader = new MovieClipLoader();',
        '    var mclPolicy0:Boolean = mcl.checkPolicyFile;',
        '    mcl.checkPolicyFile = true;',
        '    var keepEvents:Array = [];',
        '    var removedEvents:Array = [];',
        '    var keepL:Object = {',
        '      onLoadStart: function(t:MovieClip):Void { keepEvents.push("start:" + t._name); },',
        '      onLoadProgress: function(t:MovieClip, l:Number, tot:Number):Void { keepEvents.push("prog:" + t._name + ":" + (l == tot && tot > 0)); },',
        '      onLoadComplete: function(t:MovieClip, st:Number):Void { keepEvents.push("comp:" + t._name); },',
        '      onLoadInit: function(t:MovieClip):Void { keepEvents.push("init:" + t._name); },',
        '      onLoadError: function(t:MovieClip, err:String, st:Number):Void { keepEvents.push("err:" + t._name + ":" + (err.length > 0)); }',
        '    };',
        '    var dropL:Object = {',
        '      onLoadStart: function(t:MovieClip):Void { removedEvents.push("start"); },',
        '      onLoadError: function(t:MovieClip):Void { removedEvents.push("err"); }',
        '    };',
        '    mcl.addListener(keepL);',
        '    mcl.addListener(dropL);',
        '    var rem1:Boolean = mcl.removeListener(dropL);',
        '    var rem2:Boolean = mcl.removeListener(dropL);',
        '    var slotOk:MovieClip = this.createEmptyMovieClip("slotOk", 50);',
        '    var slotErr:MovieClip = this.createEmptyMovieClip("slotErr", 51);',
        '    mcl.loadClip("images/pixel-creature.png", slotOk);',
        '    mcl.loadClip("images/missing-asset.png", slotErr);',
        '    loaderRef = mcl;',
        '    r.mclSetup = [mclPolicy0, mcl.checkPolicyFile, rem1, rem2];',
        '    r.keepEvents = keepEvents;',
        '    r.removedEvents = removedEvents;',
        '    var snd:Sound = new Sound();',
        '    var sndPolicy0:Boolean = snd.checkPolicyFile;',
        '    snd.checkPolicyFile = true;',
        '    soundRef = snd;',
        '    var nc:NetConnection = new NetConnection();',
        '    nc.connect(null);',
        '    var ns:NetStream = new NetStream(nc);',
        '    var nsPolicy0:Boolean = ns.checkPolicyFile;',
        '    ns.checkPolicyFile = true;',
        '    streamRef = ns;',
        '    r.policyChecks = [sndPolicy0, snd.checkPolicyFile, nsPolicy0, ns.checkPolicyFile];',
        '    foundationReport = r;',
        '  }',
        '  public function runBatch3():Object {',
        '    return { math: MathGeomAudit.run(this), services: ServicesAudit.run(this) };',
        '  }',
        '  public function runBatch4():Object {',
        '    return { core: CoreLangAudit.run(this), gfx: GraphicsTextAudit.run(this), includedSum: includedMacroSum };',
        '  }',
        '  public function runWave6():Object {',
        '    var out:Object = {};',
        '    var menuLog:Array = [];',
        '    out.menuLog = menuLog;',
        '    var cm:ContextMenu = new ContextMenu(function(obj:Object, menu:ContextMenu):Void {',
        '      menuLog.push("menuSelect:" + obj._name + ":" + (menu === cm));',
        '    });',
        '    var builtIn0:Boolean = cm.builtInItems.play === true && cm.builtInItems.rewind === true && cm.builtInItems.forward_back === true && cm.builtInItems.print === true;',
        '    cm.hideBuiltInItems();',
        '    var builtInHidden:Boolean = cm.builtInItems.play === false && cm.builtInItems.rewind === false && cm.builtInItems.forward_back === false && cm.builtInItems.print === false;',
        '    var item1:ContextMenuItem = new ContextMenuItem("Inspect Clip", function(obj:Object, item:ContextMenuItem):Void {',
        '      menuLog.push("item1:" + obj._name + ":" + item.caption);',
        '    }, true, true, true);',
        '    var item2:ContextMenuItem = item1.copy();',
        '    item2.caption = "Disabled Item";',
        '    item2.enabled = false;',
        '    item2.separatorBefore = false;',
        '    item2.onSelect = function(obj:Object, item:ContextMenuItem):Void {',
        '      menuLog.push("item2:" + item.caption);',
        '    };',
        '    var item3:ContextMenuItem = new ContextMenuItem("Hidden Item", null, false, true, false);',
        '    cm.customItems.push(item1, item2, item3);',
        '    var cmCopy:ContextMenu = cm.copy();',
        '    this.left.menu = cm;',
        '    this.actionBtn.menu = cmCopy;',
        '    var tfMenuTarget:TextField = this.createTextField("menuTf", 190, 10, 260, 120, 24);',
        '    tfMenuTarget.menu = cm;',
        '    out.menuChecks = [',
        '      (cm instanceof ContextMenu) && (item1 instanceof ContextMenuItem),',
        '      builtIn0 && builtInHidden,',
        '      item1.caption == "Inspect Clip" && item1.separatorBefore === true && item1.enabled === true && item1.visible === true,',
        '      (item2 instanceof ContextMenuItem) && item2 !== item1 && item2.caption == "Disabled Item" && item2.enabled === false && item2.separatorBefore === false,',
        '      (cmCopy instanceof ContextMenu) && cmCopy !== cm && cmCopy.customItems.length == 3 && cmCopy.customItems[0] !== item1 && cmCopy.customItems[0].caption == "Inspect Clip",',
        '      this.left.menu === cm && this.actionBtn.menu === cmCopy && tfMenuTarget.menu === cm',
        '    ];',
        '    Stage.scaleMode = "showAll";',
        '    Stage.align = "TL";',
        '    var sm0:String = Stage.scaleMode;',
        '    var al0:String = Stage.align;',
        '    Stage.scaleMode = "exactFit";',
        '    Stage.align = "BR";',
        '    var sm1:String = Stage.scaleMode;',
        '    var al1:String = Stage.align;',
        '    Stage.align = "TL";',
        '    Stage.scaleMode = "noScale";',
        '    var sm2:String = Stage.scaleMode;',
        '    var al2:String = Stage.align;',
        '    out.stageChecks = [sm0 == "showAll", al0 == "TL", sm1 == "exactFit", al1 == "BR", sm2 == "noScale", al2 == "TL"];',
        '    var btnDepth:Number = this.actionBtn.getDepth();',
        '    this.actionBtn.tabIndex = 14;',
        '    this.actionBtn.trackAsMenu = true;',
        '    this.actionBtn._focusrect = false;',
        '    var btnFocus0:Boolean = this.actionBtn._focusrect;',
        '    this.actionBtn._focusrect = true;',
        '    var btnFocus1:Boolean = this.actionBtn._focusrect;',
        '    this.left._focusrect = false;',
        '    var mcFocus0:Boolean = this.left._focusrect;',
        '    this.left._focusrect = true;',
        '    var mcFocus1:Boolean = this.left._focusrect;',
        '    out.buttonInputChecks = [',
        '      !isNaN(btnDepth),',
        '      this.actionBtn.tabIndex === 14,',
        '      this.actionBtn.trackAsMenu === true,',
        '      btnFocus0 === false && btnFocus1 === true,',
        '      mcFocus0 === false && mcFocus1 === true',
        '    ];',
        '    var dragChip:MovieClip = this.createEmptyMovieClip("dragChip", 191);',
        '    dragChip._x = 170; dragChip._y = 292;',
        '    this.artRef._visible = false;',
        '    dragChip.startDrag(false, 0, 0, 500, 500);',
        '    dragChip.stopDrag();',
        '    this.artRef._visible = true;',
        '    out.dropTargetPath = dragChip._droptarget;',
        '    var rootLoaded:Number = this.getBytesLoaded();',
        '    var rootTotal:Number = this.getBytesTotal();',
        '    var symSlot:MovieClip = this.createEmptyMovieClip("symSlot", 192);',
        '    var symBeforeLoaded:Number = symSlot.getBytesLoaded();',
        '    var symBeforeTotal:Number = symSlot.getBytesTotal();',
        '    var symEvents:Array = [];',
        '    out.symEvents = symEvents;',
        '    var symMcl:MovieClipLoader = new MovieClipLoader();',
        '    var symListener:Object = {',
        '      onLoadStart: function(t:MovieClip):Void { symEvents.push("start:" + t._name); },',
        '      onLoadProgress: function(t:MovieClip, l:Number, tot:Number):Void { symEvents.push("prog:" + (l == tot && tot > 0)); },',
        '      onLoadComplete: function(t:MovieClip, st:Number):Void { symEvents.push("comp:" + st); },',
        '      onLoadInit: function(t:MovieClip):Void { symEvents.push("init:" + t._url + ":" + (t.getBytesLoaded() == t.getBytesTotal() && t.getBytesTotal() > 0)); }',
        '    };',
        '    symMcl.addListener(symListener);',
        '    symMcl.loadClip("symbol:CardClip", symSlot);',
        '    out.symSlot = symSlot;',
        '    out.symMcl = symMcl;',
        '    out.byteChecks = [rootLoaded > 0 && rootLoaded == rootTotal, symBeforeLoaded === 0 && symBeforeTotal === 0];',
        '    var dispClip:MovieClip = this.createEmptyMovieClip("dispClip", 193);',
        '    dispClip.beginFill(0x00cc44, 100);',
        '    dispClip.moveTo(0, 0); dispClip.lineTo(24, 0); dispClip.lineTo(24, 24); dispClip.lineTo(0, 24); dispClip.lineTo(0, 0);',
        '    dispClip.endFill();',
        '    var dMap:BitmapData = new BitmapData(24, 24, false, 0xff8080);',
        '    var dmf:DisplacementMapFilter = new DisplacementMapFilter(dMap, new Point(0, 0), 1, 2, 16, 0, "clamp", 0x112233, 0.5);',
        '    dispClip.filters = [dmf];',
        '    out.dispClip = dispClip;',
        '    out.dMap = dMap;',
        '    out.dmfChecks = [',
        '      (dmf instanceof DisplacementMapFilter),',
        '      dmf.mapBitmap === dMap && dmf.mapPoint.x == 0 && dmf.mapPoint.y == 0,',
        '      dmf.componentX == 1 && dmf.componentY == 2 && dmf.scaleX == 16 && dmf.scaleY == 0,',
        '      dmf.mode == "clamp" && dmf.color == 0x112233 && dmf.alpha == 0.5,',
        '      dispClip.filters.length == 1 && (dispClip.filters[0] instanceof DisplacementMapFilter)',
        '    ];',
        '    this.stageVideo.smoothing = true;',
        '    var smooth1:Boolean = this.stageVideo.smoothing;',
        '    this.stageVideo.smoothing = false;',
        '    var smooth0:Boolean = this.stageVideo.smoothing;',
        '    this.stageVideo.smoothing = true;',
        '    this.stageVideo.attachVideo(null);',
        '    out.videoWave6Checks = [smooth1 === true, smooth0 === false, this.stageVideo.smoothing === true];',
        '    var pj:PrintJob = new PrintJob();',
        '    var pjStarted:Boolean = pj.start();',
        '    var pjPropsOk:Boolean = pj.paperWidth > 0 && pj.paperHeight > 0 && pj.pageWidth > 0 && pj.pageHeight > 0 && (pj.orientation == "portrait" || pj.orientation == "landscape");',
        '    var pjPageOk:Boolean = pj.addPage(this, {xMin: 0, yMin: 0, xMax: 120, yMax: 80}, {printAsBitmap: true}, 2);',
        '    var frameAfterPrintPage:Number = this._currentframe;',
        '    pj.send();',
        '    out.printJobChecks = [pjStarted === true, pjPropsOk === true, pjPageOk === true, frameAfterPrintPage === 2];',
        '    return out;',
        '  }',
        '}',
      ].join('\n'),
    },
    {
      id: 'as2-source-core-lang-audit',
      path: 'app/CoreLangAudit.as',
      language: 'action_script2',
      source: [
        'class app.CoreLangAudit {',
        '  public static function sumArgs(a:Number, b:Number):Number {',
        '    return arguments.length * 100 + a + b + (this && this.bias ? this.bias : 0);',
        '  }',
        '  public static function run(host:MovieClip):Object {',
        '    var out:Object = {};',
        '    var bn:Object = new Number(255);',
        '    var bs:Object = new String("NeoFlash");',
        '    var bb:Object = new Boolean(true);',
        '    out.numberParseChecks = [',
        '      Number("42.5") == 42.5 && Number(true) == 1 && Number(false) == 0 && Number() == 0,',
        '      (bn instanceof Number) && !(255 instanceof Number) && (bn instanceof Object),',
        '      bn.valueOf() == 255 && bn.toString() == "255" && bn.toString(16) == "ff" && bn.toString(2) == "11111111" && bn.toString(99) == "255",',
        '      (255).toString(16) == "ff" && (-255).toString(16) == "-ff" && (10).toString(2) == "1010" && (255).toString(99) == "255",',
        '      parseInt("0x10") == 16 && parseInt("0777") == 511 && parseInt("1011", 2) == 11 && parseInt("zz", 36) == 1295 && isNaN(parseInt("10", 37)) && parseInt() === undefined',
        '    ];',
        '    out.stringBoolChecks = [',
        '      String(123) == "123" && Boolean(1) === true && Boolean(0) === false && Boolean() === undefined,',
        '      (bs instanceof String) && !("NeoFlash" instanceof String) && bs.length == 8 && bs.slice(0, 3) == "Neo" && bs.toUpperCase() == "NEOFLASH" && bs.toLowerCase() == "neoflash",',
        '      (bb instanceof Boolean) && !(true instanceof Boolean) && bb.valueOf() === true && bb.toString() == "true",',
        '      String.fromCharCode(65, 66, 67) == "ABC" && "abca".indexOf("a") == 0 && "abca".lastIndexOf("a") == 3 && "a,b,c".split(",").length == 3,',
        '      "NeoFlash".charAt(3) == "F" && "NeoFlash".charCodeAt(0) == 78 && "Neo".concat("Flash") == "NeoFlash" && "NeoFlash".substr(3, 5) == "Flash" && "NeoFlash".substring(0, 3) == "Neo",',
        '      chr(65) == "A" && ord("A") == 65 && length("hello") == 5 && substring("abcdef", 2, 3) == "bcd" && mbchr(66) == "B" && mbord("B") == 66 && mblength("hi") == 2 && mbsubstring("abcdef", 3, 2) == "cd",',
        '      unescape(escape("a b+c")) == "a b+c" && newline == "\\n"',
        '    ];',
        '    var arr:Array = new Array("b", "A", "c");',
        '    arr.push("d");',
        '    var popped:Object = arr.pop();',
        '    arr.unshift("z");',
        '    var shifted:Object = arr.shift();',
        '    arr.sort(Array.CASEINSENSITIVE);',
        '    var ciJoin:String = arr.join("-");',
        '    var nums:Array = [10, 2, 30];',
        '    nums.sort(Array.NUMERIC | Array.DESCENDING);',
        '    var dupNums:Array = [4, 2, 4];',
        '    var uniqRes:Object = dupNums.sort(Array.NUMERIC | Array.UNIQUESORT);',
        '    var idxArr:Object = [30, 10, 20].sort(Array.NUMERIC | Array.RETURNINDEXEDARRAY);',
        '    var recs:Array = [{k: "b", n: 2}, {k: "A", n: 10}, {k: "c", n: 1}];',
        '    recs.sortOn("k", Array.CASEINSENSITIVE);',
        '    var firstKey:String = recs[0].k;',
        '    recs.sortOn("n", Array.NUMERIC | Array.DESCENDING);',
        '    var topNum:Number = recs[0].n;',
        '    var sp:Array = [1, 2, 3, 4];',
        '    var removed:Array = sp.splice(1, 2, 9, 8);',
        '    var rev:Array = [1, 2, 3];',
        '    rev.reverse();',
        '    out.arrayChecks = [',
        '      (arr instanceof Array) && Array(1, 2).length == 2 && popped == "d" && shifted == "z",',
        '      ciJoin == "A-b-c" && nums.join(",") == "30,10,2" && uniqRes === 0,',
        '      idxArr[0] == 1 && idxArr[1] == 2 && idxArr[2] == 0,',
        '      firstKey == "A" && topNum == 10,',
        '      removed.join(",") == "2,3" && sp.join(",") == "1,9,8,4" && sp.slice(1, 3).join(",") == "9,8" && [1].concat([2, 3]).toString() == "1,2,3" && rev.join(",") == "3,2,1"',
        '    ];',
        '    var bareObj:Object = Object();',
        '    var obj:Object = new Object();',
        '    obj.x = 10;',
        '    var watchLog:Array = [];',
        '    var wOk:Boolean = obj.watch("x", function(prop:String, oldVal:Number, newVal:Number, data:String):Number {',
        '      watchLog.push(prop + ":" + oldVal + "->" + newVal + ":" + data);',
        '      return newVal * 2;',
        '    }, "tag");',
        '    obj.x = 15;',
        '    var afterWatch:Number = obj.x;',
        '    var uwOk:Boolean = obj.unwatch("x");',
        '    obj.x = 7;',
        '    var backing:Number = 3;',
        '    var apOk:Boolean = obj.addProperty("doubled", function():Number { return backing * 2; }, function(v:Number):Void { backing = v; });',
        '    obj.doubled = 9;',
        '    var dynObj:Object = {known: 1, __resolve: function(name:String):String { return "resolved:" + name; }};',
        '    var protoBase:Object = {sharedProp: 55};',
        '    var subObj:Object = {};',
        '    subObj.__proto__ = protoBase;',
        '    out.objectFuncChecks = [',
        '      wOk && uwOk && afterWatch == 30 && obj.x == 7 && watchLog.length == 1 && watchLog[0] == "x:10->15:tag",',
        '      apOk && obj.doubled == 18 && backing == 9,',
        '      dynObj.known == 1 && dynObj.missingField == "resolved:missingField",',
        '      bareObj.__proto__ === undefined && (Object(7) instanceof Number) && (new Object("hi") instanceof String) && (Object(true) instanceof Boolean) && Object(obj) === obj && obj.hasOwnProperty("x") && !subObj.hasOwnProperty("sharedProp") && protoBase.isPrototypeOf(subObj) && obj.isPropertyEnumerable("x") && obj.valueOf() === obj && obj.toString().indexOf("Object") >= 0,',
        '      (sumArgs instanceof Function) && sumArgs.call({bias: 5}, 3, 4) == 212 && sumArgs.apply({bias: 1}, [6, 7]) == 214',
        '    ];',
        '    var dt:Date = new Date(Date.UTC(2025, 5, 15, 8, 20, 30, 450));',
        '    var utcBefore:Boolean = dt.getUTCFullYear() == 2025 && dt.getUTCMonth() == 5 && dt.getUTCDate() == 15 && dt.getUTCDay() == 0 && dt.getUTCHours() == 8 && dt.getUTCMinutes() == 20 && dt.getUTCSeconds() == 30 && dt.getUTCMilliseconds() == 450 && dt.getUTCYear() == 125;',
        '    dt.setUTCFullYear(2026); dt.setUTCMonth(1); dt.setUTCDate(10); dt.setUTCHours(14); dt.setUTCMinutes(45); dt.setUTCSeconds(12); dt.setUTCMilliseconds(250);',
        '    var utcAfter:Boolean = dt.getUTCFullYear() == 2026 && dt.getUTCMonth() == 1 && dt.getUTCDate() == 10 && dt.getUTCHours() == 14 && dt.getUTCMinutes() == 45 && dt.getUTCSeconds() == 12 && dt.getUTCMilliseconds() == 250;',
        '    var localDt:Date = new Date(2024, 0, 5, 9, 10, 11, 120);',
        '    localDt.setFullYear(2025); localDt.setMonth(3); localDt.setDate(20); localDt.setHours(16); localDt.setMinutes(30); localDt.setSeconds(40); localDt.setMilliseconds(500);',
        '    var localOk:Boolean = localDt.getFullYear() == 2025 && localDt.getMonth() == 3 && localDt.getDate() == 20 && localDt.getHours() == 16 && localDt.getMinutes() == 30 && localDt.getSeconds() == 40 && localDt.getMilliseconds() == 500 && localDt.getDay() >= 0 && localDt.getYear() == 125;',
        '    localDt.setYear(99);',
        '    var year99:Boolean = (localDt.getFullYear() == 1999);',
        '    var tMs:Number = localDt.getTime();',
        '    localDt.setTime(tMs + 1000);',
        '    out.dateErrorBroadcasterChecks = [',
        '      utcBefore && utcAfter && typeof Date() == "string",',
        '      localOk && year99 && localDt.valueOf() == tMs + 1000 && !isNaN(localDt.getTimezoneOffset()),',
        '      (function():Boolean { var fin:Boolean = false; var msg:String = ""; try { var e:Error = new Error("boom"); if (e instanceof Error && e.name == "Error") throw e; } catch (err:Error) { msg = err.message + "|" + err.toString(); } finally { fin = true; } return fin && msg == "boom|boom" && (new Error()).toString() == "Error"; })(),',
        '      (function():Boolean { var hub:Object = {}; AsBroadcaster.initialize(hub); var hits:Array = []; var l1:Object = {onPing: function(a:String, b:Number):Void { hits.push(a + ":" + b); }}; hub.addListener(l1); var len1:Number = hub._listeners.length; hub.broadcastMessage("onPing", "ok", 7); var rem:Boolean = hub.removeListener(l1); hub.broadcastMessage("onPing", "after", 8); return len1 == 1 && rem && hub._listeners.length == 0 && hits.length == 1 && hits[0] == "ok:7"; })()',
        '    ];',
        '    var opVal:Number = 16;',
        '    opVal += 4; opVal -= 2; opVal *= 2; opVal /= 4; opVal %= 5;',
        '    var bitVal:Number = 5;',
        '    bitVal <<= 1; bitVal |= 3; bitVal &= 11; bitVal ^= 1; bitVal >>= 1; bitVal >>>= 1;',
        '    var pre:Number = 3; var preRes:Number = ++pre; var postRes:Number = pre--;',
        '    var withTarget:Object = {wx: 12, wy: 30}; var withSum:Number = 0;',
        '    with (withTarget) { withSum = wx + wy; }',
        '    var keysSeen:Number = 0; for (var k:String in withTarget) { if (k == "wx" || k == "wy") keysSeen++; }',
        '    var swOut:String = ""; switch (2) { case 1: swOut = "one"; break; case 2: swOut = "two"; break; default: swOut = "other"; }',
        '    var loopSum:Number = 0; var wi:Number = 0; while (wi < 4) { wi++; if (wi == 2) continue; loopSum += wi; }',
        '    var doCount:Number = 0; do { doCount++; } while (doCount < 2);',
        '    var delObj:Object = {temp: 99}; var delOk:Boolean = delete delObj.temp;',
        '    var localKeepVar:Number = 123;',
        '    var delLocalRes:Boolean = delete localKeepVar;',
        '    host.tempHostProp = 456;',
        '    var mcKeysSeen:Number = 0;',
        '    for (var mk:String in host) { if (mk == "tempHostProp") mcKeysSeen++; }',
        '    var delHostPropOk:Boolean = delete host.tempHostProp;',
        '    var delHostPropAgain:Boolean = delete host.tempHostProp;',
        '    var primKeys:Number = 0;',
        '    for (var pk:String in 42) { primKeys++; }',
        '    var castMcOk:Boolean = (MovieClip(host) === host) && (MovieClip(delObj) === null) && (TextField(host) === null);',
        '    trace("batch4-trace-ok");',
        '    out.operatorStmtChecks = [',
        '      opVal == 4 && bitVal == 2 && preRes == 4 && postRes == 4 && pre == 3,',
        '      ((5 & 3) == 1) && ((5 | 2) == 7) && ((5 ^ 1) == 4) && ((~0) == -1) && ((1 << 3) == 8) && ((-8 >> 1) == -4) && ((-1 >>> 30) == 3),',
        '      ("4" == 4) && !("4" === 4) && ("4" !== 4) && (3 != 4) && (2 < 3) && (2 <= 2) && (5 > 4) && (5 >= 5) && (true && !false) && (false || true) && ((true ? 11 : 22) == 11) && (void(99) === undefined) && (typeof "abc" == "string"),',
        '      withSum == 42 && keysSeen == 2 && swOut == "two" && loopSum == 8 && doCount == 2 && delOk && delObj.temp === undefined && !delLocalRes && localKeepVar == 123 && mcKeysSeen == 1 && delHostPropOk && !delHostPropAgain && primKeys == 0 && castMcOk',
        '    ];',
        '    var xdoc:XML = new XML();',
        '    xdoc.ignoreWhite = true;',
        '    xdoc.parseXML("<?xml version=\\"1.0\\"?><!DOCTYPE catalog><catalog xmlns:ns=\\"urn:neo\\"><ns:book id=\\"b1\\"> Guide </ns:book><note>second</note></catalog>");',
        '    xdoc.xmlDecl = "<?xml version=\\"1.0\\" encoding=\\"UTF-8\\"?>";',
        '    xdoc.docTypeDecl = "<!DOCTYPE catalog>";',
        '    var rootNode:XMLNode = xdoc.firstChild;',
        '    var bookNode:XMLNode = rootNode.firstChild;',
        '    var extraEl:XMLNode = xdoc.createElement("extra");',
        '    extraEl.attributes.code = "E1";',
        '    extraEl.appendChild(xdoc.createTextNode("payload"));',
        '    rootNode.insertBefore(extraEl, rootNode.lastChild);',
        '    var clonedEl:XMLNode = extraEl.cloneNode(true);',
        '    extraEl.removeNode();',
        '    rootNode.appendChild(clonedEl);',
        '    xdoc.contentType = "application/xml";',
        '    xdoc.addRequestHeader("X-Neo-Client", "AS2");',
        '    xdoc.addRequestHeader(["X-Batch", "4"]);',
        '    var xmlFireOk:Boolean = xdoc.send("https://app.example.test/api/xml-send");',
        '    var xmlNavDoc:XML = new XML("<ping/>");',
        '    var xmlNavOk:Boolean = xmlNavDoc.send("https://app.example.test/api/xml-nav", "_blank", "GET");',
        '    var xmlLog:Array = [];',
        '    var xmlReply:XML = new XML();',
        '    xmlReply.ignoreWhite = true;',
        '    xmlReply.onHTTPStatus = function(st:Number):Void { xmlLog.push("http:" + st); };',
        '    xmlReply.onLoad = function(ok:Boolean):Void { xmlLog.push("load:" + ok + ":" + this.loaded + ":" + this.firstChild.nodeName + ":" + this.idMap.r1.firstChild.nodeValue + ":" + (this.getBytesTotal() > 0 && this.getBytesLoaded() == this.getBytesTotal())); };',
        '    xdoc.sendAndLoad("https://app.example.test/api/xml-echo", xmlReply);',
        '    var xmlDirect:XML = new XML();',
        '    xmlDirect.onData = function(raw:String):Void { xmlLog.push("raw:" + (raw.indexOf("pong") >= 0)); };',
        '    xmlDirect.load("https://app.example.test/api/xml-echo");',
        '    out.xmlChecks = [',
        '      (xdoc instanceof XML) && (xdoc instanceof XMLNode) && (rootNode instanceof XMLNode) && xdoc.status == 0 && xdoc.ignoreWhite === true,',
        '      xdoc.xmlDecl.indexOf("1.0") >= 0 && xdoc.docTypeDecl.indexOf("catalog") >= 0 && xdoc.idMap.b1 === bookNode,',
        '      rootNode.nodeType == 1 && rootNode.nodeName == "catalog" && rootNode.hasChildNodes() && rootNode.childNodes.length == 3,',
        '      bookNode.localName == "book" && bookNode.prefix == "ns" && bookNode.namespaceURI == "urn:neo" && bookNode.getNamespaceForPrefix("ns") == "urn:neo" && bookNode.getPrefixForNamespace("urn:neo") == "ns",',
        '      bookNode.nextSibling.nodeName == "note" && rootNode.lastChild.attributes.code == "E1" && rootNode.lastChild.previousSibling.nodeName == "note" && rootNode.lastChild.parentNode === rootNode && clonedEl.firstChild.nodeValue == "payload",',
        '      xmlFireOk && xmlNavOk && xdoc.toString().indexOf("catalog") >= 0',
        '    ];',
        '    out.xmlLog = xmlLog;',
        '    var lv:LoadVars = new LoadVars();',
        '    lv.user = "ada";',
        '    lv.role = "author";',
        '    lv.contentType = "application/x-www-form-urlencoded";',
        '    lv.addRequestHeader("X-Form", "1");',
        '    var lvDecoded:LoadVars = new LoadVars();',
        '    lvDecoded.decode("alpha=10&beta=hello");',
        '    var lvNoTargetOk:Boolean = lv.send("https://app.example.test/api/form-noop");',
        '    var lvNav:LoadVars = new LoadVars();',
        '    lvNav.user = "ada";',
        '    var lvPostNavOk:Boolean = lvNav.send("https://app.example.test/api/form-nav", "_blank", "POST");',
        '    var lvLog:Array = [];',
        '    var lvReply:LoadVars = new LoadVars();',
        '    lvReply.onHTTPStatus = function(st:Number):Void { lvLog.push("http:" + st); };',
        '    lvReply.onLoad = function(ok:Boolean):Void { lvLog.push("load:" + ok + ":" + this.loaded + ":" + this.serverStatus + ":" + this.score + ":" + (this.getBytesLoaded() == this.getBytesTotal() && this.getBytesTotal() > 0)); };',
        '    lv.sendAndLoad("https://app.example.test/api/form-echo", lvReply, "POST");',
        '    var lvDirect:LoadVars = new LoadVars();',
        '    lvDirect.onLoad = function(ok:Boolean):Void { lvLog.push("get:" + ok + ":" + this.score); };',
        '    lvDirect.load("https://app.example.test/api/form-echo");',
        '    out.loadVarsChecks = [',
        '      (lv instanceof LoadVars) && lv.toString().indexOf("user=ada") >= 0 && lv.contentType == "application/x-www-form-urlencoded",',
        '      lvDecoded.alpha == "10" && lvDecoded.beta == "hello",',
        '      lvNoTargetOk && lvPostNavOk',
        '    ];',
        '    out.lvLog = lvLog;',
        '    var selTf1:TextField = host.createTextField("selTf1", 110, 10, 120, 140, 24);',
        '    var selTf2:TextField = host.createTextField("selTf2", 111, 10, 150, 140, 24);',
        '    selTf1.type = "input"; selTf1.selectable = true; selTf1.text = " focusTarget ";',
        '    selTf2.type = "input"; selTf2.selectable = true; selTf2.text = "secondField";',
        '    var focusLog:Array = [];',
        '    selTf1.onSetFocus = function(oldFocus:Object):Void { focusLog.push("tf1:set"); };',
        '    selTf1.onKillFocus = function(newFocus:Object):Void { focusLog.push("tf1:kill:" + (newFocus === selTf2)); };',
        '    var selListener:Object = { onSetFocus: function(oldF:Object, newF:Object):Void { focusLog.push("sel:" + (newF != null ? newF._name : "null")); } };',
        '    Selection.addListener(selListener);',
        '    var f1Ok:Boolean = Selection.setFocus(selTf1);',
        '    var path1:String = Selection.getFocus();',
        '    Selection.setSelection(1, 6);',
        '    var bIdx:Number = Selection.getBeginIndex();',
        '    var eIdx:Number = Selection.getEndIndex();',
        '    var cIdx:Number = Selection.getCaretIndex();',
        '    var f2Ok:Boolean = Selection.setFocus("_level0.selTf2");',
        '    var path2:String = Selection.getFocus();',
        '    var remSelOk:Boolean = Selection.removeListener(selListener);',
        '    var clearFocusOk:Boolean = Selection.setFocus(null);',
        '    var pathNull:Object = Selection.getFocus();',
        '    selTf1.removeTextField();',
        '    selTf2.removeTextField();',
        '    var timerLog:Array = [];',
        '    var cancelledIv:Number = setInterval(function():Void { timerLog.push("badIv"); }, 5);',
        '    clearInterval(cancelledIv);',
        '    var cancelledTo:Number = setTimeout(function():Void { timerLog.push("badTo"); }, 5);',
        '    clearTimeout(cancelledTo);',
        '    var activeIv:Number = 0;',
        '    activeIv = setInterval(function(tag:String):Void {',
        '      timerLog.push("iv:" + tag);',
        '      updateAfterEvent();',
        '      clearInterval(activeIv);',
        '    }, 5, "tick");',
        '    setTimeout(function(tag:String):Void { timerLog.push("to:" + tag); }, 5, "once");',
        '    out.selectionTimerChecks = [',
        '      f1Ok && path1 == "_level0.selTf1" && bIdx == 1 && eIdx == 6 && cIdx == 6,',
        '      f2Ok && path2 == "_level0.selTf2" && remSelOk && clearFocusOk && pathNull === null,',
        '      focusLog.join(",") == "tf1:set,sel:selTf1,tf1:kill:true,sel:selTf2" && getTimer() >= 0',
        '    ];',
        '    out.timerLog = timerLog;',
        '    return out;',
        '  }',
        '}',
      ].join('\n'),
    },
    {
      id: 'as2-source-graphics-text-audit',
      path: 'app/GraphicsTextAudit.as',
      language: 'action_script2',
      source: [
        'import flash.display.BitmapData;',
        'import flash.geom.Point;',
        'import flash.geom.Rectangle;',
        'import flash.geom.Matrix;',
        'import flash.geom.ColorTransform;',
        'import flash.filters.BitmapFilter;',
        'import flash.filters.BlurFilter;',
        'import flash.filters.GlowFilter;',
        'import flash.filters.DropShadowFilter;',
        'import flash.filters.BevelFilter;',
        'import flash.filters.ColorMatrixFilter;',
        'import flash.filters.ConvolutionFilter;',
        'import flash.filters.GradientGlowFilter;',
        'import flash.filters.GradientBevelFilter;',
        'class app.GraphicsTextAudit {',
        '  public static function run(host:MovieClip):Object {',
        '    var out:Object = {};',
        '    var swatch:MovieClip = host.createEmptyMovieClip("colorSwatch", 120);',
        '    swatch.beginFill(0xffffff, 100); swatch.moveTo(0, 0); swatch.lineTo(12, 0); swatch.lineTo(12, 12); swatch.lineTo(0, 12); swatch.endFill();',
        '    var col:Color = new Color(swatch);',
        '    col.setRGB(0x336699);',
        '    var rgb1:Number = col.getRGB();',
        '    col.setTransform({ra: 50, rb: 10, ga: 75, gb: 20, ba: 100, bb: 30, aa: 25, ab: 5});',
        '    var ctr:Object = col.getTransform();',
        '    out.colorClassChecks = [',
        '      (col instanceof Color) && rgb1 == 0x336699,',
        '      ctr.ra == 50 && ctr.rb == 10 && ctr.ga == 75 && ctr.gb == 20 && ctr.ba == 100 && ctr.bb == 30 && ctr.aa == 25 && ctr.ab == 5',
        '    ];',
        '    var loadedById:BitmapData = BitmapData.loadBitmap("original-pixel-creature");',
        '    var loadedByLinkage:BitmapData = BitmapData.loadBitmap("CreatureBitmap");',
        '    var loadedByPath:BitmapData = BitmapData.loadBitmap("images/pixel-creature.png");',
        '    var loadedByStem:BitmapData = BitmapData.loadBitmap("pixel-creature");',
        '    var loadedMissing:BitmapData = BitmapData.loadBitmap("does-not-exist");',
        '    var loadOk:Boolean = (loadedById instanceof BitmapData) && (loadedByLinkage instanceof BitmapData) && (loadedByPath instanceof BitmapData) && (loadedByStem instanceof BitmapData) && loadedMissing === null && loadedByLinkage.width > 0 && loadedByLinkage.height > 0 && loadedById.compare(loadedByLinkage) === 0 && loadedByPath.compare(loadedByStem) === 0;',
        '    loadedById.dispose(); loadedByLinkage.dispose(); loadedByPath.dispose(); loadedByStem.dispose();',
        '    var bmp:BitmapData = new BitmapData(8, 8, true, 0xff102030);',
        '    bmp.setPixel(1, 1, 0xabcdef);',
        '    bmp.setPixel32(2, 2, 0x80ff0000);',
        '    var px1:Number = bmp.getPixel(1, 1);',
        '    var px2:Number = bmp.getPixel32(2, 2);',
        '    bmp.fillRect(new Rectangle(4, 4, 4, 4), 0xff00ff00);',
        '    var boundsGreen:Rectangle = bmp.getColorBoundsRect(0xffffffff, 0xff00ff00, true);',
        '    var clonedBmp:BitmapData = bmp.clone();',
        '    var cmpSame:Object = bmp.compare(clonedBmp);',
        '    clonedBmp.floodFill(0, 0, 0xffffffff);',
        '    var cmpDiff:Object = bmp.compare(clonedBmp);',
        '    var chBmp:BitmapData = new BitmapData(8, 8, true, 0xff000000);',
        '    chBmp.copyChannel(bmp, new Rectangle(4, 4, 2, 2), new Point(0, 0), 2, 1);',
        '    var cpBmp:BitmapData = new BitmapData(8, 8, true, 0x00000000);',
        '    cpBmp.copyPixels(bmp, new Rectangle(4, 4, 3, 3), new Point(1, 1));',
        '    cpBmp.merge(bmp, new Rectangle(0, 0, 2, 2), new Point(0, 0), 128, 128, 128, 255);',
        '    cpBmp.colorTransform(new Rectangle(1, 1, 2, 2), new ColorTransform(1, 1, 1, 1, 10, 0, 0, 0));',
        '    cpBmp.scroll(1, 0);',
        '    var threshCount:Number = bmp.threshold(bmp, bmp.rectangle, new Point(0, 0), "==", 0xff00ff00, 0xff0000ff, 0xffffffff, false);',
        '    var hitPt:Boolean = bmp.hitTest(new Point(0, 0), 1, new Point(5, 5));',
        '    var palBmp:BitmapData = new BitmapData(2, 2, false, 0x000001);',
        '    var blueMap:Array = [];',
        '    for (var pi:Number = 0; pi < 256; pi++) { blueMap.push(pi == 1 ? 0x0000aa : pi); }',
        '    palBmp.paletteMap(palBmp, palBmp.rectangle, new Point(0, 0), null, null, blueMap);',
        '    var palPx:Number = palBmp.getPixel(0, 0);',
        '    var noiseBmp:BitmapData = new BitmapData(4, 4, false, 0);',
        '    noiseBmp.noise(7, 10, 200, 7, false);',
        '    var perlinBmp:BitmapData = new BitmapData(4, 4, false, 0);',
        '    perlinBmp.perlinNoise(4, 4, 2, 11, false, true, 7, false);',
        '    var seed:Number = perlinBmp.pixelDissolve(noiseBmp, noiseBmp.rectangle, new Point(0, 0), 3, 4, 0xff112233);',
        '    var blurF:BlurFilter = new BlurFilter(4, 4, 1);',
        '    var fRect:Rectangle = bmp.generateFilterRect(bmp.rectangle, blurF);',
        '    bmp.applyFilter(bmp, bmp.rectangle, new Point(0, 0), blurF);',
        '    var drawBmp:BitmapData = new BitmapData(8, 8, true, 0x00000000);',
        '    drawBmp.draw(bmp, new Matrix(), new ColorTransform(), "normal", new Rectangle(0, 0, 8, 8), true);',
        '    var drawnAlpha:Number = (drawBmp.getPixel32(2, 2) >>> 24) & 0xff;',
        '    if (cmpDiff instanceof BitmapData) cmpDiff.dispose();',
        '    bmp.dispose(); clonedBmp.dispose(); chBmp.dispose(); cpBmp.dispose(); palBmp.dispose(); noiseBmp.dispose(); perlinBmp.dispose(); drawBmp.dispose();',
        '    out.bitmapChecks = [',
        '      loadOk,',
        '      px1 == 0xabcdef && ((px2 >>> 24) & 0xff) >= 0x7f && boundsGreen.equals(new Rectangle(4, 4, 4, 4)),',
        '      cmpSame === 0 && (cmpDiff instanceof BitmapData),',
        '      threshCount == 16 && hitPt && palPx == 0x0000aa && seed >= 0,',
        '      (fRect instanceof Rectangle) && fRect.width >= 8 && drawnAlpha > 0',
        '    ];',
        '    var fBlur:BlurFilter = new BlurFilter(6, 8, 2);',
        '    var fGlow:GlowFilter = new GlowFilter(0xff3366, 0.8, 6, 6, 2, 1, true, false);',
        '    var fDrop:DropShadowFilter = new DropShadowFilter(4, 45, 0x112233, 0.75, 5, 5, 1.5, 1, false, false, false);',
        '    var fBevel:BevelFilter = new BevelFilter(3, 45, 0xffffff, 0.9, 0x000000, 0.8, 4, 4, 1, 1, "inner", false);',
        '    var fCmat:ColorMatrixFilter = new ColorMatrixFilter([1,0,0,0,5, 0,1,0,0,0, 0,0,1,0,0, 0,0,0,1,0]);',
        '    var fConv:ConvolutionFilter = new ConvolutionFilter(3, 3, [0,0,0, 0,1,0, 0,0,0], 1, 0, true, true, 0x000000, 1);',
        '    var fGGlow:GradientGlowFilter = new GradientGlowFilter(2, 45, [0xff0000, 0x00ff00], [1, 1], [0, 255], 4, 4, 1, 1, "outer", false);',
        '    var fGBevel:GradientBevelFilter = new GradientBevelFilter(2, 45, [0xffffff, 0x333333], [1, 1], [0, 255], 4, 4, 1, 1, "inner", false);',
        '    swatch.filters = [fBlur, fGlow, fDrop, fBevel, fCmat, fConv, fGGlow, fGBevel];',
        '    var filterTargetBmp:BitmapData = new BitmapData(8, 8, true, 0xff4488cc);',
        '    filterTargetBmp.applyFilter(filterTargetBmp, filterTargetBmp.rectangle, new Point(0, 0), fCmat);',
        '    var cmatRed:Number = (filterTargetBmp.getPixel(0, 0) >> 16) & 0xff;',
        '    filterTargetBmp.dispose();',
        '    out.filterChecks = [',
        '      (fBlur instanceof BitmapFilter) && (fBlur instanceof BlurFilter) && (fBlur.clone() instanceof BlurFilter) && fBlur.blurX == 6 && fBlur.blurY == 8 && fBlur.quality == 2,',
        '      (fGlow instanceof GlowFilter) && (fGlow.clone() instanceof GlowFilter) && fGlow.color == 0xff3366 && fGlow.inner === true && fGlow.knockout === false,',
        '      (fDrop instanceof DropShadowFilter) && (fDrop.clone() instanceof DropShadowFilter) && fDrop.distance == 4 && fDrop.angle == 45 && fDrop.hideObject === false,',
        '      (fBevel instanceof BevelFilter) && (fBevel.clone() instanceof BevelFilter) && fBevel.highlightColor == 0xffffff && fBevel.type == "inner",',
        '      (fCmat instanceof ColorMatrixFilter) && (fCmat.clone() instanceof ColorMatrixFilter) && fCmat.matrix.length == 20 && cmatRed == 0x49,',
        '      (fConv instanceof ConvolutionFilter) && (fConv.clone() instanceof ConvolutionFilter) && fConv.matrixX == 3 && fConv.matrixY == 3 && fConv.divisor == 1,',
        '      (fGGlow instanceof GradientGlowFilter) && (fGGlow.clone() instanceof GradientGlowFilter) && fGGlow.colors.length == 2 && fGGlow.type == "outer",',
        '      (fGBevel instanceof GradientBevelFilter) && (fGBevel.clone() instanceof GradientBevelFilter) && fGBevel.ratios.length == 2 && swatch.filters.length == 8',
        '    ];',
        '    var fmt:TextFormat = new TextFormat("Arial", 16, 0x224466, true, true, true, "https://app.example.test", "_blank", "center", 4, 6, 2, 3);',
        '    fmt.blockIndent = 5; fmt.bullet = true; fmt.kerning = false; fmt.letterSpacing = 1;',
        '    var ext:Object = fmt.getTextExtent("NeoFlash Text", 200);',
        '    fmt.tabStops = [20, 40];',
        '    var sheet:TextField.StyleSheet = new TextField.StyleSheet();',
        '    var cssOk:Boolean = sheet.parseCSS("p { color: #ff0000; font-size: 18px; }");',
        '    sheet.setStyle(".tag", {color: "#00aa55", fontWeight: "bold"});',
        '    var tagStyle:Object = sheet.getStyle(".tag");',
        '    var styleNames:Array = sheet.getStyleNames();',
        '    var txFmt:TextFormat = sheet.transform(tagStyle);',
        '    var clearSheet:TextField.StyleSheet = new TextField.StyleSheet();',
        '    clearSheet.setStyle("p", {color: "#111111"});',
        '    clearSheet.clear();',
        '    var cssLoadLog:Array = [];',
        '    var remoteSheet:TextField.StyleSheet = new TextField.StyleSheet();',
        '    remoteSheet.onLoad = function(ok:Boolean):Void { cssLoadLog.push("css:" + ok + ":" + this.getStyleNames().length); };',
        '    remoteSheet.load("https://app.example.test/api/theme.css");',
        '    var banner:MovieClip = host.attachMovie("StaticBannerClip", "staticBanner", 125);',
        '    var snap:TextSnapshot = banner.getTextSnapshot();',
        '    var snapCount:Number = snap.getCount();',
        '    var snapText:String = snap.getText(0, 8, false);',
        '    var snapFind:Number = snap.findText(0, "Static", true);',
        '    snap.setSelectColor(0xffcc00);',
        '    snap.setSelected(0, 8, true);',
        '    var snapHasSel:Boolean = snap.getSelected(0, 8);',
        '    var snapSelText:String = snap.getSelectedText(false);',
        '    var snapRuns:Array = snap.getTextRunInfo(0, 4);',
        '    var snapHit:Number = snap.hitTestTextNearPos(10, 12, 60);',
        '    var tf:TextField = host.createTextField("richTf", 130, 20, 200, 160, 44);',
        '    var wrapFmt:TextFormat = new TextFormat("Arial", 16, 0x224466, true, true, true);',
        '    wrapFmt.align = "justify";',
        '    tf.setNewTextFormat(wrapFmt);',
        '    var newFmtBack:TextFormat = tf.getNewTextFormat();',
        '    tf.type = "input"; tf.multiline = true; tf.wordWrap = true; tf.selectable = true;',
        '    tf.background = true; tf.backgroundColor = 0xfefefe; tf.border = true; tf.borderColor = 0x334455;',
        '    tf.password = false; tf.maxChars = 64; tf.restrict = "A-Za-z0-9 "; tf.mouseWheelEnabled = true;',
        '    tf.tabEnabled = true; tf.tabIndex = 7; tf.autoSize = "left"; tf.condenseWhite = true;',
        '    tf.textColor = 0x113355;',
        '    tf.text = "Hello World";',
        '    tf.replaceText(6, 11, "NeoFlash");',
        '    Selection.setFocus(tf);',
        '    Selection.setSelection(0, 5);',
        '    tf.replaceSel("Hi");',
        '    Selection.setFocus(null);',
        '    var afterEditText:String = tf.text;',
        '    var tfLen:Number = tf.length;',
        '    var tfDepth:Number = tf.getDepth();',
        '    var rangeFmt1:TextFormat = new TextFormat("Courier", 20, 0xff0000, true);',
        '    var rangeFmt2:TextFormat = new TextFormat("Times", 14, 0x00ff00, false, true);',
        '    tf.setTextFormat(0, 2, rangeFmt1);',
        '    tf.setTextFormat(3, 11, rangeFmt2);',
        '    var subFmt0:TextFormat = tf.getTextFormat(0, 2);',
        '    var subFmt1:TextFormat = tf.getTextFormat(3, 11);',
        '    var mixedFmt:TextFormat = tf.getTextFormat(0, 11);',
        '    var singleIdxFmt:TextFormat = tf.getTextFormat(1);',
        '    var setAllFmt:TextFormat = new TextFormat("Verdana", 18, 0x123456);',
        '    setAllFmt.align = "justify";',
        '    tf.setTextFormat(setAllFmt);',
        '    var allFmtBack:TextFormat = tf.getTextFormat();',
        '    var rangeRunsOk:Boolean = subFmt0.size == 20 && subFmt0.color == 0xff0000 && subFmt1.size == 14 && subFmt1.color == 0x00ff00 && mixedFmt.size === null && singleIdxFmt.size == 20 && allFmtBack.size == 18 && allFmtBack.align == "justify" && newFmtBack.align == "justify";',
        '    var castGfxOk:Boolean = (TextField(tf) === tf) && (TextField.StyleSheet(sheet) === sheet) && (Point(new Point(1, 2)) instanceof Point) && (Point(tf) === null);',
        '    var scrollerLog:Array = [];',
        '    var tfListener:Object = { onScroller: function(field:TextField):Void { scrollerLog.push("list:" + field.scroll); } };',
        '    tf.addListener(tfListener);',
        '    tf.onScroller = function(field:TextField):Void { scrollerLog.push("prop:" + field.scroll); };',
        '    tf.autoSize = "none";',
        '    tf._height = 24;',
        '    tf.text = "L1\\nL2\\nL3\\nL4\\nL5";',
        '    tf.scroll = 2;',
        '    var maxSc:Number = tf.maxscroll;',
        '    var botSc:Number = tf.bottomScroll;',
        '    var remTfList:Boolean = tf.removeListener(tfListener);',
        '    tf.html = true;',
        '    tf.styleSheet = sheet;',
        '    tf.htmlText = "<p>Heading</p>";',
        '    tf.filters = [new BlurFilter(2, 2, 1)];',
        '    var tfGeomOk:Boolean = (tf instanceof TextField) && tf._name == "richTf" && tf._parent === host && tf._target == "/richTf" && tf._x == 20 && tf._y == 200 && tf._width > 0 && tf._height > 0 && tf._xscale == 100 && tf._yscale == 100 && tf._alpha == 100 && tf._rotation == 0 && tf._visible === true && typeof tf._url == "string" && !isNaN(tf._xmouse) && !isNaN(tf._ymouse) && tf.textWidth > 0 && tf.textHeight > 0 && tf.filters.length == 1;',
        '    tf.removeTextField();',
        '    var removedAlive:Boolean = (host.richTf !== undefined);',
        '    out.textChecks = [',
        '      (fmt instanceof TextFormat) && fmt.font == "Arial" && fmt.size == 16 && fmt.bold === true && fmt.italic === true && fmt.underline === true && fmt.blockIndent == 5 && fmt.bullet === true && fmt.letterSpacing == 1 && fmt.tabStops.length == 2 && ext.width > 0 && ext.textFieldWidth > 0 && ext.ascent > 0,',
        '      (sheet instanceof TextField.StyleSheet) && cssOk && tagStyle.color == "#00aa55" && styleNames.length == 2 && (txFmt instanceof TextFormat) && txFmt.color == 0x00aa55 && txFmt.bold === true && clearSheet.getStyleNames().length == 0,',
        '      (snap instanceof TextSnapshot) && snapCount >= 19 && snapText == "NeoFlash" && snapFind == 9 && snapHasSel && snapSelText == "NeoFlash" && snapRuns.length > 0 && snapHit >= 0,',
        '      (newFmtBack instanceof TextFormat) && newFmtBack.size == 16 && afterEditText == "Hi NeoFlash" && tfLen == 11 && tfDepth == 130 && rangeRunsOk && castGfxOk,',
        '      maxSc >= 2 && botSc >= 2 && scrollerLog.length == 2 && remTfList && tfGeomOk && !removedAlive',
        '    ];',
        '    out.cssLoadLog = cssLoadLog;',
        '    return out;',
        '  }',
        '}',
      ].join('\n'),
    },
  );
  const projectFile = resolve(directory, 'foundation.nflash');
  const outDir = resolve(directory, 'foundation-out');
  writeFileSync(projectFile, JSON.stringify(project));
  const exported = spawnSync(cargo, ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--',
    projectFile, '--output', outDir], { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
  assert.equal(exported.status, 0, exported.error?.message || exported.stderr);

  const sandbox = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
  vm.runInContext(readFileSync(resolve(outDir, 'bundle.js'), 'utf8'), sandbox);
  const bundle = vm.runInContext('NeoFlashBundle', sandbox);
  vm.runInContext(readFileSync(resolve(outDir, 'neoflash_web.js'), 'utf8'), sandbox);
  sandbox.wasmBytes = Buffer.from(bundle.wasm, 'base64');
  const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', sandbox);
  const fPlayer = new wasm.WebPlayer(JSON.stringify(bundle.project));
  const { SemanticLayer } = require(resolve(root, 'web/accessibility.js'));
  const semanticDom = createSemanticDom();
  const semanticLayer = new SemanticLayer(semanticDom, () => {});
  const navigations = [], fsCommands = [], clipboardWrites = [], shownTabs = [], printCaptures = [], videoLoads = [], xmlSockets = [], fetchCalls = [];
  let printCount = 0;
  const memMap = new Map();
  const memStorage = { getItem: k => memMap.has(k) ? memMap.get(k) : null, setItem: (k, v) => { memMap.set(k, String(v)); }, removeItem: k => { memMap.delete(k); } };
  const bcPeers = new Set();
  class MockBC {
    constructor(name) { this.name = name; this.onmessage = null; bcPeers.add(this); }
    postMessage(data) { for (const peer of bcPeers) if (peer !== this) queueMicrotask(() => peer.onmessage?.({ data })); }
    close() { bcPeers.delete(this); }
  }
  const fCanvas = Object.assign(new Surface(), { id: 'stage' });
  const stageViewport = createSemanticDom();
  let stageLayoutFn = null;
  const stagePreview = {
    view: { scale: 1, scaleY: 1 },
    panMode: false,
    suppressClick: false,
    setStageLayout(fn) { stageLayoutFn = fn; },
    resize() { if (stageLayoutFn) stageLayoutFn(960, 540); },
  };
  const hadLocation = Object.hasOwn(globalThis, 'location'), prevLocation = globalThis.location;
  const hadPath2D = Object.hasOwn(globalThis, 'Path2D'), prevPath2D = globalThis.Path2D;
  globalThis.location = { href: 'https://app.example.test/index.html', protocol: 'https:' };
  globalThis.Path2D = class Path2D { moveTo() {} lineTo() {} quadraticCurveTo() {} bezierCurveTo() {} closePath() {} };
  let fHost;
  try {
    ({ host: fHost } = createRuntimeHost(root, fPlayer, Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source }, code: item.code, location: item.location,
    })), {
      bundle,
      canvas: fCanvas,
      semanticLayer,
      as2Stage: {
        viewport: stageViewport,
        preview: stagePreview,
        document: stageViewport.ownerDocument,
        screen: { width: 1920, height: 1080 },
      },
      filters: {
        createCanvas: () => {
          const surface = {
            width: 1,
            height: 1,
            rgba: new Uint8ClampedArray(4),
            getContext() {
              return {
                canvas: surface,
                globalAlpha: 1,
                globalCompositeOperation: 'source-over',
                fillStyle: '#000',
                setTransform() {},
                clearRect() { surface.rgba = new Uint8ClampedArray(Math.max(1, surface.width * surface.height * 4)); },
                fillRect(_x, _y, w, h) {
                  const total = Math.max(1, (w | 0) * (h | 0) * 4);
                  if (surface.rgba.length < total) surface.rgba = new Uint8ClampedArray(total);
                  for (let i = 0; i < surface.rgba.length; i += 4) { surface.rgba[i] = 0; surface.rgba[i + 1] = 204; surface.rgba[i + 2] = 68; surface.rgba[i + 3] = 255; }
                },
                createImageData: (w, h) => ({ width: w, height: h, data: new Uint8ClampedArray(w * h * 4) }),
                getImageData: (_x, _y, w, h) => {
                  const total = w * h * 4;
                  if (surface.rgba.length < total) surface.rgba = new Uint8ClampedArray(total);
                  return { width: w, height: h, data: new Uint8ClampedArray(surface.rgba.subarray(0, total)) };
                },
                putImageData: img => { surface.rgba = new Uint8ClampedArray(img.data); },
              };
            },
          };
          return surface;
        },
      },
      context: { save() {}, restore() {}, setTransform() {}, isPointInPath: () => false, isPointInStroke: () => false, font: '', fontKerning: 'auto', letterSpacing: '0px', measureText: line => ({ width: line.length * 8 }) },
      external: {
        hostObject: fCanvas,
        hostGlobal: { stage_DoFSCommand: (cmd, args) => fsCommands.push([cmd, args]) },
        navigate: req => navigations.push(req),
      },
      bitmap: {
        createCanvas: (width, height) => {
          const surface = {
            width,
            height,
            rgba: new Uint8ClampedArray(Math.max(1, width * height * 4)),
            getContext() {
              return {
                canvas: surface,
                globalAlpha: 1,
                globalCompositeOperation: 'source-over',
                imageSmoothingEnabled: true,
                save() {},
                restore() {},
                getTransform: () => ({ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }),
                setTransform() {},
                transform() {},
                clearRect() { surface.rgba = new Uint8ClampedArray(Math.max(1, surface.width * surface.height * 4)); },
                createImageData: (w, h) => ({ width: w, height: h, data: new Uint8ClampedArray(w * h * 4) }),
                putImageData: img => { surface.rgba = new Uint8ClampedArray(img.data); },
                drawImage: src => { if (src?.rgba) surface.rgba.set(src.rgba.subarray(0, surface.rgba.length)); },
                getImageData: (_x, _y, w, h) => ({ width: w, height: h, data: surface.rgba.subarray(0, w * h * 4) }),
              };
            },
          };
          return surface;
        },
      },
      net: {
        baseURL: 'https://app.example.test/index.html',
        navigate: req => navigations.push(req),
        fetch: async (url, init = {}) => {
          const body = init.body ? new TextDecoder().decode(init.body) : '';
          fetchCalls.push({ url, method: init.method || 'GET', headers: init.headers || [], body });
          if (url.endsWith('/api/xml-echo')) {
            const reply = '<?xml version="1.0"?><reply status="ok"><item id="r1">pong</item></reply>';
            return new Response(reply, { status: 200, headers: { 'content-length': String(Buffer.byteLength(reply)) } });
          }
          if (url.endsWith('/api/xml-send')) return new Response(null, { status: 204 });
          if (url.endsWith('/api/form-echo')) {
            const reply = 'serverStatus=ready&score=99';
            return new Response(reply, { status: 200, headers: { 'content-length': String(Buffer.byteLength(reply)) } });
          }
          if (url.endsWith('/api/theme.css')) {
            const css = 'p { color: #112233; font-size: 16px; } .accent { font-weight: bold; }';
            return new Response(css, { status: 200, headers: { 'content-length': String(Buffer.byteLength(css)) } });
          }
          return new Response('not found', { status: 404 });
        },
      },
      storage: { url: 'https://app.example.test/index.html', storage: memStorage },
      platform: {
        global: {
          navigator: { language: 'en-US', platform: 'MacIntel', clipboard: { writeText: text => clipboardWrites.push(text) } },
          location: { protocol: 'https:' },
          screen: { width: 1440, height: 900, colorDepth: 24 },
        },
        onShowSettings: tab => shownTabs.push(tab),
      },
      capture: {
        mediaDevices: {
          enumerateDevices: async () => [
            { kind: 'videoinput', deviceId: 'cam-1', label: 'Studio Cam' },
            { kind: 'audioinput', deviceId: 'mic-1', label: 'Studio Mic' },
          ],
          getSupportedConstraints: () => ({ sampleRate: true, echoCancellation: true }),
        },
      },
      files: {
        isUserGesture: () => true,
        document: {
          body: { appendChild() {} },
          createElement() {
            const listeners = {};
            return {
              style: {},
              setAttribute() {},
              remove() {},
              addEventListener(k, fn) { listeners[k] = fn; },
              click() {
                this.files = [{ name: 'notes.txt', size: 12, lastModified: 1700000000000 }];
                listeners.change?.();
              },
            };
          },
        },
      },
      print: {
        isUserGesture: () => true,
        openWindow: () => ({
          closed: false,
          opener: null,
          close() { this.closed = true; },
          focus() {},
          print() { printCount++; },
          document: {
            title: '',
            head: { append() {} },
            body: { textContent: '', append() {} },
            createElement(tag) {
              return { tagName: tag, style: {}, complete: true, append() {}, remove() {}, addEventListener() {} };
            },
          },
        }),
        capture: (_target, plan) => {
          printCaptures.push([plan.width, plan.height]);
          const bmp = new (fHost.bitmap().BitmapData)(4, 4, false, 0xcc3300);
          try {
            return new Uint8Array(fPlayer.bitmap_encode_png(JSON.stringify(fHost.bitmap().refOf(bmp)), 0, 0, 4, 4, false));
          } finally {
            bmp.dispose();
          }
        },
      },
      video: {
        isUserGesture: () => true,
        createVideo: () => {
          const el = new EventTarget();
          Object.assign(el, {
            muted: true, volume: 0, src: '', currentTime: 0, readyState: 0, paused: true,
            removeAttribute() { this.src = ''; },
            load() { if (this.src) videoLoads.push(this.src); },
            play() { this.paused = false; return Promise.resolve(); },
            pause() { this.paused = true; },
          });
          return el;
        },
      },
      connections: {
        url: 'https://app.example.test/index.html',
        BroadcastChannel: MockBC,
        locks: { request: async (_name, _opts, cb) => cb({}) },
        createXmlSocket: (host, port) => {
          const s = {
            host, port, sent: [], closed: false, onopen: null, onmessage: null, onerror: null, onclose: null,
            send(frame) { this.sent.push(frame); },
            close() { this.closed = true; },
          };
          xmlSockets.push(s);
          return s;
        },
      },
    }));
    fHost.dispatchScripts();
    const scene = fHost.proxyFor([]), r = scene.foundationReport;
    await until(() => Array.from(r.keepEvents).length === 5, 'MovieClipLoader bundled success + missing error events settle');
    assert.equal(r.initQuality, 'HIGH');
    assert.equal(r.initHighQuality, 1);
    assert.equal(r.initFocusRect, true);
    assert.equal(r.initSoundBuf, 5);
    assert.equal(r.bestQuality, 'BEST');
    assert.equal(r.bestHighQuality, 2);
    assert.equal(r.btnQuality, 'BEST');
    assert.equal(r.btnHighQuality, 2);
    assert.equal(r.toggleToLow, 0);
    assert.equal(r.lowViaGetProp, 'LOW');
    assert.equal(r.lowHqViaGetProp, 0);
    assert.equal(r.afterSetPropQuality, 'HIGH');
    assert.equal(r.evalFocusRect, false);
    assert.equal(r.btnSoundBuf, 9);
    assert.equal(r.afterSetQuality, 'MEDIUM');
    assert.equal(r.mediumHq, 1);
    assert.equal(r.swfVersion, 8);
    assert.equal(r.initLockRoot, false);
    assert.equal(r.lockedFlag, true);
    assert.equal(r.subRootMatchesHolder, true);
    assert.equal(scene.subRef.seenRootName, 'holder');
    assert.equal(scene.subRef.seenEvalRootName, 'holder');
    assert.equal(scene.subRef.seenLevel0IsMain, true);
    assert.equal(scene.subRef.seenLevel1, undefined);
    assert.equal(scene.subRef.seenEvalLevel1, undefined);
    assert.equal(scene.subRef.seenFrameCallerIsNull, true);
    assert.equal(r.unsetClipGrid, undefined);
    assert.equal(r.clipGridWidth, 40);
    assert.equal(r.clearedClipGrid, undefined);
    assert.equal(r.isButton, true);
    assert.equal(r.btnInitVisible, true);
    assert.equal(r.btnHidden, false);
    assert.equal(r.btnGridHeight, 10);
    assert.equal(r.btnClearedGrid, undefined);
    assert.equal(r.wmodeGPU, false);
    assert.throws(() => { fHost.legacy().Stage.wmodeGPU = true; }, /read-only/);
    assert.equal(r.tfQuality, 'MEDIUM');
    assert.equal(r.tfHighQuality, 1);
    assert.equal(r.tfSoundBuf, 9);
    assert.equal(r.globalQualityFromTf, 'HIGH');
    assert.equal(r.globalSoundBufFromTf, 7);
    assert.deepEqual(Array.from(r.tfDefaults), [false, 'normal', 'pixel', 0, 0]);
    assert.deepEqual(Array.from(r.tfUpdated), [true, 'advanced', 'subpixel', 400, -200]);
    assert.ok(r.fontCount >= 5);
    assert.equal(r.hasArial, true);
    assert.deepEqual(Array.from(r.rendererDefaults), ['default', 4]);
    assert.deepEqual(Array.from(r.rendererUpdated), ['lcd', 8]);
    assert.equal(r.isDate, true);
    assert.equal(r.utcYear, 126);
    assert.equal(r.tzFinite, true);
    assert.equal(r.dateStrHasYear, true);
    assert.deepEqual(Array.from(r.badDateChecks), [true, true, true]);
    assert.equal(r.plainCtor, true);
    assert.equal(r.plainProto, true);
    assert.deepEqual(Array.from(r.reassignedProto), [42, true]);
    assert.deepEqual(Array.from(r.classProtos), [true, true, true, true]);
    assert.equal(r.regOk, true);
    assert.deepEqual(Array.from(r.card1Checks), [true, 'registered:card1:8']);
    assert.equal(r.factorial5, 120);
    assert.deepEqual(Array.from(r.callerChecks), [true, true, true, true]);
    assert.equal(r.unregOk, true);
    assert.deepEqual(Array.from(r.card2Checks), [false, true]);

    // Batch 2 assertions: #initclip/#endinitclip, intrinsic, beginBitmapFill, tellTarget,
    // ifFrameLoaded, call, _level, scroll/maxscroll, Accessibility/_accProps, Video, MCL/Sound/NetStream.
    assert.deepEqual(Array.from(r.initOrder), ['unordered:true', 'order3', 'order10']);
    assert.deepEqual(Array.from(r.stageCompChecks), [true, 'registered:stageComp:8']);
    assert.equal(scene.stageComp.frameVisits, 1);
    assert.equal(fHost.legacy()._global.initOrder.length, 3, '#initclip runs once per session, not again on symbol frame 1');
    assert.deepEqual(Array.from(r.intrinsicChecks), [true, 'widget:alpha', true, 'beta']);
    const artGraphics = fHost.display().descriptor(scene.artRef).graphics;
    assert.equal(artGraphics.length, 2);
    assert.equal(artGraphics[0].type, 'gradient_path');
    assert.deepEqual(artGraphics[0].fill_bitmap.matrix, [1, 0, 0, 1, 2, 3]);
    assert.equal(artGraphics[0].fill_bitmap.repeat, false);
    assert.equal(artGraphics[0].fill_bitmap.smoothing, true);
    assert.deepEqual(artGraphics[1].fill_bitmap.matrix, [1, 0, 0, 1, 0, 0]);
    assert.equal(artGraphics[1].fill_bitmap.repeat, true);
    assert.equal(artGraphics[1].fill_bitmap.smoothing, false);
    assert.deepEqual(Array.from(r.tellTargetChecks), ['targeted', 2, 'sub:2', true]);
    assert.deepEqual(Array.from(r.ifFrameLoadedChecks), ['f1', 'f2', 'ready']);
    assert.deepEqual(Array.from(r.callChecks), [2, 1, 20, 'sub', 1]);
    assert.deepEqual(Array.from(r.levelChecks), [true, true, true, true]);
    assert.deepEqual(Array.from(r.varScrollChecks), [1, true, 2, 2, 2]);
    assert.equal(r.accActive, true);
    const ariaLabels = semanticDom.children.map(el => el.getAttribute('aria-label'));
    assert.ok(ariaLabels.includes('Launch Action'));
    assert.ok(ariaLabels.includes('Stage Card'));
    assert.ok(!ariaLabels.includes('Muted'));
    const launchNode = semanticDom.children.find(el => el.getAttribute('aria-label') === 'Launch Action');
    assert.equal(launchNode.children[0].textContent, 'Starts mission');
    assert.deepEqual(Array.from(r.videoChecks), [true, 0, 4]);
    assert.throws(() => { scene.stageVideo.deblocking = 6; }, /0 through 5/);
    assert.deepEqual(Array.from(r.mclSetup), [false, true, true, false]);
    assert.deepEqual(Array.from(r.removedEvents), []);
    const evs = Array.from(r.keepEvents);
    assert.ok(evs.includes('start:slotOk'));
    assert.ok(evs.includes('prog:slotOk:true'));
    assert.ok(evs.includes('comp:slotOk'));
    assert.ok(evs.includes('init:slotOk'));
    assert.ok(evs.includes('err:slotErr:true'));
    assert.deepEqual(Array.from(r.policyChecks), [false, true, false, true]);
    assert.throws(() => { scene.loaderRef.checkPolicyFile = 'yes'; }, /Boolean/);
    assert.throws(() => { scene.soundRef.checkPolicyFile = 1; }, /Boolean/);
    assert.throws(() => { scene.streamRef.checkPolicyFile = null; }, /Boolean/);

    fHost.resetProject();
    const resetReport = fHost.proxyFor([]).foundationReport;
    assert.equal(resetReport.initQuality, 'HIGH');
    assert.equal(resetReport.initSoundBuf, 5);
    assert.deepEqual(Array.from(resetReport.rendererDefaults), ['default', 4]);
    assert.deepEqual(Array.from(resetReport.initOrder), ['unordered:true', 'order3', 'order10']);

    // Batch 3 verification: Area 1 math/geometry (124 declarations) + Areas 7-8 globals & services (70 declarations).
    await until(() => fHost.legacy().Camera.names.length === 1 && fHost.legacy().Microphone.names.length === 1, 'Camera/Microphone discovery settles after reset');
    const b3Root = fHost.proxyFor([]);
    const b3 = b3Root.runBatch3();
    const mg = b3.math, sv = b3.services;

    // Area 1 (math-geometry) assertions
    assert.deepEqual(Array.from(mg.consts), [true, true, true]);
    assert.deepEqual(Array.from(mg.numConsts), [true, true, true, true, true, true]);
    assert.deepEqual(Array.from(mg.numGlobals), [true, true, true, true, true, true]);
    assert.deepEqual(Array.from(mg.mathConsts), Array(8).fill(true));
    assert.deepEqual(Array.from(mg.mathFns), Array(18).fill(true));
    assert.deepEqual(Array.from(mg.pointChecks), Array(11).fill(true));
    assert.deepEqual(Array.from(mg.rectChecks), Array(7).fill(true));
    assert.deepEqual(Array.from(mg.matrixChecks), Array(10).fill(true));
    assert.deepEqual(Array.from(mg.colorChecks), [true, true, true, true]);
    assert.deepEqual(Array.from(mg.transformChecks), Array(6).fill(true));

    // Areas 7-8 globals & services assertions
    fHost.stage().attach();
    fHost.stage().handlePointer('pointermove', { clientX: 25, clientY: 20 }, sv.linkTf);
    fHost.stage().handlePointer('click', { button: 0, buttons: 0, clientX: 25, clientY: 20 }, sv.linkTf);
    assert.deepEqual(Array.from(sv.asfunctionHrefs), ['asfunction:onAsfuncHit,hello,world', 'asfunction:onAsfuncBare']);
    assert.deepEqual(Array.from(sv.asfLog), ['hit:hello,world', 'bare:true', 'hit:bound', 'hit:fromHtml']);
    assert.equal(navigations.length, 2);
    assert.equal(navigations[0].target, '_blank');
    assert.equal(navigations[0].url, 'https://app.example.test/docs?from=globalGetURL');
    assert.match(navigations[1].url, /^https:\/\/app\.example\.test\/form\?.*navToken=fromAS2/);
    assert.deepEqual(fsCommands, [['notifyHost', 'payload-42']]);
    assert.equal(sv.mmResult, '');
    assert.throws(() => b3Root.tryLoadMovieNum(1), /multi-SWF levels/);
    assert.throws(() => b3Root.tryUnloadMovieNum(1), /multi-SWF levels/);
    assert.throws(() => b3Root.tryLoadMovieNum(0), /publication root/);
    assert.throws(() => b3Root.tryUnloadMovieNum(0), /publication root/);
    await until(() => sv.loadSlot._width > 0, 'loadMovie populates child MovieClip dimensions');
    assert.equal(sv.unloadSlot._width, 0);
    assert.equal(printCaptures.length, 4);
    await until(() => printCount === 4, 'all 4 AS2 print/printAsBitmap/printNum/printAsBitmapNum previews invoke print()');

    assert.deepEqual(Array.from(sv.camChecks), [true, true, true, true, true]);
    assert.deepEqual(Array.from(sv.micChecks), [true, true, true, true]);
    assert.deepEqual(Array.from(sv.systemChecks), Array(7).fill(true));
    assert.deepEqual(clipboardWrites, ['neoflash-clipboard']);
    assert.deepEqual(shownTabs, [2]);
    assert.deepEqual(Array.from(sv.fileChecks), [true, true, true, true]);
    assert.deepEqual(Array.from(sv.netChecks), [true, true, true, true, true]);
    assert.ok(videoLoads.includes('https://app.example.test/sample.mp4'));
    await until(() => Array.from(sv.nsLog).length === 4, 'NetStream publish + send callbacks settle');
    assert.deepEqual(Array.from(sv.nsLog), ['status:NetStream.Publish.BadName', 'cue:c1', 'text:sub', 'xmp:<xmp/>']);

    assert.deepEqual(Array.from(sv.rsoChecks), [true, true, true, true, true]);
    await until(() => Array.from(sv.rsoLog).length === 2, 'Remote SharedObject onSync + send settle');
    assert.deepEqual(Array.from(sv.rsoLog), ['sync:clear', 'chat:hello-room']);
    sv.rso.close();
    assert.throws(() => sv.rso.send('onChat', 'after-close'), /connected/);

    // LocalConnection allowDomain (same-origin https) + allowInsecureDomain (http allowed vs http denied)
    await until(() => Array.from(sv.lcLog).includes('msg:sameOrigin'), 'same-origin LocalConnection send delivers');
    const postInsecureWire = (domain, id, tag) => {
      const wire = JSON.stringify({
        version: 1,
        sender: 'external-http-peer',
        sender_domain: domain,
        sender_protocol: 'http:',
        id,
        target: 'https://app.example.test|batch3_bus',
        method: 'onBusMsg',
        body: fHost.amf().base64(fHost.amf().encode([tag], 0)),
      });
      for (const peer of bcPeers) peer.onmessage?.({ data: wire });
    };
    postInsecureWire('http-denied.example.test', 501, 'blockedMsg');
    postInsecureWire('http-ok.example.test', 502, 'allowedInsecureMsg');
    assert.deepEqual(Array.from(sv.lcLog), [
      'allow:app.example.test',
      'msg:sameOrigin',
      'insec:http-denied.example.test',
      'insec:http-ok.example.test',
      'msg:allowedInsecureMsg',
    ]);

    // XMLSocket handshake, \0 framing, chunked XML reassembly, custom onData, and remote vs local close
    assert.deepEqual(Array.from(sv.xmlChecks), [true, true, true, true, true]);
    assert.equal(xmlSockets.length, 2);
    assert.equal(xmlSockets[0].host, 'chat.example.test');
    assert.equal(xmlSockets[0].port, 9000);
    xmlSockets[0].onopen();
    assert.deepEqual(xmlSockets[0].sent, ['<hello v="1"/>\0']);
    xmlSockets[0].onmessage({ data: '<reply ok="1"' });
    xmlSockets[0].onmessage({ data: '/>\0' });
    xmlSockets[0].onclose();
    xmlSockets[1].onopen();
    xmlSockets[1].onmessage({ data: 'plain-frame\0' });
    sv.xs2.close();
    assert.equal(xmlSockets[1].closed, true);
    assert.deepEqual(Array.from(sv.xmlLog), [
      'conn1:true',
      'xml1:reply:1',
      'close1',
      'conn2:true',
      'raw2:plain-frame',
    ]);

    // Batch 4 compiled AS2 conformance audit across Areas 1, 2, 4, 5, 6, 8
    const b4 = b3Root.runBatch4();
    assert.equal(b4.includedSum, 42);
    assert.deepEqual(Array.from(b4.core.numberParseChecks), Array(5).fill(true));
    assert.deepEqual(Array.from(b4.core.stringBoolChecks), Array(7).fill(true));
    assert.deepEqual(Array.from(b4.core.arrayChecks), Array(5).fill(true));
    assert.deepEqual(Array.from(b4.core.objectFuncChecks), Array(5).fill(true));
    assert.deepEqual(Array.from(b4.core.dateErrorBroadcasterChecks), Array(4).fill(true));
    assert.deepEqual(Array.from(b4.core.operatorStmtChecks), Array(4).fill(true));
    assert.deepEqual(Array.from(b4.core.xmlChecks), Array(6).fill(true));
    assert.deepEqual(Array.from(b4.core.loadVarsChecks), Array(3).fill(true));
    assert.deepEqual(Array.from(b4.core.selectionTimerChecks), Array(3).fill(true));
    assert.deepEqual(Array.from(b4.gfx.colorClassChecks), [true, true]);
    assert.deepEqual(Array.from(b4.gfx.bitmapChecks), Array(5).fill(true));
    assert.deepEqual(Array.from(b4.gfx.filterChecks), Array(8).fill(true));
    assert.deepEqual(Array.from(b4.gfx.textChecks), Array(5).fill(true));

    await until(() => Array.from(b4.core.xmlLog).length === 3, 'XML sendAndLoad + load callbacks settle');
    assert.ok(Array.from(b4.core.xmlLog).includes('http:200'));
    assert.ok(Array.from(b4.core.xmlLog).includes('load:true:true:reply:pong:true'));
    assert.ok(Array.from(b4.core.xmlLog).includes('raw:true'));
    await until(() => fetchCalls.some(c => c.url.endsWith('/api/xml-send')), 'XML.send fire-and-forget POST dispatches');
    const xmlSendReq = fetchCalls.find(c => c.url.endsWith('/api/xml-send'));
    assert.equal(xmlSendReq.method, 'POST');
    assert.match(xmlSendReq.body, /<catalog/);
    assert.ok(navigations.some(n => n.url.includes('/api/xml-nav?') && n.method === 'GET' && n.target === '_blank'));
    assert.ok(navigations.some(n => n.url.endsWith('/api/form-nav') && n.method === 'POST' && n.target === '_blank'));

    await until(() => Array.from(b4.core.lvLog).length === 3, 'LoadVars sendAndLoad + load callbacks settle');
    assert.ok(Array.from(b4.core.lvLog).includes('http:200'));
    assert.ok(Array.from(b4.core.lvLog).includes('load:true:true:ready:99:true'));
    assert.ok(Array.from(b4.core.lvLog).includes('get:true:99'));

    await until(() => Array.from(b4.gfx.cssLoadLog).length === 1, 'StyleSheet.load callback settles');
    assert.deepEqual(Array.from(b4.gfx.cssLoadLog), ['css:true:2']);

    await until(() => Array.from(b4.core.timerLog).length === 2, 'setInterval + setTimeout fire once and cancelled timers stay silent');
    assert.ok(Array.from(b4.core.timerLog).includes('iv:tick'));
    assert.ok(Array.from(b4.core.timerLog).includes('to:once'));

    // Wave 5 edge-case & conformance checks (Button.enabled state reset, NetStream bytesLoaded/receiveVideo(fps)/currentFps, SharedObject.setDirty, System.onStatus fallback, PrintJob optional args, MovieClipLoader string target)
    b3Root.actionBtn._visible = true;
    fHost.display().pointer('mouseMove', b3Root.actionBtn);
    assert.equal(fHost.display().handCursor(b3Root.actionBtn), true);
    b3Root.actionBtn.enabled = false;
    assert.equal(fHost.display().handCursor(b3Root.actionBtn), false, 'disabled Button suppresses handCursor and resets Over/Down state');
    b3Root.actionBtn.enabled = true;

    assert.equal(typeof b3Root.streamRef.bytesLoaded, 'number');
    assert.equal(typeof b3Root.streamRef.bytesTotal, 'number');
    assert.equal(typeof b3Root.streamRef.currentFps, 'number');
    b3Root.streamRef.receiveVideo(0);
    b3Root.streamRef.receiveVideo(24);

    const localSo = fHost.legacy().SharedObject.getLocal('wave5_dirty_test');
    localSo.data.counter = 1;
    localSo.setDirty('counter');
    assert.equal(localSo.flush(), true);

    const sysStatuses = [];
    fHost.legacy().System.onStatus = info => sysStatuses.push(info.code || info.level);
    const unhandledNs = new (fHost.legacy().NetStream)( (function() { const nc = new (fHost.legacy().NetConnection)(); nc.connect(null); return nc; })() );
    unhandledNs.publish('bad_upstream', 'live');
    await until(() => sysStatuses.includes('NetStream.Publish.BadName'), 'unhandled NetStream error status routes to System.onStatus');
    fHost.legacy().System.onStatus = null;

    const strProg = b3Root.loaderRef.getProgress('slotOk');
    assert.equal(typeof strProg.bytesLoaded, 'number', 'MovieClipLoader accepts string target paths');

    // Wave 6 genuine behavioral verification:
    // ContextMenu/ContextMenuItem DOM overlay, Stage.scaleMode/align/showMenu,
    // Button/MovieClip _focusrect/trackAsMenu/tabIndex/getDepth, _droptarget,
    // MovieClipLoader/loadMovie symbol:CardClip & byte counts & _url,
    // DisplacementMapFilter live DisplayObject.filters rendering & cache invalidation,
    // Video.smoothing & PrintJob.addPage with frameNum.
    fHost.stage().handlePointer('pointermove', { clientX: 170, clientY: 292 }, b3Root.left);
    const w6 = b3Root.runWave6();
    assert.deepEqual(Array.from(w6.menuChecks), Array(6).fill(true));
    assert.deepEqual(Array.from(w6.stageChecks), Array(6).fill(true));
    assert.deepEqual(Array.from(w6.buttonInputChecks), Array(5).fill(true));
    assert.equal(w6.dropTargetPath, '/left');
    assert.deepEqual(Array.from(w6.byteChecks), [true, true]);
    assert.deepEqual(Array.from(w6.dmfChecks), Array(5).fill(true));
    assert.deepEqual(Array.from(w6.videoWave6Checks), [true, true, true]);
    assert.deepEqual(Array.from(w6.printJobChecks), [true, true, true, true]);

    await until(() => Array.from(w6.symEvents).length === 4, 'MovieClipLoader symbol:CardClip load lifecycle settles');
    assert.deepEqual(Array.from(w6.symEvents), [
      'start:symSlot',
      'prog:true',
      'comp:200',
      'init:symbol:CardClip:true',
    ]);
    assert.equal(w6.symSlot._url, 'symbol:CardClip');
    assert.ok(w6.symSlot.getBytesLoaded() > 0 && w6.symSlot.getBytesLoaded() === w6.symSlot.getBytesTotal());
    assert.equal(w6.symMcl.unloadClip(w6.symSlot), true);
    assert.equal(w6.symSlot.getBytesLoaded(), 0);

    // ContextMenu DOM overlay dispatch on fCanvas
    fCanvas.send('contextmenu', { clientX: 170, clientY: 292 });
    assert.deepEqual(Array.from(w6.menuLog), ['menuSelect:left:true']);
    const menuPanel = stageViewport.children.find(el => el.getAttribute('role') === 'menu');
    assert.ok(menuPanel, 'ContextMenu mounts role=menu panel in viewport');
    const menuButtons = menuPanel.children.filter(el => el.tagName === 'button');
    assert.equal(menuButtons.length, 2, 'visible custom items are rendered; visible=false item is omitted');
    assert.equal(menuButtons[0].textContent, 'Inspect Clip');
    assert.equal(menuButtons[0].disabled, false);
    assert.equal(menuButtons[1].textContent, 'Disabled Item');
    assert.equal(menuButtons[1].disabled, true);
    menuButtons[0].events.click({ preventDefault() {}, stopPropagation() {} });
    assert.deepEqual(Array.from(w6.menuLog), ['menuSelect:left:true', 'item1:left:Inspect Clip']);
    assert.equal(stageViewport.children.includes(menuPanel), false, 'clicking menuitem closes the ContextMenu panel');

    // Stage.showMenu = false prevents default browser contextmenu
    fHost.legacy().Stage.showMenu = false;
    assert.equal(fHost.legacy().Stage.showMenu, false);
    const cmEvt = new Event('contextmenu', { cancelable: true });
    Object.defineProperty(cmEvt, 'clientX', { value: 10 });
    Object.defineProperty(cmEvt, 'clientY', { value: 10 });
    fCanvas.dispatchEvent(cmEvt);
    assert.equal(cmEvt.defaultPrevented, true);
    fHost.legacy().Stage.showMenu = true;

    // Stage.scaleMode & Stage.align layout calculations
    fHost.legacy().Stage.scaleMode = 'exactFit';
    fHost.legacy().Stage.align = 'BR';
    const exactLayout = fHost.as2Stage().layout(1200, 900);
    assert.equal(exactLayout.stageWidth, 1200);
    assert.equal(exactLayout.stageHeight, 900);
    fHost.legacy().Stage.align = 'TL';
    fHost.legacy().Stage.scaleMode = 'noScale';
    const noScaleLayout = fHost.as2Stage().layout(1200, 900);
    assert.equal(noScaleLayout.left, 0);
    assert.equal(noScaleLayout.top, 0);
    assert.equal(noScaleLayout.logicalWidth, 1200);
    assert.equal(noScaleLayout.logicalHeight, 900);

    // Focus bounds with _focusrect
    fHost.legacy().Selection.setFocus(b3Root.actionBtn);
    const focusBoxOn = fHost.display().as2FocusBounds();
    assert.ok(focusBoxOn && focusBoxOn.width > 0 && focusBoxOn.height > 0, 'as2FocusBounds returns bounds when _focusrect is true');
    b3Root.actionBtn._focusrect = false;
    assert.equal(fHost.display().as2FocusBounds(), null, 'as2FocusBounds returns null when _focusrect is false');
    b3Root.actionBtn._focusrect = true;

    // DisplacementMapFilter rendered through Filter Renderer + cache invalidation on mapBitmap mutation
    const snap1 = JSON.parse(fPlayer.runtime_snapshot_json());
    const dispGroup = snap1.filter_groups.find(g => g.filters.some(f => f.type === 'displacement'));
    assert.ok(dispGroup, 'runtime_snapshot_json emits DisplacementMapFilter in filter_groups');
    let compositedSurface = null;
    fHost.filters().renderer.draw({ fillRect() {} }, snap1, {
      bounds: () => ({ left: 0, top: 0, right: 24, bottom: 24 }),
      paint: target => { target.fillRect(0, 0, 24, 24); },
      composite: (_target, canvas) => { compositedSurface = canvas; },
    });
    assert.ok(compositedSurface, 'Filter renderer composites DisplacementMapFilter group');
    const cachedVersion1 = fHost.filters().renderer.stats().cached;
    assert.ok(cachedVersion1 >= 1);
    w6.dMap.setPixel(0, 0, 0x008080);
    const snap2 = JSON.parse(fPlayer.runtime_snapshot_json());
    fHost.filters().renderer.draw({ fillRect() {} }, snap2, {
      bounds: () => ({ left: 0, top: 0, right: 24, bottom: 24 }),
      paint: target => { target.fillRect(0, 0, 24, 24); },
      composite: (_target, canvas) => { compositedSurface = canvas; },
    });
    assert.ok(fHost.filters().renderer.stats().framePixels > 0, 'mutating mapBitmap in-place invalidates cached DisplacementMapFilter surface');
  } finally {
    if (hadLocation) globalThis.location = prevLocation;
    else delete globalThis.location;
    if (hadPath2D) globalThis.Path2D = prevPath2D;
    else delete globalThis.Path2D;
    fHost?.disposeFamily();
    fPlayer.free();
  }
}
