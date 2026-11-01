const test = require('node:test');
const assert = require('node:assert/strict');
const Events = require('../events.js');
const { create, Stage, StageScaleMode, StageAlign, StageDisplayState, StageQuality,
  Keyboard, KeyLocation, KeyboardType, MouseCursor } = require('../stage.js');

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
  assert.throws(() => HostMouse.registerCursor('custom', {}), /flash\.ui\.Mouse\.registerCursor/);
  assert.throws(() => HostMouse.unregisterCursor('custom'), /flash\.ui\.Mouse\.unregisterCursor/);

  controller.reset();
  assert.equal(HostMouse.cursor, 'auto');
  assert.equal(HostKeyboard.capsLock, false);
});
