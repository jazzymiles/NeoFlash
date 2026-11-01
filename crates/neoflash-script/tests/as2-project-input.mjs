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
  focus() { this.ownerDocument.activeElement = this; this.events.focus?.(); }
  scrollIntoView() {}
  get firstElementChild() { return this.children[0] || null; }
  get nextElementSibling() {
    if (!this.parentNode) return null;
    return this.parentNode.children[this.parentNode.children.indexOf(this) + 1] || null;
  }
}

function createSemanticDom() {
  const doc = { activeElement: null, createElement(tag) { return new DomElement(doc, tag); } };
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
  project.resources = [pngResource];
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
      source: 'rootCalled = (rootCalled ? rootCalled : 0) + 1;',
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
      id: 'as2-source-foundation-main',
      path: 'app/FoundationMain.as',
      language: 'action_script2',
      source: [
        'import app.RegisteredCard;',
        'import app.Inspector;',
        'import app.SubInspector;',
        'import ext.HostWidget;',
        'import flash.display.BitmapData;',
        'import flash.geom.Matrix;',
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
  let fHost;
  try {
    ({ host: fHost } = createRuntimeHost(root, fPlayer, Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source }, code: item.code, location: item.location,
    })), { bundle, semanticLayer, context: { measureText: line => ({ width: line.length * 8 }) } }));
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
  } finally {
    fHost?.disposeFamily();
    fPlayer.free();
  }
}
