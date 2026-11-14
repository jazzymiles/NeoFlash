const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const runtimeDir = path.resolve(__dirname, '../../target/web-runtime');
const webDir = path.resolve(__dirname, '..');

function loadWasmAndModules(extraGlobals = {}) {
  const btoaFn = (str) => Buffer.from(str, 'binary').toString('base64');
  const atobFn = (b64) => Buffer.from(b64, 'base64').toString('binary');
  const context = vm.createContext({
    TextEncoder,
    TextDecoder,
    WebAssembly,
    console,
    URL,
    Request,
    Response,
    Uint8Array,
    Float32Array,
    DataView,
    ArrayBuffer,
    Map,
    Set,
    WeakMap,
    WeakSet,
    Promise,
    Math,
    Date,
    JSON,
    btoa: btoaFn,
    atob: atobFn,
    queueMicrotask,
    setTimeout,
    clearTimeout,
    ...extraGlobals,
  });
  context.globalThis = context;
  vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'), 'utf8'), context);
  const wasm = vm.runInContext('NeoFlashWasm', context);
  context.NeoFlashWasm = wasm;
  context.wasmBytes = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm'));
  const ready = vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', context);
  for (const file of [
    'events.js',
    'geom.js',
    'byte-array.js',
    'net.js',
    'amf.js',
    'storage.js',
    'path.js',
    'text.js',
    'text-input.js',
    'stage.js',
    'audio-formats.js',
    'audio.js',
    'video.js',
    'dom-layout.js',
  ]) {
    vm.runInContext(fs.readFileSync(path.join(webDir, file), 'utf8'), context);
  }
  return { context, wasm, ready };
}

function makeFakeElement(tag = 'div') {
  const attrs = new Map();
  const listeners = new Map();
  const children = [];
  const styleProps = new Map();
  const style = new Proxy(
    {},
    {
      get(_t, prop) {
        if (prop === 'setProperty') {
          return (k, v) => styleProps.set(k, String(v));
        }
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
    style,
    dataset: {},
    children,
    parentNode: null,
    textContent: '',
    innerHTML: '',
    value: '',
    checked: false,
    disabled: false,
    readOnly: false,
    placeholder: '',
    name: '',
    type: '',
    tabIndex: 0,
    setAttribute(k, v) {
      attrs.set(String(k), String(v));
    },
    getAttribute(k) {
      return attrs.has(String(k)) ? attrs.get(String(k)) : null;
    },
    removeAttribute(k) {
      attrs.delete(String(k));
    },
    hasAttribute(k) {
      return attrs.has(String(k));
    },
    appendChild(child) {
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
    remove() {
      if (el.parentNode) el.parentNode.removeChild(el);
    },
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
      const arr = listeners.get(evt.type) || [];
      for (const fn of [...arr]) fn.call(el, evt);
      return !evt.defaultPrevented;
    },
    focus() {
      if (el.ownerDocument) el.ownerDocument.activeElement = el;
    },
    blur() {
      if (el.ownerDocument && el.ownerDocument.activeElement === el) {
        el.ownerDocument.activeElement = null;
      }
    },
    setSelectionRange(s, e) {
      el.selectionStart = s;
      el.selectionEnd = e;
    },
    getBoundingClientRect() {
      return { left: 0, top: 0, width: 400, height: 300 };
    },
  };
  return el;
}

function makeFakeDocument() {
  const docListeners = new Map();
  const doc = {
    currentScript: null,
    activeElement: null,
    fullscreenElement: null,
    createElement(tag) {
      const el = makeFakeElement(tag);
      el.ownerDocument = doc;
      return el;
    },
    createElementNS(_ns, tag) {
      const el = makeFakeElement(tag);
      el.ownerDocument = doc;
      return el;
    },
    addEventListener(type, fn) {
      if (!docListeners.has(type)) docListeners.set(type, []);
      docListeners.get(type).push(fn);
    },
    removeEventListener(type, fn) {
      const arr = docListeners.get(type);
      if (!arr) return;
      const idx = arr.indexOf(fn);
      if (idx >= 0) arr.splice(idx, 1);
    },
    dispatchEvent(evt) {
      const arr = docListeners.get(evt.type) || [];
      for (const fn of [...arr]) fn.call(doc, evt);
    },
  };
  return doc;
}

function makeBlankProject() {
  return {
    format_version: 1,
    name: 'Test Project',
    width: 400,
    height: 300,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 2,
          layers: [
            {
              id: 'l1',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [{ frame: 0, label: '', tween: null, elements: [] }],
            },
          ],
          scripts: [],
        },
      },
    ],
  };
}

test('DOM layout: responsive breakpoints, SemanticDomTextLayer, DomFormsHost and <neoflash-stage>', async () => {
  const doc = makeFakeDocument();
  const { context, ready } = loadWasmAndModules({ document: doc });
  await ready;
  const domLayout = context.NeoFlashDomLayout;

  // 1. Responsive layout & breakpoints
  const breakpoints = [
    { name: 'mobile', maxWidth: 480, scaleMode: 'exactFit', stageWidth: 320, stageHeight: 480 },
    { name: 'desktop', minWidth: 481, scaleMode: 'showAll', stageWidth: 800, stageHeight: 600 },
  ];
  const mobileLayout = domLayout.computeResponsiveLayout({
    containerWidth: 360,
    containerHeight: 640,
    stageWidth: 800,
    stageHeight: 600,
    scaleMode: 'showAll',
    align: 'TL',
    breakpoints,
  });
  assert.equal(mobileLayout.breakpoint, 'mobile');
  assert.equal(mobileLayout.stageWidth, 320);
  assert.equal(mobileLayout.stageHeight, 480);
  assert.equal(mobileLayout.scaleX, 360 / 320);
  assert.equal(mobileLayout.scaleY, 640 / 480);

  const desktopLayout = domLayout.computeResponsiveLayout({
    containerWidth: 1000,
    containerHeight: 800,
    stageWidth: 800,
    stageHeight: 600,
    scaleMode: 'showAll',
    align: '',
    breakpoints,
  });
  assert.equal(desktopLayout.breakpoint, 'desktop');
  assert.equal(desktopLayout.scaleX, 1.25);
  assert.equal(desktopLayout.scaleY, 1.25);
  assert.equal(desktopLayout.offsetY, 25);

  // 2. SemanticDomTextLayer mirrors static and dynamic text into accessible DOM nodes
  const container = doc.createElement('div');
  const textLayer = new domLayout.SemanticDomTextLayer(container, doc);
  textLayer.sync(
    [
      {
        instance_path: ['banner_txt'],
        transform: [1, 0, 0, 1, 20, 30],
        bounds: { x: 0, y: 0, width: 200, height: 40 },
        kind: {
          type: 'text',
          text: 'Welcome to NeoFlash',
          font_family: 'Inter',
          font_size: 18,
          color: [255, 255, 255, 255],
        },
      },
      {
        instance_path: ['status_txt'],
        transform: [1, 0, 0, 1, 20, 80],
        bounds: { x: 0, y: 0, width: 240, height: 30 },
        text_field: {
          text: 'Visit NeoFlash Docs',
          field_type: 'dynamic',
          selectable: true,
          multiline: false,
          format: {
            font: 'Arial',
            size: 14,
            color: 0x3366ff,
            bold: true,
            italic: false,
            underline: true,
            align: 'center',
            url: 'https://example.com/docs',
            target: '_blank',
          },
        },
      },
    ],
    new Set()
  );
  assert.equal(container.children.length, 2);
  assert.equal(container.children[0].textContent, 'Welcome to NeoFlash');
  assert.equal(container.children[1].getAttribute('aria-live'), 'polite');
  assert.equal(container.children[1].children.length, 1);
  assert.equal(container.children[1].children[0].tagName, 'A');
  assert.equal(container.children[1].children[0].getAttribute('href'), 'https://example.com/docs');

  // 3. DomFormsHost mounts form controls, reads/writes values, and dispatches change/submit events
  const formsContainer = doc.createElement('div');
  const events = [];
  const formsHost = new domLayout.DomFormsHost(formsContainer, {
    document: doc,
    onEvent: (pathArr, type, payload) => events.push({ pathArr, type, payload }),
  });
  formsHost.mountControl(['loginForm', 'username'], {
    type: 'text',
    value: 'alice',
    placeholder: 'Username',
    x: 10,
    y: 20,
    width: 180,
    height: 28,
  });
  formsHost.mountControl(['loginForm', 'remember'], {
    type: 'checkbox',
    checked: true,
    label: 'Remember me',
    x: 10,
    y: 60,
    width: 140,
    height: 24,
  });
  formsHost.mountControl(['loginForm', 'role'], {
    type: 'select',
    options: [
      { value: 'user', label: 'User' },
      { value: 'admin', label: 'Admin' },
    ],
    value: 'admin',
    x: 10,
    y: 95,
    width: 140,
    height: 28,
  });
  assert.equal(formsHost.getControlValue(['loginForm', 'username']), 'alice');
  assert.equal(formsHost.getControlChecked(['loginForm', 'remember']), true);
  formsHost.setControlValue(['loginForm', 'username'], 'bob');
  assert.equal(formsHost.getControlValue(['loginForm', 'username']), 'bob');

  const usernameRecord = formsHost.controls.get(JSON.stringify(['loginForm', 'username']));
  usernameRecord.control.value = 'charlie';
  usernameRecord.control.dispatchEvent({ type: 'change' });
  usernameRecord.control.dispatchEvent({
    type: 'keydown',
    key: 'Enter',
    preventDefault() {},
  });
  assert.equal(events.length, 2);
  assert.equal(events[0].type, 'change');
  assert.equal(events[0].payload.value, 'charlie');
  assert.equal(events[1].type, 'submit');

  formsHost.destroy();
  assert.equal(formsContainer.children.length, 0);
});

test('AMF & SharedObject: native .sol binary container with typed aliases and BroadcastChannel remote sync', async () => {
  const channels = new Map();
  class FakeBroadcastChannel {
    constructor(name) {
      this.name = String(name);
      this.onmessage = null;
      if (!channels.has(this.name)) channels.set(this.name, new Set());
      channels.get(this.name).add(this);
    }
    postMessage(msg) {
      const peers = channels.get(this.name) || new Set();
      const clone = JSON.parse(JSON.stringify(msg));
      for (const peer of peers) {
        if (peer !== this && typeof peer.onmessage === 'function') {
          peer.onmessage({ data: clone });
        }
      }
    }
    close() {
      const peers = channels.get(this.name);
      if (peers) peers.delete(this);
    }
  }

  const memStore = new Map();
  const fakeLocalStorage = {
    getItem: (k) => (memStore.has(k) ? memStore.get(k) : null),
    setItem: (k, v) => memStore.set(String(k), String(v)),
    removeItem: (k) => memStore.delete(String(k)),
  };

  const { context, wasm, ready } = loadWasmAndModules({
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: fakeLocalStorage,
    location: { href: 'https://example.com/app/index.html' },
  });
  await ready;

  const player = new wasm.WebPlayer(JSON.stringify(makeBlankProject()));
  const binaryA = context.NeoFlashByteArray.create({ wasm });
  const binaryB = context.NeoFlashByteArray.create({ wasm });

  const amfA = context.NeoFlashAMF.create({
    binary: binaryA,
    encode: (fmt, graph) => player.amf_encode(fmt, graph),
    decode: (fmt, bytes) => player.amf_decode(fmt, bytes),
    encodeSol: (fmt, name, graph) => wasm.sol_encode(fmt, name, graph),
    decodeSol: (bytes) => wasm.sol_decode(bytes),
  });
  const amfB = context.NeoFlashAMF.create({
    binary: binaryB,
    encode: (fmt, graph) => player.amf_encode(fmt, graph),
    decode: (fmt, bytes) => player.amf_decode(fmt, bytes),
    encodeSol: (fmt, name, graph) => wasm.sol_encode(fmt, name, graph),
    decodeSol: (bytes) => wasm.sol_decode(bytes),
  });

  class PlayerProfile {
    constructor(username = '', score = 0) {
      this._username = username;
      this.score = score;
    }
    get username() {
      return this._username;
    }
    set username(v) {
      this._username = String(v);
    }
  }
  amfA.registerClassAlias('com.neoflash.PlayerProfile', PlayerProfile);
  amfB.registerClassAlias('com.neoflash.PlayerProfile', PlayerProfile);

  // 1. Direct .sol binary encode/decode with typed alias + getter/setter accessor
  const solBytes = amfA.encodeSol(
    'saveSlot1',
    {
      profile: new PlayerProfile('Hero', 9001),
      unlocked: ['stage1', 'stage2'],
    },
    3
  );
  assert.equal(solBytes[0], 0x00);
  assert.equal(solBytes[1], 0xbf);
  assert.equal(String.fromCharCode(solBytes[6], solBytes[7], solBytes[8], solBytes[9]), 'TCSO');

  const decodedSol = amfB.decodeSol(solBytes);
  assert.equal(decodedSol.name, 'saveSlot1');
  assert.equal(decodedSol.objectEncoding, 3);
  assert.equal(decodedSol.data.profile instanceof PlayerProfile, true);
  assert.equal(decodedSol.data.profile.username, 'Hero');
  assert.equal(decodedSol.data.profile.score, 9001);
  assert.deepEqual(Array.from(decodedSol.data.unlocked), ['stage1', 'stage2']);

  // 2. SharedObject .sol import/export and remote BroadcastChannel sync + send()
  const storageA = context.NeoFlashStorage.create({
    amf: amfA,
    events: context.NeoFlashEvents,
    storage: fakeLocalStorage,
    url: 'https://example.com/app/index.html',
  });
  const storageB = context.NeoFlashStorage.create({
    amf: amfB,
    events: context.NeoFlashEvents,
    storage: fakeLocalStorage,
    url: 'https://example.com/app/index.html',
  });

  const localSo = storageA.SharedObject.getLocal('settings');
  localSo.data.profile = new PlayerProfile('LocalUser', 42);
  const exported = localSo.exportSolBytes();

  const importedSo = storageB.SharedObject.fromSolBytes(exported);
  assert.equal(importedSo.data.profile instanceof PlayerProfile, true);
  assert.equal(importedSo.data.profile.username, 'LocalUser');
  assert.equal(importedSo.data.profile.score, 42);

  // Remote SharedObject sync across two instances
  const remoteA = storageA.SharedObject.getRemote('lobby', 'rtmp://example.com/app');
  const remoteB = storageB.SharedObject.getRemote('lobby', 'rtmp://example.com/app');
  remoteA.connect({});
  remoteB.connect({});

  const syncEventsB = [];
  remoteB.addEventListener('sync', (e) => syncEventsB.push(e));
  const rpcCallsB = [];
  remoteB.client = {
    onChatMessage(sender, text) {
      rpcCallsB.push({ sender, text });
    },
  };

  remoteA.setProperty('leader', new PlayerProfile('Alice', 120));

  assert.equal(syncEventsB.length, 1);
  assert.equal(syncEventsB[0].changeList[0].name, 'leader');
  assert.equal(syncEventsB[0].changeList[0].code, 'change');
  assert.equal(remoteB.data.leader instanceof PlayerProfile, true);
  assert.equal(remoteB.data.leader.username, 'Alice');
  assert.equal(remoteB.data.leader.score, 120);

  remoteA.send('onChatMessage', 'Alice', 'Ready to start!');
  assert.deepEqual(rpcCallsB, [{ sender: 'Alice', text: 'Ready to start!' }]);

  remoteA.close();
  remoteB.close();
  storageA.dispose();
  storageB.dispose();
  player.free();
});

test('Portable text metrics, outlines, HTML/CSS formatting and autoSize without Canvas 2D', async () => {
  const { context, wasm, ready } = loadWasmAndModules();
  await ready;

  // 1. Portable measurement works identically without any host Canvas 2D context
  const widths1 = JSON.parse(
    wasm.portable_measure_text_json(
      JSON.stringify(['Hello', 'NeoFlash', '12345']),
      JSON.stringify({
        font: 'Arial',
        size: 16,
        color: 0,
        bold: false,
        italic: false,
        leading: 0,
        align: 'left',
        leftMargin: 0,
        rightMargin: 0,
        indent: 0,
        blockIndent: 0,
        underline: false,
        letterSpacing: 0,
        kerning: false,
        bullet: false,
        tabStops: [],
        url: '',
        target: '',
      })
    )
  );
  assert.equal(widths1.length, 3);
  assert.ok(widths1.every((w) => w > 0));

  // 2. NeoFlashText uses portable fallback when measureText is omitted and converts text to vector outlines
  const proj = makeBlankProject();
  const player = new wasm.WebPlayer(JSON.stringify(proj));
  const ref = player.create_display('text_field');

  const textService = context.NeoFlashText.create({
    player,
    wasm,
    events: context.NeoFlashEvents,
    onUpdate: () => {},
  });
  textService.update(ref, {
    text: 'Initial',
    width: 100,
    height: 24,
    multiline: true,
    word_wrap: false,
  });
  const fieldState = textService.info(ref);
  const layout = textService.layout(fieldState);
  assert.ok(layout.text_width > 0);
  assert.ok(layout.text_height > 0);

  // Auto-size center adjusts width and shifts x using portable metrics
  textService.set(ref, 'autoSize', 'center');
  textService.set(ref, 'text', 'Centered Portable AutoSize Heading');
  const afterState = textService.info(ref);
  assert.ok(afterState.box.width > 100);

  // HTML/CSS rich text round-trip
  textService.set(ref, 'htmlText', '<p align="center"><b>Bold</b> and <font color="#ff0000" size="22"><i>Red</i></font></p>');
  assert.equal(textService.get(ref, 'text').trim(), 'Bold and Red');
  const serializedHtml = textService.get(ref, 'htmlText');
  assert.ok(serializedHtml.includes('<b>Bold</b>'));

  // Convert text field to vector outline commands
  const outlines = textService.toOutlines(ref);
  assert.ok(Array.isArray(outlines));
  assert.ok(outlines.length > 10);
  assert.ok(outlines.some((cmd) => cmd.op === 'move_to'));
  player.free();
});

test('Touch/multi-touch, IME composition events, and external fullscreen synchronization', async () => {
  const doc = makeFakeDocument();
  const { context, wasm, ready } = loadWasmAndModules({ document: doc });
  await ready;

  const canvas = doc.createElement('canvas');
  canvas.width = 400;
  canvas.height = 300;
  const viewport = doc.createElement('div');

  const eventFamilies = context.NeoFlashEvents.createFamilies(wasm, {});
  const childTarget = {};
  const alive = () => true;
  let controller = null;
  const hub = context.NeoFlashEvents.create({
    alive,
    parentOf: (t) => (t === childTarget ? controller.stage : null),
    localPoint: (_t, x, y) => [x, y],
    globalPoint: (_t, x, y) => [x, y],
  });
  Object.assign(childTarget, hub.methods(childTarget));

  controller = context.NeoFlashStage.create({
    events: hub,
    FullScreenEvent: eventFamilies.FullScreenEvent,
    TouchEvent: eventFamilies.TouchEvent,
    IMEEvent: eventFamilies.IMEEvent,
    width: 400,
    height: 300,
    color: 0x000000,
    viewport,
    canvas,
    alive,
    parentOf: (t) => (t === childTarget ? controller.stage : null),
    localPoint: (_t, x, y) => [x, y],
    hitTest: () => childTarget,
    coordinates: (ev) => [Number(ev.clientX) || 0, Number(ev.clientY) || 0],
    canFocus: () => true,
  });
  controller.attach();

  const dispatched = [];
  for (const type of ['touchBegin', 'touchMove', 'touchEnd', 'touchTap']) {
    childTarget.addEventListener(type, (evt) => dispatched.push({ type, evt }));
  }

  // 1. Multi-touch: primary + secondary touch points
  controller.handleTouch('touchstart', {
    changedTouches: [
      { identifier: 10, clientX: 50, clientY: 50 },
      { identifier: 20, clientX: 60, clientY: 60 },
    ],
    preventDefault() {},
  });
  const begins = dispatched.filter((d) => d.type === 'touchBegin');
  assert.equal(begins.length, 2);
  assert.equal(begins[0].evt.touchPointID, 10);
  assert.equal(begins[0].evt.isPrimaryTouchPoint, true);
  assert.equal(begins[1].evt.touchPointID, 20);
  assert.equal(begins[1].evt.isPrimaryTouchPoint, false);

  controller.handleTouch('touchend', {
    changedTouches: [{ identifier: 10, clientX: 50, clientY: 50 }],
  });
  assert.ok(dispatched.some((d) => d.type === 'touchEnd' && d.evt.touchPointID === 10));
  assert.ok(dispatched.some((d) => d.type === 'touchTap' && d.evt.touchPointID === 10));

  // 2. IME composition events on focused target
  controller.stage.focus = childTarget;
  const imeEvents = [];
  for (const type of ['imeStartComposition', 'imeComposition', 'textInput']) {
    childTarget.addEventListener(type, (evt) => imeEvents.push({ type, text: evt.text || evt.data || '' }));
  }
  controller.handleComposition('compositionstart', { data: '' });
  controller.handleComposition('compositionupdate', { data: 'にほん' });
  controller.handleComposition('compositionend', { data: '日本' });
  const imeTypes = imeEvents.map((d) => d.type);
  assert.ok(imeTypes.includes('imeStartComposition'));
  assert.ok(imeTypes.includes('imeComposition'));
  assert.ok(imeTypes.includes('textInput'));

  // 3. External fullscreen entry & exit via document fullscreenchange listener
  const stageEvents = [];
  controller.stage.addEventListener('fullScreen', (e) => stageEvents.push(e.fullScreen));
  doc.fullscreenElement = viewport;
  doc.dispatchEvent({ type: 'fullscreenchange' });
  assert.equal(controller.stage.displayState, 'fullScreen');
  doc.fullscreenElement = null;
  doc.dispatchEvent({ type: 'fullscreenchange' });
  assert.equal(controller.stage.displayState, 'normal');
  assert.deepEqual(stageEvents, [true, false]);

  controller.dispose();
});

test('Sound sampleData dynamic streaming and NetStream step/syncToTimeline', async () => {
  const { context, wasm, ready } = loadWasmAndModules();
  await ready;

  const player = new wasm.WebPlayer(JSON.stringify(makeBlankProject()));
  const binary = context.NeoFlashByteArray.create({ wasm });
  const eventFamilies = context.NeoFlashEvents.createFamilies(wasm, {
    isByteArray: (v) => Boolean(binary.ByteArray.__nfIs(v)),
  });
  const net = context.NeoFlashNet.create({
    events: { ...context.NeoFlashEvents, ...eventFamilies },
    binary,
    resources: [],
  });

  const fakeAudioContext = () => ({
    sampleRate: 44100,
    state: 'running',
    destination: {},
    createGain: () => ({ gain: { value: 1 }, connect() {}, disconnect() {} }),
    createStereoPanner: () => ({ pan: { value: 0 }, connect() {}, disconnect() {} }),
    createBuffer: (channels, length, sampleRate) => {
      const chData = Array.from({ length: channels }, () => new Float32Array(length));
      return {
        numberOfChannels: channels,
        length,
        sampleRate,
        duration: length / sampleRate,
        getChannelData: (c) => chData[c],
      };
    },
    createBufferSource: () => ({
      buffer: null,
      loop: false,
      connect() {},
      disconnect() {},
      start() {},
      stop() {},
    }),
    decodeAudioData: async () => ({
      numberOfChannels: 2,
      length: 44100,
      sampleRate: 44100,
      duration: 1,
      getChannelData: () => new Float32Array(44100),
    }),
  });

  // 1. Sound dynamic streaming via sampleData
  const audioHost = context.NeoFlashAudio.create({
    player,
    wasm,
    events: { ...context.NeoFlashEvents, ...eventFamilies },
    net,
    binary,
    resources: [],
    createContext: fakeAudioContext,
  });
  const sound = new audioHost.Sound();
  let sampleCalls = 0;
  sound.addEventListener('sampleData', (evt) => {
    sampleCalls += 1;
    assert.equal(typeof evt.position, 'number');
    if (evt.position < 4096) {
      for (let i = 0; i < 2048; i += 1) {
        evt.data.writeFloat(0.25);
        evt.data.writeFloat(-0.25);
      }
    }
  });
  const channel = sound.play();
  assert.ok(channel);
  assert.equal(sampleCalls, 3);
  channel.stop();

  // 2. NetStream step() and syncToTimeline()
  const videoHost = context.NeoFlashVideo.create({
    player,
    wasm,
    events: context.NeoFlashEvents,
    net,
    getFrameRate: () => 24,
  });
  const nc = new videoHost.NetConnection();
  nc.connect(null);
  const ns = new videoHost.NetStream(nc);
  const statusCodes = [];
  ns.addEventListener('netStatus', (e) => statusCodes.push(e.info.code));

  ns.step(3);
  await Promise.resolve();
  assert.ok(Math.abs(ns.time - 3 / 24) < 1e-6);
  assert.ok(statusCodes.includes('NetStream.Step.Notify'));

  const syncedTime = ns.syncToTimeline(48, 24);
  assert.ok(Math.abs(syncedTime - 2.0) < 1e-6);
  assert.ok(Math.abs(ns.time - 2.0) < 1e-6);

  player.free();
});

test('Alpha masks: cacheAsBitmap on target and mask emits RuntimeAlphaMask and merges into clipTable', async () => {
  const { context, wasm, ready } = loadWasmAndModules();
  await ready;

  const player = new wasm.WebPlayer(JSON.stringify(makeBlankProject()));
  const targetRef = player.create_display('sprite');
  const maskRef = player.create_display('sprite');

  // Add both to stage
  player.stage_mutate(JSON.stringify({ op: 'add', child: JSON.parse(targetRef), index: null }));
  player.stage_mutate(JSON.stringify({ op: 'add', child: JSON.parse(maskRef), index: null }));

  // Draw semi-transparent shapes in target and mask
  player.graphics_apply(
    targetRef,
    JSON.stringify([
      { op: 'begin_fill', color: 0xff0000, alpha: 1.0 },
      { op: 'draw_rect', x: 0, y: 0, width: 100, height: 100 },
      { op: 'end_fill' },
    ])
  );
  player.graphics_apply(
    maskRef,
    JSON.stringify([
      { op: 'begin_fill', color: 0x000000, alpha: 0.45 },
      { op: 'draw_rect', x: 10, y: 10, width: 50, height: 50 },
      { op: 'end_fill' },
    ])
  );

  // Enable cacheAsBitmap on both target and mask, and assign mask
  for (const r of [targetRef, maskRef]) {
    player.as2_render_command(
      r,
      JSON.stringify({ op: 'set', name: 'cacheAsBitmap', value: true, as3: true })
    );
  }
  player.object_set_mask(targetRef, maskRef);

  const snapshot = JSON.parse(player.runtime_snapshot_json());
  assert.ok(Array.isArray(snapshot.filter_groups));
  const groupWithAlphaMask = snapshot.filter_groups.find(
    (g) => Array.isArray(g.alpha_masks) && g.alpha_masks.length > 0
  );
  assert.ok(groupWithAlphaMask, 'filter_group carries alpha_masks when both target and mask have cacheAsBitmap=true');
  assert.ok(Math.abs(groupWithAlphaMask.alpha_masks[0].components[0].alpha - 0.45) < 0.02);

  // Verify NeoFlashPaths.clipTable merges alpha_masks into clip components
  const clipMap = context.NeoFlashPaths.clipTable(snapshot.clips || [], snapshot.filter_groups || []);
  const maskId = groupWithAlphaMask.alpha_masks[0].clip_id;
  const merged = clipMap.get(maskId);
  assert.ok(merged);
  assert.ok(Math.abs(merged.components[0].alpha - 0.45) < 0.02);

  player.free();
});

test('TextInput: clicking Dynamic/Static text keeps Canvas text visible without DOM scrollbars; Input text inherits field properties', async () => {
  const doc = makeFakeDocument();
  const { context, wasm, ready } = loadWasmAndModules({ document: doc });
  await ready;

  const project = {
    format_version: 2,
    name: 'Text Input Regression',
    width: 400,
    height: 300,
    fps: 24,
    background: [15, 21, 35, 255],
    root: 'root',
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'l1',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  label: '',
                  tween: null,
                  elements: [
                    {
                      id: 'badge-label',
                      name: 'Badge label',
                      transform: { x: 40, y: 30, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'text',
                        text: 'BUILT TO BE INTERACTIVE',
                        font_family: 'Arial',
                        font_size: 12,
                        color: [167, 243, 208, 255],
                      },
                      scripts: [],
                    },
                    {
                      id: 'authored-input',
                      name: 'heroInput',
                      transform: { x: 40, y: 80, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'text_box',
                        text: 'Type here',
                        width: 220,
                        height: 36,
                        word_wrap: true,
                        multiline: true,
                        format: {
                          font: 'Georgia',
                          size: 16,
                          color: 0x336699,
                          bold: true,
                          italic: true,
                          leading: 4,
                          align: 'center',
                          leftMargin: 6,
                          rightMargin: 8,
                          indent: 10,
                          blockIndent: 4,
                          underline: true,
                          letterSpacing: 2,
                          kerning: true,
                          bullet: false,
                          tabStops: [],
                          url: '',
                          target: '',
                          fieldOptions: {
                            field_type: 'input',
                            selectable: true,
                            border: true,
                            border_color: 0x112233,
                            background: true,
                            background_color: 0xfefefe,
                          },
                        },
                      },
                      scripts: [],
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
    ],
  };

  const player = new wasm.WebPlayer(JSON.stringify(project));
  const surface = doc.createElement('div');
  const canvas = doc.createElement('canvas');
  canvas.width = 400;
  canvas.height = 300;
  surface.appendChild(canvas);

  const textService = context.NeoFlashText.create({
    player,
    wasm,
    events: context.NeoFlashEvents,
    onUpdate: () => {},
  });

  const snap = JSON.parse(player.runtime_snapshot_json());
  const badgeNode = snap.objects.find((n) => n.name === 'Badge label');
  const inputNode = snap.objects.find((n) => n.name === 'heroInput');
  assert.ok(badgeNode && inputNode);

  const proxies = new Map();
  const refs = new WeakMap();
  for (const node of [badgeNode, inputNode]) {
    const p = { name: node.name };
    const key = JSON.stringify(node.object_ref);
    proxies.set(key, p);
    refs.set(p, { key, objectRef: node.object_ref });
  }

  let currentFocus = null;
  let textInput = null;
  const stageObj = {
    get focus() {
      return currentFocus;
    },
    set focus(v) {
      const prev = currentFocus;
      currentFocus = v;
      textInput?.focusChanged(v, prev);
    },
  };
  const stageHost = {
    stage: stageObj,
    changeFocus(next) {
      stageObj.focus = next;
    },
    handlePointer() {},
  };
  const display = {
    alive: () => true,
    descriptor: (target) => (target === proxies.get(JSON.stringify(badgeNode.object_ref)) ? badgeNode : inputNode),
    targetPath: (target) => `_level0.${target.name}`,
  };
  const hub = { dispatch: () => true };

  textInput = context.NeoFlashTextInput.create({
    player,
    text: textService,
    display,
    events: context.NeoFlashEvents,
    hub,
    stage: stageHost,
    referenceOf: (t) => refs.get(t),
    proxyForRef: (ref) => proxies.get(JSON.stringify(ref)),
    canvas,
    surface,
    document: doc,
    project,
  });

  // 1. Focusing static/dynamic text ('BUILT TO BE INTERACTIVE') must NOT create a DOM textarea or hide Canvas text
  const badgeProxy = proxies.get(JSON.stringify(badgeNode.object_ref));
  stageHost.changeFocus(badgeProxy);
  assert.equal(textInput.hidden(badgeNode.object_ref), false, 'Canvas text stays visible when static/dynamic text is clicked');
  assert.equal(surface.children.length, 1, 'No DOM textarea/input overlay is appended for non-input text');

  // 2. Focusing Input text ('heroInput') creates a scrollbar-free, outline-free DOM textarea inheriting editor properties
  const inputProxy = proxies.get(JSON.stringify(inputNode.object_ref));
  stageHost.changeFocus(inputProxy);
  assert.equal(textInput.hidden(inputNode.object_ref), true, 'Active input text hides duplicate Canvas draw while editing');
  assert.equal(surface.children.length, 2, 'DOM input overlay is mounted for field_type=input');

  // Find the mounted textarea inside surface.children[1]
  let control = null;
  const walk = (el) => {
    if (el.getAttribute?.('data-neoflash-text-input') === 'true') control = el;
    for (const ch of el.children || []) walk(ch);
  };
  walk(surface.children[1]);
  assert.ok(control, 'Found mounted [data-neoflash-text-input] control');
  const localizedScene = JSON.parse(player.runtime_snapshot_json());
  const localizedDraw = localizedScene.draws.find((draw) => JSON.stringify(draw.owner_ref) === JSON.stringify(inputNode.object_ref));
  Object.assign(localizedDraw.kind.field, {
    locale: 'zh_Hans_CN',
    ligatures: 'none',
    digit_case: 'old_style',
    digit_width: 'tabular',
  });
  textInput.update(localizedScene);
  assert.equal(control.style.outline, 'none', 'No blue #2780e3 outline around focused text input');
  assert.equal(control.style.overflow, 'hidden', 'No native browser scrollbars around focused text input');
  assert.equal(control.style.scrollbarWidth, 'none');
  assert.equal(control.style.textAlign, 'center');
  assert.equal(control.style.letterSpacing, '2px');
  assert.equal(control.style.fontKerning, 'normal');
  assert.equal(control.style.textIndent, '10px');
  assert.equal(control.style.textDecoration, 'underline');
  assert.equal(control.style.fontVariantLigatures, 'none');
  assert.equal(control.style.fontVariantNumeric, 'oldstyle-nums tabular-nums');
  assert.equal(control.lang, 'zh-Hans-CN');
  assert.equal(control.style.paddingLeft, '12px'); // 2px pad + 6px leftMargin + 4px blockIndent
  assert.equal(control.style.paddingRight, '10px'); // 2px pad + 8px rightMargin
  assert.match(control.style.border, /1px solid rgba\(17,\s*34,\s*51/);
  assert.match(control.style.background, /rgba\(254,\s*254,\s*254/);

  // Reusing the active native control after returning to the default locale must not retain
  // language or OpenType settings from the previous authored field state.
  textInput.update(JSON.parse(player.runtime_snapshot_json()));
  assert.equal(control.lang, '');
  assert.equal(control.style.fontVariantLigatures, 'common-ligatures');
  assert.equal(control.style.fontVariantNumeric, 'normal');

  textInput.dispose();
  player.free();
});

function packSfntToWoff1ForTest(sfntBuf) {
  const zlib = require('node:zlib');
  const sfntVer = sfntBuf.readUInt32BE(0);
  const numTables = sfntBuf.readUInt16BE(4);
  const tableRecords = [];
  let totalSfntSize = 12 + numTables * 16;
  for (let i = 0; i < numTables; i += 1) {
    const off = 12 + i * 16;
    const tag = sfntBuf.subarray(off, off + 4);
    const checksum = sfntBuf.readUInt32BE(off + 4);
    const offset = sfntBuf.readUInt32BE(off + 8);
    const length = sfntBuf.readUInt32BE(off + 12);
    const raw = sfntBuf.subarray(offset, offset + length);
    totalSfntSize += (length + 3) & ~3;
    const compressed = zlib.deflateSync(raw, { level: 6 });
    const data = compressed.length < raw.length ? compressed : raw;
    tableRecords.push({ tag, checksum, origLength: length, data });
  }
  const dirStart = 44;
  let dataOffset = dirStart + numTables * 20;
  for (const rec of tableRecords) {
    rec.outOffset = dataOffset;
    dataOffset += (rec.data.length + 3) & ~3;
  }
  const out = Buffer.alloc(dataOffset);
  out.write('wOFF', 0, 'ascii');
  out.writeUInt32BE(sfntVer, 4);
  out.writeUInt32BE(out.length, 8);
  out.writeUInt16BE(numTables, 12);
  out.writeUInt16BE(0, 14);
  out.writeUInt32BE(totalSfntSize, 16);
  out.writeUInt16BE(1, 20);
  out.writeUInt16BE(0, 22);
  for (let i = 0; i < tableRecords.length; i += 1) {
    const rec = tableRecords[i];
    const entryOff = dirStart + i * 20;
    rec.tag.copy(out, entryOff);
    out.writeUInt32BE(rec.outOffset, entryOff + 4);
    out.writeUInt32BE(rec.data.length, entryOff + 8);
    out.writeUInt32BE(rec.origLength, entryOff + 12);
    out.writeUInt32BE(rec.checksum, entryOff + 16);
    rec.data.copy(out, rec.outOffset);
  }
  return out;
}

test('Portable text metrics resolve embedded TTF and WOFF1 fonts via WebPlayer without Canvas 2D', async () => {
  const { context, wasm, ready } = loadWasmAndModules();
  await ready;
  const interBytes = fs.readFileSync(
    path.resolve(__dirname, '../../crates/neoflash-ui/assets/fonts/Inter-Regular.ttf')
  );
  const woffBytes = packSfntToWoff1ForTest(interBytes);

  const makeFmt = (font) => ({
    font,
    size: 24,
    color: 0,
    bold: false,
    italic: false,
    leading: 0,
    align: 'left',
    leftMargin: 0,
    rightMargin: 0,
    indent: 0,
    blockIndent: 0,
    underline: false,
    letterSpacing: 0,
    kerning: false,
    bullet: false,
    tabStops: [],
    url: '',
    target: '',
  });

  const builtinWidths = JSON.parse(
    wasm.portable_measure_text_json(JSON.stringify(['MMMM', 'iiii']), JSON.stringify(makeFmt('NonExistentCleanFont294')))
  );

  for (const [label, family, mediaType, bytes] of [
    ['TTF', 'NeoCleanWasmTtf294', 'font/ttf', interBytes],
    ['WOFF1', 'NeoCleanWasmWoff294', 'font/woff', woffBytes],
  ]) {
    const proj = makeBlankProject();
    proj.format_version = 7;
    proj.resources = [
      {
        id: 'font-res',
        path: `fonts/${family}.${label === 'WOFF1' ? 'woff' : 'ttf'}`,
        media_type: mediaType,
        data_base64: bytes.toString('base64'),
      },
      {
        id: 'font-meta',
        path: 'fonts/embedding.json',
        media_type: 'application/json',
        data_base64: Buffer.from(
          JSON.stringify([{ id: 'font-res', family, style: 'Regular', ranges: [], extra_chars: '' }])
        ).toString('base64'),
      },
    ];

    const player = new wasm.WebPlayer(JSON.stringify(proj));
    const embeddedWidths = JSON.parse(
      player.portable_measure_text_json(JSON.stringify(['MMMM', 'iiii']), JSON.stringify(makeFmt(family)))
    );
    assert.ok(
      embeddedWidths[0] > embeddedWidths[1] * 3.0,
      `${label}: expected Inter M width >> i width, got ${JSON.stringify(embeddedWidths)}`
    );
    assert.ok(
      Math.abs(embeddedWidths[0] - builtinWidths[0]) > 2.0 ||
        Math.abs(embeddedWidths[1] - builtinWidths[1]) > 0.5,
      `${label}: expected embedded SFNT/WOFF1 advance widths (${JSON.stringify(embeddedWidths)}) to differ from builtin heuristic (${JSON.stringify(builtinWidths)})`
    );

    const textService = context.NeoFlashText.create({
      player,
      wasm,
      events: context.NeoFlashEvents,
      project: proj,
      onUpdate: () => {},
    });
    const ref = player.create_display('text_field');
    textService.update(ref, {
      text: 'MMMM',
      width: 20,
      height: 28,
      multiline: false,
      word_wrap: false,
      embed_fonts: true,
      format: { font: family, size: 24 },
    });
    textService.set(ref, 'autoSize', 'left');
    const twM = textService.get(ref, 'textWidth');
    assert.ok(
      Math.abs(twM - embeddedWidths[0]) < 1.0,
      `${label}: autoSize textWidth (${twM}) must match embedded portable measurement (${embeddedWidths[0]})`
    );
    textService.set(ref, 'text', 'iiii');
    const twI = textService.get(ref, 'textWidth');
    assert.ok(twM > twI * 3.0, `${label}: embedded autoSize ratio check (${twM} vs ${twI})`);

    textService.dispose();
    player.free();
  }
});

test('Clean-machine browser runtime loads embedded @font-face before Frame 0 layout, measurement, Canvas 2D paint, and DOM overlay', async () => {
  const { spawn } = require('node:child_process');
  const http = require('node:http');
  const os = require('node:os');

  const chromeCandidates = [
    process.env.CHROME_BIN,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
  ].filter(Boolean);
  const chromeBin = chromeCandidates.find((p) => fs.existsSync(p));
  assert.ok(chromeBin, `Headless Chrome binary not found; checked ${chromeCandidates.join(', ')}`);

  const interBytes = fs.readFileSync(
    path.resolve(__dirname, '../../crates/neoflash-ui/assets/fonts/Inter-Regular.ttf')
  );
  const woffBytes = packSfntToWoff1ForTest(interBytes);
  const cleanFamily = 'NeoCleanMachineUnique294';
  const cleanWoffFamily = 'NeoCleanMachineWoff294';

  const frame0Script = `
    this.tfM.autoSize = 'left';
    this.tfI.autoSize = 'left';
    trace('CLEAN_METRICS=' + JSON.stringify({
      wM: this.tfM.textWidth,
      wI: this.tfI.textWidth,
      bM: this.tfM.width,
      bI: this.tfI.width,
      woffM: this.tfWoffM.textWidth,
      woffI: this.tfWoffI.textWidth
    }));
  `;

  const makeBox = (id, name, font, text, x, y) => ({
    id,
    name,
    transform: { x, y, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
    kind: {
      type: 'text_box',
      text,
      width: 16,
      height: 36,
      word_wrap: false,
      multiline: false,
      format: {
        font,
        size: 24,
        color: 0xffffff,
        bold: false,
        italic: false,
        leading: 0,
        align: 'left',
        leftMargin: 0,
        rightMargin: 0,
        indent: 0,
        blockIndent: 0,
        underline: false,
        letterSpacing: 0,
        kerning: false,
        bullet: false,
        tabStops: [],
        url: '',
        target: '',
        fieldOptions: {
          field_type: 'dynamic',
          selectable: false,
          embed_fonts: true,
          auto_size: 'left',
        },
      },
    },
    scripts: [],
  });

  const bundle = {
    project: {
      format_version: 7,
      name: 'CleanMachineFont294',
      width: 480,
      height: 320,
      fps: 24,
      background: [18, 22, 31, 255],
      root: 'root',
      resources: [
        {
          id: 'clean-ttf',
          path: `fonts/${cleanFamily}.ttf`,
          media_type: 'font/ttf',
          data_base64: interBytes.toString('base64'),
        },
        {
          id: 'clean-woff',
          path: `fonts/${cleanWoffFamily}.woff`,
          media_type: 'font/woff',
          data_base64: woffBytes.toString('base64'),
        },
        {
          id: 'font-meta',
          path: 'fonts/embedding.json',
          media_type: 'application/json',
          data_base64: Buffer.from(
            JSON.stringify([
              { id: 'clean-ttf', family: cleanFamily, style: 'Regular', ranges: [], extra_chars: '' },
              { id: 'clean-woff', family: cleanWoffFamily, style: 'Regular', ranges: [], extra_chars: '' },
            ])
          ).toString('base64'),
        },
      ],
      symbols: [
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [
              {
                id: 'layer-main',
                name: 'Layer 1',
                visible: true,
                locked: false,
                keyframes: [
                  {
                    frame: 0,
                    label: '',
                    tween: null,
                    elements: [
                      makeBox('box-m', 'tfM', cleanFamily, 'MMMM', 20, 20),
                      makeBox('box-i', 'tfI', cleanFamily, 'iiii', 20, 70),
                      makeBox('box-wm', 'tfWoffM', cleanWoffFamily, 'MMMM', 240, 20),
                      makeBox('box-wi', 'tfWoffI', cleanWoffFamily, 'iiii', 240, 70),
                      {
                        id: 'static-txt',
                        name: '',
                        transform: { x: 20, y: 150, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: {
                          type: 'text',
                          text: 'StaticClean294',
                          font_family: cleanFamily,
                          font_size: 28,
                          color: [245, 245, 250, 255],
                        },
                        scripts: [],
                      },
                    ],
                  },
                ],
              },
            ],
            scripts: [{ frame: 0, script: { language: 'java_script', source: frame0Script } }],
          },
        },
      ],
    },
    wasm_url: '/runtime/neoflash_web_bg.wasm',
    fonts: {
      policy: 'subset',
      // Deliberately omit <style> from index.html so runtime.js ensureBundleFontsLoaded()
      // must inject css_rules and await only those faces before Frame 0.
      css_rules:
        `@font-face { font-family: "${cleanFamily}"; font-style: normal; font-weight: 400; src: url('data:font/ttf;base64,${interBytes.toString('base64')}'); font-display: block; }\n` +
        `@font-face { font-family: "${cleanWoffFamily}"; font-style: normal; font-weight: 400; src: url('data:font/woff;base64,${woffBytes.toString('base64')}'); font-display: block; }\n`,
      metrics: {},
      subset_stats: [],
      outlined_elements: 0,
    },
    scripts: [
      {
        language: 'java_script',
        source: frame0Script,
        code: frame0Script,
        location: 'Scene 1 / frame 1',
        locations: ['Scene 1 / frame 1'],
        source_url: 'neoflash-script-0.js',
      },
    ],
    warnings: [],
    player_config: { autoplay: false, preloader: true },
  };

  const hostModuleOrder = [
    'path.js', 'accessibility.js', 'preview.js', 'events.js', 'stage.js', 'legacy-input.js',
    'geom.js', 'geom3d.js', 'as2-strings.js', 'as2-platform.js', 'as2-object.js',
    'as2-transitions.js', 'as2-depth-manager.js', 'as2-print.js', 'as2-boxing.js',
    'as2-capture.js', 'as2-connections.js', 'as2-stage.js', 'as2-media.js', 'as2-services.js',
    'as2-locale.js', 'as2-files.js', 'as2-network.js', 'as2-text.js', 'as2-data.js',
    'as2-binding.js', 'as3-strings.js', 'as3-fte-3d.js', 'easing.js', 'math.js', 'date.js',
    'number-format.js', 'broadcaster.js', 'timers.js', 'byte-array.js', 'net.js',
    'external.js', 'bitmap.js', 'text.js', 'text-input.js', 'filters.js', 'audio-formats.js',
    'audio.js', 'video.js', 'xml.js', 'loader.js', 'amf.js', 'storage.js', 'air.js',
    'as3-net-sys.js', 'as3-media-events.js', 'display.js', 'dom-layout.js',
  ];

  const html = `<!doctype html>
<html>
<head><meta charset="utf-8"><title>CleanMachine294</title></head>
<body style="margin:0;background:#12161f;color:#fff;">
  <div id="loading">Loading</div>
  <div id="error" style="display:none"></div>
  <div id="name"></div>
  <div id="load-info"></div>
  <div id="status">Booting</div>
  <button id="toggle">Pause</button>
  <button id="restart">Restart</button>
  <button id="audio-toggle"></button>
  <span id="audio-status"></span>
  <select id="zoom"><option value="fit">Fit</option></select>
  <button id="pan"></button>
  <button id="fullscreen-toggle"></button>
  <button id="report-toggle" hidden></button>
  <pre id="size-report"></pre>
  <pre id="trace"></pre>
  <div id="viewport" style="width:480px;height:320px;position:relative;">
    <div id="stage-space"><div id="stage-surface" style="position:relative;">
      <canvas id="stage" width="480" height="320"></canvas>
      <div id="semantic-layer"></div>
    </div></div>
  </div>
  <script src="/runtime/neoflash_web.js"></script>
  ${hostModuleOrder.map((f) => `<script src="/web/${f}"></script>`).join('\n  ')}
  <script>window.NeoFlashBundle = ${JSON.stringify(bundle)};</script>
  <script src="/web/runtime.js"></script>
</body>
</html>`;

  const server = http.createServer((req, res) => {
    const reqPath = decodeURIComponent((req.url || '/').split('?')[0]);
    if (reqPath === '/' || reqPath === '/index.html') {
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
      res.end(html);
      return;
    }
    const target = reqPath.startsWith('/runtime/')
      ? path.join(runtimeDir, path.basename(reqPath))
      : reqPath.startsWith('/web/')
        ? path.join(webDir, path.basename(reqPath))
        : null;
    if (!target || !fs.existsSync(target)) {
      res.writeHead(404);
      res.end('Not found');
      return;
    }
    const ext = path.extname(target);
    const mime =
      ext === '.js' ? 'text/javascript; charset=utf-8'
        : ext === '.wasm' ? 'application/wasm'
          : 'application/octet-stream';
    res.writeHead(200, { 'Content-Type': mime });
    fs.createReadStream(target).pipe(res);
  });

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-clean-font-294-'));
  let chromeProc = null;
  let ws = null;
  try {
    chromeProc = spawn(
      chromeBin,
      [
        '--headless=new',
        '--remote-debugging-port=0',
        `--user-data-dir=${userDataDir}`,
        '--no-first-run',
        '--no-default-browser-check',
        'about:blank',
      ],
      { stdio: ['ignore', 'pipe', 'pipe'] }
    );
    const wsUrl = await new Promise((resolve, reject) => {
      let stderrBuf = '';
      const timer = setTimeout(() => reject(new Error(`Chrome DevTools timeout: ${stderrBuf}`)), 15000);
      chromeProc.stderr.on('data', async (chunk) => {
        stderrBuf += chunk.toString();
        const match = stderrBuf.match(/DevTools listening on (ws:\/\/\S+)/);
        if (match) {
          clearTimeout(timer);
          try {
            const devOrigin = match[1].replace(/^ws:/, 'http:').replace(/\/devtools\/.*/, '');
            const resp = await fetch(`${devOrigin}/json/list`);
            const targets = await resp.json();
            const page = targets.find((t) => t.type === 'page' && t.webSocketDebuggerUrl);
            if (!page) reject(new Error('No page CDP target'));
            else resolve(page.webSocketDebuggerUrl);
          } catch (err) {
            reject(err);
          }
        }
      });
      chromeProc.once('exit', (code) => {
        clearTimeout(timer);
        reject(new Error(`Chrome exited prematurely (${code}): ${stderrBuf}`));
      });
    });

    ws = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve, { once: true });
      ws.addEventListener('error', reject, { once: true });
    });
    let nextId = 1;
    const pending = new Map();
    const consoleErrors = [];
    let loadResolve = null;
    ws.addEventListener('message', (event) => {
      const msg = JSON.parse(String(event.data));
      if (msg.method === 'Page.loadEventFired' && loadResolve) {
        const fn = loadResolve;
        loadResolve = null;
        fn();
      } else if (msg.method === 'Runtime.exceptionThrown') {
        consoleErrors.push(JSON.stringify(msg.params.exceptionDetails));
      } else if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
        consoleErrors.push(JSON.stringify(msg.params.args));
      } else if (msg.id && pending.has(msg.id)) {
        const { resolve, reject } = pending.get(msg.id);
        pending.delete(msg.id);
        if (msg.error) reject(new Error(msg.error.message || JSON.stringify(msg.error)));
        else resolve(msg.result);
      }
    });
    const cdpSend = (method, params = {}) =>
      new Promise((resolve, reject) => {
        const id = nextId++;
        pending.set(id, { resolve, reject });
        ws.send(JSON.stringify({ id, method, params }));
      });
    await cdpSend('Page.enable');
    await cdpSend('Runtime.enable');
    const loadedPromise = new Promise((resolve) => {
      loadResolve = resolve;
    });
    await cdpSend('Page.navigate', { url: `http://127.0.0.1:${port}/index.html` });
    await loadedPromise;
    const evalJson = async (expression) => {
      const res = await cdpSend('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (res.exceptionDetails) throw new Error(JSON.stringify(res.exceptionDetails));
      return res.result?.value;
    };

    const report = await evalJson(`
      new Promise((resolve, reject) => {
        const deadline = Date.now() + 15000;
        const tick = () => {
          const loadingEl = document.getElementById('loading');
          const errEl = document.getElementById('error');
          if (errEl && errEl.textContent.trim()) {
            reject(new Error('Runtime error element: ' + errEl.textContent));
            return;
          }
          const traceText = document.getElementById('trace')?.textContent || '';
          if (loadingEl && loadingEl.style.display === 'none' && traceText.includes('CLEAN_METRICS=') && window.NeoFlashPlayer) {
            const faces = Array.from(document.fonts).map((f) => ({
              family: f.family.replace(/^["']|["']$/g, ''),
              status: f.status,
              weight: f.weight,
              style: f.style,
            }));
            const canvas = document.getElementById('stage');
            const ctx = canvas.getContext('2d');
            const sx = canvas.width / 480;
            const sy = canvas.height / 320;
            const countLitPixels = (rx, ry, rw, rh) => {
              const img = ctx.getImageData(Math.floor(rx * sx), Math.floor(ry * sy), Math.max(1, Math.floor(rw * sx)), Math.max(1, Math.floor(rh * sy))).data;
              let lit = 0;
              for (let i = 0; i < img.length; i += 4) {
                if (Math.abs(img[i] - 18) > 12 || Math.abs(img[i + 1] - 22) > 12 || Math.abs(img[i + 2] - 31) > 12) lit++;
              }
              return lit;
            };
            const domOverlayHost = document.createElement('div');
            document.body.appendChild(domOverlayHost);
            const domOverlay = new window.NeoFlashDomLayout.SemanticDomTextLayer(domOverlayHost, document);
            domOverlay.sync(
              [
                {
                  instance_path: ['static-txt'],
                  transform: [1, 0, 0, 1, 20, 150],
                  bounds: { x: 0, y: 0, width: 280, height: 40 },
                  kind: {
                    type: 'text',
                    text: 'StaticClean294',
                    font_family: ${JSON.stringify(cleanFamily)},
                    font_size: 28,
                    color: [245, 245, 250, 255],
                  },
                },
                {
                  instance_path: ['dom-m'],
                  transform: [1, 0, 0, 1, 20, 200],
                  bounds: { x: 0, y: 0, width: 280, height: 40 },
                  kind: {
                    type: 'text',
                    text: 'MMMM',
                    font_family: ${JSON.stringify(cleanWoffFamily)},
                    font_size: 24,
                    color: [245, 245, 250, 255],
                  },
                },
                {
                  instance_path: ['dom-i'],
                  transform: [1, 0, 0, 1, 20, 240],
                  bounds: { x: 0, y: 0, width: 280, height: 40 },
                  kind: {
                    type: 'text',
                    text: 'iiii',
                    font_family: ${JSON.stringify(cleanWoffFamily)},
                    font_size: 24,
                    color: [245, 245, 250, 255],
                  },
                },
              ],
              { x: 1, y: 1 }
            );
            const domTexts = Array.from(domOverlayHost.querySelectorAll('.neoflash-dom-text-node')).map((el) => {
              const range = document.createRange();
              range.selectNodeContents(el);
              return {
                text: el.textContent,
                fontFamily: el.style.fontFamily,
                fontWeight: el.style.fontWeight,
                fontStyle: el.style.fontStyle,
                glyphWidth: range.getBoundingClientRect().width,
              };
            });
            resolve({
              faces,
              traceText,
              litTtfBox: countLitPixels(18, 18, 120, 40),
              litWoffBox: countLitPixels(238, 18, 120, 40),
              litStaticText: countLitPixels(18, 145, 220, 45),
              domTexts,
              styleInjected: document.querySelectorAll('style[data-neoflash-fonts="true"]').length,
            });
            return;
          }
          if (Date.now() > deadline) {
            reject(new Error('Timeout waiting for Frame 0 render: status=' + document.getElementById('status')?.textContent + ' trace=' + traceText));
            return;
          }
          setTimeout(tick, 40);
        };
        tick();
      })
    `);

    assert.deepEqual(consoleErrors, [], `Browser console errors: ${consoleErrors.join('; ')}`);
    assert.equal(report.styleInjected, 1, 'runtime.js must inject bundle.fonts.css_rules dynamically when absent from <head>');

    const ttfFace = report.faces.find((f) => f.family === cleanFamily);
    const woffFace = report.faces.find((f) => f.family === cleanWoffFamily);
    assert.equal(ttfFace?.status, 'loaded', `Clean-machine TTF @font-face must load: ${JSON.stringify(report.faces)}`);
    assert.equal(woffFace?.status, 'loaded', `Clean-machine WOFF1 @font-face must load: ${JSON.stringify(report.faces)}`);

    const match = report.traceText.match(/CLEAN_METRICS=(\{[^\n]+\})/);
    assert.ok(match, `Missing Frame 0 metrics trace: ${report.traceText}`);
    const metrics = JSON.parse(match[1]);
    assert.ok(metrics.wM > metrics.wI * 3.0, `Frame 0 TTF Canvas metrics must reflect Inter M/i proportion, got ${JSON.stringify(metrics)}`);
    assert.ok(metrics.woffM > metrics.woffI * 3.0, `Frame 0 WOFF1 Canvas metrics must reflect Inter M/i proportion, got ${JSON.stringify(metrics)}`);
    assert.ok(metrics.bM > metrics.bI * 2.5, `Frame 0 autoSize width must grow with loaded @font-face: ${JSON.stringify(metrics)}`);

    assert.ok(report.litTtfBox > 30, `Canvas 2D TTF TextBox drew ${report.litTtfBox} pixels`);
    assert.ok(report.litWoffBox > 30, `Canvas 2D WOFF1 TextBox drew ${report.litWoffBox} pixels`);
    assert.ok(report.litStaticText > 30, `Canvas 2D ElementKind::Text drew ${report.litStaticText} pixels`);

    const staticDom = report.domTexts.find((d) => d.text.includes('StaticClean294'));
    assert.ok(staticDom && staticDom.fontFamily.includes(cleanFamily) && staticDom.fontWeight === '400', `DOM overlay typography mismatch: ${JSON.stringify(report.domTexts)}`);
    const domM = report.domTexts.find((d) => d.text === 'MMMM');
    const domI = report.domTexts.find((d) => d.text === 'iiii');
    assert.ok(domM && domI && domM.glyphWidth > domI.glyphWidth * 3.0, `DOM overlay glyph layout must reflect loaded @font-face: ${JSON.stringify(report.domTexts)}`);
  } finally {
    try { ws?.close(); } catch {}
    if (chromeProc && chromeProc.exitCode === null) {
      await new Promise((resolve) => {
        chromeProc.once('exit', resolve);
        chromeProc.kill('SIGKILL');
        setTimeout(resolve, 1500);
      });
    }
    await new Promise((resolve) => server.close(resolve));
    try {
      fs.rmSync(userDataDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 50 });
    } catch {}
  }
});
