'use strict';

// End-to-end verification of the T-460 AS3 Flash Player backlog families:
// 1. flash.system.{Worker, WorkerDomain} — real OS background thread execution
//    (Node worker_threads + Browser Web Worker) for compiled AVM2 SWFs & NeoFlash self-workers,
//    SharedArrayBuffer shareable ByteArray atomics, and blocking MessageChannel.receive(true).
// 2. flash.system.{SystemUpdater, SystemUpdaterType} — live HTTP streaming download,
//    single-active-update lock (#2037), AbortController cancel(), SHA-256 verification,
//    and Adobe StatusEvent / SecurityErrorEvent / IOErrorEvent error mapping.
// 3. flash.net.drm.*, flash.events.DRM*, and encrypted NetStream — binary FADRM/CENC
//    DRMContentData, live HTTP license protocol (FIFO authenticate, token clearing,
//    DRMLicenseRequestEvent serverURL override, loadVoucher/loadPreviewVoucher,
//    returnVoucher, addToDeviceGroup/removeFromDeviceGroup, networkIdleTimeout),
//    and FIPS-197 AES-128-CTR media decryption in NetStream.appendBytes + resetDRMVouchers.

const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const os = require('node:os');
const http = require('node:http');
const crypto = require('node:crypto');
const path = require('node:path');
const vm = require('node:vm');
const { execFileSync, spawn } = require('node:child_process');

const repoRoot = path.resolve(__dirname, '../..');
const runtimeDir = path.join(repoRoot, 'target/web-runtime');
const exportBin = path.join(repoRoot, 'target/debug/neoflash-export');

const Events = require('../events.js');
const Binary = require('../byte-array.js');
const AMF = require('../amf.js');
const Net = require('../net.js');
const Storage = require('../storage.js');
const Video = require('../video.js');
const XML = require('../xml.js');
const External = require('../external.js');
const NeoFlashAS3NetSys = require('../as3-net-sys.js');
const NeoFlashAS3MediaEvents = require('../as3-media-events.js');

function findCargoBinary() {
  const homeCargo = path.join(process.env.HOME || '', '.cargo/bin/cargo');
  if (process.env.CARGO) return process.env.CARGO;
  if (fs.existsSync(homeCargo)) return homeCargo;
  return 'cargo';
}

function findChromeBinary() {
  const candidates = [
    process.env.CHROME_BIN,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
  ].filter(Boolean);
  for (const bin of candidates) {
    if (bin.startsWith('/') && fs.existsSync(bin)) return bin;
  }
  return null;
}

let exportBuiltOnce = false;
function ensureFreshExportBinary() {
  if (exportBuiltOnce && fs.existsSync(exportBin)) return exportBin;
  const watchedInputs = [
    path.join(repoRoot, 'web/as3-net-sys.js'),
    path.join(repoRoot, 'web/as3-media-events.js'),
    path.join(repoRoot, 'web/loader.js'),
    path.join(repoRoot, 'web/runtime.js'),
    path.join(repoRoot, 'crates/neoflash-core/src/as3_net_sys/file_print_sampler.rs'),
    path.join(repoRoot, 'crates/neoflash-export/src/lib.rs'),
  ];
  if (fs.existsSync(exportBin)) {
    const binMtime = fs.statSync(exportBin).mtimeMs;
    const newestInput = Math.max(
      ...watchedInputs.map(p => (fs.existsSync(p) ? fs.statSync(p).mtimeMs : 0)),
    );
    if (binMtime >= newestInput) {
      exportBuiltOnce = true;
      return exportBin;
    }
  }
  const cargo = findCargoBinary();
  execFileSync(cargo, ['build', '--quiet', '--locked', '-p', 'neoflash-export', '--bin', 'neoflash-export'], {
    cwd: repoRoot,
    stdio: 'inherit',
    env: {
      ...process.env,
      PATH: `${path.join(process.env.HOME || '', '.cargo/bin')}:/opt/homebrew/bin:/usr/local/bin:${process.env.PATH || ''}`,
    },
  });
  exportBuiltOnce = true;
  return exportBin;
}

let wasmPromise = null;
function loadWasmRuntime() {
  if (!wasmPromise) {
    wasmPromise = (async () => {
      const jsPath = path.join(runtimeDir, 'neoflash_web.js');
      const wasmPath = path.join(runtimeDir, 'neoflash_web_bg.wasm');
      const sandbox = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Uint8Array });
      vm.runInContext(fs.readFileSync(jsPath, 'utf8'), sandbox);
      sandbox.wasmBytes = fs.readFileSync(wasmPath);
      return vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', sandbox);
    })();
  }
  return wasmPromise;
}

async function until(predicate, label, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate() && Date.now() < deadline) {
    await new Promise(r => setTimeout(r, 12));
  }
  assert.ok(predicate(), `Timed out waiting for: ${label}`);
}

function createBacklogHost(wasm, customOptions = {}) {
  const traces = [];
  const binary = Binary.create({ wasm });
  const eventFamilies = Events.createFamilies(wasm, {
    isByteArray: v => binary.ByteArray.__nfIs(v),
    requestRender: () => {},
  });
  const events = { ...Events, ...eventFamilies };

  const dummyProject = {
    format_version: 6,
    name: 'Backlog Host',
    width: 640,
    height: 480,
    fps: 24,
    background: [0, 0, 0, 255],
    root: 'root',
    resources: [],
    symbols: [
      {
        id: 'root',
        name: 'Root',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [{ id: 'l1', name: 'L1', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
          scripts: [],
        },
      },
    ],
  };

  const player = new wasm.WebPlayer(JSON.stringify(dummyProject));
  const amf = AMF.create({
    encode: (format, graph) => player.amf_encode(format, graph),
    decode: (format, bytes) => player.amf_decode(format, bytes),
    binary,
  });
  binary.setObjectCodec(amf);

  const storeMap = new Map();
  const mockLocalStorage = {
    getItem: k => (storeMap.has(k) ? storeMap.get(k) : null),
    setItem: (k, v) => storeMap.set(k, String(v)),
    removeItem: k => storeMap.delete(k),
  };
  const storage = Storage.create({
    amf,
    events,
    storage: mockLocalStorage,
    url: 'https://app.neoflash.local/main.swf',
    channelName: 'nf.t460.rso.test',
  });
  const net = Net.create({
    wasm,
    events,
    binary,
    baseURL: 'https://app.neoflash.local/main.swf',
    resources: [],
  });
  const xml = XML.create({
    parseLegacy: src => wasm.parse_as2_xml_json(src),
    namespaceLookup: (anc, val, rev) =>
      JSON.parse(wasm.as2_xml_namespace_json(JSON.stringify(anc), val, rev)),
    parse: src => wasm.parse_xml_json(src),
    net,
  });
  const external = External.create({
    hostObject: {},
    hostGlobal: {},
    onFscommand: () => {},
  });
  const video = Video.create({
    player,
    wasm,
    events,
    net,
    binary,
    schedule: fn => queueMicrotask(fn),
    createCanvas: (w, h) => ({
      width: w,
      height: h,
      getContext: () => ({
        fillStyle: '#000',
        fillRect() {},
        drawImage() {},
        clearRect() {},
        save() {},
        restore() {},
        getImageData: () => ({ data: new Uint8ClampedArray(w * h * 4) }),
        putImageData() {},
      }),
    }),
    isUserGesture: () => true,
  });

  let mediaEvents = null;
  const builtinsBag = {
    Object,
    Array,
    String,
    Number,
    Boolean,
    XML: xml.XML,
    XMLList: xml.XMLList,
    QName: xml.QName,
    Namespace: xml.Namespace,
    __nfXML: xml,
    'flash.events.Event': events.Event,
    'flash.events.ProgressEvent': net.ProgressEvent || events.ProgressEvent,
    'flash.events.IOErrorEvent': net.IOErrorEvent || events.IOErrorEvent,
    'flash.events.SecurityErrorEvent': events.SecurityErrorEvent,
    'flash.events.HTTPStatusEvent': net.HTTPStatusEvent || events.HTTPStatusEvent,
    'flash.events.DataEvent': events.DataEvent,
    'flash.events.StatusEvent': events.StatusEvent,
    'flash.events.NetStatusEvent': video.NetStatusEvent,
    'flash.net.NetConnection': video.NetConnection,
    'flash.net.NetStream': video.NetStream,
    'flash.net.URLRequest': net.URLRequest,
    'flash.utils.ByteArray': binary.ByteArray,
    'flash.utils.Endian': binary.Endian,
  };

  mediaEvents = NeoFlashAS3MediaEvents.create({
    wasm,
    events,
    binary,
    net,
    getDRMContentData: () => netSys?.builtins?.['flash.net.drm.DRMContentData'] || null,
  });
  Object.assign(builtinsBag, mediaEvents.builtins);

  const netSys = NeoFlashAS3NetSys.create({
    wasm,
    events,
    binary,
    amf,
    net,
    video,
    storage,
    xml,
    external,
    url: 'https://app.neoflash.local/main.swf',
    onWorkerTrace: text => traces.push(String(text)),
    trace: (...args) => traces.push(args.join(' ')),
    getBuiltins: () => builtinsBag,
    ...customOptions,
  });
  Object.assign(builtinsBag, netSys.builtins);

  return {
    wasm,
    player,
    events,
    binary,
    amf,
    net,
    storage,
    video,
    netSys,
    mediaEvents,
    builtins: builtinsBag,
    traces,
    dispose() {
      try { mediaEvents?.dispose?.(); } catch (_) {}
      try { netSys.dispose(); } catch (_) {}
      try { storage.dispose?.(); } catch (_) {}
      try { net.dispose?.(); } catch (_) {}
      try { video.dispose?.(); } catch (_) {}
      try { player.free?.(); } catch (_) {}
    },
  };
}

// Helper to build a real binary AVM2 FWS SWF (`DoABC2` + `SymbolClass`) for WorkerDomain.createWorker
function buildAvm2WorkerSwfBytes() {
  const writeU30 = (arr, val) => {
    let v = val >>> 0;
    do {
      let b = v & 0x7f;
      v >>>= 7;
      if (v !== 0) b |= 0x80;
      arr.push(b);
    } while (v !== 0);
  };
  const makeTag = (code, payload) => {
    const p = Buffer.from(payload);
    if (p.length < 0x3f) {
      const h = Buffer.alloc(2);
      h.writeUInt16LE((code << 6) | p.length, 0);
      return Buffer.concat([h, p]);
    }
    const h = Buffer.alloc(6);
    h.writeUInt16LE((code << 6) | 0x3f, 0);
    h.writeUInt32LE(p.length, 2);
    return Buffer.concat([h, p]);
  };

  const abc = [16, 0, 46, 0];
  writeU30(abc, 4); // ints: [1]=100, [2]=777, [3]=1000
  writeU30(abc, 100);
  writeU30(abc, 777);
  writeU30(abc, 1000);
  writeU30(abc, 1); // uints
  writeU30(abc, 1); // doubles

  const strs = [
    '',                         // 0
    '',                         // 1: ""
    'flash.display',            // 2
    'flash.system',             // 3
    'WorkerDoc',                // 4
    'Sprite',                   // 5
    'Worker',                   // 6
    'current',                  // 7
    'isPrimordial',             // 8
    'getSharedProperty',        // 9
    'sharedBuf',                // 10
    'inCh',                     // 11
    'outCh',                    // 12
    'atomicCompareAndSwapIntAt',// 13
    'receive',                  // 14
    'send',                     // 15
    'terminate',                // 16
    'factor',                   // 17
    'answer',                   // 18
    'workerIsPrimordial',       // 19
    'prevCas',                  // 20
    'trace',                    // 21
    'AVM2_BG_WORKER_TRACE_OK',  // 22
  ];
  writeU30(abc, strs.length);
  for (let i = 1; i < strs.length; i++) {
    const b = Buffer.from(strs[i], 'utf8');
    writeU30(abc, b.length);
    for (const c of b) abc.push(c);
  }
  // namespaces: 1=Package(""), 2=Package("flash.display"), 3=Package("flash.system")
  writeU30(abc, 4);
  abc.push(0x16); writeU30(abc, 1);
  abc.push(0x16); writeU30(abc, 2);
  abc.push(0x16); writeU30(abc, 3);
  writeU30(abc, 1); // ns_sets

  // multinames:
  const mns = [
    [0x07, 1, 4],  // 1: WorkerDoc
    [0x07, 2, 5],  // 2: flash.display.Sprite
    [0x07, 3, 6],  // 3: flash.system.Worker
    [0x07, 1, 7],  // 4: current
    [0x07, 1, 8],  // 5: isPrimordial
    [0x07, 1, 9],  // 6: getSharedProperty
    [0x07, 1, 13], // 7: atomicCompareAndSwapIntAt
    [0x07, 1, 14], // 8: receive
    [0x07, 1, 15], // 9: send
    [0x07, 1, 16], // 10: terminate
    [0x07, 1, 17], // 11: factor
    [0x07, 1, 18], // 12: answer
    [0x07, 1, 19], // 13: workerIsPrimordial
    [0x07, 1, 20], // 14: prevCas
    [0x07, 1, 21], // 15: trace
  ];
  writeU30(abc, 1 + mns.length);
  for (const [k, ns, nm] of mns) {
    abc.push(k); writeU30(abc, ns); writeU30(abc, nm);
  }

  // methods: 0=cinit, 1=iinit, 2=script_init
  writeU30(abc, 3);
  for (let m = 0; m < 3; m++) {
    writeU30(abc, 0); writeU30(abc, 0); writeU30(abc, 0); abc.push(0);
  }
  writeU30(abc, 0); // metadata
  writeU30(abc, 1); // 1 class: WorkerDoc : Sprite
  writeU30(abc, 1); writeU30(abc, 2); abc.push(0); writeU30(abc, 0); writeU30(abc, 1); writeU30(abc, 0);
  writeU30(abc, 0); writeU30(abc, 0); // cinit=0
  writeU30(abc, 1); writeU30(abc, 2); writeU30(abc, 0); // script init=2

  // WorkerDoc.iinit (method 1):
  // local1 = Worker.current
  // local2 = local1.getSharedProperty("sharedBuf")
  // local3 = local2.atomicCompareAndSwapIntAt(0, 100, 777)
  // local4 = local1.getSharedProperty("inCh").receive(true)  <-- blocks on Atomics.wait!
  // trace("AVM2_BG_WORKER_TRACE_OK")
  // local1.getSharedProperty("outCh").send({ answer: local4.factor * 7 + 1000, workerIsPrimordial: local1.isPrimordial, prevCas: local3 })
  // local1.terminate()
  const iinit = [
    0xd0, 0x30,
    0xd0, 0x49, 0,
    // local1 = Worker.current
    0x60, 3, 0x66, 4, 0xd5,
    // local2 = local1.getSharedProperty("sharedBuf")
    0xd1, 0x2c, 10, 0x46, 6, 1, 0xd6,
    // local3 = local2.atomicCompareAndSwapIntAt(0, 100, 777)
    0xd2, 0x24, 0, 0x2d, 1, 0x2d, 2, 0x46, 7, 3, 0xd7,
    // local4 = local1.getSharedProperty("inCh").receive(true)
    0xd1, 0x2c, 11, 0x46, 6, 1, 0x26, 0x46, 8, 1, 0x63, 4,
    // trace("AVM2_BG_WORKER_TRACE_OK")
    0x5d, 15, 0x2c, 22, 0x4f, 15, 1,
    // outCh.send({ answer: local4.factor * 7 + 1000, workerIsPrimordial: local1.isPrimordial, prevCas: local3 })
    0xd1, 0x2c, 12, 0x46, 6, 1,
    0x2c, 18, 0x62, 4, 0x66, 11, 0x24, 7, 0xa2, 0x2d, 3, 0xa0,
    0x2c, 19, 0xd1, 0x66, 5,
    0x2c, 20, 0xd3,
    0x55, 3, // newobject(3)
    0x4f, 9, 1, // callpropvoid send(1)
    // local1.terminate()
    0xd1, 0x4f, 10, 0,
    0x47,
  ];

  writeU30(abc, 1); // 1 method body
  writeU30(abc, 1); // method 1
  writeU30(abc, 10); // max_stack
  writeU30(abc, 6); // local_count
  writeU30(abc, 0);
  writeU30(abc, 4);
  writeU30(abc, iinit.length);
  for (const b of iinit) abc.push(b);
  writeU30(abc, 0);
  writeU30(abc, 0);

  const symPayload = Buffer.concat([
    Buffer.from([1, 0, 0, 0]), // count=1, tagId=0 (root)
    Buffer.from('WorkerDoc\0', 'utf8'),
  ]);
  const tags = Buffer.concat([
    makeTag(69, Buffer.from([0x08, 0, 0, 0])), // FileAttributes (AS3)
    makeTag(82, Buffer.concat([Buffer.from([1, 0, 0, 0, ...Buffer.from('wk\0', 'utf8')]), Buffer.from(abc)])),
    makeTag(76, symPayload),
    makeTag(1, Buffer.alloc(0)),
    Buffer.from([0, 0]),
  ]);
  // Standard 15-bit RECT(0, 11000, 0, 8000) [9 bytes] + fps(24.0) [2 bytes] + frames(1) [2 bytes]
  const rectAndHdr = Buffer.from([0x78, 0x00, 0x05, 0x5f, 0x00, 0x00, 0x0f, 0xa0, 0x00, 0x00, 24, 0x01, 0x00]);
  const totalLen = 8 + rectAndHdr.length + tags.length;
  const swfHdr = Buffer.alloc(8);
  swfHdr.write('FWS', 0, 'ascii');
  swfHdr[3] = 14;
  swfHdr.writeUInt32LE(totalLen, 4);
  return Buffer.concat([swfHdr, rectAndHdr, tags]);
}

// Build binary Flash Access DRMContentData (`FADRM\x01` TLV header)
function buildBinaryFadrmHeader({
  serverUrl,
  licenseId,
  domain = '',
  authMethod = 'usernameAndPassword',
  policyId = 'hd-offline-policy',
  accessList = [],
}) {
  const chunks = [Buffer.from('FADRM\x01', 'ascii')];
  const pushStr = (tag, s) => {
    if (!s) return;
    const b = Buffer.from(String(s), 'utf8');
    const hdr = Buffer.alloc(3);
    hdr[0] = tag;
    hdr.writeUInt16BE(b.length, 1);
    chunks.push(hdr, b);
  };
  pushStr(0x01, serverUrl);
  pushStr(0x02, licenseId);
  pushStr(0x03, domain);
  pushStr(0x04, authMethod);
  pushStr(0x05, policyId);
  const firstAcc = accessList[0] || null;
  if (firstAcc?.displayName) pushStr(0x06, firstAcc.displayName);
  if (firstAcc?.deviceGroup?.name) pushStr(0x07, firstAcc.deviceGroup.name);
  return Buffer.concat(chunks);
}

// ============================================================================
// 1. Worker & WorkerDomain — Real Background OS Thread Execution
// ============================================================================
test('T-460 Family 2: Worker & WorkerDomain execute AVM2 SWF bytecode and NeoFlash AS3 self-workers on real background OS threads', async () => {
  const wasm = await loadWasmRuntime();
  const h = createBacklogHost(wasm);
  try {
    const Worker = h.builtins['flash.system.Worker'];
    const WorkerDomain = h.builtins['flash.system.WorkerDomain'];
    const WorkerState = h.builtins['flash.system.WorkerState'];
    const ByteArray = h.binary.ByteArray;

    // 1. Direct instantiation throws ArgumentError #2012
    assert.throws(
      () => new Worker(),
      err => err && err.name === 'ArgumentError' && err.errorID === 2012,
    );
    assert.throws(
      () => new WorkerDomain(),
      err => err && err.name === 'ArgumentError' && err.errorID === 2012,
    );

    assert.equal(Worker.isSupported, true);
    assert.equal(WorkerDomain.isSupported, true);
    assert.equal(Worker.current.isPrimordial, true);
    assert.equal(Worker.current.state, WorkerState.RUNNING);
    assert.equal(WorkerDomain.current.listWorkers().length, 1);
    assert.equal(WorkerDomain.current.listWorkers()[0], Worker.current);

    // 2. Real AVM2 SWF bytecode worker executing in Node worker_threads with Atomics.wait + shareable ByteArray CAS
    const swfBuf = buildAvm2WorkerSwfBytes();
    const swfBa = h.binary.fromBytes(swfBuf);

    const sharedBa = new ByteArray();
    sharedBa.endian = 'littleEndian';
    sharedBa.shareable = true;
    sharedBa.writeInt(100);
    sharedBa.position = 0;

    const bgWorker = WorkerDomain.current.createWorker(swfBa);
    assert.equal(bgWorker.isPrimordial, false);
    assert.equal(bgWorker.state, WorkerState.NEW);
    assert.equal(WorkerDomain.current.listWorkers().length, 1, 'NEW worker is not yet in listWorkers() per Adobe spec');

    const toBg = Worker.current.createMessageChannel(bgWorker);
    const fromBg = bgWorker.createMessageChannel(Worker.current);
    bgWorker.setSharedProperty('sharedBuf', sharedBa);
    bgWorker.setSharedProperty('inCh', toBg);
    bgWorker.setSharedProperty('outCh', fromBg);

    const states = [];
    let workerReply = null;
    bgWorker.addEventListener('workerState', () => states.push(bgWorker.state));
    fromBg.addEventListener('channelMessage', () => {
      workerReply = fromBg.receive();
    });

    bgWorker.start();
    await until(() => states.includes(WorkerState.RUNNING), 'AVM2 background worker entered RUNNING');
    assert.equal(WorkerDomain.current.listWorkers().length, 2, 'RUNNING worker is included in listWorkers()');

    // Delay sending by 35ms so the background OS thread genuinely blocks inside MessageChannel.receive(true) (Atomics.wait)!
    await new Promise(r => setTimeout(r, 35));
    toBg.send({ factor: 6 });

    await until(() => workerReply !== null && states.includes(WorkerState.TERMINATED), 'AVM2 background worker replied and self-terminated');
    assert.deepEqual(workerReply, {
      answer: 1042,
      workerIsPrimordial: false,
      prevCas: 100,
    });
    assert.ok(h.traces.includes('AVM2_BG_WORKER_TRACE_OK'), 'Background worker trace() relayed to primordial host');

    sharedBa.position = 0;
    assert.equal(sharedBa.readInt(), 777, 'Background worker atomicCompareAndSwapIntAt mutated SharedArrayBuffer ByteArray');
    assert.equal(WorkerDomain.current.listWorkers().length, 1, 'Terminated worker removed from WorkerDomain.listWorkers()');
    assert.equal(bgWorker.terminate(), false, 'terminate() on already-terminated worker returns false');

    // 3. Real NeoFlash AS3 self-worker (`WorkerDomain.current.createWorker(this.loaderInfo.bytes)`)
    ensureFreshExportBinary();
    const workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-t460-selfwk-'));
    try {
      const selfWorkerProject = {
        format_version: 6,
        name: 'SelfWorkerCheck',
        width: 200,
        height: 150,
        fps: 24,
        background: [0, 0, 0, 255],
        root: 'root',
        document_class: 'SelfWorkerMain',
        resources: [],
        sources: [
          {
            id: 'src-self-wk',
            path: 'SelfWorkerMain.as',
            language: 'action_script3',
            source: `package {
  import flash.display.MovieClip;
  import flash.events.Event;
  import flash.system.MessageChannel;
  import flash.system.Worker;
  import flash.system.WorkerDomain;
  import flash.system.WorkerState;

  public class SelfWorkerMain extends MovieClip {
    private var child:Worker;
    private var toChild:MessageChannel;
    private var fromChild:MessageChannel;

    public function SelfWorkerMain() {
      if (Worker.current.isPrimordial) {
        child = WorkerDomain.current.createWorker(this.loaderInfo.bytes);
        toChild = Worker.current.createMessageChannel(child);
        fromChild = child.createMessageChannel(Worker.current);
        child.setSharedProperty("cmdIn", toChild);
        child.setSharedProperty("cmdOut", fromChild);
        child.setSharedProperty("seed", 9);
        fromChild.addEventListener(Event.CHANNEL_MESSAGE, function(e:Event):void {
          var computed:int = int(fromChild.receive());
          var stopped:Boolean = child.terminate();
          trace("SELF_WORKER_RESULT=" + computed + ":" + stopped);
        });
        child.addEventListener(Event.WORKER_STATE, function(e:Event):void {
          if (child.state == WorkerState.RUNNING) {
            toChild.send(21);
          }
        });
        child.start();
      } else {
        var rx:MessageChannel = Worker.current.getSharedProperty("cmdIn") as MessageChannel;
        var tx:MessageChannel = Worker.current.getSharedProperty("cmdOut") as MessageChannel;
        var seed:int = int(Worker.current.getSharedProperty("seed"));
        var input:int = int(rx.receive(true));
        trace("BG_SELF_BRANCH=" + Worker.current.isPrimordial);
        tx.send(input * 2 + seed);
      }
    }
  }
}`,
          },
        ],
        symbols: [
          {
            id: 'root',
            name: 'Root',
            kind: 'movie_clip',
            class_name: 'SelfWorkerMain',
            timeline: {
              frame_count: 1,
              layers: [{ id: 'l0', name: 'L0', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
              scripts: [],
            },
          },
        ],
      };

      const projFile = path.join(workDir, 'self-worker.nflash');
      const outDir = path.join(workDir, 'pub');
      fs.writeFileSync(projFile, JSON.stringify(selfWorkerProject));
      execFileSync(exportBin, ['--runtime', runtimeDir, projFile, '--output', outDir], { cwd: repoRoot, stdio: 'pipe' });

      const bundleSandbox = vm.createContext({ TextEncoder, TextDecoder, URL, Uint8Array });
      vm.runInContext(fs.readFileSync(path.join(outDir, 'bundle.js'), 'utf8'), bundleSandbox);
      const bundle = vm.runInContext('NeoFlashBundle', bundleSandbox);

      const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
      const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
      const swPlayer = new runtimeWasm.WebPlayer(JSON.stringify(bundle.project));
      const { host: swHost, traceCalls } = createRuntimeHost(repoRoot, swPlayer, [], {
        project: bundle.project,
        bundle,
      });
      try {
        swHost.attachNewInstances();
        swHost.dispatchScripts();
        await until(
          () => traceCalls.some(args => args.join(' ').includes('SELF_WORKER_RESULT=51:true')),
          'Self-worker executed non-primordial branch and returned 21*2+9 = 51',
        );
        assert.ok(
          traceCalls.some(args => args.join(' ').includes('BG_SELF_BRANCH=false')),
          'Self-worker background branch saw isPrimordial === false',
        );
      } finally {
        swHost.disposeFamily();
      }
    } finally {
      fs.rmSync(workDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    }
  } finally {
    h.dispose();
  }
});

// ============================================================================
// 2. SystemUpdater & SystemUpdaterType over Live Local HTTP Server
// ============================================================================
test('T-460 Family 3: SystemUpdater & SystemUpdaterType stream packages over HTTP, enforce single-update lock, support AbortController cancel(), and map Adobe status/error codes', async () => {
  const wasm = await loadWasmRuntime();

  const pkgChunk1 = Buffer.alloc(16384, 0x41);
  const pkgChunk2 = Buffer.alloc(16384, 0x42);
  const pkgChunk3 = Buffer.alloc(16384, 0x43);
  const fullPkg = Buffer.concat([pkgChunk1, pkgChunk2, pkgChunk3]);
  const validSha256 = crypto.createHash('sha256').update(fullPkg).digest('hex');

  let releaseSlowStream = null;
  const observedQueries = [];

  const updateSockets = new Set();
  const server = http.createServer((req, res) => {
    const u = new URL(req.url || '/', 'http://127.0.0.1');
    observedQueries.push(`${u.pathname}?${u.searchParams.toString()}`);

    if (u.pathname === '/pkg-ok') {
      res.writeHead(200, {
        'Content-Type': 'application/octet-stream',
        'Content-Length': String(fullPkg.length),
        'X-Package-SHA256': validSha256,
      });
      res.write(pkgChunk1);
      setTimeout(() => {
        res.write(pkgChunk2);
        setTimeout(() => {
          res.end(pkgChunk3);
        }, 15);
      }, 15);
      return;
    }
    if (u.pathname === '/pkg-slow') {
      res.writeHead(200, {
        'Content-Type': 'application/octet-stream',
        'Content-Length': '65536',
      });
      res.write(Buffer.alloc(4096, 0x55));
      releaseSlowStream = () => {
        try { res.end(); } catch (_) {}
      };
      req.on('close', () => { releaseSlowStream = null; });
      return;
    }
    if (u.pathname === '/pkg-bad-hash') {
      res.writeHead(200, {
        'Content-Type': 'application/octet-stream',
        'Content-Length': '32',
        'X-Package-SHA256': '0'.repeat(64),
      });
      res.end(Buffer.alloc(32, 0x99));
      return;
    }
    if (u.pathname === '/status-404') {
      res.writeHead(404);
      res.end('Not currently available');
      return;
    }
    if (u.pathname === '/status-422') {
      res.writeHead(422);
      res.end('Unsupported platform');
      return;
    }
    if (u.pathname === '/status-426') {
      res.writeHead(426);
      res.end('Incompatible');
      return;
    }
    if (u.pathname === '/status-403') {
      res.writeHead(403);
      res.end('Forbidden');
      return;
    }
    if (u.pathname === '/pkg-drop') {
      req.socket.destroy();
      return;
    }
    res.writeHead(500);
    res.end('Server failure');
  });

  server.on('connection', sock => {
    updateSockets.add(sock);
    sock.on('close', () => updateSockets.delete(sock));
  });
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  const port = server.address().port;
  const h = createBacklogHost(wasm);

  try {
    const SystemUpdater = h.builtins['flash.system.SystemUpdater'];
    const SystemUpdaterType = h.builtins['flash.system.SystemUpdaterType'];
    assert.equal(SystemUpdaterType.SYSTEM, 'system');
    assert.equal(SystemUpdaterType.DRM, 'drm');

    // 1. Invalid SystemUpdaterType throws ArgumentError #2004; idle cancel() is a no-op
    const u1 = new SystemUpdater();
    let idleCancelFired = false;
    u1.addEventListener('cancel', () => { idleCancelFired = true; });
    u1.cancel();
    await new Promise(r => setTimeout(r, 15));
    assert.equal(idleCancelFired, false, 'cancel() when no update is active does not dispatch Event.CANCEL');
    assert.throws(
      () => u1.update('bogus'),
      err => err && err.name === 'ArgumentError' && err.errorID === 2004,
    );

    // 2. Chunked HTTP 200 download + concurrent update lock (#2037) + SHA-256 check
    u1._endpointUrl = `http://127.0.0.1:${port}/pkg-ok`;
    const u1Log = [];
    const u1Progress = [];
    u1.addEventListener('open', () => u1Log.push('open'));
    u1.addEventListener('progress', e => u1Progress.push([e.bytesLoaded, e.bytesTotal]));
    u1.addEventListener('complete', () => u1Log.push('complete'));

    u1.update(SystemUpdaterType.SYSTEM);

    // Concurrent update() on either the same or a second SystemUpdater throws IllegalOperationError #2037!
    const u2 = new SystemUpdater();
    u2._endpointUrl = `http://127.0.0.1:${port}/pkg-ok`;
    assert.throws(
      () => u2.update(SystemUpdaterType.DRM),
      err => err && err.name === 'IllegalOperationError' && err.errorID === 2037,
    );

    await until(() => u1Log.includes('complete'), 'SystemUpdater chunked HTTP download completed');
    assert.deepEqual(u1Log, ['open', 'complete']);
    assert.ok(u1Progress.length >= 2, 'Received incremental progress events across HTTP chunks');
    assert.deepEqual(u1Progress[u1Progress.length - 1], [fullPkg.length, fullPkg.length]);
    assert.ok(observedQueries.includes('/pkg-ok?type=system'), 'SystemUpdater forwarded ?type=system query');

    // 3. Mid-stream cancel() aborts HTTP fetch via AbortController and dispatches Event.CANCEL
    const uCancel = new SystemUpdater();
    uCancel._endpointUrl = `http://127.0.0.1:${port}/pkg-slow`;
    const cancelEvents = [];
    uCancel.addEventListener('open', () => cancelEvents.push('open'));
    uCancel.addEventListener('progress', () => {
      cancelEvents.push('progress');
      uCancel.cancel();
    });
    uCancel.addEventListener('cancel', () => cancelEvents.push('cancel'));
    uCancel.addEventListener('complete', () => cancelEvents.push('complete'));
    uCancel.addEventListener('ioError', () => cancelEvents.push('ioError'));

    uCancel.update(SystemUpdaterType.DRM);
    await until(() => cancelEvents.includes('cancel'), 'Mid-stream cancel() dispatched Event.CANCEL');
    await new Promise(r => setTimeout(r, 25));
    assert.equal(cancelEvents.includes('complete'), false);
    assert.equal(cancelEvents.includes('ioError'), false);
    releaseSlowStream?.();

    // 4. Adobe StatusEvent codes (bad SHA-256, 404, 422, 426), SecurityErrorEvent (403 & ftp://), IOErrorEvent (dropped socket)
    const runSingleOutcome = async (url, expectedEvent, expectedCode = null) => {
      const u = new SystemUpdater();
      u._endpointUrl = url;
      let got = null;
      for (const evType of ['status', 'securityError', 'ioError', 'complete']) {
        u.addEventListener(evType, e => { got = e; });
      }
      u.update(SystemUpdaterType.DRM);
      await until(() => got !== null, `Outcome for ${url}`);
      assert.equal(got.type, expectedEvent);
      if (expectedCode !== null) {
        assert.equal(got.code, expectedCode);
        assert.equal(got.level, 'error');
      }
    };

    await runSingleOutcome(`http://127.0.0.1:${port}/pkg-bad-hash`, 'status', 'DRM.UpdateFailed');
    await runSingleOutcome(`http://127.0.0.1:${port}/status-404`, 'status', 'DRM.UpdateFailedNotCurrentlyAvailable');
    await runSingleOutcome(`http://127.0.0.1:${port}/status-422`, 'status', 'DRM.UpdateFailedNotSupported');
    await runSingleOutcome(`http://127.0.0.1:${port}/status-426`, 'status', 'DRM.UpdateNeededButIncompatible');
    await runSingleOutcome(`http://127.0.0.1:${port}/status-403`, 'securityError');
    await runSingleOutcome('ftp://updates.example.org/flash.pkg', 'securityError');
    await runSingleOutcome(`http://127.0.0.1:${port}/pkg-drop`, 'ioError');
  } finally {
    h.dispose();
    for (const sock of updateSockets) {
      try { sock.destroy(); } catch (_) {}
    }
    server.closeAllConnections?.();
    await new Promise(r => server.close(r));
  }
});

// ============================================================================
// 3. DRMManager, DRMContentData, DRM Events & Encrypted NetStream (AES-128-CTR)
// ============================================================================
test('T-460 Family 4: DRMManager live HTTP license protocol, binary FADRM/CENC DRMContentData, DRM events, and FIPS-197 AES-128-CTR NetStream decryption', async () => {
  const wasm = await loadWasmRuntime();

  const serverLog = [];
  const validTokens = new Set();
  const registeredGroups = new Set();
  const revokedLicenses = new Set();

  const drmSockets = new Set();
  const drmServer = http.createServer((req, res) => {
    let raw = '';
    req.setEncoding('utf8');
    req.on('data', c => { raw += c; });
    req.on('end', () => {
      const pathOnly = (req.url || '/').split('?')[0];
      if (pathOnly === '/stalled-license') {
        // Intentionally never respond so DRMManager.networkIdleTimeout aborts via AbortController!
        return;
      }
      let body = {};
      try { body = JSON.parse(raw || '{}'); } catch (_) {}
      const authHdr = req.headers.authorization || '';
      serverLog.push({ path: pathOnly, action: body.action, authHdr, body });

      res.setHeader('Content-Type', 'application/json');

      if (body.action === 'authenticate') {
        if (body.username === 'alice' && body.password === 's3cr3t') {
          const tokB64 = Buffer.from(`tok_${body.domain}_${serverLog.length}`, 'utf8').toString('base64');
          validTokens.add(tokB64);
          res.writeHead(200);
          res.end(JSON.stringify({ tokenBase64: tokB64 }));
        } else {
          res.writeHead(401);
          res.end(JSON.stringify({ error: 'Invalid credentials', errorID: 3305, subErrorID: 401 }));
        }
        return;
      }

      if (body.action === 'loadVoucher' || body.action === 'loadPreviewVoucher') {
        const bearer = authHdr.startsWith('Bearer ') ? authHdr.slice(7).trim() : '';
        if (body.authenticationMethod === 'usernameAndPassword' && !validTokens.has(bearer)) {
          res.writeHead(401);
          res.end(JSON.stringify({ error: 'Authentication token required', errorID: 3305, subErrorID: 401 }));
          return;
        }
        if (revokedLicenses.has(body.licenseID)) {
          res.writeHead(403);
          res.end(JSON.stringify({ error: 'License revoked', errorID: 3305, subErrorID: 403 }));
          return;
        }
        const now = Date.now();
        res.writeHead(200);
        res.end(JSON.stringify({
          voucher: {
            licenseID: body.licenseID,
            policyID: body.preview ? 'preview-policy' : 'hd-offline-policy',
            serverURL: `http://127.0.0.1:${drmPort}${pathOnly}`,
            startMs: now - 5000,
            endMs: now + 7_200_000,
            offlineStartMs: body.preview ? 0 : now - 1000,
            offlineEndMs: body.preview ? 0 : now + 86_400_000,
            windowPeriodSec: 3600,
            policies: { allowHD: true, tier: body.preview ? 'preview' : 'full' },
          },
        }));
        return;
      }

      if (body.action === 'returnVoucher') {
        if (body.policyID === 'fail-policy') {
          res.writeHead(400);
          res.end(JSON.stringify({ error: 'Policy return rejected', errorID: 3307, subErrorID: 400 }));
          return;
        }
        if (body.licenseID) revokedLicenses.add(body.licenseID);
        res.writeHead(200);
        res.end(JSON.stringify({ count: 1 }));
        return;
      }

      if (body.action === 'addToDeviceGroup') {
        if (body.name === 'blocked-group') {
          res.writeHead(403);
          res.end(JSON.stringify({ error: 'Group limit reached', errorID: 3315, subErrorID: 403, drmUpdateNeeded: true }));
          return;
        }
        registeredGroups.add(body.name);
        res.writeHead(200);
        res.end(JSON.stringify({ ok: true }));
        return;
      }

      if (body.action === 'removeFromDeviceGroup') {
        registeredGroups.delete(body.name);
        res.writeHead(200);
        res.end(JSON.stringify({ ok: true }));
        return;
      }

      res.writeHead(404);
      res.end(JSON.stringify({ error: 'Unknown action' }));
    });
  });

  drmServer.on('connection', sock => {
    drmSockets.add(sock);
    sock.on('close', () => drmSockets.delete(sock));
  });
  await new Promise(r => drmServer.listen(0, '127.0.0.1', r));
  const drmPort = drmServer.address().port;
  const baseUrl = `http://127.0.0.1:${drmPort}/license`;
  const altUrl = `http://127.0.0.1:${drmPort}/alt-license`;

  const h = createBacklogHost(wasm);
  try {
    const DRMManager = h.builtins['flash.net.drm.DRMManager'];
    const DRMContentData = h.builtins['flash.net.drm.DRMContentData'];
    const DRMDeviceGroup = h.builtins['flash.net.drm.DRMDeviceGroup'];
    const AuthenticationMethod = h.builtins['flash.net.drm.AuthenticationMethod'];
    const LoadVoucherSetting = h.builtins['flash.net.drm.LoadVoucherSetting'];
    const VoucherAccessInfo = h.builtins['flash.net.drm.VoucherAccessInfo'];
    const DRMMetadataEvent = h.builtins['flash.events.DRMMetadataEvent'];
    const DRMLicenseRequestEvent = h.builtins['flash.events.DRMLicenseRequestEvent'];

    assert.equal(DRMManager.isSupported, true);
    assert.throws(() => new DRMManager(), /cannot be instantiated directly/i);
    const dm = DRMManager.getDRMManager();
    assert.equal(dm, DRMManager.getDRMManager(), 'getDRMManager() returns singleton');

    // 1. Binary FADRM\x01 TLV + ISO-BMFF pssh DRMContentData & DRMMetadataEvent(ByteArray)
    assert.throws(() => new DRMContentData(h.binary.fromBytes(Buffer.alloc(0))), /ArgumentError/);
    const fadrmBuf = buildBinaryFadrmHeader({
      serverUrl: baseUrl,
      fmrmsUrl: `${baseUrl}/FMRMS`,
      licenseId: 'lic-movie-101',
      domain: 'studio-domain',
      authMethod: AuthenticationMethod.USERNAME_AND_PASSWORD,
      maxEmbeddedVersion: 29,
      accessList: [
        {
          authMethod: AuthenticationMethod.USERNAME_AND_PASSWORD,
          displayName: 'Studio Subscriber',
          domain: 'studio-domain',
          url: baseUrl,
          deviceGroup: {
            name: 'home-livingroom-group',
            serverURL: baseUrl,
            domain: 'studio-domain',
            authMethod: AuthenticationMethod.USERNAME_AND_PASSWORD,
          },
        },
      ],
    });
    const fadrmBa = h.binary.fromBytes(fadrmBuf);
    const cd = new DRMContentData(fadrmBa);
    assert.equal(cd.serverURL, baseUrl);
    assert.equal(cd.FMRMSURL, baseUrl, 'FMRMSURL aliases serverURL per Adobe AS3 Reference');
    assert.equal(cd.licenseID, 'lic-movie-101');
    assert.equal(cd.domain, 'studio-domain');
    assert.equal(cd.authenticationMethod, AuthenticationMethod.USERNAME_AND_PASSWORD);
    assert.ok(typeof cd.maxEmbeddedVersion === 'number' && cd.maxEmbeddedVersion >= 1);

    const accessInfos = cd.getVoucherAccessInfo();
    assert.equal(accessInfos.length, 1);
    assert.equal(accessInfos[0] instanceof VoucherAccessInfo, true);
    assert.equal(accessInfos[0].displayName, 'Studio Subscriber');
    assert.equal(accessInfos[0].policyID, 'hd-offline-policy');
    assert.equal(accessInfos[0].deviceGroup instanceof DRMDeviceGroup, true);
    assert.equal(accessInfos[0].deviceGroup.name, 'home-livingroom-group');
    assert.equal(accessInfos[0].deviceGroup.serverURL, baseUrl);
    assert.equal(accessInfos[0].deviceGroup.domain, 'studio-domain');

    // ISO-BMFF pssh CENC v1 box parsing with embedded protection data
    const psshPayload = Buffer.from(JSON.stringify({ serverURL: baseUrl, domain: 'cenc-dom' }), 'utf8');
    const psshBox = Buffer.alloc(12 + 16 + 4 + 16 + 4 + psshPayload.length);
    psshBox.writeUInt32BE(psshBox.length, 0);
    psshBox.write('pssh', 4, 'ascii');
    psshBox[8] = 1; // v1
    crypto.randomBytes(16).copy(psshBox, 12); // SystemID
    psshBox.writeUInt32BE(1, 28); // 1 KID
    Buffer.alloc(16, 0xab).copy(psshBox, 32); // KID -> kid-abab...
    psshBox.writeUInt32BE(psshPayload.length, 48);
    psshPayload.copy(psshBox, 52);
    const cencCd = new DRMContentData(h.binary.fromBytes(psshBox));
    assert.equal(cencCd.serverURL, baseUrl);
    assert.equal(cencCd.licenseID, 'kid-' + 'ab'.repeat(16));

    // DRMMetadataEvent parses raw ByteArray into DRMContentData
    const metaEv = new DRMMetadataEvent(DRMMetadataEvent.DRM_METADATA, false, false, h.binary.fromBytes(fadrmBuf), 12.5);
    assert.equal(metaEv.drmMetadata instanceof DRMContentData, true);
    assert.equal(metaEv.drmMetadata.licenseID, 'lic-movie-101');
    assert.equal(metaEv.timestamp, 12.5);
    assert.equal(metaEv.clone().drmMetadata.licenseID, 'lic-movie-101');

    // 2. Encrypted NetStream BEFORE authentication -> dispatches DRMMetadataEvent + onDRMContentData + DRMErrorEvent #3305
    const clearFlvBytes = new Uint8Array([
      0x46, 0x4c, 0x56, 0x01, 0x05, 0x00, 0x00, 0x00, 0x09, // FLV v1 header
      0x00, 0x00, 0x00, 0x00,                               // PrevTagSize0
      0x09, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Video tag (5 bytes)
      0x17, 0x00, 0x00, 0x00, 0x00,                         // Keyframe AVC header
      0x00, 0x00, 0x00, 0x10,
    ]);
    const encryptedStreamBa = DRMManager._packageEncryptedStream(cd, clearFlvBytes);
    const encRaw = h.binary.toBytes(encryptedStreamBa);
    assert.equal(Buffer.from(encRaw.subarray(0, 6)).toString('ascii'), 'NFDRM1');
    assert.equal(encRaw.includes(Buffer.from([0x46, 0x4c, 0x56, 0x01, 0x05])), false, 'Media payload is genuinely AES-128-CTR encrypted');

    const nc = new h.video.NetConnection();
    nc.connect(null);
    const ns = new h.video.NetStream(nc);
    let clientContentData = null;
    ns.client = {
      onMetaData() {},
      onDRMContentData(data) { clientContentData = data; },
    };
    const nsMetaEvents = [];
    const nsStatusEvents = [];
    const nsErrorEvents = [];
    ns.addEventListener('drmMetadata', e => nsMetaEvents.push(e));
    ns.addEventListener('drmStatus', e => nsStatusEvents.push(e));
    ns.addEventListener('drmError', e => nsErrorEvents.push(e));
    ns.play(null);

    encryptedStreamBa.position = 0;
    ns.appendBytes(encryptedStreamBa);
    await until(() => nsErrorEvents.length === 1 && nsMetaEvents.length === 1, 'Unauthenticated NetStream rejected encrypted media with DRMErrorEvent');
    assert.equal(clientContentData?.licenseID, 'lic-movie-101');
    assert.equal(nsErrorEvents[0].errorID, 3305);
    assert.equal(ns._lastDecryptedBytes || null, null);

    // 3. Queued FIFO authenticate() (failure + success) & setAuthenticationToken(..., null) clearing
    const authOutcomes = [];
    dm.addEventListener('authenticationError', e => authOutcomes.push(`err:${e.subErrorID}:${e.domain}`));
    dm.addEventListener('authenticationComplete', e => authOutcomes.push(`ok:${e.domain}:${e.token.length > 0}`));

    dm.authenticate(baseUrl, 'studio-domain', 'alice', 'wrong-pass');
    dm.authenticate(baseUrl, 'studio-domain', 'alice', 's3cr3t');
    await until(() => authOutcomes.length === 2, 'Queued FIFO authenticate() calls settled in order');
    assert.deepEqual(authOutcomes, ['err:401:studio-domain', 'ok:studio-domain:true']);

    // Clear token via null -> loadVoucher(FORCE_REFRESH) must fail with 401!
    dm.setAuthenticationToken(baseUrl, 'studio-domain', null);
    let unauthErr = null;
    const onUnauthErr = e => { unauthErr = e; };
    dm.addEventListener('drmError', onUnauthErr);
    dm.loadVoucher(cd, LoadVoucherSetting.FORCE_REFRESH);
    await until(() => unauthErr !== null, 'Cleared auth token caused HTTP 401 DRMErrorEvent');
    assert.equal(unauthErr.subErrorID, 401);
    dm.removeEventListener('drmError', onUnauthErr);

    // Re-authenticate cleanly
    authOutcomes.length = 0;
    dm.authenticate(baseUrl, 'studio-domain', 'alice', 's3cr3t');
    await until(() => authOutcomes.length === 1, 'Re-authenticated alice');

    // 4. DRMLicenseRequestEvent serverURL override + loadVoucher + loadPreviewVoucher + DRMStatusEvent properties
    let interceptedOrigUrl = '';
    const onLicReq = e => {
      interceptedOrigUrl = e.serverURL;
      e.serverURL = altUrl;
    };
    dm.addEventListener(DRMLicenseRequestEvent.LICENSE_REQUEST, onLicReq);

    let fullStatusEv = null;
    dm.addEventListener('drmStatus', e => { fullStatusEv = e; });
    dm.loadVoucher(cd, LoadVoucherSetting.FORCE_REFRESH);
    await until(() => fullStatusEv !== null, 'loadVoucher succeeded against overridden altUrl');
    dm.removeEventListener(DRMLicenseRequestEvent.LICENSE_REQUEST, onLicReq);

    assert.equal(interceptedOrigUrl, baseUrl);
    assert.ok(serverLog.some(entry => entry.path === '/alt-license' && entry.action === 'loadVoucher'), 'HTTP license request routed to overridden DRMLicenseRequestEvent.serverURL');
    assert.equal(fullStatusEv.isLocal, false);
    assert.equal(fullStatusEv.isAnonymous, false);
    assert.equal(fullStatusEv.isAvailableOffline, true);
    assert.ok(fullStatusEv.offlineLeasePeriod > 0);
    assert.equal(fullStatusEv.policies.tier, 'full');
    assert.equal(fullStatusEv.voucher.policyID, 'hd-offline-policy');
    assert.ok(fullStatusEv.voucher.playbackTimeWindow.period === 3600);
    assert.ok(fullStatusEv.voucherEndDate instanceof Date);
    assert.ok(fullStatusEv.voucher.toByteArray().length > 0);

    // Preview voucher has null offline lease dates -> isAvailableOffline === false
    fullStatusEv = null;
    dm.loadPreviewVoucher(cd);
    await until(() => fullStatusEv !== null, 'loadPreviewVoucher completed');
    assert.equal(fullStatusEv.voucher.policyID, 'preview-policy');
    assert.equal(fullStatusEv.voucher.offlineLeaseStartDate, null);
    assert.equal(fullStatusEv.isAvailableOffline, false);

    // Re-load full voucher into cache and decrypt encrypted media on NetStream!
    fullStatusEv = null;
    dm.loadVoucher(cd, LoadVoucherSetting.FORCE_REFRESH);
    await until(() => fullStatusEv !== null, 'Reloaded full voucher');

    nsStatusEvents.length = 0;
    encryptedStreamBa.position = 0;
    ns.appendBytes(encryptedStreamBa);
    await until(() => nsStatusEvents.length === 1, 'NetStream decrypted AES-128-CTR media and fired DRMStatusEvent');
    assert.deepEqual(Array.from(ns._lastDecryptedBytes), Array.from(clearFlvBytes), 'AES-128-CTR decrypted media bytes match original FLV cleartext bit-for-bit');
    assert.equal(nsStatusEvents[0].isLocal, true);

    // 5. Anonymous encrypted stream auto-fetches voucher over HTTP when appended to NetStream!
    const anonCd = new DRMContentData(h.binary.fromBytes(buildBinaryFadrmHeader({
      serverUrl: baseUrl,
      licenseId: 'lic-anon-clip',
      authMethod: AuthenticationMethod.ANONYMOUS,
    })));
    const anonEncBa = DRMManager._packageEncryptedStream(anonCd, clearFlvBytes);
    nsStatusEvents.length = 0;
    ns.appendBytes(anonEncBa);
    await until(() => nsStatusEvents.length === 1, 'Anonymous encrypted NetStream auto-acquired HTTP voucher and decrypted');
    assert.equal(nsStatusEvents[0].isAnonymous, true);
    assert.deepEqual(Array.from(ns._lastDecryptedBytes), Array.from(clearFlvBytes));

    // 6. Device group registration & error events
    const devGroup = accessInfos[0].deviceGroup;
    let addGrpEv = null;
    let rmGrpEv = null;
    let errGrpEv = null;
    dm.addEventListener('addToDeviceGroupComplete', e => { addGrpEv = e; });
    dm.addEventListener('removeFromDeviceGroupComplete', e => { rmGrpEv = e; });
    dm.addEventListener('addToDeviceGroupError', e => { errGrpEv = e; });

    dm.addToDeviceGroup(devGroup, false);
    await until(() => addGrpEv !== null, 'addToDeviceGroup succeeded');
    assert.equal(addGrpEv.deviceGroup, devGroup);
    assert.equal(registeredGroups.has('home-livingroom-group'), true);

    dm.removeFromDeviceGroup(devGroup);
    await until(() => rmGrpEv !== null, 'removeFromDeviceGroup succeeded');
    assert.equal(registeredGroups.has('home-livingroom-group'), false);

    const blockedCd = new DRMContentData(h.binary.fromBytes(buildBinaryFadrmHeader({
      serverUrl: baseUrl,
      licenseId: 'lic-blocked-grp',
      domain: 'studio-domain',
      accessList: [{ displayName: 'Blocked Group', deviceGroup: { name: 'blocked-group' } }],
    })));
    const blockedGrp = blockedCd.getVoucherAccessInfo()[0].deviceGroup;
    dm.addToDeviceGroup(blockedGrp, true);
    await until(() => errGrpEv !== null, 'Blocked device group dispatched DRMDeviceGroupErrorEvent');
    assert.equal(errGrpEv.errorID, 3315);
    assert.equal(errGrpEv.drmUpdateNeeded, true);

    // 7. returnVoucher validation + complete + error + revocation enforcement
    assert.throws(
      () => dm.returnVoucher(baseUrl, true, null, null),
      err => err && err.name === 'ArgumentError' && err.errorID === 2004,
    );

    let retDone = null;
    let retErr = null;
    dm.addEventListener('returnVoucherComplete', e => { retDone = e; });
    dm.addEventListener('returnVoucherError', e => { retErr = e; });

    dm.returnVoucher(baseUrl, true, 'lic-movie-101', 'hd-offline-policy');
    await until(() => retDone !== null, 'returnVoucher completed');
    assert.equal(retDone.numberOfVouchersReturned, 1);
    assert.equal(retDone.licenseID, 'lic-movie-101');
    assert.equal(retDone.policyID, 'hd-offline-policy');
    assert.equal(retDone.serverURL, baseUrl);

    dm.returnVoucher(baseUrl, false, 'lic-other', 'fail-policy');
    await until(() => retErr !== null, 'Failed returnVoucher dispatched DRMReturnVoucherErrorEvent');
    assert.equal(retErr.errorID, 3307);

    // 8. NetStream.resetDRMVouchers() flushes cached vouchers -> re-appending authenticated stream fails again!
    ns.resetDRMVouchers();
    nsErrorEvents.length = 0;
    encryptedStreamBa.position = 0;
    ns.appendBytes(encryptedStreamBa);
    await until(() => nsErrorEvents.length === 1, 'After resetDRMVouchers(), encrypted stream requires fresh voucher');

    // 9. DRMManager.networkIdleTimeout aborts stalled license server via AbortController
    const prevTimeout = DRMManager.networkIdleTimeout;
    DRMManager.networkIdleTimeout = 45;
    try {
      const stalledCd = new DRMContentData(h.binary.fromBytes(buildBinaryFadrmHeader({
        serverUrl: `http://127.0.0.1:${drmPort}/stalled-license`,
        licenseId: 'lic-stall',
        authMethod: AuthenticationMethod.ANONYMOUS,
      })));
      let stallErr = null;
      dm.addEventListener('drmError', e => { stallErr = e; });
      dm.loadVoucher(stalledCd, LoadVoucherSetting.FORCE_REFRESH);
      await until(() => stallErr !== null, 'networkIdleTimeout aborted stalled HTTP license request');
      assert.equal(stallErr.errorID, 3315);
    } finally {
      DRMManager.networkIdleTimeout = prevTimeout;
    }
  } finally {
    h.dispose();
    for (const sock of drmSockets) {
      try { sock.destroy(); } catch (_) {}
    }
    drmServer.closeAllConnections?.();
    await new Promise(r => drmServer.close(r));
  }
});

// ============================================================================
// 4. Real Headless Chrome End-to-End Verification (Self-Worker + SystemUpdater + DRM NetStream)
// ============================================================================
function decodeWsFrames(buf) {
  const frames = [];
  let off = 0;
  while (off + 2 <= buf.length) {
    const b0 = buf[off];
    const b1 = buf[off + 1];
    const opcode = b0 & 0x0f;
    const masked = (b1 & 0x80) !== 0;
    let len = b1 & 0x7f;
    let hdr = 2;
    if (len === 126) {
      if (off + 4 > buf.length) break;
      len = buf.readUInt16BE(off + 2);
      hdr = 4;
    } else if (len === 127) {
      if (off + 10 > buf.length) break;
      len = Number(buf.readBigUInt64BE(off + 2));
      hdr = 10;
    }
    const maskLen = masked ? 4 : 0;
    if (off + hdr + maskLen + len > buf.length) break;
    const payload = Buffer.from(buf.subarray(off + hdr + maskLen, off + hdr + maskLen + len));
    if (masked) {
      const m = buf.subarray(off + hdr, off + hdr + 4);
      for (let i = 0; i < payload.length; i++) payload[i] ^= m[i & 3];
    }
    frames.push({ opcode, payload });
    off += hdr + maskLen + len;
  }
  return { frames, consumed: off };
}

function connectSimpleCdp(wsUrl) {
  const netMod = require('node:net');
  const u = new URL(wsUrl);
  return new Promise((resolve, reject) => {
    const sock = netMod.createConnection({ host: u.hostname, port: Number(u.port) }, () => {
      const key = crypto.randomBytes(16).toString('base64');
      sock.write(
        `GET ${u.pathname}${u.search} HTTP/1.1\r\n` +
        `Host: ${u.host}\r\n` +
        `Upgrade: websocket\r\n` +
        `Connection: Upgrade\r\n` +
        `Sec-WebSocket-Key: ${key}\r\n` +
        `Sec-WebSocket-Version: 13\r\n\r\n`,
      );
    });
    let upgraded = false;
    let rx = Buffer.alloc(0);
    const listeners = [];
    sock.on('error', reject);
    sock.on('data', chunk => {
      rx = Buffer.concat([rx, chunk]);
      if (!upgraded) {
        const idx = rx.indexOf('\r\n\r\n');
        if (idx < 0) return;
        upgraded = true;
        rx = rx.subarray(idx + 4);
        resolve({
          send(text) {
            const payload = Buffer.from(text, 'utf8');
            const mask = crypto.randomBytes(4);
            let hdr;
            if (payload.length < 126) {
              hdr = Buffer.from([0x81, 0x80 | payload.length]);
            } else {
              hdr = Buffer.alloc(4);
              hdr[0] = 0x81;
              hdr[1] = 0x80 | 126;
              hdr.writeUInt16BE(payload.length, 2);
            }
            const masked = Buffer.alloc(payload.length);
            for (let i = 0; i < payload.length; i++) masked[i] = payload[i] ^ mask[i & 3];
            sock.write(Buffer.concat([hdr, mask, masked]));
          },
          onMessage(fn) { listeners.push(fn); },
          close() { try { sock.destroy(); } catch (_) {} },
        });
      }
      const { frames, consumed } = decodeWsFrames(rx);
      if (consumed > 0) rx = rx.subarray(consumed);
      for (const f of frames) {
        if (f.opcode === 1) {
          const s = f.payload.toString('utf8');
          for (const fn of listeners) fn(s);
        }
      }
    });
  });
}

test('T-460 Headless Chrome E2E: Self-worker Web Worker + SharedArrayBuffer ByteArray CAS, HTTP SystemUpdater SHA-256, and HTTP DRMManager + encrypted NetStream', async t => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Headless Chrome binary not installed on this machine');
    return;
  }
  ensureFreshExportBinary();
  const wasm = await loadWasmRuntime();
  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-t460-chrome-'));

  // Pre-build a real FADRM header + AES-128-CTR encrypted FLV payload to serve to Chrome!
  let port = 0;
  const updateBytes = Buffer.from('NeoFlash-Chrome-DRM-Module-v1-Payload-Bytes!', 'utf8');
  const updateSha = crypto.createHash('sha256').update(updateBytes).digest('hex');

  let pubDir = '';
  let encryptedMediaBuf = Buffer.alloc(0);
  const clearChromeFlv = new Uint8Array([
    0x46, 0x4c, 0x56, 0x01, 0x05, 0x00, 0x00, 0x00, 0x09,
    0x00, 0x00, 0x00, 0x00,
    0x09, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x17, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x10,
  ]);

  const mimeByExt = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'application/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.css': 'text/css; charset=utf-8',
  };

  const chromeSockets = new Set();
  const server = http.createServer((req, res) => {
    const urlPath = (req.url || '/').split('?')[0];
    // Cross-Origin Isolation headers enable SharedArrayBuffer + Atomics inside Chrome Web Workers!
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
    res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
    res.setHeader('Cross-Origin-Resource-Policy', 'same-origin');

    if (urlPath === '/sys-update') {
      res.writeHead(200, {
        'Content-Type': 'application/octet-stream',
        'Content-Length': String(updateBytes.length),
        'X-Package-SHA256': updateSha,
      });
      res.end(updateBytes);
      return;
    }
    if (urlPath === '/encrypted.flv') {
      res.writeHead(200, { 'Content-Type': 'application/octet-stream' });
      res.end(encryptedMediaBuf);
      return;
    }
    if (urlPath === '/drm-license') {
      let raw = '';
      req.setEncoding('utf8');
      req.on('data', c => { raw += c; });
      req.on('end', () => {
        const body = JSON.parse(raw || '{}');
        res.setHeader('Content-Type', 'application/json');
        if (body.action === 'authenticate') {
          res.writeHead(200);
          res.end(JSON.stringify({ tokenBase64: Buffer.from('chrome_tok_1', 'utf8').toString('base64') }));
          return;
        }
        if (body.action === 'loadVoucher') {
          const now = Date.now();
          res.writeHead(200);
          res.end(JSON.stringify({
            voucher: {
              licenseID: body.licenseID,
              policyID: 'chrome-hd-policy',
              serverURL: `http://127.0.0.1:${port}/drm-license`,
              startMs: now - 1000,
              endMs: now + 3_600_000,
              offlineStartMs: now - 1000,
              offlineEndMs: now + 86_400_000,
              windowPeriodSec: 1800,
              policies: { browser: 'chrome' },
            },
          }));
          return;
        }
        res.writeHead(400);
        res.end('{}');
      });
      return;
    }

    const safeRel = urlPath === '/' ? 'index.html' : urlPath.replace(/^\/+/, '');
    const filePath = path.join(pubDir, safeRel);
    if (pubDir && filePath.startsWith(pubDir) && fs.existsSync(filePath) && fs.statSync(filePath).isFile()) {
      const ext = path.extname(filePath).toLowerCase();
      res.writeHead(200, { 'Content-Type': mimeByExt[ext] || 'application/octet-stream' });
      res.end(fs.readFileSync(filePath));
      return;
    }
    res.writeHead(404);
    res.end('Not found');
  });

  server.on('connection', sock => {
    chromeSockets.add(sock);
    sock.on('close', () => chromeSockets.delete(sock));
  });
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  port = server.address().port;

  // Encrypt media with the exact port-bound FADRM header
  {
    const prepHost = createBacklogHost(wasm);
    try {
      const prepDm = prepHost.builtins['flash.net.drm.DRMManager'];
      const prepCd = new prepHost.builtins['flash.net.drm.DRMContentData'](
        prepHost.binary.fromBytes(buildBinaryFadrmHeader({
          serverUrl: `http://127.0.0.1:${port}/drm-license`,
          licenseId: 'chrome-lic-77',
          policyId: 'chrome-hd-policy',
          domain: 'chrome-dom',
          authMethod: 'usernameAndPassword',
        })),
      );
      const encBa = prepDm._packageEncryptedStream(prepCd, clearChromeFlv);
      encryptedMediaBuf = Buffer.from(prepHost.binary.toBytes(encBa));
    } finally {
      prepHost.dispose();
    }
  }

  const project = {
    format_version: 6,
    name: 'T460 Chrome Backlog',
    width: 320,
    height: 240,
    fps: 24,
    background: [12, 16, 24, 255],
    root: 'root',
    document_class: 'ChromeBacklogMain',
    resources: [],
    sources: [
      {
        id: 'chrome-t460',
        path: 'ChromeBacklogMain.as',
        language: 'action_script3',
        source: `package {
  import flash.display.MovieClip;
  import flash.events.*;
  import flash.net.*;
  import flash.net.drm.*;
  import flash.system.*;
  import flash.utils.ByteArray;
  import flash.utils.Endian;

  public class ChromeBacklogMain extends MovieClip {
    private var checks:Object = {};
    private var bg:Worker;
    private var toBg:MessageChannel;
    private var fromBg:MessageChannel;
    private var sharedMem:ByteArray;
    private var updater:SystemUpdater;
    private var dm:DRMManager;
    private var nc:NetConnection;
    private var ns:NetStream;

    public function ChromeBacklogMain() {
      if (!Worker.current.isPrimordial) {
        var rx:MessageChannel = Worker.current.getSharedProperty("inCh") as MessageChannel;
        var tx:MessageChannel = Worker.current.getSharedProperty("outCh") as MessageChannel;
        var mem:ByteArray = Worker.current.getSharedProperty("mem") as ByteArray;
        mem.atomicCompareAndSwapIntAt(0, 11, 88);
        var v:int = int(rx.receive(true));
        trace("CHROME_BG_TRACE=" + v);
        tx.send(v * 3);
        return;
      }

      // 1. Self-worker in real Browser Web Worker with SharedArrayBuffer CAS
      sharedMem = new ByteArray();
      sharedMem.endian = Endian.LITTLE_ENDIAN;
      sharedMem.shareable = true;
      sharedMem.writeInt(11);
      sharedMem.position = 0;

      bg = WorkerDomain.current.createWorker(this.loaderInfo.bytes);
      toBg = Worker.current.createMessageChannel(bg);
      fromBg = bg.createMessageChannel(Worker.current);
      bg.setSharedProperty("inCh", toBg);
      bg.setSharedProperty("outCh", fromBg);
      bg.setSharedProperty("mem", sharedMem);
      fromBg.addEventListener(Event.CHANNEL_MESSAGE, function(e:Event):void {
        var res:int = int(fromBg.receive());
        sharedMem.position = 0;
        var casVal:int = sharedMem.readInt();
        bg.terminate();
        checks.workerOk = (res == 45 && casVal == 88);
        maybeDone();
      });
      bg.addEventListener(Event.WORKER_STATE, function(e:Event):void {
        if (bg.state == WorkerState.RUNNING) {
          toBg.send(15);
        }
      });
      bg.start();

      // 2. SystemUpdater over HTTP with WebCrypto SHA-256 verification
      updater = new SystemUpdater();
      var gotProgress:Boolean = false;
      updater.addEventListener(ProgressEvent.PROGRESS, function(e:ProgressEvent):void {
        if (e.bytesLoaded > 0) gotProgress = true;
      });
      updater.addEventListener(Event.COMPLETE, function(e:Event):void {
        checks.updaterOk = gotProgress;
        maybeDone();
      });
      updater.update(SystemUpdaterType.DRM);

      // 3. DRMManager HTTP authenticate + loadVoucher + AES-128-CTR NetStream decryption
      var streamLoader:URLLoader = new URLLoader();
      streamLoader.dataFormat = URLLoaderDataFormat.BINARY;
      streamLoader.addEventListener(Event.COMPLETE, function(ev:Event):void {
        var encBytes:ByteArray = streamLoader.data as ByteArray;
        dm = DRMManager.getDRMManager();
        nc = new NetConnection();
        nc.connect(null);
        ns = new NetStream(nc);
        var gotClientMeta:Boolean = false;
        ns.client = {
          onMetaData: function(m:Object):void {},
          onDRMContentData: function(cd:DRMContentData):void { gotClientMeta = (cd.licenseID == "chrome-lic-77"); }
        };
        var seenMeta:DRMContentData = null;
        ns.addEventListener(DRMMetadataEvent.DRM_METADATA, function(me:DRMMetadataEvent):void {
          seenMeta = me.drmMetadata;
        });
        var onInitialMissingVoucher:Function = function(ee:DRMErrorEvent):void {
          ns.removeEventListener(DRMErrorEvent.DRM_ERROR, onInitialMissingVoucher);
          if (seenMeta != null && ee.errorID == 3305) {
            dm.addEventListener(DRMAuthenticationCompleteEvent.AUTHENTICATION_COMPLETE, function(ae:DRMAuthenticationCompleteEvent):void {
              dm.addEventListener(DRMStatusEvent.DRM_STATUS, function(se:DRMStatusEvent):void {
                ns.addEventListener(DRMStatusEvent.DRM_STATUS, function(nse:DRMStatusEvent):void {
                  checks.drmOk = (gotClientMeta && nse.isLocal && nse.voucher.policyID == "chrome-hd-policy");
                  maybeDone();
                });
                encBytes.position = 0;
                ns.appendBytes(encBytes);
              });
              dm.loadVoucher(seenMeta, LoadVoucherSetting.FORCE_REFRESH);
            });
            dm.authenticate("http://127.0.0.1:${port}/drm-license", "chrome-dom", "alice", "s3cr3t");
          }
        };
        ns.addEventListener(DRMErrorEvent.DRM_ERROR, onInitialMissingVoucher);
        ns.play(null);
        encBytes.position = 0;
        ns.appendBytes(encBytes);
      });
      streamLoader.load(new URLRequest("http://127.0.0.1:${port}/encrypted.flv"));
    }

    private function maybeDone():void {
      if (checks.workerOk && checks.updaterOk && checks.drmOk) {
        trace("T460_CHROME_ALL_OK=1");
      }
    }
  }
}`,
      },
    ],
    symbols: [
      {
        id: 'root',
        name: 'Root',
        kind: 'movie_clip',
        class_name: 'ChromeBacklogMain',
        timeline: {
          frame_count: 1,
          layers: [{ id: 'l1', name: 'L1', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
          scripts: [],
        },
      },
    ],
  };

  pubDir = path.join(workDir, 'pub');
  const projFile = path.join(workDir, 't460-chrome.nflash');
  fs.writeFileSync(projFile, JSON.stringify(project));
  execFileSync(exportBin, ['--runtime', runtimeDir, projFile, '--output', pubDir], { cwd: repoRoot, stdio: 'pipe' });

  const userDataDir = path.join(workDir, 'chrome-profile');
  const chromeProc = spawn(
    chromeBin,
    [
      '--headless=new',
      '--disable-gpu',
      '--no-first-run',
      '--no-default-browser-check',
      `--user-data-dir=${userDataDir}`,
      '--remote-debugging-port=0',
      'about:blank',
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  );

  let cdp = null;
  try {
    const wsEndpoint = await new Promise((resolve, reject) => {
      let stderr = '';
      const timer = setTimeout(() => reject(new Error(`Chrome DevTools timeout: ${stderr}`)), 10000);
      chromeProc.stderr.on('data', chunk => {
        stderr += chunk.toString();
        const m = /DevTools listening on (ws:\/\/[^\s]+)/.exec(stderr);
        if (m) {
          clearTimeout(timer);
          resolve(m[1]);
        }
      });
      chromeProc.once('exit', code => {
        clearTimeout(timer);
        reject(new Error(`Chrome exited early (${code}): ${stderr}`));
      });
    });

    const jsonRes = await fetch(wsEndpoint.replace(/^ws:/, 'http:').replace(/\/devtools\/browser\/.*$/, '/json/list'));
    const pages = await jsonRes.json();
    const pageWsUrl = pages.find(p => p.type === 'page')?.webSocketDebuggerUrl;
    assert.ok(pageWsUrl, 'Found Chrome page CDP target');

    cdp = new WebSocket(pageWsUrl);
    await new Promise((resolve, reject) => {
      const tmr = setTimeout(() => reject(new Error('CDP WebSocket open timeout')), 10000);
      cdp.addEventListener('open', () => { clearTimeout(tmr); resolve(); });
      cdp.addEventListener('error', err => { clearTimeout(tmr); reject(err); });
    });
    let nextId = 1;
    const pending = new Map();
    const cdpExceptions = [];
    const seenTraces = new Set();
    cdp.addEventListener('message', ev => {
      const msg = JSON.parse(String(ev.data));
      if (msg.method === 'Runtime.exceptionThrown') {
        cdpExceptions.push(JSON.stringify(msg.params?.exceptionDetails));
      }
      if (msg.id && pending.has(msg.id)) {
        const { resolve, reject, tmr } = pending.get(msg.id);
        clearTimeout(tmr);
        pending.delete(msg.id);
        if (msg.error) reject(new Error(JSON.stringify(msg.error)));
        else resolve(msg.result);
      }
    });
    const sendCdp = (method, params = {}) =>
      new Promise((resolve, reject) => {
        const id = nextId++;
        const tmr = setTimeout(() => {
          pending.delete(id);
          reject(new Error(`CDP timeout waiting for ${method}`));
        }, 20000);
        pending.set(id, { resolve, reject, tmr });
        cdp.send(JSON.stringify({ id, method, params }));
      });

    await sendCdp('Page.enable');
    await sendCdp('Runtime.enable');
    await sendCdp('Page.addScriptToEvaluateOnNewDocument', {
      source: `window.NEOFLASH_SYSTEM_UPDATE_URL = "http://127.0.0.1:${port}/sys-update";`,
    });
    await sendCdp('Page.navigate', { url: `http://127.0.0.1:${port}/index.html` });

    let finalTrace = '';
    const deadline = Date.now() + 15000;
    while (Date.now() < deadline) {
      try {
        const snap = await sendCdp('Runtime.evaluate', {
          expression: `({
            href: String(location.href || ''),
            err: document.getElementById('error')?.textContent?.trim() || '',
            trace: document.getElementById('trace')?.textContent || ''
          })`,
          returnByValue: true,
        });
        const val = snap?.result?.value;
        if (val && val.href.startsWith('http://127.0.0.1:')) {
          if (val.err) {
            throw new Error(`Chrome player UI error: ${val.err}; trace=${val.trace}`);
          }
          finalTrace = val.trace || '';
          if (finalTrace) seenTraces.add(finalTrace);
          if (finalTrace.includes('T460_CHROME_ALL_OK=1')) {
            break;
          }
        }
      } catch (pollErr) {
        if (String(pollErr.message || pollErr).includes('Chrome player UI error:')) {
          throw pollErr;
        }
        // Ignore transient execution-context-destroyed errors during COOP/COEP BrowsingInstance swap
      }
      await new Promise(r => setTimeout(r, 40));
    }
    assert.ok(
      finalTrace.includes('T460_CHROME_ALL_OK=1'),
      `Expected T460_CHROME_ALL_OK=1 in Chrome trace, got: ${[...seenTraces].join(' | ')} ; exceptions=${cdpExceptions.join(' || ')}`,
    );
  } finally {
    cdp?.close();
    try { chromeProc.kill('SIGTERM'); } catch (_) {}
    if (chromeProc.exitCode === null) {
      await new Promise(resolve => {
        const tmr = setTimeout(() => {
          try { chromeProc.kill('SIGKILL'); } catch (_) {}
          resolve();
        }, 1500);
        tmr.unref?.();
        chromeProc.once('exit', () => { clearTimeout(tmr); resolve(); });
      });
    }
    for (const sock of chromeSockets) {
      try { sock.destroy(); } catch (_) {}
    }
    server.closeAllConnections?.();
    await new Promise(r => server.close(r));
    fs.rmSync(workDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});
