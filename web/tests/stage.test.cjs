const test = require('node:test');
const assert = require('node:assert/strict');
const Events = require('../events.js');
const { create, computeStageLayout, Stage, StageScaleMode, StageAlign, StageDisplayState, StageQuality,
  Keyboard, KeyLocation, KeyboardType, MouseCursor, MouseCursorData } = require('../stage.js');

// A real EventTarget drives the adapter's installed native listeners; no direct callback invocation.
class Surface extends EventTarget {
  focus() { this.focusCalls = (this.focusCalls || 0) + 1; }
  send(type, data = {}) {
    const event = new globalThis.Event(type, { cancelable: true });
    for (const [key, value] of Object.entries(data)) Object.defineProperty(event, key, { value });
    this.dispatchEvent(event); return event;
  }
}
function setup(extra = {}) {
  const canvas = new Surface(), keyboard = new Surface(), resizeTarget = new Surface();
  const root = { name: 'root' }, child = { name: 'child', parent: root };
  const live = new Set([root, child]);
  let controller, errors = [], input = [], rate = 24, after = 0;
  const alive = node => node === controller?.stage || live.has(node);
  const hub = Events.create({ parentOf: node => node === root ? controller.stage : node.parent,
    alive, globalPoint: (_target, x, y) => [x + 100, y + 50] });
  Object.assign(root, hub.methods(root)); Object.assign(child, hub.methods(child));
  controller = create({ events: hub, width: 800, height: 600, frameRate: 24, canvas,
    keyboardTarget: keyboard, resizeTarget, alive, hitTest: event => event.hit || child,
    coordinates: event => Number.isFinite(event.clientX) ? [event.clientX / 2, event.clientY / 2] : null,
    localPoint: (_node, x, y) => [x - 100, y - 50],
    getFrameRate: () => rate, setFrameRate: value => { rate = value; },
    onError: error => errors.push(error), onLegacyInput: (...args) => input.push(args),
    afterInput: () => after++, ...extra });
  return { controller, stage: controller.stage, hub, root, child, live, canvas, keyboard, resizeTarget, errors, input, after: () => after };
}

test('Stage singleton exposes logical dimensions, transient frame rate and validated focus', () => {
  const { stage, child, live, canvas } = setup();
  assert.equal(stage instanceof Stage, true);
  assert.equal(Events.EventDispatcher.__nfIs(stage), true);
  assert.equal(Stage.__nfIs(stage), true);
  assert.throws(() => new Stage(), /created by the player/);
  assert.deepEqual([stage.stageWidth, stage.stageHeight], [800, 600]);
  assert.equal(Reflect.set(stage, 'stageWidth', 1), false);
  assert.equal(Reflect.set(stage, 'allowsFullScreen', true), false);
  assert.equal(Reflect.set(stage, 'wmodeGPU', true), false);
  stage.frameRate = 30;
  assert.equal(stage.frameRate, 30);
  assert.throws(() => { stage.frameRate = Infinity; }, /frameRate/);
  assert.equal(stage.frameRate, 30);
  stage.focus = child;
  assert.equal(stage.focus, child); assert.equal(canvas.focusCalls, 1);
  live.delete(child); assert.equal(stage.focus, null);
  assert.throws(() => { stage.focus = child; }, /active display/);
});

test('installed pointer listeners use stage/local coordinates, focus, bubbling, default prevention and legacy bridge once', () => {
  const { controller, stage, root, child, canvas, input } = setup();
  const seen = [];
  stage.addEventListener(Events.MouseEvent.MOUSE_DOWN, event => seen.push(['capture', event.eventPhase]), true);
  child.addEventListener(Events.MouseEvent.MOUSE_DOWN, event => {
    seen.push(['target', event.localX, event.localY, event.stageX, event.stageY, event.buttonDown]); event.preventDefault();
  });
  root.addEventListener(Events.MouseEvent.MOUSE_DOWN, event => seen.push(['bubble', event.target.name]));
  controller.attach(); controller.attach();
  const event = canvas.send('pointerdown', { clientX: 240, clientY: 160, buttons: 1, button: 0 });
  assert.deepEqual(seen, [['capture', 1], ['target', 20, 30, 120, 80, true], ['bubble', 'child']]);
  assert.equal(event.defaultPrevented, true);
  assert.deepEqual([stage.mouseX, stage.mouseY, stage.focus], [120, 80, child]);
  assert.equal(input.length, 1); assert.equal(input[0][0], 'mouseDown');
  canvas.send('click', { clientX: 240, clientY: 160 });
  assert.equal(input.length, 1, 'runtime retains sole ownership of authored Click dispatch');
  const clicks = [];
  child.addEventListener('click', event => clicks.push(event.target));
  controller.handlePointer('click', { isPrimary: false, pointerType: 'mouse', button: 0 }, child);
  assert.deepEqual(clicks, [child], 'click does not inherit the pointer tracking primary-contact filter');
  controller.handlePointer('pointerdown', { isPrimary: false, pointerType: 'touch' }, child);
  assert.equal(input.length, 2, 'secondary pointer tracking remains ignored');
  const native = setup({onFocusChanged:()=>true});native.controller.attach();
  assert.equal(native.canvas.send('pointerdown',{button:0}).defaultPrevented,true,'Canvas focus default must not steal focus from a newly opened native editor');
  assert.equal(native.canvas.focusCalls,undefined);
  const editor = new Surface();editor.addEventListener('pointerdown',event=>native.controller.handlePointer('pointerdown',event,native.child));
  assert.equal(editor.send('pointerdown',{button:0}).defaultPrevented,false,'native editor retains browser caret and selection behavior');
  native.controller.dispose();

});

test('keyboard routes to current focus, bubbles to Stage, preserves modifiers and supports native key-code fallback', () => {
  const { controller, stage, root, child, keyboard } = setup(), seen = [];
  stage.focus = child;
  child.addEventListener('keyDown', event => {
    seen.push([event.target.name, event.charCode, event.keyCode, event.shiftKey, event.commandKey]); event.preventDefault();
  });
  stage.addEventListener('keyDown', event => seen.push(['stage', event.currentTarget === stage]));
  controller.attach();
  assert.equal(keyboard.send('keydown', { key: 'A', code: 'KeyA', shiftKey: true, metaKey: true }).defaultPrevented, true);
  assert.deepEqual(seen, [['child', 65, 65, true, true], ['stage', true]]);
  stage.focus = root;
  keyboard.send('keydown', { key: 'ArrowLeft', code: 'ArrowLeft' });
  assert.deepEqual(seen.at(-1), ['stage', true]);
  const count = seen.length;
  keyboard.send('keydown', { isComposing: true, key: 'x' });
  assert.equal(seen.length, count, 'IME composition is not fabricated as Flash keyboard input');
});

test('logical resize only emits on changes; tick broadcasts without bubbling; reset clears focus and Stage handlers', () => {
  let dimensions = [800, 600];
  const { controller, stage, root, child, resizeTarget } = setup({ getSize: () => dimensions }), seen = [];
  stage.addEventListener('resize', event => seen.push(['resize', event.target.stageWidth, event.target.stageHeight]));
  stage.addEventListener('enterFrame', () => seen.push('stage'));
  root.addEventListener('enterFrame', () => seen.push('root'));
  child.addEventListener('enterFrame', () => seen.push('child'));
  controller.attach();
  resizeTarget.send('resize'); assert.equal(seen.length, 0);
  dimensions = [900, 650]; resizeTarget.send('resize');
  controller.tick([root, child]);
  assert.deepEqual(seen, [['resize', 900, 650], 'stage', 'root', 'child']);
  stage.focus = child; stage.frameRate = 60; controller.reset();
  assert.equal(stage.focus, null); assert.equal(stage.hasEventListener('enterFrame'), false);
  assert.deepEqual([stage.stageWidth, stage.stageHeight, stage.frameRate], [800, 600, 24]);
});

test('input errors preserve thrown value and dispose removes actual installed DOM listeners', () => {
  const { controller, child, canvas, errors, input } = setup();
  const token = {};
  child.addEventListener('mouseUp', () => { throw token; });
  controller.attach(); canvas.send('pointerup', { clientX: 20, clientY: 30 });
  assert.equal(errors[0], token); assert.equal(input.length, 0);
  controller.dispose();
  canvas.send('pointerup'); assert.equal(errors.length, 1);
  controller.attach(); canvas.send('pointerup'); assert.equal(errors.length, 2);
});

test('semantic clicks accept an explicit live target; replacement focus falls back to Stage', () => {
  const { controller, stage, child, root, live } = setup(), seen = [];
  root.addEventListener('click', event => seen.push(event.target));
  controller.handlePointer('click', {}, root);
  assert.deepEqual(seen, [root]);
  stage.focus = child; live.delete(child);
  stage.addEventListener('keyUp', event => seen.push(event.target));
  controller.handleKey('keyup', { key: 'Escape', code: 'Escape' });
  assert.equal(seen.at(-1), stage);
});

test('hover callback removal does not dispatch to a dead lifetime; editor text input is not hijacked', () => {
  const { controller, stage, child, live, input } = setup();
  child.addEventListener('mouseOver', () => live.delete(child));
  assert.equal(controller.handlePointer('pointermove', { clientX: 30, clientY: 40 }), true);
  assert.equal(input.length, 0);
  let keys = 0;
  stage.addEventListener('keyDown', () => keys++);
  controller.handleKey('keydown', { key: 'a', target: { tagName: 'INPUT' } });
  controller.handleKey('keydown', { key: 'a', target: { isContentEditable: true } });
  assert.equal(keys, 0);
});

test('actual runtime proxies share Stage ancestry, event facades, keyboard routing and replacement lifetimes', async () => {
  const { resolve } = require('node:path');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  let childLifetime = '2';
  const nodes = () => [
    { path: [], name: 'Scene', lifetime_id: '1', owner: 'movie_clip', world: [1, 0, 0, 1, 0, 0], class_name: null },
    { path: ['card'], name: 'card', lifetime_id: childLifetime, owner: 'movie_clip', world: [1, 0, 0, 1, 100, 50], class_name: null },
  ];
  const player = {
    lifetime_id: key => key === '[]' ? '1' : childLifetime,
    display_nodes_json: () => JSON.stringify(nodes()),
    child_path: (_key, name) => JSON.stringify(name === 'card' ? ['card'] : null),
    scripts_json: () => '[]', script_overflowed: () => false,
  };
  const { host } = createRuntimeHost(resolve(__dirname, '../..'), player, [], { project: { width: 800, height: 600, fps: 24 } });
  const root = host.proxyFor([]), card = root.card, stage = root.stage, seen = [];
  assert.equal(root.parent, stage); assert.equal(card.stage, stage);
  assert.equal(Events.EventDispatcher.__nfIs(card), true);
  assert.equal(host.program().type('flash.events.Event'), Events.Event);
  stage.addEventListener('click', event => seen.push(['stage', event.eventPhase]), true);
  card.addEventListener('click', event => seen.push(['card', event.currentTarget === card]));
  root.addEventListener('click', event => seen.push(['root', event.target === card]));
  host.stage().handlePointer('click', {}, card);
  assert.deepEqual(seen, [['stage', 1], ['card', true], ['root', true]]);
  stage.focus = card;
  card.addEventListener('keyDown', event => seen.push(['key', event.keyCode]));
  host.stage().handleKey('keydown', { key: 'ArrowRight', code: 'ArrowRight' });
  assert.deepEqual(seen.at(-1), ['key', 39]);
  childLifetime = '3'; host.attachNewInstances();
  assert.equal(stage.focus, null);
  const replacement = root.card;
  assert.notEqual(replacement, card);
  assert.equal(replacement.hasEventListener('click'), false);
  assert.throws(() => card.dispatchEvent(new Events.Event('click')), /removed or replaced/);
});

test('Stage scaleMode, align, quality, displayState, color, metrics, flags and invalidate/render contract', () => {
  let requestedFs = null, exitedFs = 0, invalidatedCount = 0;
  const { controller, stage, root, child, canvas } = setup({
    color: 0x336699,
    screen: { width: 1920, height: 1080 },
    devicePixelRatio: 2,
    numChildren: () => 3,
    allowsFullScreen: false,
    onInvalidate: () => invalidatedCount++,
  });
  assert.equal(stage.scaleMode, StageScaleMode.SHOW_ALL);
  stage.scaleMode = StageScaleMode.NO_SCALE;
  assert.equal(stage.scaleMode, 'noScale');
  assert.throws(() => { stage.scaleMode = 'bogus'; }, err => err.name === 'ArgumentError');

  assert.equal(stage.align, '');
  stage.align = 'lt';
  assert.equal(stage.align, StageAlign.TOP_LEFT);
  stage.align = StageAlign.BOTTOM_RIGHT;
  assert.equal(stage.align, 'BR');
  stage.align = '';
  assert.equal(stage.align, '');
  assert.throws(() => { stage.align = 'CENTER'; }, err => err.name === 'ArgumentError');

  assert.equal(stage.quality, 'HIGH');
  stage.quality = StageQuality.HIGH_8X8_LINEAR;
  assert.equal(stage.quality, '8X8LINEAR');
  stage.quality = StageQuality.LOW;
  assert.equal(stage.quality, 'LOW');
  assert.throws(() => { stage.quality = 'ultra'; }, err => err.name === 'ArgumentError');

  assert.equal(stage.displayState, StageDisplayState.NORMAL);
  assert.equal(stage.allowsFullScreen, false);
  assert.equal(stage.allowsFullScreenInteractive, false);
  assert.throws(() => { stage.displayState = StageDisplayState.FULL_SCREEN; }, err => err.name === 'SecurityError');
  const fsHost = setup({
    allowsFullScreen: true,
    allowsFullScreenInteractive: true,
    requestFullscreen: state => { requestedFs = state; return true; },
    exitFullscreen: () => { exitedFs++; },
  });
  fsHost.stage.displayState = StageDisplayState.FULL_SCREEN_INTERACTIVE;
  assert.equal(requestedFs, 'fullScreenInteractive');
  assert.equal(fsHost.stage.displayState, 'fullScreenInteractive');
  fsHost.stage.displayState = StageDisplayState.NORMAL;
  assert.equal(exitedFs, 1);
  assert.equal(fsHost.stage.displayState, 'normal');

  assert.equal(stage.color, 0x336699);
  stage.color = 0xff112233;
  assert.equal(stage.color, 0x112233);
  assert.equal(stage.fullScreenWidth, 1920);
  assert.equal(stage.fullScreenHeight, 1080);
  assert.equal(stage.contentsScaleFactor, 2);
  assert.equal(stage.numChildren, 3);
  assert.equal(stage.tabChildren, true);
  stage.tabChildren = false;
  assert.equal(stage.tabChildren, false);
  assert.equal(stage.stageFocusRect, true);
  stage.stageFocusRect = false;
  assert.equal(stage.stageFocusRect, false);

  controller.attach();
  assert.equal(canvas.send('contextmenu').defaultPrevented, false);
  stage.showDefaultContextMenu = false;
  assert.equal(canvas.send('contextmenu').defaultPrevented, true);

  const downTargets = [];
  stage.addEventListener(Events.MouseEvent.MOUSE_DOWN, event => downTargets.push(event.target === stage ? 'stage' : event.target.name));
  stage.mouseChildren = false;
  canvas.send('pointerdown', { clientX: 40, clientY: 40, button: 0, buttons: 1 });
  assert.deepEqual(downTargets, ['stage']);

  const renders = [];
  stage.addEventListener(Events.Event.RENDER, () => {
    renders.push('stage');
    if (renders.length === 1) stage.invalidate();
  });
  child.addEventListener(Events.Event.RENDER, () => renders.push('child'));
  stage.invalidate();
  assert.equal(invalidatedCount, 1);
  assert.deepEqual(renders, [], 'Event.RENDER waits for tick/render flush rather than firing synchronously');
  controller.tick([root, child]);
  assert.deepEqual(renders, ['stage', 'child'], 're-invalidating inside RENDER schedules next render without recursion');
  controller.flushRender([root, child]);
  assert.deepEqual(renders, ['stage', 'child', 'stage', 'child']);
  controller.flushRender([root, child]);
  assert.equal(renders.length, 4);

  controller.reset();
  assert.equal(stage.scaleMode, 'showAll');
  assert.equal(stage.align, '');
  assert.equal(stage.quality, 'HIGH');
  assert.equal(stage.color, 0x336699);
  assert.equal(stage.mouseChildren, true);
  assert.equal(stage.tabChildren, true);
  assert.equal(stage.showDefaultContextMenu, true);
  assert.equal(stage.stageFocusRect, true);
});

test('flash.ui Keyboard, KeyLocation, KeyboardType, Mouse and MouseCursor contracts', () => {
  let mouseUpdates = 0;
  const { controller, keyboard } = setup({ onMouseChanged: () => mouseUpdates++ });
  const { Keyboard: HostKeyboard, Mouse: HostMouse } = controller;
  for (const EnumType of [StageScaleMode, StageAlign, StageDisplayState, StageQuality, Keyboard, HostKeyboard, KeyLocation, KeyboardType, HostMouse, MouseCursor]) {
    assert.throws(() => new EnumType(), TypeError);
  }
  assert.equal(HostKeyboard.A, 65);
  assert.equal(HostKeyboard.RED, 0x01000000);
  assert.equal(HostKeyboard.PLAY_PAUSE, 0x01000020);
  assert.equal(HostKeyboard.KEYNAME_UPARROW, 'Up');
  assert.equal(HostKeyboard.STRING_UPARROW, '\uf700');
  assert.equal(HostKeyboard.STRING_MODESWITCH, '\uf747');
  assert.equal(HostKeyboard.CharCodeStrings.length, 72);
  assert.equal(HostKeyboard.CharCodeStrings[0], 'Up');
  assert.equal(HostKeyboard.CharCodeStrings[71], 'ModeSw');
  assert.equal(HostKeyboard.capsLock, false);
  assert.equal(HostKeyboard.numLock, false);
  assert.equal(HostKeyboard.hasVirtualKeyboard, false);
  assert.equal(HostKeyboard.physicalKeyboardType, KeyboardType.ALPHANUMERIC);
  assert.equal(HostKeyboard.isAccessible(), false);
  assert.deepEqual([KeyLocation.STANDARD, KeyLocation.LEFT, KeyLocation.RIGHT, KeyLocation.NUM_PAD, KeyLocation.D_PAD], [0, 1, 2, 3, 4]);

  controller.attach();
  keyboard.send('keydown', { key: 'A', code: 'KeyA', getModifierState: name => name === 'CapsLock' });
  assert.equal(HostKeyboard.capsLock, true);
  assert.equal(HostKeyboard.numLock, false);

  assert.equal(HostMouse.cursor, MouseCursor.AUTO);
  assert.equal(HostMouse.supportsCursor, true);
  assert.equal(HostMouse.supportsNativeCursor, false);
  HostMouse.cursor = MouseCursor.HAND;
  assert.equal(HostMouse.cursor, 'hand');
  assert.deepEqual(controller.mouseState(), { cursor: 'hand', visible: true });
  HostMouse.hide();
  assert.deepEqual(controller.mouseState(), { cursor: 'hand', visible: false });
  HostMouse.show();
  assert.deepEqual(controller.mouseState(), { cursor: 'hand', visible: true });
  assert.equal(mouseUpdates, 3);
  assert.throws(() => { HostMouse.cursor = 'crosshair'; }, err => err.name === 'ArgumentError');
  assert.throws(() => HostMouse.registerCursor('custom', {}), TypeError);
  assert.throws(() => HostMouse.unregisterCursor(null), TypeError);
  const cursorData = new MouseCursorData();
  assert.equal(cursorData instanceof MouseCursorData, true);
  assert.equal(cursorData.data, null);
  assert.deepEqual([cursorData.hotSpot.x, cursorData.hotSpot.y], [0, 0]);
  assert.equal(cursorData.frameRate, 0);
  const fakeBmp = { width: 16, height: 16 };
  const NeoFlashStage = require('../stage.js');
  NeoFlashStage.setStageAdapters({ isBitmapData: v => Boolean(v && typeof v.width === 'number'), isPoint: v => Boolean(v && Number.isFinite(v.x) && Number.isFinite(v.y)), isRectangle: v => Boolean(v && Number.isFinite(v.width) && Number.isFinite(v.height)) });
  cursorData.data = [fakeBmp];
  cursorData.hotSpot = { x: 4, y: 5 };
  cursorData.frameRate = 12;
  assert.equal(cursorData.data.length, 1);
  assert.deepEqual([cursorData.hotSpot.x, cursorData.hotSpot.y], [4, 5]);
  assert.equal(cursorData.frameRate, 12);
  HostMouse.registerCursor('custom', cursorData);
  HostMouse.cursor = 'custom';
  assert.equal(HostMouse.cursor, 'custom');
  HostMouse.unregisterCursor('custom');
  assert.equal(HostMouse.cursor, 'auto');

  const { stage, root, child } = setup({ allowsFullScreen: true, allowsFullScreenInteractive: true, getStageContentSize: () => ({ width: 320, height: 180 }) });
  assert.equal(stage.width, 320);
  assert.equal(stage.height, 180);
  assert.throws(() => { stage.width = 400; }, err => err.name === 'IllegalOperationError' && err.errorID === 2071);
  assert.throws(() => { stage.height = 300; }, err => err.name === 'IllegalOperationError' && err.errorID === 2071);
  assert.throws(() => stage.textSnapshot, err => err.name === 'IllegalOperationError' && err.errorID === 2071);
  assert.equal(stage.wmodeGPU, false);
  assert.equal(stage.browserZoomFactor, 1);
  assert.equal(stage.colorCorrection, 'default');
  stage.colorCorrection = 'on';
  assert.equal(stage.colorCorrection, 'on');
  assert.equal(stage.colorCorrectionSupport, 'unsupported');
  assert.equal(stage.isFocusInaccessible(), false);
  stage.fullScreenSourceRect = { x: 10, y: 20, width: 100, height: 80 };
  assert.deepEqual([stage.fullScreenSourceRect.x, stage.fullScreenSourceRect.y, stage.fullScreenSourceRect.width, stage.fullScreenSourceRect.height], [10, 20, 100, 80]);
  stage.fullScreenSourceRect = null;
  assert.equal(stage.fullScreenSourceRect, null);
  assert.throws(() => { stage.mouseLock = true; }, err => err.name === 'IllegalOperationError');
  const fsEvents = [], fsAccepted = [], leaveEvents = [], resizeEvents = [], focusDirections = [], frameOrder = [];
  stage.addEventListener('fullScreen', e => fsEvents.push([e.fullScreen, e.interactive, e.clone().fullScreen, e.toString().includes('FullScreenEvent')]));
  stage.addEventListener('fullScreenInteractiveAccepted', e => fsAccepted.push([e.fullScreen, e.interactive]));
  stage.addEventListener(Events.Event.MOUSE_LEAVE, () => leaveEvents.push('leave'));
  stage.addEventListener(Events.Event.RESIZE, () => resizeEvents.push([stage.stageWidth, stage.stageHeight]));
  child.addEventListener(Events.FocusEvent.FOCUS_IN, e => focusDirections.push(e.direction));
  root.addEventListener(Events.Event.ENTER_FRAME, () => frameOrder.push('enterFrame'));
  root.addEventListener(Events.Event.FRAME_CONSTRUCTED, () => frameOrder.push('frameConstructed'));
  root.addEventListener(Events.Event.EXIT_FRAME, () => frameOrder.push('exitFrame'));
  stage.fullScreenSourceRect = { x: 10, y: 20, width: 640, height: 360 };
  stage.displayState = StageDisplayState.FULL_SCREEN_INTERACTIVE;
  assert.deepEqual([stage.stageWidth, stage.stageHeight], [640, 360]);
  stage.mouseLock = true;
  assert.equal(stage.mouseLock, true);
  stage.displayState = StageDisplayState.NORMAL;
  stage.fullScreenSourceRect = null;
  assert.equal(stage.mouseLock, false);
  assert.deepEqual(fsEvents, [[true, true, true, true], [false, true, false, true]]);
  assert.deepEqual(fsAccepted, [[true, true]]);
  stage.assignFocus(child, 'bottom');
  assert.equal(stage.focus, child);
  assert.deepEqual(focusDirections, ['bottom']);

  const extraA = { name: 'extraA' }, extraB = { name: 'extraB' };
  stage.addChild(extraA);
  stage.addChildAt(extraB, 0);
  assert.equal(stage.getChildAt(0), extraB);
  assert.equal(stage.getChildByName('extraA'), extraA);
  assert.equal(stage.getChildIndex(extraA), 1);
  assert.equal(stage.contains(extraA), true);
  stage.swapChildren(extraA, extraB);
  assert.equal(stage.getChildAt(0), extraA);
  stage.swapChildrenAt(0, 1);
  assert.equal(stage.getChildAt(0), extraB);
  stage.setChildIndex(extraA, 0);
  assert.equal(stage.getChildAt(0), extraA);
  assert.equal(stage.removeChildAt(0), extraA);
  assert.equal(stage.removeChild(extraB), extraB);

  controller.reset();
  assert.equal(HostMouse.cursor, 'auto');
  assert.equal(HostKeyboard.capsLock, false);
});

test('plain interactive clicks never draw focus-rect chrome while keyboard focus and explicit focusRect still do (T-214)', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const { resolve } = require('node:path');
  const root = resolve(__dirname, '../..');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
  vm.runInContext(fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web.js'), 'utf8'), context);
  context.wasmBytes = fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web_bg.wasm'));
  const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', context);
  const project = JSON.parse(fs.readFileSync(resolve(root, 'examples/display-family.nflash'), 'utf8'));
  project.sources = [];
  delete project.document_class;
  for (const sym of project.symbols) {
    delete sym.class_name;
    sym.timeline.frame_scripts = [];
    for (const layer of sym.timeline.layers) {
      for (const kf of layer.keyframes) {
        delete kf.script;
        delete kf.actions;
        kf.scripts = [];
        for (const el of kf.elements) el.scripts = [];
      }
    }
  }
  const player = new wasm.WebPlayer(JSON.stringify(project));
  player.scripts_json();
  player.runtime_scripts_json?.();
  const { host } = createRuntimeHost(root, player, [], { project });
  const rootClip = host.proxyFor([]), btn = rootClip.widget, stage = rootClip.stage;

  // 1. Mouse pointerdown focuses btn for events, but focusReason is 'mouse' -> NO #ffcc00 focus box!
  host.stage().handlePointer('pointerdown', { clientX: 50, clientY: 60, button: 0, buttons: 1 }, btn);
  assert.equal(stage.focus, btn);
  assert.equal(host.stage().focusReason(), 'mouse');
  assert.equal(host.display().as2FocusBounds(), null, 'mouse pointer click on plain interactive clip must never draw focus rectangle');

  // 2. Default AS3 programmatic stage.focus = btn without focusRect=true -> NO #ffcc00 focus box!
  stage.focus = null;
  stage.focus = btn;
  assert.equal(host.stage().focusReason(), 'programmatic');
  assert.equal(host.display().as2FocusBounds(), null, 'plain programmatic focus without focusRect opt-in does not draw focus rectangle');

  // 3. Keyboard Tab focus ('key') DOES draw focus rectangle when stageFocusRect is true
  host.stage().changeFocus(btn, 'key', {});
  assert.equal(host.stage().focusReason(), 'key');
  const keyBounds = host.display().as2FocusBounds();
  assert.ok(keyBounds && keyBounds.width > 0 && keyBounds.height > 0, 'keyboard focus shows focus bounds');

  // 4. Subsequent mouse click on the same object clears keyboard focus chrome immediately
  host.stage().handlePointer('pointerdown', { clientX: 50, clientY: 60, button: 0, buttons: 1 }, btn);
  assert.equal(host.stage().focusReason(), 'mouse');
  assert.equal(host.display().as2FocusBounds(), null, 'mouse click on already-focused object hides keyboard focus ring');

  // 5. Explicit focusRect = true with programmatic focus shows focus bounds
  btn.focusRect = true;
  stage.focus = null;
  stage.focus = btn;
  const explicitBounds = host.display().as2FocusBounds();
  assert.ok(explicitBounds && explicitBounds.width > 0 && explicitBounds.height > 0, 'explicit focusRect=true shows focus bounds on programmatic focus');
});

test('computeStageLayout matches Rust ScriptAS2Stage across all 4 scaleModes, all 9 Stage.align anchors, and fullScreenSourceRect (T-245)', async () => {
  const fs = require('node:fs');
  const vm = require('node:vm');
  const { resolve } = require('node:path');
  const root = resolve(__dirname, '../..');
  const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
  vm.runInContext(fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web.js'), 'utf8'), context);
  context.wasmBytes = fs.readFileSync(resolve(root, 'target/web-runtime/neoflash_web_bg.wasm'));
  const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', context);

  const modes = ['showAll', 'noScale', 'exactFit', 'noBorder'];
  const aligns = ['', 'T', 'B', 'L', 'R', 'TL', 'TR', 'BL', 'BR'];
  const core = new wasm.ScriptAS2Stage(800, 600);
  try {
    for (const mode of modes) {
      core.command(JSON.stringify({ op: 'set', name: 'scaleMode', value: mode }));
      for (const align of aligns) {
        core.command(JSON.stringify({ op: 'set', name: 'align', value: align }));
        const rust = JSON.parse(core.command(JSON.stringify({ op: 'layout', width: 1280, height: 900, fullscreen: false })));
        const js = computeStageLayout(800, 600, 1280, 900, { scaleMode: mode, align, fullscreen: false });
        for (const key of ['scale', 'scaleY', 'stageWidth', 'stageHeight', 'left', 'top', 'originX', 'originY', 'spaceWidth', 'spaceHeight', 'logicalWidth', 'logicalHeight']) {
          assert.ok(Math.abs(js[key] - rust[key]) < 1e-9, `mode=${mode} align='${align}' key=${key}: js=${js[key]} rust=${rust[key]}`);
        }
      }
    }

    // Verify noScale originX/originY explicitly for Center, TopLeft, and BottomRight
    const centerNoScale = computeStageLayout(800, 600, 1200, 800, { scaleMode: 'noScale', align: '' });
    assert.deepEqual([centerNoScale.originX, centerNoScale.originY, centerNoScale.logicalWidth, centerNoScale.logicalHeight], [200, 100, 1200, 800]);
    const tlNoScale = computeStageLayout(800, 600, 1200, 800, { scaleMode: 'noScale', align: 'TL' });
    assert.deepEqual([tlNoScale.originX, tlNoScale.originY], [0, 0]);
    const brNoScale = computeStageLayout(800, 600, 1200, 800, { scaleMode: 'noScale', align: 'BR' });
    assert.deepEqual([brNoScale.originX, brNoScale.originY], [400, 200]);

    // Verify fullScreenSourceRect parity when fullscreen=true
    const rect = { x: 100, y: 50, width: 400, height: 300 };
    core.command(JSON.stringify({ op: 'set', name: 'fullScreenSourceRect', value: rect }));
    const rustFs = JSON.parse(core.command(JSON.stringify({ op: 'layout', width: 1600, height: 900, fullscreen: true })));
    const jsFs = computeStageLayout(800, 600, 1600, 900, { scaleMode: 'showAll', align: 'TL', fullscreen: true, fullScreenSourceRect: rect });
    for (const key of ['scale', 'scaleY', 'stageWidth', 'stageHeight', 'left', 'top', 'originX', 'originY', 'logicalWidth', 'logicalHeight']) {
      assert.ok(Math.abs(jsFs[key] - rustFs[key]) < 1e-9, `fullscreen sourceRect key=${key}: js=${jsFs[key]} rust=${rustFs[key]}`);
    }
  } finally {
    core.free();
  }
});
