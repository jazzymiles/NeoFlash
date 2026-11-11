// End-to-end WASM + host runtime tests for T-247 (AVI, SVG, BMP, TIFF, WebP, GIF, SWF, WAV)
// and T-248 (dynamic loading of published NeoFlash bundles via loadMovie / Loader / loadPublish).
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const rootDir = path.resolve(__dirname, '../..');
const runtimeDir = path.join(rootDir, 'target/web-runtime');

const wasmContext = vm.createContext({
  TextEncoder,
  TextDecoder,
  WebAssembly,
  console,
  URL,
  Request,
  Response,
});
vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'), 'utf8'), wasmContext);
const wasm = vm.runInContext('NeoFlashWasm', wasmContext);
wasmContext.wasmBytes = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm'));
const wasmReady = vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', wasmContext);

const eventsMod = require('../events.js');
const byteArrayMod = require('../byte-array.js');
const netMod = require('../net.js');
const loaderMod = require('../loader.js');
const videoMod = require('../video.js');

function buildBmp2x2() {
  // 2x2 24-bit BI_RGB BMP (row stride = 8 bytes, pixel data = 16 bytes, total = 70 bytes)
  const buf = Buffer.alloc(70, 0);
  buf.write('BM', 0, 'ascii');
  buf.writeUInt32LE(70, 2);
  buf.writeUInt32LE(54, 10);
  buf.writeUInt32LE(40, 14); // biSize
  buf.writeInt32LE(2, 18);   // width
  buf.writeInt32LE(2, 22);   // height
  buf.writeUInt16LE(1, 26);  // planes
  buf.writeUInt16LE(24, 28); // bitCount
  buf.writeUInt32LE(0, 30);  // BI_RGB
  buf.writeUInt32LE(16, 34); // biSizeImage
  // Bottom row: red, green; Top row: blue, white (BGR)
  buf.set([0, 0, 255, 0, 255, 0, 0, 0], 54);
  buf.set([255, 0, 0, 255, 255, 255, 0, 0], 62);
  return new Uint8Array(buf);
}

function buildTiff2x2() {
  // Minimal valid uncompressed RGB 2x2 baseline TIFF (little-endian II*\0)
  // Header (8) + BitsPerSample values (6) + Pixel data (12) + IFD (2 + 9*12 + 4 = 114) = 140 bytes
  const buf = Buffer.alloc(140, 0);
  buf.set([0x49, 0x49, 0x2a, 0x00], 0);
  const ifdOffset = 26;
  buf.writeUInt32LE(ifdOffset, 4);
  // BitsPerSample at offset 8: [8, 8, 8]
  buf.writeUInt16LE(8, 8);
  buf.writeUInt16LE(8, 10);
  buf.writeUInt16LE(8, 12);
  // Pixel data at offset 14 (2x2 RGB = 12 bytes)
  buf.set([255, 64, 32, 32, 200, 64, 16, 80, 240, 250, 250, 120], 14);

  let p = ifdOffset;
  const entries = [
    [256, 4, 1, 2],   // ImageWidth = 2
    [257, 4, 1, 2],   // ImageLength = 2
    [258, 3, 3, 8],   // BitsPerSample -> offset 8
    [259, 3, 1, 1],   // Compression = 1 (None)
    [262, 3, 1, 2],   // PhotometricInterpretation = 2 (RGB)
    [273, 4, 1, 14],  // StripOffsets = 14
    [277, 3, 1, 3],   // SamplesPerPixel = 3
    [278, 4, 1, 2],   // RowsPerStrip = 2
    [279, 4, 1, 12],  // StripByteCounts = 12
  ];
  buf.writeUInt16LE(entries.length, p);
  p += 2;
  for (const [tag, type, count, value] of entries) {
    buf.writeUInt16LE(tag, p);
    buf.writeUInt16LE(type, p + 2);
    buf.writeUInt32LE(count, p + 4);
    if (type === 3 && count === 1) {
      buf.writeUInt16LE(value, p + 8);
    } else {
      buf.writeUInt32LE(value, p + 8);
    }
    p += 12;
  }
  buf.writeUInt32LE(0, p);
  return new Uint8Array(buf);
}

function buildGif1x1() {
  return Buffer.from(
    'R0lGODlhAQABAIABAP8AAP///yH5BAEAAAEALAAAAAABAAEAAAICRAEAOw==',
    'base64',
  );
}

function buildWebp1x1() {
  // Minimal 1x1 lossless VP8L WebP
  return Buffer.from(
    'UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==',
    'base64',
  );
}

function buildAvi2x2TwoFrames() {
  // Valid RIFF AVI with 2x2 24-bit BI_RGB video stream (10 fps, 2 frames: red then cyan)
  function chunk(fourcc, payload) {
    const pad = payload.length % 2;
    const out = Buffer.alloc(8 + payload.length + pad, 0);
    out.write(fourcc, 0, 'ascii');
    out.writeUInt32LE(payload.length, 4);
    payload.copy(out, 8);
    return out;
  }
  function list(type, children) {
    const body = Buffer.concat([Buffer.from(type, 'ascii'), ...children]);
    return chunk('LIST', body);
  }
  const avih = Buffer.alloc(56, 0);
  avih.writeUInt32LE(100_000, 0); // dwMicroSecPerFrame = 100ms (10 fps)
  avih.writeUInt32LE(2, 16);      // dwTotalFrames = 2
  avih.writeUInt32LE(1, 24);      // dwStreams = 1
  avih.writeUInt32LE(2, 32);      // dwWidth = 2
  avih.writeUInt32LE(2, 36);      // dwHeight = 2

  const strh = Buffer.alloc(56, 0);
  strh.write('vids', 0, 'ascii');
  strh.write('DIB ', 4, 'ascii');
  strh.writeUInt32LE(1, 20);  // dwScale = 1
  strh.writeUInt32LE(10, 24); // dwRate = 10
  strh.writeUInt32LE(2, 32);  // dwLength = 2

  const strf = Buffer.alloc(40, 0);
  strf.writeUInt32LE(40, 0);
  strf.writeInt32LE(2, 4);
  strf.writeInt32LE(2, 8);
  strf.writeUInt16LE(1, 12);
  strf.writeUInt16LE(24, 14);
  strf.writeUInt32LE(0, 16); // BI_RGB
  strf.writeUInt32LE(16, 20);

  // Frame 0: solid red (BGR = 0, 0, 255); Frame 1: solid cyan (BGR = 255, 255, 0)
  const f0 = Buffer.from([0, 0, 255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255, 0, 0]);
  const f1 = Buffer.from([255, 255, 0, 255, 255, 0, 0, 0, 255, 255, 0, 255, 255, 0, 0, 0]);

  const hdrl = list('hdrl', [chunk('avih', avih), list('strl', [chunk('strh', strh), chunk('strf', strf)])]);
  const movi = list('movi', [chunk('00db', f0), chunk('00db', f1)]);
  const riffBody = Buffer.concat([Buffer.from('AVI ', 'ascii'), hdrl, movi]);
  const riff = chunk('RIFF', riffBody);
  return new Uint8Array(riff);
}

const transform = (x = 0, y = 0) => ({ x, y, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 });

test('T-247: runtime Loader decodes BMP, TIFF, WebP, GIF, SVG and SWF; NetStream decodes AVI via Rust WASM', async () => {
  await wasmReady;
  const aviBytes = buildAvi2x2TwoFrames();
  const baseProj = {
    format_version: 3,
    name: 'MediaHost',
    width: 320,
    height: 240,
    fps: 24,
    background: [20, 24, 30, 255],
    root: 'root',
    resources: [
      { id: 'r_bmp', path: 'img/sample.bmp', media_type: 'image/bmp', data_base64: Buffer.from(buildBmp2x2()).toString('base64') },
      { id: 'r_tif', path: 'img/sample.tiff', media_type: 'image/tiff', data_base64: Buffer.from(buildTiff2x2()).toString('base64') },
      { id: 'r_webp', path: 'img/sample.webp', media_type: 'image/webp', data_base64: buildWebp1x1().toString('base64') },
      { id: 'r_gif', path: 'img/sample.gif', media_type: 'image/gif', data_base64: buildGif1x1().toString('base64') },
      {
        id: 'r_svg',
        path: 'vec/badge.svg',
        media_type: 'image/svg+xml',
        data_base64: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="36" height="24"><rect width="36" height="24" fill="#ff6600"/></svg>').toString('base64'),
      },
      {
        id: 'r_swf',
        path: 'clips/anim.swf',
        media_type: 'application/x-shockwave-flash',
        data_base64: Buffer.from([0x46, 0x57, 0x53, 10, 8, 0, 0, 0]).toString('base64'),
      },
      { id: 'r_avi', path: 'video/demo.avi', media_type: 'video/x-msvideo', data_base64: Buffer.from(aviBytes).toString('base64') },
    ],
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: { frame_count: 1, layers: [{ id: 'l0', name: 'Layer 1', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], label: '', tween: null }] }], scripts: [] },
      },
    ],
  };

  const player = new wasm.WebPlayer(JSON.stringify(baseProj));
  const binary = byteArrayMod.create();
  const tasks = [];
  const flushTasks = async () => {
    for (let round = 0; round < 8; round++) {
      while (tasks.length) {
        tasks.shift()();
        await Promise.resolve();
      }
      await Promise.resolve();
    }
  };
  const net = netMod.create({
    events: eventsMod,
    binary,
    resources: baseProj.resources,
    schedule: fn => tasks.push(fn),
  });

  const refs = new WeakMap();
  const proxies = new Map();
  const proxyForRef = ref => {
    const k = JSON.stringify(ref);
    if (proxies.has(k)) return proxies.get(k);
    const obj = { __ref: ref };
    refs.set(obj, { key: k, objectRef: ref });
    proxies.set(k, obj);
    return obj;
  };
  const displayStub = {
    alive: target => {
      const r = refs.get(target);
      return Boolean(r && player.object_alive(r.key));
    },
    descriptor: target => JSON.parse(player.object_describe(refs.get(target).key)),
  };

  const loaderHost = loaderMod.create({
    player,
    events: eventsMod,
    net,
    binary,
    project: baseProj,
    display: displayStub,
    proxyForRef,
    referenceOf: t => refs.get(t),
    schedule: fn => tasks.push(fn),
  });

  for (const [assetPath, expectedW, expectedH, expectedMime] of [
    ['img/sample.bmp', 2, 2, 'image/bmp'],
    ['img/sample.tiff', 2, 2, 'image/tiff'],
    ['img/sample.webp', 1, 1, 'image/webp'],
    ['img/sample.gif', 1, 1, 'image/gif'],
    ['vec/badge.svg', 36, 24, 'image/svg+xml'],
    ['clips/anim.swf', 550, 400, 'application/x-shockwave-flash'],
  ]) {
    const ldr = new loaderHost.Loader();
    const methods = loaderHost.methods(ldr);
    const info = loaderHost.get(ldr, 'contentLoaderInfo');
    let completed = false;
    info.addEventListener('complete', () => { completed = true; });
    methods.load(new net.URLRequest(assetPath));
    await flushTasks();
    assert.equal(completed, true, `expected ${assetPath} to complete loading`);
    assert.equal(info.width, expectedW, `width mismatch for ${assetPath}`);
    assert.equal(info.height, expectedH, `height mismatch for ${assetPath}`);
    assert.equal(info.contentType, expectedMime, `mime mismatch for ${assetPath}`);
    methods.unload();
  }

  // Test AVI runtime decoding through NeoFlashVideo + Rust WASM avi_inspect_json & avi_frame_rgba
  const painted = [];
  const videoHost = videoMod.create({
    player,
    wasm,
    events: eventsMod,
    net,
    referenceOf: t => refs.get(t),
    proxyForRef,
    schedule: fn => tasks.push(fn),
    isUserGesture: () => true,
    createCanvas: (w, h) => ({
      width: w,
      height: h,
      getContext: () => ({
        createImageData: (cw, ch) => ({ width: cw, height: ch, data: new Uint8ClampedArray(cw * ch * 4) }),
        putImageData: img => { painted.push(Array.from(img.data.slice(0, 4))); },
        drawImage: () => {},
      }),
    }),
  });

  const vid = new videoHost.Video(160, 120);
  const nc = new videoHost.NetConnection();
  nc.connect(null);
  await flushTasks();
  const ns = new videoHost.NetStream(nc);
  let meta = null;
  ns.client = { onMetaData: info => { meta = info; } };
  videoHost.methods(vid).attachNetStream(ns);
  ns.play('video/demo.avi');
  await flushTasks();

  assert.ok(meta, 'AVI onMetaData should fire');
  assert.equal(meta.width, 2);
  assert.equal(meta.height, 2);
  assert.equal(meta.framerate, 10);
  assert.ok(painted.length >= 1, 'AVI frame 0 should paint onto Video surface');
  assert.deepEqual(painted[0], [255, 0, 0, 255], 'AVI frame 0 should decode BGR red -> RGBA red');

  ns.seek(0.12);
  await flushTasks();
  assert.deepEqual(painted[painted.length - 1], [0, 255, 255, 255], 'AVI seek to 120ms should decode frame 1 (cyan)');

  ns.close();
  videoHost.dispose();
  loaderHost.dispose();
  player.free();
});

test('T-248: loadPublish mounts published NeoFlash bundles (bundle.js and index.html), steps nested timelines, runs child scripts, and unloads cleanly', async () => {
  await wasmReady;
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');

  const childProject = {
    format_version: 3,
    name: 'ChildSubApp',
    width: 200,
    height: 150,
    fps: 24,
    background: [10, 20, 40, 255],
    root: 'child_root',
    resources: [
      { id: 'c_bmp', path: 'sub_badge.bmp', media_type: 'image/bmp', data_base64: Buffer.from(buildBmp2x2()).toString('base64') },
    ],
    symbols: [
      {
        id: 'child_root',
        name: 'ChildScene',
        kind: 'movie_clip',
        timeline: {
          frame_count: 3,
          layers: [
            {
              id: 'cl0',
              name: 'ChildLayer',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  label: 'intro',
                  tween: 'linear',
                  elements: [
                    { id: 'c_inst', name: 'spinner', transform: transform(10, 20), kind: { type: 'instance', symbol_id: 'wheel_sym', playback: 'independent' }, scripts: [] },
                  ],
                },
                {
                  frame: 2,
                  label: 'end',
                  tween: null,
                  elements: [
                    { id: 'c_inst', name: 'spinner', transform: transform(50, 20), kind: { type: 'instance', symbol_id: 'wheel_sym', playback: 'independent' }, scripts: [] },
                  ],
                },
              ],
            },
          ],
          scripts: [
            { frame: 0, script: { language: 'action_script2', source: 'trace("child-f1:" + this.spinner._x);' } },
            { frame: 1, script: { language: 'action_script2', source: 'trace("child-f2:" + this.spinner._x);' } },
          ],
        },
      },
      {
        id: 'wheel_sym',
        name: 'Wheel',
        kind: 'movie_clip',
        timeline: {
          frame_count: 2,
          layers: [
            {
              id: 'wl0',
              name: 'Art',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  label: '',
                  tween: null,
                  elements: [
                    { id: 'w_rect', name: '', transform: transform(0, 0), kind: { type: 'rectangle', width: 24, height: 24, fill: [0, 220, 140, 255], stroke: [0, 0, 0, 0], stroke_width: 0 }, scripts: [] },
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

  const childBundleJs = 'globalThis.NeoFlashBundle = ' + JSON.stringify({
    project: childProject,
    scripts: [
      {
        language: 'action_script2',
        source: 'trace("child-f1:" + this.spinner._x);',
        code: 'trace("child-f1:" + this.spinner._x);',
        location: 'ChildScene:frame1',
      },
      {
        language: 'action_script2',
        source: 'trace("child-f2:" + this.spinner._x);',
        code: 'trace("child-f2:" + this.spinner._x);',
        location: 'ChildScene:frame2',
      },
    ],
    warnings: [],
  }) + ';\n';

  const childIndexHtml = '<!doctype html><html><head><title>ChildSubApp — NeoFlash</title></head><body><script src="bundle.js"></script></body></html>';

  const parentProject = {
    format_version: 3,
    name: 'ParentApp',
    width: 400,
    height: 300,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: [
      { id: 'p_sub_bundle', path: 'subapp/bundle.js', media_type: 'text/javascript', data_base64: Buffer.from(childBundleJs, 'utf8').toString('base64') },
      { id: 'p_sub_html', path: 'subapp/index.html', media_type: 'text/html', data_base64: Buffer.from(childIndexHtml, 'utf8').toString('base64') },
    ],
    symbols: [
      {
        id: 'root',
        name: 'ParentScene',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'pl0',
              name: 'HostLayer',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  label: '',
                  tween: null,
                  elements: [
                    { id: 'holder_el', name: 'holder', transform: transform(40, 30), kind: { type: 'instance', symbol_id: 'empty_clip', playback: 'independent' }, scripts: [] },
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
      {
        id: 'empty_clip',
        name: 'EmptyHolder',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [{ id: 'el0', name: 'L', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
          scripts: [],
        },
      },
    ],
  };

  const tasks = [];
  const flushTasks = async () => {
    for (let round = 0; round < 8; round++) {
      let guard = 0;
      while (tasks.length && guard++ < 64) {
        tasks.shift()();
        await Promise.resolve();
      }
      await Promise.resolve();
    }
  };

  const player = new wasm.WebPlayer(JSON.stringify(parentProject));
  const { host, traceCalls } = createRuntimeHost(rootDir, player, [], {
    project: parentProject,
    bundle: { project: parentProject, scripts: [] },
    net: { schedule: fn => tasks.push(fn) },
    loader: { schedule: fn => tasks.push(fn) },
  });

  const rootClip = host.proxyFor([]);
  const holder = rootClip.holder;
  assert.ok(holder, 'holder MovieClip exists on parent Stage');

  // 1. Load published child app via index.html entry redirect using loadPublish()
  let resolvedClip = null;
  host.legacy().loadPublish('subapp/index.html', holder).then(clip => {
    resolvedClip = clip;
  });
  await flushTasks();

  assert.equal(resolvedClip, holder, 'loadPublish resolves to mounted holder MovieClip');
  assert.ok(traceCalls.some(args => args[0] === 'child-f1:10'), `expected child frame 1 script trace, got: ${JSON.stringify(traceCalls)}`);
  assert.ok(holder.spinner, 'mounted child instance spinner is accessible inside holder');
  assert.equal(holder._totalframes, 3, 'holder adopts mounted child timeline frame count');

  // Tick the player and verify nested child timeline tween + frame 2 script run!
  player.tick();
  host.dispatchScripts();
  assert.equal(holder._currentframe, 2);
  assert.ok(traceCalls.some(args => args[0] === 'child-f2:30'), `expected tweened child frame 2 trace, got: ${JSON.stringify(traceCalls)}`);

  // Unload published child app and verify clean teardown
  holder.unloadMovie();
  await flushTasks();
  assert.equal(holder.spinner, undefined, 'child instances removed after unloadMovie');
  assert.equal(holder._totalframes, 1, 'holder restored to empty 1-frame clip after unloadMovie');

  // 2. Also verify AS3 Loader mounting and unloadAndStop()
  const LoaderCtor = host.loader().Loader;
  const URLRequestCtor = host.net().URLRequest;
  const as3Loader = new LoaderCtor();
  rootClip.addChild(as3Loader);
  let as3InitFired = false;
  as3Loader.contentLoaderInfo.addEventListener('complete', () => {
    as3InitFired = true;
  });
  as3Loader.load(new URLRequestCtor('subapp/bundle.js'));
  await flushTasks();

  assert.equal(as3InitFired, true, 'AS3 Loader complete event fired for published bundle');
  assert.equal(as3Loader.contentLoaderInfo.width, 200);
  assert.equal(as3Loader.contentLoaderInfo.height, 150);
  assert.ok(as3Loader.content, 'AS3 Loader.content holds mounted child MovieClip');
  assert.ok(as3Loader.content.spinner, 'AS3 Loader.content exposes child nested MovieClip');

  as3Loader.unloadAndStop(true);
  await flushTasks();
  assert.equal(as3Loader.content, null, 'AS3 Loader.content cleared after unloadAndStop');

  host.disposeFamily();
  player.free();
});
