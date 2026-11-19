'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const vm = require('node:vm');
const { execFileSync, spawn } = require('node:child_process');

const repoRoot = path.resolve(__dirname, '../..');
const runtimeDir = path.join(repoRoot, 'target/web-runtime');
const exportBin = path.join(repoRoot, 'target/debug/neoflash-export');

const Events = require('../events.js');
const Binary = require('../byte-array.js');
const Net = require('../net.js');
const AudioFormats = require('../audio-formats.js');
const Audio = require('../audio.js');
const Video = require('../video.js');

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

async function loadWasmRuntime() {
  const jsPath = path.join(runtimeDir, 'neoflash_web.js');
  const wasmPath = path.join(runtimeDir, 'neoflash_web_bg.wasm');
  const sandbox = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Uint8Array });
  vm.runInContext(fs.readFileSync(jsPath, 'utf8'), sandbox);
  sandbox.wasmBytes = fs.readFileSync(wasmPath);
  return vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', sandbox);
}

function buildCueMarkedStereoWav({ sampleRate = 8000, durationSec = 0.45, cueTimeSec = 0.12, cueLabel = 'chrome_cue' } = {}) {
  const numFrames = Math.max(1, Math.round(sampleRate * durationSec));
  const channels = 2;
  const blockAlign = channels * 2;
  const byteRate = sampleRate * blockAlign;
  const dataSize = numFrames * blockAlign;

  const labelRaw = Buffer.from(`${cueLabel}\0`, 'utf8');
  const lablPayloadLen = 4 + labelRaw.length;
  const lablPadded = lablPayloadLen + (lablPayloadLen & 1);
  const listPayloadLen = 4 + 8 + lablPadded;
  const cuePayloadLen = 4 + 24;
  const riffPayloadLen = 4 + 24 + (8 + dataSize) + (8 + cuePayloadLen) + (8 + listPayloadLen);

  const buf = Buffer.alloc(8 + riffPayloadLen);
  let off = 0;
  buf.write('RIFF', off); off += 4;
  buf.writeUInt32LE(riffPayloadLen, off); off += 4;
  buf.write('WAVE', off); off += 4;

  buf.write('fmt ', off); off += 4;
  buf.writeUInt32LE(16, off); off += 4;
  buf.writeUInt16LE(1, off); off += 2;
  buf.writeUInt16LE(channels, off); off += 2;
  buf.writeUInt32LE(sampleRate, off); off += 4;
  buf.writeUInt32LE(byteRate, off); off += 4;
  buf.writeUInt16LE(blockAlign, off); off += 2;
  buf.writeUInt16LE(16, off); off += 2;

  buf.write('data', off); off += 4;
  buf.writeUInt32LE(dataSize, off); off += 4;
  for (let i = 0; i < numFrames; i++) {
    const t = i / sampleRate;
    buf.writeInt16LE(Math.round(Math.sin(2 * Math.PI * 440 * t) * 16000), off);
    buf.writeInt16LE(Math.round(Math.cos(2 * Math.PI * 660 * t) * 10000), off + 2);
    off += 4;
  }

  const cueSampleOffset = Math.min(numFrames - 1, Math.max(0, Math.round(cueTimeSec * sampleRate)));
  buf.write('cue ', off); off += 4;
  buf.writeUInt32LE(cuePayloadLen, off); off += 4;
  buf.writeUInt32LE(1, off); off += 4;
  buf.writeUInt32LE(1, off); off += 4;
  buf.writeUInt32LE(cueSampleOffset, off); off += 4;
  buf.write('data', off); off += 4;
  buf.writeUInt32LE(0, off); off += 4;
  buf.writeUInt32LE(0, off); off += 4;
  buf.writeUInt32LE(cueSampleOffset, off); off += 4;

  buf.write('LIST', off); off += 4;
  buf.writeUInt32LE(listPayloadLen, off); off += 4;
  buf.write('adtl', off); off += 4;
  buf.write('labl', off); off += 4;
  buf.writeUInt32LE(lablPayloadLen, off); off += 4;
  buf.writeUInt32LE(1, off); off += 4;
  labelRaw.copy(buf, off);

  return buf;
}

function shiftMp4ChunkOffsets(buf, start, end, threshold, delta) {
  let p = start;
  while (p + 8 <= end) {
    const sz = buf.readUInt32BE(p);
    const typ = buf.subarray(p + 4, p + 8).toString('ascii');
    const len = sz === 0 ? end - p : sz;
    if (len < 8 || p + len > end) break;
    if (typ === 'trak' || typ === 'mdia' || typ === 'minf' || typ === 'stbl') {
      shiftMp4ChunkOffsets(buf, p + 8, p + len, threshold, delta);
    } else if (typ === 'stco' && len >= 16) {
      const count = buf.readUInt32BE(p + 12);
      for (let i = 0; i < count && p + 16 + (i + 1) * 4 <= p + len; i++) {
        const off = buf.readUInt32BE(p + 16 + i * 4);
        if (off >= threshold) buf.writeUInt32BE(off + delta, p + 16 + i * 4);
      }
    } else if (typ === 'co64' && len >= 16) {
      const count = buf.readUInt32BE(p + 12);
      for (let i = 0; i < count && p + 16 + (i + 1) * 8 <= p + len; i++) {
        const off = buf.readBigUInt64BE(p + 16 + i * 8);
        if (off >= BigInt(threshold)) buf.writeBigUInt64BE(off + BigInt(delta), p + 16 + i * 8);
      }
    }
    p += len;
  }
}

function injectMp4ChapterCue(mp4Bytes, cueTimeSec = 0.2, cueName = 'mp4_cue_1') {
  let pos = 0;
  while (pos + 8 <= mp4Bytes.length) {
    const sz = mp4Bytes.readUInt32BE(pos);
    const typ = mp4Bytes.subarray(pos + 4, pos + 8).toString('ascii');
    const boxLen = sz === 0 ? mp4Bytes.length - pos : sz;
    if (boxLen < 8 || pos + boxLen > mp4Bytes.length) break;
    if (typ === 'moov') {
      const nameBytes = Buffer.from(cueName, 'utf8');
      const chplPayload = Buffer.alloc(9 + 9 + nameBytes.length);
      chplPayload[0] = 1;
      chplPayload[8] = 1;
      chplPayload.writeBigUInt64BE(BigInt(Math.round(cueTimeSec * 10_000_000)), 9);
      chplPayload[17] = nameBytes.length;
      nameBytes.copy(chplPayload, 18);

      const chplBox = Buffer.alloc(8 + chplPayload.length);
      chplBox.writeUInt32BE(chplBox.length, 0);
      chplBox.write('chpl', 4);
      chplPayload.copy(chplBox, 8);

      const moovEnd = pos + boxLen;
      const beforeMoov = mp4Bytes.subarray(0, pos);
      const moovBody = Buffer.from(mp4Bytes.subarray(pos + 8, moovEnd));
      const afterMoov = mp4Bytes.subarray(moovEnd);

      let uPos = 0;
      let foundUdta = null;
      while (uPos + 8 <= moovBody.length) {
        const uSz = moovBody.readUInt32BE(uPos);
        const uTyp = moovBody.subarray(uPos + 4, uPos + 8).toString('ascii');
        const uLen = uSz === 0 ? moovBody.length - uPos : uSz;
        if (uLen < 8 || uPos + uLen > moovBody.length) break;
        if (uTyp === 'udta') {
          foundUdta = { uPos, uLen };
          break;
        }
        uPos += uLen;
      }

      if (foundUdta) {
        const delta = chplBox.length;
        shiftMp4ChunkOffsets(moovBody, 0, moovBody.length, moovEnd, delta);
        const newUdtaHdr = Buffer.alloc(8);
        newUdtaHdr.writeUInt32BE(foundUdta.uLen + delta, 0);
        newUdtaHdr.write('udta', 4);
        const newMoovHdr = Buffer.alloc(8);
        newMoovHdr.writeUInt32BE(boxLen + delta, 0);
        newMoovHdr.write('moov', 4);
        return Buffer.concat([
          beforeMoov,
          newMoovHdr,
          moovBody.subarray(0, foundUdta.uPos),
          newUdtaHdr,
          moovBody.subarray(foundUdta.uPos + 8, foundUdta.uPos + foundUdta.uLen),
          chplBox,
          moovBody.subarray(foundUdta.uPos + foundUdta.uLen),
          afterMoov,
        ]);
      }

      const udtaBox = Buffer.alloc(8 + chplBox.length);
      udtaBox.writeUInt32BE(udtaBox.length, 0);
      udtaBox.write('udta', 4);
      chplBox.copy(udtaBox, 8);

      const delta = udtaBox.length;
      shiftMp4ChunkOffsets(moovBody, 0, moovBody.length, moovEnd, delta);
      const newMoovHeader = Buffer.alloc(8);
      newMoovHeader.writeUInt32BE(8 + moovBody.length + udtaBox.length, 0);
      newMoovHeader.write('moov', 4);
      return Buffer.concat([beforeMoov, newMoovHeader, moovBody, udtaBox, afterMoov]);
    }
    pos += boxLen;
  }
  return mp4Bytes;
}

async function launchCdpSession(chromeBin, userDataDir) {
  const chromeProc = spawn(
    chromeBin,
    [
      '--headless=new',
      '--remote-debugging-port=0',
      `--user-data-dir=${userDataDir}`,
      '--no-first-run',
      '--no-default-browser-check',
      '--autoplay-policy=no-user-gesture-required',
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

  const ws = new WebSocket(wsUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', reject, { once: true });
  });

  let nextId = 1;
  const pending = new Map();
  const errors = [];
  let loadResolve = null;

  ws.addEventListener('message', (event) => {
    const msg = JSON.parse(String(event.data));
    if (msg.method === 'Page.loadEventFired' && loadResolve) {
      const fn = loadResolve;
      loadResolve = null;
      fn();
    } else if (msg.method === 'Runtime.exceptionThrown') {
      errors.push(`exception: ${JSON.stringify(msg.params.exceptionDetails)}`);
    } else if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
      errors.push(`console.error: ${JSON.stringify(msg.params.args)}`);
    } else if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(msg.error.message || JSON.stringify(msg.error)));
      else resolve(msg.result);
    }
  });

  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });

  await send('Page.enable');
  await send('Runtime.enable');

  // Install non-invasive lifecycle tracker for Blob URLs, <video>/<audio> elements, Web Audio nodes, and AbortControllers!
  await send('Page.addScriptToEvaluateOnNewDocument', {
    source: `(() => {
      const liveBlobs = new Set();
      let totalBlobsCreated = 0;
      let totalBlobsRevoked = 0;
      const origCreate = URL.createObjectURL.bind(URL);
      const origRevoke = URL.revokeObjectURL.bind(URL);
      URL.createObjectURL = function(obj) {
        const u = origCreate(obj);
        totalBlobsCreated++;
        liveBlobs.add(u);
        return u;
      };
      URL.revokeObjectURL = function(u) {
        if (liveBlobs.delete(u)) totalBlobsRevoked++;
        return origRevoke(u);
      };

      const createdMedia = [];
      const origCreateEl = Document.prototype.createElement;
      Document.prototype.createElement = function(tag, opts) {
        const el = origCreateEl.call(this, tag, opts);
        const lower = String(tag || '').toLowerCase();
        if (lower === 'video' || lower === 'audio') {
          createdMedia.push(el);
        }
        return el;
      };

      let abortCount = 0;
      const origAbort = AbortController.prototype.abort;
      AbortController.prototype.abort = function(...args) {
        abortCount++;
        return origAbort.apply(this, args);
      };

      let splitterCount = 0;
      let mergerCount = 0;
      let pannerCount = 0;
      let mediaElemSourceCount = 0;
      const AC = window.AudioContext || window.webkitAudioContext;
      if (AC && AC.prototype) {
        const origSplit = AC.prototype.createChannelSplitter;
        if (origSplit) AC.prototype.createChannelSplitter = function(...a) { splitterCount++; return origSplit.apply(this, a); };
        const origMerge = AC.prototype.createChannelMerger;
        if (origMerge) AC.prototype.createChannelMerger = function(...a) { mergerCount++; return origMerge.apply(this, a); };
        const origPan = AC.prototype.createStereoPanner;
        if (origPan) AC.prototype.createStereoPanner = function(...a) { pannerCount++; return origPan.apply(this, a); };
        const origMes = AC.prototype.createMediaElementSource;
        if (origMes) AC.prototype.createMediaElementSource = function(...a) { mediaElemSourceCount++; return origMes.apply(this, a); };
      }

      const traceLines = [];
      const origConsoleLog = console.log.bind(console);
      console.log = function(...args) {
        if (args[0] === '[NeoFlash]') {
          traceLines.push(args.slice(1).map(String).join(' '));
        }
        return origConsoleLog(...args);
      };
      window.__nfTraceLog = () => traceLines.join('\\n');
      window.__nfClearTraceLog = () => { traceLines.length = 0; };

      window.__nfMediaAudit = () => ({
        liveBlobCount: liveBlobs.size,
        totalBlobsCreated,
        totalBlobsRevoked,
        abortCount,
        splitterCount,
        mergerCount,
        pannerCount,
        mediaElemSourceCount,
        totalMediaElements: createdMedia.length,
        activeMediaElements: createdMedia.filter(el => !el.paused || Boolean(el.getAttribute('src'))).length,
      });
    })();`,
  });

  const navigateAndWait = async (url) => {
    const loaded = new Promise((resolve) => {
      loadResolve = resolve;
    });
    await send('Page.navigate', { url });
    await loaded;
  };

  const evalJson = async (expression, { userGesture = false } = {}) => {
    const res = await send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
      userGesture,
    });
    if (res.exceptionDetails) {
      throw new Error(`Eval failed: ${JSON.stringify(res.exceptionDetails)}`);
    }
    return res.result?.value;
  };

  const waitTrace = async (marker, timeoutMs = 20000) => {
    await evalJson(`
      new Promise((resolve, reject) => {
        const deadline = Date.now() + ${timeoutMs};
        const poll = () => {
          const errEl = document.getElementById('error');
          if (errEl && errEl.textContent.trim()) {
            reject(new Error('Runtime error UI: ' + errEl.textContent));
            return;
          }
          const loadingEl = document.getElementById('loading');
          const traceText = (document.getElementById('trace')?.textContent || '') + '\\n' + (window.__nfTraceLog ? window.__nfTraceLog() : '');
          if (loadingEl && loadingEl.style.display === 'none' && traceText.includes(${JSON.stringify(marker)})) {
            resolve(true);
            return;
          }
          if (Date.now() > deadline) {
            reject(new Error('Timeout waiting for trace ' + ${JSON.stringify(marker)} + '; current=' + traceText));
            return;
          }
          setTimeout(poll, 25);
        };
        poll();
      })
    `);
  };

  const close = async () => {
    try { ws.close(); } catch (_) {}
    try { chromeProc.kill('SIGTERM'); } catch (_) {}
    await new Promise((resolve) => {
      const t = setTimeout(() => {
        try { chromeProc.kill('SIGKILL'); } catch (_) {}
        resolve();
      }, 2000);
      chromeProc.once('exit', () => { clearTimeout(t); resolve(); });
    });
  };

  return { chromeProc, ws, send, navigateAndWait, evalJson, waitTrace, errors, close };
}

test('T-327 Node WASM & host unit suite: NetStream reset modes (0/1/2/3), Seek.InvalidTime, SoundTransform 2x2 matrix & StereoPanner, and Sound.isURLInaccessible', async () => {
  const wasm = await loadWasmRuntime();
  const exampleProj = JSON.parse(fs.readFileSync(path.join(repoRoot, 'examples/video-playback.nflash'), 'utf8'));
  const rawMotionRes = exampleProj.resources.find((r) => r.path === 'media/motion.mp4');
  const cuedWav = buildCueMarkedStereoWav();

  const minProject = {
    format_version: 9,
    name: 'T-327 Unit',
    width: 320,
    height: 180,
    fps: 24,
    background: [0, 0, 0, 255],
    root: 'root',
    resources: [
      rawMotionRes,
      {
        id: 'res-wav',
        name: 'cued.wav',
        path: 'audio/cued.wav',
        media_type: 'audio/wav',
        data_base64: cuedWav.toString('base64'),
      },
    ],
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

  const player = new wasm.WebPlayer(JSON.stringify(minProject));
  const binary = Binary.create({ wasm });
  const net = Net.create({
    events: Events,
    binary,
    baseURL: 'http://127.0.0.1:18080/index.html',
    resources: minProject.resources,
    fetch: async (url) => {
      if (String(url).includes('no-cors.example.test')) {
        throw new TypeError('Failed to fetch (simulated browser CORS block)');
      }
      return new Response(cuedWav, {
        status: 200,
        headers: { 'Content-Type': 'audio/wav', 'Content-Length': String(cuedWav.length) },
      });
    },
  });

  const audioElements = [];
  const audioCtxNodes = { gains: [], panners: [], splitters: [], mergers: [] };
  const audioHost = Audio.create({
    events: Events,
    net,
    binary,
    player,
    wasm,
    formats: AudioFormats,
    isUserGesture: () => true,
    createAudio: () => {
      const listeners = new Map();
      const el = {
        muted: true,
        volume: 0,
        paused: true,
        duration: 0.45,
        currentTime: 0,
        _src: '',
        get src() { return this._src; },
        set src(v) { this._src = v; },
        addEventListener(t, fn) { if (!listeners.has(t)) listeners.set(t, []); listeners.get(t).push(fn); },
        removeEventListener(t, fn) { if (listeners.has(t)) listeners.set(t, listeners.get(t).filter((x) => x !== fn)); },
        removeAttribute(k) { if (k === 'src') this._src = ''; },
        load() {
          if (!this._src) return;
          queueMicrotask(() => {
            for (const fn of listeners.get('loadedmetadata') || []) fn();
          });
        },
        play() { this.paused = false; return Promise.resolve(); },
        pause() { this.paused = true; },
      };
      audioElements.push(el);
      return el;
    },
    createContext: () => ({
      state: 'running',
      sampleRate: 44100,
      currentTime: 0,
      destination: { kind: 'dest' },
      resume() { return Promise.resolve(); },
      close() { return Promise.resolve(); },
      decodeAudioData() { return Promise.resolve({ numberOfChannels: 2, sampleRate: 8000, length: 3600, duration: 0.45 }); },
      createGain() { const g = { gain: { value: 1 }, connect() {}, disconnect() {} }; audioCtxNodes.gains.push(g); return g; },
      createStereoPanner() { const p = { pan: { value: 0 }, connect() {}, disconnect() {} }; audioCtxNodes.panners.push(p); return p; },
      createChannelSplitter() { const s = { connect() {}, disconnect() {} }; audioCtxNodes.splitters.push(s); return s; },
      createChannelMerger() { const m = { connect() {}, disconnect() {} }; audioCtxNodes.mergers.push(m); return m; },
      createMediaElementSource() { return { connect() {}, disconnect() {} }; },
      createBufferSource() {
        return { buffer: null, loop: false, onended: null, connect() {}, disconnect() {}, start() {}, stop() {} };
      },
    }),
  });
  await audioHost.resumeFromGesture();

  // Verify browser SOP fetch failure -> loadOpaqueAudio -> isURLInaccessible === true!
  const opaqueSound = new audioHost.Sound();
  let opaqueLoaded = false;
  opaqueSound.addEventListener('complete', () => { opaqueLoaded = true; });
  opaqueSound.load(new net.URLRequest('http://no-cors.example.test/opaque.wav'));
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(opaqueLoaded, true, 'browser SOP fetch failure falls back to opaque <audio> load');
  assert.equal(opaqueSound.isURLInaccessible, true);
  const opCh = opaqueSound.play(0, 0, new audioHost.SoundTransform(0.6, 0));
  assert.equal(audioHost.SoundMixer.areSoundsInaccessible(), true);
  const outBa = new binary.ByteArray();
  assert.throws(() => opaqueSound.extract(outBa, 32, 0), /SecurityError|#2122/);
  assert.throws(() => audioHost.SoundMixer.computeSpectrum(outBa, false, 0), /SecurityError|#2121/);
  opCh.stop();
  assert.equal(audioHost.SoundMixer.areSoundsInaccessible(), false);

  // Verify Adobe reset modes 2 (append + aggregate messages) and 3
  // (reset playlist + aggregate messages), ordinary playlist Switch/time, and play(false).
  const videoElements = [];
  const blobs = new Map();
  const displayRefs = new Map();
  const videoHost = Video.create({
    player,
    wasm,
    net,
    events: Events,
    isUserGesture: () => true,
    getSoundTransform: () => audioHost.SoundTransform,
    referenceOf: (obj) => obj?.__ref || null,
    proxyForRef: (ref) => {
      const k = JSON.stringify(ref);
      if (!displayRefs.has(k)) displayRefs.set(k, { __ref: { key: k, objectRef: ref } });
      return displayRefs.get(k);
    },
    createObjectURL: (b) => {
      const kind = String(b?.type || '').startsWith('audio/') ? 'audio' : 'video';
      const u = `blob:unit-${kind}-${blobs.size + 1}`;
      blobs.set(u, b);
      return u;
    },
    revokeObjectURL: (u) => { blobs.delete(u); },
    createCanvas: (w, h) => ({
      width: w,
      height: h,
      getContext: () => ({ drawImage() {}, save() {}, restore() {}, getImageData: () => ({ data: new Uint8ClampedArray(w * h * 4) }), putImageData() {} }),
    }),
    createVideo: () => {
      const listeners = new Map();
      const el = {
        muted: false,
        volume: 1,
        paused: true,
        ended: false,
        readyState: 0,
        videoWidth: 320,
        videoHeight: 180,
        duration: 4,
        currentTime: 0,
        _src: '',
        buffered: { length: 1, start: () => 0, end: () => (el._src.startsWith('blob:') ? 4 : 1) },
        get src() { return this._src; },
        set src(v) { this._src = v; },
        addEventListener(t, fn) { if (!listeners.has(t)) listeners.set(t, []); listeners.get(t).push(fn); },
        removeEventListener(t, fn) { if (listeners.has(t)) listeners.set(t, listeners.get(t).filter((x) => x !== fn)); },
        removeAttribute(k) { if (k === 'src') this._src = ''; },
        emit(t) { for (const fn of [...(listeners.get(t) || [])]) fn(); },
        load() {
          if (!this._src) return;
          const isAudio = this._src.includes('unit-audio-');
          this.videoWidth = isAudio ? 0 : 320;
          this.videoHeight = isAudio ? 0 : 180;
          queueMicrotask(() => {
            this.readyState = 3;
            this.emit('loadedmetadata');
            this.emit('loadeddata');
            this.emit('canplay');
          });
        },
        play() {
          this.paused = false;
          queueMicrotask(() => { if (!this.paused) this.emit('playing'); });
          return Promise.resolve();
        },
        pause() { this.paused = true; this.emit('pause'); },
      };
      videoElements.push(el);
      return el;
    },
  });
  await videoHost.resumeFromGesture();

  const nc = new videoHost.NetConnection();
  nc.connect(null);
  const ns = new videoHost.NetStream(nc);
  const statuses = [];
  const metas = [];
  ns.client = { onMetaData: (m) => metas.push(m), onPlayStatus: (p) => statuses.push(`playStatus:${p.code}`) };
  ns.addEventListener('netStatus', (e) => statuses.push(e.info.code));

  ns.play('media/motion.mp4', 0, -1, 1);
  // Mode 2 appends while busy; local files have no stream-message batch to aggregate.
  ns.play('audio/cued.wav', 2, -1, 2);
  await new Promise((r) => setTimeout(r, 25));
  assert.equal(metas.length, 1, 'reset=2 queues instead of replacing the active clip');
  videoElements.at(-1).currentTime = 1.25;
  videoElements.at(-1).ended = true;
  videoElements.at(-1).emit('ended');
  await new Promise((r) => setTimeout(r, 25));
  assert.ok(statuses.includes('playStatus:NetStream.Play.Switch'), 'ordinary playlist transition reports onPlayStatus NetStream.Play.Switch');
  assert.ok(!statuses.includes('NetStream.Play.Transition'), 'ordinary playlist transition must not report bitrate-switch NetStream.Play.Transition');
  assert.ok(!statuses.includes('playStatus:NetStream.Play.TransitionComplete'), 'ordinary playlist transition must not report bitrate-switch TransitionComplete');
  assert.equal(metas.at(-1).width, 0, 'reset=2 appended audio/cued.wav');
  assert.ok(Math.abs(ns.time - 1.25) < 1e-6, 'playlist time carries across the element boundary');
  videoElements.at(-1).currentTime = 2.2;
  assert.ok(Math.abs(ns.time - 1.45) < 1e-6, 'playlist time continues increasing in the next element');

  // Mode 3 clears the pending playlist and starts the new element immediately.
  ns.play('audio/cued.wav', 0, -1, 0);
  const beforeReset3Elements = videoElements.length;
  ns.play('media/motion.mp4', 0, -1, 3);
  await new Promise((r) => setTimeout(r, 25));
  assert.equal(videoElements.length, beforeReset3Elements + 1, 'reset=3 replaces the active clip immediately');
  assert.equal(metas.at(-1).width, 320, 'reset=3 starts the replacement video');
  assert.equal(ns.time, 0, 'reset=3 resets playlist time');
  const switchesBeforeReset3End = statuses.filter((code) => code === 'playStatus:NetStream.Play.Switch').length;
  videoElements.at(-1).currentTime = 0.5;
  videoElements.at(-1).ended = true;
  videoElements.at(-1).emit('ended');
  await new Promise((r) => setTimeout(r, 15));
  assert.equal(statuses.filter((code) => code === 'playStatus:NetStream.Play.Switch').length, switchesBeforeReset3End, 'reset=3 cleared the old pending playlist');

  // play(false) stops active playback and emits NetStream.Play.Stop
  statuses.length = 0;
  ns.play(false);
  await new Promise((r) => setTimeout(r, 15));
  assert.ok(statuses.includes('NetStream.Play.Stop'), 'ns.play(false) stops active stream cleanly');
  assert.equal(blobs.size, 0, 'ns.play(false) revokes active Blob URL');

  // Progressive len=0 clip on partially buffered HTTP stream preserves partial bytesLoaded < bytesTotal
  statuses.length = 0;
  ns.play('http://127.0.0.1:18080/partial-prog.mp4', 0, 0, 1);
  await new Promise((r) => setTimeout(r, 25));
  assert.ok(statuses.includes('NetStream.Play.Stop'), 'progressive len=0 single-frame completes');
  assert.equal(ns.bytesTotal, cuedWav.length);
  assert.ok(ns.bytesLoaded > 0 && ns.bytesLoaded < ns.bytesTotal, `partial progressive len=0 bytesLoaded (${ns.bytesLoaded}) < bytesTotal (${ns.bytesTotal})`);

  // Verify 3-stage channel * target * mixer SoundTransform WASM plan does not clamp stage1 to 1.0
  const plan3 = JSON.parse(wasm.as3_sound_transform_plan_json(
    JSON.stringify({ volume: 1, pan: 0, leftToLeft: 1, leftToRight: 0.8, rightToLeft: 0, rightToRight: 1 }),
    JSON.stringify({ volume: 1, pan: 0, leftToLeft: 1, leftToRight: 0, rightToLeft: 1, rightToRight: 1 }),
    JSON.stringify({ volume: 0.5, pan: 0, leftToLeft: 0.5, leftToRight: 0, rightToLeft: 0, rightToRight: 0.5 }),
  ));
  assert.equal(plan3.isMatrix, true);
  assert.ok(Math.abs(plan3.gains[0] - 0.9) < 1e-6, `3-stage gain[0] must be 1.8 * 0.5 = 0.9, got ${plan3.gains[0]}`);

  // Disposed streams release the publication's 64-stream quota and remain
  // idempotently disposed instead of silently continuing to operate.
  ns.dispose();
  assert.doesNotThrow(() => ns.dispose());
  assert.throws(() => ns.play('media/motion.mp4'), /disposed/);
  for (let i = 0; i < 65; i++) {
    const transient = new videoHost.NetStream(nc);
    transient.dispose();
  }

  videoHost.dispose();
  audioHost.dispose();
  net.dispose();
  player.free();
});

test('T-327 Real Headless Chrome Canvas + DOM end-to-end suite: AS2/AS3 Video pixels, NetStream.play(start, len, reset), 206 Range, 302 redirects, CORS Sound.isURLInaccessible, Web Audio 2x2 matrix, close() & Restart', async (t) => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Headless Chrome binary not available on this machine');
    return;
  }

  const bin = ensureFreshExportBinary();
  const workRoot = fs.mkdtempSync(path.join(repoRoot, 'target/chrome-media-netstream-'));
  const chromeProfileDir = path.join(workRoot, 'chrome-profile');
  fs.mkdirSync(chromeProfileDir, { recursive: true });

  const exampleProj = JSON.parse(fs.readFileSync(path.join(repoRoot, 'examples/video-playback.nflash'), 'utf8'));
  const rawMotionRes = exampleProj.resources.find((r) => r.path === 'media/motion.mp4');
  const rawMp4 = Buffer.from(rawMotionRes.data_base64, 'base64');
  const cuedMp4 = injectMp4ChapterCue(rawMp4, 0.2, 'mp4_cue_1');
  const cuedWav = buildCueMarkedStereoWav({ sampleRate: 8000, durationSec: 0.45, cueTimeSec: 0.12, cueLabel: 'chrome_cue' });

  let pubServer = null;
  let corsAllowServer = null;
  let corsDenyServer = null;
  let cdp = null;

  const httpStats = {
    rangeHeaders: [],
    redirects: 0,
    corsAllowedHits: 0,
    corsDeniedHits: 0,
    slowAborted: 0,
  };

  try {
    // 1. Start cross-origin CORS-allowed and CORS-denied servers first so we know their ports!
    corsAllowServer = http.createServer((req, res) => {
      httpStats.corsAllowedHits++;
      if ((req.url || '').startsWith('/cors-motion.mp4')) {
        const range = req.headers.range;
        if (range) {
          const m = /^bytes=(\d+)-(\d*)$/i.exec(range.trim());
          if (m) {
            const s = Math.min(cuedMp4.length - 1, Number(m[1]));
            const e = m[2] ? Math.min(cuedMp4.length - 1, Number(m[2])) : cuedMp4.length - 1;
            res.writeHead(206, {
              'Content-Type': 'video/mp4',
              'Accept-Ranges': 'bytes',
              'Content-Range': `bytes ${s}-${e}/${cuedMp4.length}`,
              'Content-Length': e - s + 1,
              'Access-Control-Allow-Origin': '*',
              'Access-Control-Expose-Headers': 'Content-Range, Accept-Ranges, Content-Length',
            });
            res.end(cuedMp4.subarray(s, e + 1));
            return;
          }
        }
        res.writeHead(200, {
          'Content-Type': 'video/mp4',
          'Accept-Ranges': 'bytes',
          'Content-Length': cuedMp4.length,
          'Access-Control-Allow-Origin': '*',
        });
        res.end(cuedMp4);
        return;
      }
      res.writeHead(200, {
        'Content-Type': 'audio/wav',
        'Content-Length': cuedWav.length,
        'Access-Control-Allow-Origin': '*',
      });
      res.end(cuedWav);
    });
    await new Promise((r) => corsAllowServer.listen(0, '127.0.0.1', r));
    const corsAllowOrigin = `http://127.0.0.1:${corsAllowServer.address().port}`;

    corsDenyServer = http.createServer((req, res) => {
      httpStats.corsDeniedHits++;
      res.writeHead(200, {
        'Content-Type': 'audio/wav',
        'Content-Length': cuedWav.length,
        // Intentionally omit Access-Control-Allow-Origin so Chrome enforces real cross-origin opacity!
      });
      res.end(cuedWav);
    });
    await new Promise((r) => corsDenyServer.listen(0, '127.0.0.1', r));
    const corsDenyOrigin = `http://127.0.0.1:${corsDenyServer.address().port}`;

    // 2. Start main publication & Range/Redirect server
    const mimeByExt = {
      '.html': 'text/html; charset=utf-8',
      '.js': 'text/javascript; charset=utf-8',
      '.mjs': 'text/javascript; charset=utf-8',
      '.wasm': 'application/wasm',
      '.json': 'application/json; charset=utf-8',
      '.mp4': 'video/mp4',
      '.wav': 'audio/wav',
    };
    pubServer = http.createServer((req, res) => {
      const cleanPath = decodeURIComponent((req.url || '/').split('?')[0]);
      if (cleanPath === '/media/redirect-motion.mp4') {
        httpStats.redirects++;
        res.writeHead(302, { Location: '/media/range-motion.mp4', 'Cache-Control': 'no-store' });
        res.end();
        return;
      }
      if (cleanPath === '/media/redirect-cors-wav') {
        httpStats.redirects++;
        res.writeHead(302, { Location: `${corsAllowOrigin}/cors-tone.wav`, 'Cache-Control': 'no-store' });
        res.end();
        return;
      }
      if (cleanPath === '/media/range-motion.mp4') {
        const range = req.headers.range;
        if (range) {
          httpStats.rangeHeaders.push(range);
          const m = /^bytes=(\d+)-(\d*)$/i.exec(range.trim());
          if (m) {
            const s = Math.min(cuedMp4.length - 1, Number(m[1]));
            const e = m[2] ? Math.min(cuedMp4.length - 1, Number(m[2])) : cuedMp4.length - 1;
            res.writeHead(206, {
              'Content-Type': 'video/mp4',
              'Accept-Ranges': 'bytes',
              'Content-Range': `bytes ${s}-${e}/${cuedMp4.length}`,
              'Content-Length': e - s + 1,
              'Cache-Control': 'no-store',
            });
            res.end(cuedMp4.subarray(s, e + 1));
            return;
          }
        }
        res.writeHead(200, {
          'Content-Type': 'video/mp4',
          'Accept-Ranges': 'bytes',
          'Content-Length': cuedMp4.length,
          'Cache-Control': 'no-store',
        });
        res.end(cuedMp4);
        return;
      }
      if (cleanPath === '/media/cued-audio.wav') {
        res.writeHead(200, { 'Content-Type': 'audio/wav', 'Content-Length': cuedWav.length, 'Cache-Control': 'no-store' });
        res.end(cuedWav);
        return;
      }
      if (cleanPath === '/media/slow-hang.mp4') {
        res.writeHead(200, { 'Content-Type': 'video/mp4', 'Content-Length': cuedMp4.length, 'Cache-Control': 'no-store' });
        res.write(cuedMp4.subarray(0, 32));
        req.on('close', () => {
          httpStats.slowAborted++;
          try { res.end(); } catch (_) {}
        });
        return;
      }

      const relPath = cleanPath === '/' ? 'index.html' : cleanPath.replace(/^\/+/, '');
      const candidate = path.resolve(workRoot, relPath);
      if (!candidate.startsWith(path.resolve(workRoot) + path.sep) || !fs.existsSync(candidate) || fs.statSync(candidate).isDirectory()) {
        res.writeHead(404, { 'Cache-Control': 'no-store' });
        res.end('Not found');
        return;
      }
      const ext = path.extname(candidate).toLowerCase();
      res.writeHead(200, {
        'Content-Type': mimeByExt[ext] || 'application/octet-stream',
        'Cache-Control': 'no-store',
      });
      fs.createReadStream(candidate).pipe(res);
    });
    await new Promise((r) => pubServer.listen(0, '127.0.0.1', r));
    const pubOrigin = `http://127.0.0.1:${pubServer.address().port}`;

    // 3. Build dual AS3 + AS2 project driving all T-327 workflows in real Chrome!
    const buildChromeProject = () => ({
      format_version: 9,
      name: 'T-327 Chrome Canvas+DOM Media Suite',
      width: 600,
      height: 220,
      fps: 24,
      background: [15, 23, 42, 255],
      root: 'root',
      document_class: 'app.ChromeMediaMain',
      resources: [
        {
          id: 'res-motion',
          name: 'motion.mp4',
          path: 'media/motion.mp4',
          media_type: 'video/mp4',
          data_base64: cuedMp4.toString('base64'),
        },
        {
          id: 'res-cued-wav',
          name: 'cued.wav',
          path: 'audio/cued.wav',
          media_type: 'audio/wav',
          data_base64: cuedWav.toString('base64'),
        },
      ],
      sources: [
        {
          id: 'chrome-main',
          path: 'app/ChromeMediaMain.as',
          language: 'action_script3',
          source: `package app {
  import flash.display.MovieClip;
  import flash.display.Sprite;
  import flash.events.Event;
  import flash.events.NetStatusEvent;
  import flash.external.ExternalInterface;
  import flash.media.Sound;
  import flash.media.SoundChannel;
  import flash.media.SoundLoaderContext;
  import flash.media.SoundMixer;
  import flash.media.SoundTransform;
  import flash.media.Video;
  import flash.net.NetConnection;
  import flash.net.NetStream;
  import flash.net.URLRequest;
  import flash.utils.ByteArray;
  import flash.utils.setTimeout;

  public dynamic class ChromeMediaMain extends MovieClip {
    public var holder:Sprite;
    public var vid:Video;
    public var nc:NetConnection;
    public var ns:NetStream;
    public var rtmpStatus:String = "";
    public var statusCodes:Array = [];
    public var playStatuses:Array = [];
    public var cues:Array = [];
    public var metas:Array = [];
    private var pollTicks:int = 0;

    public function ChromeMediaMain() {
      stop();
      holder = new Sprite();
      holder.x = 20;
      holder.y = 20;
      addChild(holder);
      vid = new Video(240, 135);
      vid.smoothing = true;
      holder.addChild(vid);

      var badNc:NetConnection = new NetConnection();
      badNc.addEventListener(NetStatusEvent.NET_STATUS, function(e:NetStatusEvent):void {
        rtmpStatus = e.info.code;
      });
      badNc.connect("rtmp://live.example.test/app");

      nc = new NetConnection();
      nc.addEventListener(NetStatusEvent.NET_STATUS, onConn);
      nc.connect(null);
    }

    private function onConn(ev:NetStatusEvent):void {
      if (ev.info.code != "NetConnection.Connect.Success") return;
      ns = new NetStream(nc);
      ns.client = {
        onMetaData: function(info:Object):void { metas.push(info); },
        onPlayStatus: function(info:Object):void { playStatuses.push(info.code); },
        onCuePoint: function(info:Object):void { cues.push(info.name); }
      };
      ns.addEventListener(NetStatusEvent.NET_STATUS, function(se:NetStatusEvent):void {
        statusCodes.push(se.info.code);
      });
      vid.attachNetStream(ns);
      if (ExternalInterface.available) {
        ExternalInterface.addCallback("startMediaSuite", step1SingleFrameAndVideoPixels);
        ExternalInterface.addCallback("closeAs3Stream", closeAllForAudit);
      }
      trace("BOOT_READY=1");
    }

    private function step1SingleFrameAndVideoPixels():void {
      statusCodes = [];
      playStatuses = [];
      pollTicks = 0;
      // Single-frame load at start=0.8s, len=0!
      ns.play("media/motion.mp4", 0.8, 0, true);
      setTimeout(pollStep1, 50);
    }

    private function pollStep1():void {
      pollTicks++;
      if (playStatuses.indexOf("NetStream.Play.Complete") < 0 && pollTicks < 120) {
        setTimeout(pollStep1, 50);
        return;
      }
      checkStep1();
    }

    private function checkStep1():void {
      var ok1:Boolean = disposeQuotaProbe()
        && (rtmpStatus == "NetConnection.Connect.Rejected")
        && (statusCodes.indexOf("NetStream.Play.Start") >= 0)
        && (statusCodes.indexOf("NetStream.Buffer.Full") >= 0)
        && (statusCodes.indexOf("NetStream.Play.Stop") >= 0)
        && (playStatuses.indexOf("NetStream.Play.Complete") >= 0)
        && (ns.bytesLoaded == ${cuedMp4.length})
        && (ns.bytesTotal == ${cuedMp4.length});
      trace("STEP1_SINGLE_FRAME=" + ok1 + " codes=" + statusCodes.join(","));
      step2PlaylistAndCues();
    }

    private function disposeQuotaProbe():Boolean {
      try {
        for (var i:int = 0; i < 65; i++) {
          var transient:NetStream = new NetStream(nc);
          transient.dispose();
        }
        return true;
      } catch (error:*) {
        return false;
      }
    }

    private function step2PlaylistAndCues():void {
      statusCodes = [];
      playStatuses = [];
      cues = [];
      metas = [];
      pollTicks = 0;
      // Apply stereo pan then 2x2 custom matrix on NetStream + parent Sprite transform!
      holder.soundTransform = new SoundTransform(0.5, 0);
      var st:SoundTransform = new SoundTransform(0.6, -0.4);
      ns.soundTransform = st;
      var mat:SoundTransform = new SoundTransform(0.5, 0);
      mat.leftToLeft = 0.2;
      mat.leftToRight = 0.7;
      mat.rightToLeft = 0.5;
      mat.rightToRight = 0.3;
      ns.soundTransform = mat;

      // Bounded clip 1 (start=0.05, len=0.30 -> crosses mp4_cue_1 @ 0.20s) + queued clip 2 (audio/cued.wav, reset=false -> crosses chrome_cue @ 0.12s)!
      ns.play("media/motion.mp4", 0.05, 0.30, true);
      ns.play("/media/cued-audio.wav", -2, -1, false);
      setTimeout(pollStep2, 60);
    }

    private function pollStep2():void {
      pollTicks++;
      var ready2:Boolean = (playStatuses.indexOf("NetStream.Play.Switch") >= 0)
        && (playStatuses.indexOf("NetStream.Play.Complete") >= 0)
        && (cues.indexOf("mp4_cue_1") >= 0)
        && (cues.indexOf("chrome_cue") >= 0)
        && (metas.length >= 2);
      if (!ready2 && pollTicks < 120) {
        setTimeout(pollStep2, 60);
        return;
      }
      checkStep2();
    }

    private function checkStep2():void {
      var ok2:Boolean = (statusCodes.indexOf("NetStream.Play.Transition") < 0)
        && (playStatuses.indexOf("NetStream.Play.Switch") >= 0)
        && (playStatuses.indexOf("NetStream.Play.TransitionComplete") < 0)
        && (playStatuses.indexOf("NetStream.Play.Complete") >= 0)
        && (cues.indexOf("mp4_cue_1") >= 0)
        && (cues.indexOf("chrome_cue") >= 0)
        && (metas.length >= 2);
      trace("STEP2_PLAYLIST_CUES=" + ok2 + " cues=" + cues.join(",") + " play=" + playStatuses.join(","));
      step3RangeRedirectAndSeek();
    }

    private function step3RangeRedirectAndSeek():void {
      statusCodes = [];
      metas = [];
      pollTicks = 0;
      // Progressive HTTP 302 -> 206 Partial Content Range video!
      ns.play("/media/redirect-motion.mp4", 0, -1, true);
      setTimeout(pollStep3Meta, 50);
    }

    private function pollStep3Meta():void {
      pollTicks++;
      if ((metas.length < 1 || ns.bytesTotal <= 0 || ns.bytesLoaded <= 0) && pollTicks < 120) {
        setTimeout(pollStep3Meta, 50);
        return;
      }
      doSeekChecks();
    }

    private function doSeekChecks():void {
      pollTicks = 0;
      ns.pause();
      ns.seek(1.2);
      ns.seek(-2);
      ns.seek(99);
      setTimeout(pollStep3Seek, 50);
    }

    private function pollStep3Seek():void {
      pollTicks++;
      var seekDone:Boolean = (statusCodes.indexOf("NetStream.Seek.Notify") >= 0)
        && (statusCodes.indexOf("NetStream.Seek.InvalidTime") >= 0);
      if (!seekDone && pollTicks < 120) {
        setTimeout(pollStep3Seek, 50);
        return;
      }
      checkStep3();
    }

    private function checkStep3():void {
      var ok3:Boolean = (metas.length >= 1)
        && (ns.bytesTotal == ${cuedMp4.length})
        && (ns.bytesLoaded > 0)
        && (statusCodes.indexOf("NetStream.Seek.Notify") >= 0)
        && (statusCodes.indexOf("NetStream.Seek.InvalidTime") >= 0);
      trace("STEP3_HTTP_RANGE_SEEK=" + ok3 + " loaded=" + ns.bytesLoaded + "/" + ns.bytesTotal + " codes=" + statusCodes.join(","));
      // Resume playing embedded MP4 so pixel check sees live playing video in both AS3 & AS2 viewports!
      ns.play("media/motion.mp4", 0.5, -1, true);
      step4CorsAndSoundMatrix();
    }

    private function step4CorsAndSoundMatrix():void {
      var corsOkSound:Sound = new Sound();
      corsOkSound.addEventListener(Event.COMPLETE, function(_e:Event):void {
        var okAccess:Boolean = (corsOkSound.isURLInaccessible == false);
        var matTf:SoundTransform = new SoundTransform(0.4, 0);
        matTf.leftToLeft = 0.1;
        matTf.leftToRight = 0.8;
        matTf.rightToLeft = 0.6;
        matTf.rightToRight = 0.2;
        var ch1:SoundChannel = corsOkSound.play(0, 0, matTf);

        var denySound:Sound = new Sound();
        denySound.addEventListener(Event.COMPLETE, function(_e2:Event):void {
          var denyFlag:Boolean = (denySound.isURLInaccessible == true);
          var ch2:SoundChannel = denySound.play(0, 0, new SoundTransform(0.2, 0));
          var mixerInacc:Boolean = SoundMixer.areSoundsInaccessible();
          var extractBlocked:Boolean = false;
          var spectrumBlocked:Boolean = false;
          var ba:ByteArray = new ByteArray();
          try { denySound.extract(ba, 32, 0); } catch (_err1:Error) { extractBlocked = true; }
          try { SoundMixer.computeSpectrum(ba, false, 0); } catch (_err2:Error) { spectrumBlocked = true; }
          ch2.stop();
          ch1.stop();
          var mixerCleared:Boolean = (SoundMixer.areSoundsInaccessible() == false);
          trace("STEP4_CORS_SOUND=" + (okAccess && denyFlag && mixerInacc && extractBlocked && spectrumBlocked && mixerCleared));
          trace("ALL_MEDIA_CHECKS_READY=1");
        });
        denySound.load(new URLRequest("${corsDenyOrigin}/opaque-tone.wav"));
      });
      corsOkSound.load(new URLRequest("${pubOrigin}/media/redirect-cors-wav"), new SoundLoaderContext(1000, true));
    }

    public function closeAllForAudit():void {
      // Start an in-flight slow request and immediately close() to verify AbortController + Blob + media element cleanup!
      ns.play("/media/slow-hang.mp4");
      ns.close();
      trace("CLOSED_FOR_AUDIT=1");
    }
  }
}`,
        },
      ],
      symbols: [
        {
          id: 'mc-as2-snd',
          name: 'As2SndClip',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [{ id: 'al', name: 'L', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
            scripts: [],
          },
        },
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [
              {
                id: 'main-layer',
                name: 'Main',
                visible: true,
                locked: false,
                keyframes: [
                  {
                    frame: 0,
                    label: '',
                    tween: null,
                    elements: [
                      {
                        id: 'as2-vid',
                        name: 'as2Video',
                        transform: { x: 320, y: 20, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'video', width: 240, height: 135 },
                        scripts: [],
                      },
                      {
                        id: 'as2-snd-inst',
                        name: 'as2SndMc',
                        transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                        kind: { type: 'instance', symbol_id: 'mc-as2-snd', playback: 'independent' },
                        scripts: [],
                      },
                    ],
                  },
                ],
              },
            ],
            scripts: [
              {
                frame: 0,
                script: {
                  language: 'action_script2',
                  source: `import flash.external.ExternalInterface;
var as2Nc:NetConnection = new NetConnection();
var as2Ns:NetStream = null;
as2Nc.onStatus = function(info:Object):Void {
  if (info.code == "NetConnection.Connect.Success") {
    as2Ns = new NetStream(as2Nc);
    as2Video.attachVideo(as2Ns);
    as2Ns.play("media/motion.mp4", 0.25, -1, true);
  }
};
as2Nc.connect(null);
function closeAs2():Void {
  if (as2Ns != null) {
    as2Ns.close();
  }
}
if (ExternalInterface.available) {
  ExternalInterface.addCallback("closeAs2Stream", this, closeAs2);
}

var as2SndA:Sound = new Sound(as2SndMc);
var as2SndB:Sound = new Sound(as2SndMc);
as2SndA.setTransform({ ll: 30, lr: 70, rl: 40, rr: 60 });
var as2Tf:Object = as2SndB.getTransform();
trace("AS2_SND_SHARED=" + (as2Tf.ll == 30 && as2Tf.lr == 70 && as2Tf.rl == 40 && as2Tf.rr == 60));
`,
                },
              },
            ],
          },
        },
      ],
    });

    const projPath = path.join(workRoot, 'media-suite.nflash');
    fs.writeFileSync(projPath, JSON.stringify(buildChromeProject(), null, 2));

    for (const mode of ['canvas', 'dom']) {
      execFileSync(
        bin,
        ['--runtime', runtimeDir, projPath, '--output', path.join(workRoot, `pub-${mode}`), '--render-mode', mode],
        { cwd: repoRoot, encoding: 'utf8' }
      );
    }

    cdp = await launchCdpSession(chromeBin, chromeProfileDir);

    for (const mode of ['canvas', 'dom']) {
      await cdp.send('Emulation.setDeviceMetricsOverride', {
        width: 640,
        height: 300,
        deviceScaleFactor: 1,
        mobile: false,
      });

      await cdp.navigateAndWait(`${pubOrigin}/pub-${mode}/index.html`);
      await cdp.waitTrace('BOOT_READY=1');
      // Click audio-toggle with a real trusted user gesture so Web Audio & unmuted media paths run unlocked in Chrome!
      const toggleRect = await cdp.evalJson(`(() => {
        const btn = document.getElementById('audio-toggle');
        if (!btn || btn.hidden) return null;
        const r = btn.getBoundingClientRect();
        return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
      })()`);
      if (toggleRect) {
        await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: toggleRect.x, y: toggleRect.y, button: 'left', clickCount: 1 });
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: toggleRect.x, y: toggleRect.y, button: 'left', clickCount: 1 });
      } else {
        await cdp.evalJson(`(() => { const btn = document.getElementById('audio-toggle'); if (btn) btn.click(); return true; })()`, { userGesture: true });
      }
      await cdp.evalJson(`(() => {
        const stage = document.getElementById('stage');
        if (typeof stage?.startMediaSuite === 'function') stage.startMediaSuite();
        return true;
      })()`, { userGesture: true });

      await cdp.waitTrace('ALL_MEDIA_CHECKS_READY=1', 25000);

      const traceDump = await cdp.evalJson(`window.__nfTraceLog ? window.__nfTraceLog() : (document.getElementById('trace')?.textContent || '')`);
      assert.ok(traceDump.includes('AS2_SND_SHARED=true'), `[${mode}] AS2 Sound(mc) shared 2x2 matrix failed:\n${traceDump}`);
      assert.ok(traceDump.includes('STEP1_SINGLE_FRAME=true'), `[${mode}] Step 1 (RTMP reject + len=0 single-frame) failed:\n${traceDump}`);
      assert.ok(traceDump.includes('STEP2_PLAYLIST_CUES=true'), `[${mode}] Step 2 (len>0 + reset=false playlist + MP4/WAV cues) failed:\n${traceDump}`);
      assert.ok(traceDump.includes('STEP3_HTTP_RANGE_SEEK=true'), `[${mode}] Step 3 (HTTP 302 -> 206 Range + Seek.InvalidTime) failed:\n${traceDump}`);
      assert.ok(traceDump.includes('STEP4_CORS_SOUND=true'), `[${mode}] Step 4 (CORS Sound.isURLInaccessible + SecurityError) failed:\n${traceDump}`);

      // Verify real decoded video pixels in BOTH AS3 (attachNetStream) and AS2 (attachVideo) viewports!
      if (mode === 'canvas') {
        let pixelAudit = { as3Lit: 0, as2Lit: 0 };
        const pixelDeadline = Date.now() + 5000;
        while (Date.now() < pixelDeadline) {
          pixelAudit = await cdp.evalJson(`(() => {
            const c = document.getElementById('stage');
            const ctx = c.getContext('2d');
            function countNonBg(x, y, w, h) {
              const d = ctx.getImageData(x, y, w, h).data;
              let lit = 0;
              for (let i = 0; i < d.length; i += 16) {
                if (Math.abs(d[i] - 15) > 10 || Math.abs(d[i + 1] - 23) > 10 || Math.abs(d[i + 2] - 42) > 10) lit++;
              }
              return lit;
            }
            return {
              as3Lit: countNonBg(30, 30, 200, 100),
              as2Lit: countNonBg(330, 30, 200, 100),
            };
          })()`);
          if (pixelAudit.as3Lit > 50 && pixelAudit.as2Lit > 50) break;
          await new Promise((r) => setTimeout(r, 50));
        }
        assert.ok(pixelAudit.as3Lit > 50, `[canvas] AS3 Video.attachNetStream must paint real decoded MP4 pixels (lit=${pixelAudit.as3Lit})`);
        assert.ok(pixelAudit.as2Lit > 50, `[canvas] AS2 Video.attachVideo must paint real decoded MP4 pixels (lit=${pixelAudit.as2Lit})`);
      } else {
        let domVideoAudit = { count: 0, samples: [] };
        const domDeadline = Date.now() + 5000;
        while (Date.now() < domDeadline) {
          domVideoAudit = await cdp.evalJson(`(() => {
            const domRoot = document.getElementById('stage-dom-root');
            const vids = [...(domRoot?.querySelectorAll('canvas.nf-dom-video[data-nf-kind="video"]') || [])];
            const samples = vids.map((vc) => {
              const ctx = vc.getContext('2d');
              const d = ctx.getImageData(0, 0, vc.width, vc.height).data;
              let lit = 0;
              for (let i = 0; i < d.length; i += 16) {
                if (d[i + 3] > 0 && (d[i] > 5 || d[i + 1] > 5 || d[i + 2] > 5)) lit++;
              }
              return { width: vc.width, height: vc.height, lit };
            });
            return { count: vids.length, samples };
          })()`);
          if (domVideoAudit.count === 2 && domVideoAudit.samples.every((s) => s.lit > 50)) break;
          await new Promise((r) => setTimeout(r, 50));
        }
        assert.equal(domVideoAudit.count, 2, `[dom] Both AS3 and AS2 Video instances must mount .nf-dom-video leaves`);
        assert.ok(
          domVideoAudit.samples.every((s) => s.lit > 50),
          `[dom] Both .nf-dom-video leaves must paint real decoded MP4 pixels: ${JSON.stringify(domVideoAudit.samples)}`
        );
      }

      // Verify Web Audio nodes (StereoPannerNode + ChannelSplitter/Merger 2x2 matrix + MediaElementAudioSourceNode) were genuinely constructed!
      const preCloseAudit = await cdp.evalJson(`window.__nfMediaAudit()`);
      assert.ok(preCloseAudit.splitterCount >= 2 && preCloseAudit.mergerCount >= 2, `[${mode}] Web Audio 2x2 ChannelSplitter/Merger matrix must activate for NetStream & SoundChannel`);
      assert.ok(preCloseAudit.pannerCount >= 1, `[${mode}] Web Audio StereoPannerNode must activate for pure stereo pan`);
      assert.ok(preCloseAudit.mediaElemSourceCount >= 1, `[${mode}] MediaElementAudioSourceNode must route NetStream through Web Audio`);

      // Trigger close() on AS3 (including in-flight slow-hang fetch abort) & AS2 streams, then verify zero live Blobs / active media elements!
      const closeAudit = await cdp.evalJson(`(() => {
        const stage = document.getElementById('stage');
        if (typeof stage?.closeAs3Stream === 'function') stage.closeAs3Stream();
        if (typeof stage?.closeAs2Stream === 'function') stage.closeAs2Stream();
        return window.__nfMediaAudit();
      })()`);
      assert.equal(closeAudit.liveBlobCount, 0, `[${mode}] NetStream.close() must revoke all Blob object URLs`);
      assert.equal(closeAudit.activeMediaElements, 0, `[${mode}] NetStream.close() must pause and detach all <video>/<audio> elements`);
      assert.ok(closeAudit.abortCount >= 1, `[${mode}] NetStream.close() must abort pending fetch AbortControllers`);

      // Re-open streams via player #restart, verify prior session cleanup, then close again to confirm zero leaks!
      await cdp.evalJson(`(() => {
        if (window.__nfClearTraceLog) window.__nfClearTraceLog();
        const restartBtn = document.getElementById('restart');
        if (restartBtn) restartBtn.click();
        return true;
      })()`, { userGesture: true });
      await cdp.waitTrace('BOOT_READY=1');

      await cdp.evalJson(`new Promise((resolve) => setTimeout(resolve, 80))`);
      const postRestartCloseAudit = await cdp.evalJson(`(() => {
        const stage = document.getElementById('stage');
        if (typeof stage?.closeAs3Stream === 'function') stage.closeAs3Stream();
        if (typeof stage?.closeAs2Stream === 'function') stage.closeAs2Stream();
        return window.__nfMediaAudit();
      })()`);
      assert.equal(postRestartCloseAudit.liveBlobCount, 0, `[${mode}] Post-Restart close must leave 0 live Blob URLs`);
      assert.equal(postRestartCloseAudit.activeMediaElements, 0, `[${mode}] Post-Restart close must leave 0 active <video>/<audio> elements`);
      assert.ok(postRestartCloseAudit.totalBlobsRevoked >= 4, `[${mode}] Restart + close must revoke all created Blob URLs (revoked=${postRestartCloseAudit.totalBlobsRevoked})`);
      assert.ok(httpStats.slowAborted >= 1 || postRestartCloseAudit.abortCount >= 2, `[${mode}] Restart/close lifecycle must abort pending fetch AbortControllers`);
    }

    assert.ok(httpStats.redirects >= 2, 'Chrome suite must exercise real 302 HTTP redirects');
    assert.ok(httpStats.rangeHeaders.length >= 2, 'Chrome suite must exercise real 206 HTTP Range requests');
    assert.ok(httpStats.corsAllowedHits >= 1 && httpStats.corsDeniedHits >= 1, 'Chrome suite must hit both CORS-allowed and CORS-denied origins');

    await cdp.navigateAndWait('about:blank');
    // Filter out Chrome's expected console CORS network message from the intentional corsDenyServer SOP block!
    const unexpectedErrors = cdp.errors.filter((e) => !e.includes('Access-Control-Allow-Origin') && !e.includes('Failed to load resource'));
    assert.deepEqual(unexpectedErrors, [], `Uncaught runtime errors in Chrome T-327 suite:\n${unexpectedErrors.join('\n')}`);
  } finally {
    if (cdp) await cdp.close();
    for (const srv of [pubServer, corsAllowServer, corsDenyServer]) {
      if (srv) {
        srv.closeAllConnections?.();
        await new Promise((r) => srv.close(r));
      }
    }
    fs.rmSync(workRoot, { recursive: true, force: true });
  }
});
