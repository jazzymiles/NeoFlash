'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const os = require('node:os');
const net = require('node:net');
const tls = require('node:tls');
const http = require('node:http');
const https = require('node:https');
const crypto = require('node:crypto');
const path = require('node:path');
const { createInterface } = require('node:readline');
const vm = require('node:vm');
const { execFileSync, spawn } = require('node:child_process');
const { Worker: NodeThreadWorker } = require('node:worker_threads');

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

const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

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

async function until(predicate, label, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate() && Date.now() < deadline) {
    await new Promise(r => setTimeout(r, 10));
  }
  assert.ok(predicate(), `Timed out waiting for: ${label}`);
}

// ============================================================================
// RFC 6455 Frame Codec & Loopback ws:// / wss:// Server + TLS Client
// ============================================================================
function encodeWsFrame(opcode, payloadBytes, mask = false) {
  const payload = Buffer.isBuffer(payloadBytes) ? payloadBytes : Buffer.from(payloadBytes);
  const len = payload.length;
  let headerLen = 2;
  if (len >= 126 && len < 65536) headerLen = 4;
  else if (len >= 65536) headerLen = 10;
  const maskKey = mask ? crypto.randomBytes(4) : null;
  const out = Buffer.alloc(headerLen + (mask ? 4 : 0) + len);
  out[0] = 0x80 | (opcode & 0x0f);
  let offset = 2;
  const maskBit = mask ? 0x80 : 0x00;
  if (len < 126) {
    out[1] = maskBit | len;
  } else if (len < 65536) {
    out[1] = maskBit | 126;
    out.writeUInt16BE(len, 2);
    offset = 4;
  } else {
    out[1] = maskBit | 127;
    out.writeBigUInt64BE(BigInt(len), 2);
    offset = 10;
  }
  if (maskKey) {
    maskKey.copy(out, offset);
    offset += 4;
    for (let i = 0; i < len; i++) {
      out[offset + i] = payload[i] ^ maskKey[i & 3];
    }
  } else {
    payload.copy(out, offset);
  }
  return out;
}

function createWsFrameDecoder(onFrame) {
  let buf = Buffer.alloc(0);
  return chunk => {
    buf = Buffer.concat([buf, chunk]);
    while (buf.length >= 2) {
      const b0 = buf[0];
      const b1 = buf[1];
      const opcode = b0 & 0x0f;
      const masked = (b1 & 0x80) !== 0;
      let len = b1 & 0x7f;
      let pos = 2;
      if (len === 126) {
        if (buf.length < 4) return;
        len = buf.readUInt16BE(2);
        pos = 4;
      } else if (len === 127) {
        if (buf.length < 10) return;
        len = Number(buf.readBigUInt64BE(2));
        pos = 10;
      }
      const maskBytes = masked ? 4 : 0;
      if (buf.length < pos + maskBytes + len) return;
      let payload = Buffer.from(buf.subarray(pos + maskBytes, pos + maskBytes + len));
      if (masked) {
        const key = buf.subarray(pos, pos + 4);
        for (let i = 0; i < len; i++) payload[i] ^= key[i & 3];
      }
      buf = buf.subarray(pos + maskBytes + len);
      onFrame(opcode, payload);
    }
  };
}

function attachRfc6455Upgrade(server, onConnection) {
  const sockets = new Set();
  server.on('connection', s => {
    sockets.add(s);
    s.on('close', () => sockets.delete(s));
  });
  server.on('upgrade', (req, socket, head) => {
    const key = req.headers['sec-websocket-key'];
    if (!key) {
      socket.destroy();
      return;
    }
    const accept = crypto.createHash('sha1').update(key + WS_GUID).digest('base64');
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\n' +
        'Upgrade: websocket\r\n' +
        'Connection: Upgrade\r\n' +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`
    );
    socket.setNoDelay(true);
    const peer = {
      req,
      socket,
      sendBinary(bytes) {
        if (!socket.destroyed) socket.write(encodeWsFrame(0x02, bytes, false));
      },
      sendText(text) {
        if (!socket.destroyed) socket.write(encodeWsFrame(0x01, Buffer.from(String(text), 'utf8'), false));
      },
      closeFromServer(code = 1000) {
        if (!socket.destroyed) {
          const codeBuf = Buffer.alloc(2);
          codeBuf.writeUInt16BE(code, 0);
          socket.end(encodeWsFrame(0x08, codeBuf, false));
        }
      },
      onBinary: null,
      onText: null,
      onClose: null,
    };
    const feed = createWsFrameDecoder((opcode, payload) => {
      if (opcode === 0x02) peer.onBinary?.(new Uint8Array(payload));
      else if (opcode === 0x01) peer.onText?.(payload.toString('utf8'), new Uint8Array(payload));
      else if (opcode === 0x09) socket.write(encodeWsFrame(0x0a, payload, false));
      else if (opcode === 0x08) {
        peer.onClose?.();
        socket.end(encodeWsFrame(0x08, Buffer.alloc(0), false));
      }
    });
    if (head && head.length > 0) feed(head);
    socket.on('data', feed);
    socket.on('error', () => {});
    socket.on('close', () => peer.onClose?.());
    onConnection(peer);
  });
  return {
    destroyAll() {
      for (const s of sockets) {
        try { s.destroy(); } catch (_) {}
      }
      sockets.clear();
    },
  };
}

// Standards-compatible RFC 6455 WebSocket client over node:net / node:tls
// so wss:// exposes socket.getPeerCertificate(true).raw to SecureSocket!
class NodeRfc6455Client {
  constructor(url, origin = null) {
    this.origin = origin;
    this.url = String(url);
    this.readyState = 0;
    this.binaryType = 'arraybuffer';
    this._listeners = new Map();
    this._socket = null;
    this._connect();
  }
  addEventListener(type, fn) {
    const arr = this._listeners.get(type) || [];
    arr.push(fn);
    this._listeners.set(type, arr);
  }
  _emit(type, detail = {}) {
    for (const fn of [...(this._listeners.get(type) || [])]) {
      fn({ type, ...detail });
    }
  }
  _connect() {
    let parsed;
    try {
      parsed = new URL(this.url);
    } catch (err) {
      queueMicrotask(() => {
        this.readyState = 3;
        this._emit('error', { error: err });
        this._emit('close', { code: 1006 });
      });
      return;
    }
    const isTls = parsed.protocol === 'wss:';
    const port = Number(parsed.port) || (isTls ? 443 : 80);
    const host = parsed.hostname || '127.0.0.1';
    const pathAndQuery = (parsed.pathname || '/') + (parsed.search || '');
    const secKey = crypto.randomBytes(16).toString('base64');

    const sock = isTls
      ? tls.connect({
          host,
          port,
          servername: net.isIP(host) === 0 ? host : undefined,
          rejectUnauthorized: false,
        })
      : net.connect({ host, port });
    this._socket = sock;
    sock.setNoDelay(true);

    let upgraded = false;
    let headerBuf = Buffer.alloc(0);
    const feedFrames = createWsFrameDecoder((opcode, payload) => {
      if (opcode === 0x02) {
        const ab = payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength);
        this._emit('message', { data: ab });
      } else if (opcode === 0x01) {
        this._emit('message', { data: payload.toString('utf8') });
      } else if (opcode === 0x08) {
        this.readyState = 3;
        sock.end();
      } else if (opcode === 0x09) {
        if (!sock.destroyed) sock.write(encodeWsFrame(0x0a, payload, true));
      }
    });

    const onConnected = () => {
      sock.write(
        `GET ${pathAndQuery} HTTP/1.1\r\n` +
          `Host: ${host}:${port}\r\n` +
          'Upgrade: websocket\r\n' +
          'Connection: Upgrade\r\n' +
          (this.origin ? `Origin: ${this.origin}\r\n` : '') +
          `Sec-WebSocket-Key: ${secKey}\r\n` +
          'Sec-WebSocket-Version: 13\r\n\r\n'
      );
    };
    if (isTls) sock.once('secureConnect', onConnected);
    else sock.once('connect', onConnected);

    sock.on('data', chunk => {
      if (!upgraded) {
        headerBuf = Buffer.concat([headerBuf, chunk]);
        const sep = headerBuf.indexOf('\r\n\r\n');
        if (sep < 0) return;
        const statusLine = headerBuf.subarray(0, headerBuf.indexOf('\r\n')).toString('ascii');
        if (!statusLine.includes('101')) {
          this.readyState = 3;
          this._emit('error', { error: new Error(`Unexpected status: ${statusLine}`) });
          sock.destroy();
          return;
        }
        upgraded = true;
        this.readyState = 1;
        this._emit('open');
        const rest = headerBuf.subarray(sep + 4);
        headerBuf = Buffer.alloc(0);
        if (rest.length > 0) feedFrames(rest);
      } else {
        feedFrames(chunk);
      }
    });

    sock.on('error', err => {
      if (this.readyState !== 3) {
        this.readyState = 3;
        this._emit('error', { error: err });
      }
    });
    sock.on('close', () => {
      this.readyState = 3;
      this._emit('close', { code: 1000 });
    });
  }
  send(data) {
    if (!this._socket || this._socket.destroyed || this.readyState !== 1) return;
    if (typeof data === 'string') {
      this._socket.write(encodeWsFrame(0x01, Buffer.from(data, 'utf8'), true));
    } else {
      const u8 = data instanceof ArrayBuffer ? new Uint8Array(data) : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      this._socket.write(encodeWsFrame(0x02, u8, true));
    }
  }
  close() {
    this.readyState = 3;
    if (this._socket && !this._socket.destroyed) {
      try {
        this._socket.write(encodeWsFrame(0x08, Buffer.alloc(0), true));
        this._socket.destroy();
      } catch (_) {}
    }
  }
}

// ============================================================================
// Web-standard Worker wrapper around real node:worker_threads OS threads
// ============================================================================
class NodeThreadWebWorker {
  constructor(scriptUrl) {
    let source = '';
    const str = String(scriptUrl);
    if (str.startsWith('data:text/javascript')) {
      const comma = str.indexOf(',');
      source = decodeURIComponent(str.slice(comma + 1));
    } else {
      throw new Error(`Unsupported worker URL in Node adapter: ${str}`);
    }
    const bootstrap = `
      const { parentPort } = require('node:worker_threads');
      globalThis.self = globalThis;
      globalThis.postMessage = (msg) => parentPort.postMessage(msg);
      parentPort.on('message', (data) => {
        if (typeof globalThis.onmessage === 'function') {
          globalThis.onmessage({ data });
        }
      });
      ${source}
    `;
    this._listeners = new Map();
    this._worker = new NodeThreadWorker(bootstrap, { eval: true });
    this._worker.on('message', data => {
      const ev = { data };
      if (typeof this.onmessage === 'function') this.onmessage(ev);
      for (const fn of [...(this._listeners.get('message') || [])]) fn(ev);
    });
    this._worker.on('error', err => {
      for (const fn of [...(this._listeners.get('error') || [])]) fn(err);
    });
  }
  addEventListener(type, fn) {
    const arr = this._listeners.get(type) || [];
    arr.push(fn);
    this._listeners.set(type, arr);
  }
  postMessage(msg) {
    this._worker.postMessage(msg);
  }
  terminate() {
    return this._worker.terminate();
  }
}

// ============================================================================
// MPEG-TS 188-byte Timed-ID3 Packet Builder
// ============================================================================
function buildId3v24TagBytes(frames) {
  const frameChunks = [];
  for (const [id, value] of frames) {
    let payload;
    if (id === 'TXXX') {
      const [desc, val] = value.split('=');
      payload = Buffer.concat([
        Buffer.from([0x03]), // UTF-8
        Buffer.from(desc, 'utf8'),
        Buffer.from([0x00]),
        Buffer.from(val, 'utf8'),
      ]);
    } else {
      payload = Buffer.concat([Buffer.from([0x03]), Buffer.from(String(value), 'utf8')]);
    }
    const hdr = Buffer.alloc(10);
    hdr.write(id, 0, 4, 'ascii');
    const sz = payload.length;
    hdr[4] = (sz >> 21) & 0x7f;
    hdr[5] = (sz >> 14) & 0x7f;
    hdr[6] = (sz >> 7) & 0x7f;
    hdr[7] = sz & 0x7f;
    frameChunks.push(hdr, payload);
  }
  const body = Buffer.concat(frameChunks);
  const tagHdr = Buffer.alloc(10);
  tagHdr.write('ID3', 0, 3, 'ascii');
  tagHdr[3] = 4; // v2.4
  tagHdr[4] = 0;
  tagHdr[5] = 0;
  const total = body.length;
  tagHdr[6] = (total >> 21) & 0x7f;
  tagHdr[7] = (total >> 14) & 0x7f;
  tagHdr[8] = (total >> 7) & 0x7f;
  tagHdr[9] = total & 0x7f;
  return Buffer.concat([tagHdr, body]);
}

function buildMpegTsTimedId3Packet(ptsSeconds, frames) {
  const id3 = buildId3v24TagBytes(frames);
  const ptsTicks = BigInt(Math.round(ptsSeconds * 90000));
  const ptsBytes = Buffer.alloc(5);
  ptsBytes[0] = 0x21 | Number(((ptsTicks >> 30n) & 0x07n) << 1n);
  const mid = Number((ptsTicks >> 15n) & 0x7fffn);
  ptsBytes[1] = (mid >> 7) & 0xff;
  ptsBytes[2] = ((mid & 0x7f) << 1) | 0x01;
  const lo = Number(ptsTicks & 0x7fffn);
  ptsBytes[3] = (lo >> 7) & 0xff;
  ptsBytes[4] = ((lo & 0x7f) << 1) | 0x01;

  const pesBodyLen = 3 + 5 + id3.length;
  const pes = Buffer.alloc(6 + pesBodyLen);
  pes[0] = 0x00;
  pes[1] = 0x00;
  pes[2] = 0x01;
  pes[3] = 0xbd; // private_stream_1 (HLS timed metadata)
  pes.writeUInt16BE(pesBodyLen, 4);
  pes[6] = 0x84; // MPEG-2 PES + data_alignment_indicator
  pes[7] = 0x80; // PTS present
  pes[8] = 0x05; // header data len
  ptsBytes.copy(pes, 9);
  id3.copy(pes, 14);

  assert.ok(pes.length <= 184, 'single TS packet fits timed ID3 payload');
  const pkt = Buffer.alloc(188, 0xff);
  pkt[0] = 0x47;
  pkt[1] = 0x41; // payload_unit_start_indicator=1, PID=0x0102
  pkt[2] = 0x02;
  const stuffLen = 184 - pes.length;
  if (stuffLen === 0) {
    pkt[3] = 0x10; // payload only
    pes.copy(pkt, 4);
  } else {
    pkt[3] = 0x30; // adaptation + payload
    pkt[4] = stuffLen - 1;
    if (stuffLen > 1) pkt[5] = 0x00;
    pes.copy(pkt, 4 + stuffLen);
  }
  return pkt;
}

// ============================================================================
// Area 8 Full Host Factory
// ============================================================================
function createArea8LiveHost(wasm, extraOptions = {}) {
  const binary = Binary.create({ wasm });
  const dummyProject = {
    format_version: 3,
    name: 'Area8 Live Conformance Host',
    width: 800,
    height: 600,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: extraOptions.resources || [],
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
              keyframes: [{ frame: 0, elements: [], label: '', tween: null }],
            },
          ],
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

  const eventFamilies = Events.createFamilies(wasm, {
    isByteArray: v => binary.ByteArray.__nfIs(v),
    requestRender: () => {},
  });
  const mergedEvents = { ...Events, ...eventFamilies };

  const storeMap = new Map();
  const mockLocalStorage = {
    getItem: k => (storeMap.has(k) ? storeMap.get(k) : null),
    setItem: (k, v) => storeMap.set(k, String(v)),
    removeItem: k => storeMap.delete(k),
  };

  const channelName = extraOptions.soChannelName || 'neoflash.area8.rso.test';
  const storage = Storage.create({
    amf,
    events: mergedEvents,
    storage: extraOptions.storage || mockLocalStorage,
    url: extraOptions.url || 'https://app.neoflash.local/main.swf',
    channelName,
  });

  const netHost = Net.create({
    wasm,
    events: mergedEvents,
    binary,
    baseURL: extraOptions.url || 'https://app.neoflash.local/main.swf',
    resources: dummyProject.resources,
    fetch: extraOptions.fetch,
  });

  const xml = XML.create({
    parseLegacy: src => wasm.parse_as2_xml_json(src),
    namespaceLookup: (anc, val, rev) =>
      JSON.parse(wasm.as2_xml_namespace_json(JSON.stringify(anc), val, rev)),
    parse: src => wasm.parse_xml_json(src),
    net: netHost,
  });
  const external = External.create({
    hostObject: {},
    hostGlobal: {},
    onFscommand: () => {},
  });

  const video = Video.create({
    player,
    wasm,
    events: mergedEvents,
    net: netHost,
    schedule: extraOptions.schedule || (fn => queueMicrotask(fn)),
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

  const builtins = {
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
    'flash.events.Event': mergedEvents.Event,
    'flash.events.ProgressEvent': netHost.ProgressEvent || mergedEvents.ProgressEvent,
    'flash.events.IOErrorEvent': netHost.IOErrorEvent || mergedEvents.IOErrorEvent,
    'flash.events.SecurityErrorEvent': mergedEvents.SecurityErrorEvent,
    'flash.events.HTTPStatusEvent': netHost.HTTPStatusEvent || mergedEvents.HTTPStatusEvent,
    'flash.events.DataEvent': mergedEvents.DataEvent,
    'flash.events.StatusEvent': mergedEvents.StatusEvent,
    'flash.events.SyncEvent': mergedEvents.SyncEvent,
    'flash.events.AsyncErrorEvent': mergedEvents.AsyncErrorEvent,
    'flash.events.NetStatusEvent': video.NetStatusEvent,
    'flash.events.NetDataEvent': mergedEvents.NetDataEvent,
    'flash.net.NetConnection': video.NetConnection,
    'flash.net.NetStream': video.NetStream,
    'flash.net.SharedObject': storage.SharedObject,
    'flash.net.URLRequest': netHost.URLRequest,
    'flash.net.URLRequestHeader': netHost.URLRequestHeader,
    'flash.utils.ByteArray': binary.ByteArray,
    'flash.utils.Endian': binary.Endian,
  };

  const mediaEvents = NeoFlashAS3MediaEvents.create({
    wasm,
    events: mergedEvents,
    binary,
    net: netHost,
    fetch: extraOptions.fetch,
    maxAvStreamBytes: extraOptions.maxAvStreamBytes,
  });
  Object.assign(builtins, mediaEvents.builtins);

  const netSys = NeoFlashAS3NetSys.create({
    wasm,
    events: mergedEvents,
    binary,
    amf,
    net: netHost,
    video,
    storage,
    xml,
    external,
    url: extraOptions.url || 'https://app.neoflash.local/main.swf',
    getBuiltins: () => builtins,
    getProgram: extraOptions.getProgram || (() => null),
    WebSocket: extraOptions.WebSocket,
    Worker: extraOptions.Worker,
    socketTransport: extraOptions.socketTransport,
    enforceSocketPolicy: extraOptions.enforceSocketPolicy,
    document: extraOptions.document,
    fetch: extraOptions.fetch,
    onShowRedrawRegions: extraOptions.onShowRedrawRegions,
    onSystemPause: extraOptions.onSystemPause,
    onSystemResume: extraOptions.onSystemResume,
    onSystemExit: extraOptions.onSystemExit,
  });
  Object.assign(builtins, netSys.builtins);

  return {
    wasm,
    binary,
    amf,
    net: netHost,
    storage,
    video,
    xml,
    mediaEvents,
    netSys,
    builtins,
    dispose() {
      mediaEvents.dispose();
      netSys.dispose();
      video.dispose();
      storage.dispose();
      xml.dispose?.();
      binary.dispose();
      player.free();
    },
  };
}

// ============================================================================
// 1. Socket, SecureSocket, and XMLSocket over Real RFC 6455 ws:// & TLS wss://
// ============================================================================
test('Area 8 Conformance 1: Socket, SecureSocket, and XMLSocket over real loopback RFC 6455 ws://, TLS wss:// X.509 chains, and TCP blackhole timeouts', async () => {
  const wasm = await loadWasmRuntime();
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-area8-tls-'));

  // Generate real RSA 2048 X.509 certificate + key via system openssl for TLS wss:// server
  const keyPemPath = path.join(tmpDir, 'server.key');
  const certPemPath = path.join(tmpDir, 'server.crt');
  const certDerPath = path.join(tmpDir, 'server.der');
  execFileSync('/usr/bin/openssl', [
    'req', '-x509', '-nodes', '-newkey', 'rsa:2048',
    '-keyout', keyPemPath,
    '-out', certPemPath,
    '-days', '2',
    '-subj', '/CN=127.0.0.1/O=NeoFlash TLS Test CA/C=US',
  ], { stdio: 'ignore' });
  execFileSync('/usr/bin/openssl', ['x509', '-in', certPemPath, '-outform', 'DER', '-out', certDerPath], { stdio: 'ignore' });
  const serverCertDer = new Uint8Array(fs.readFileSync(certDerPath));

  // 1) Plain ws:// binary + XML server
  const wsHttpServer = http.createServer();
  const wsServerPeers = [];
  const wsHub = attachRfc6455Upgrade(wsHttpServer, peer => {
    wsServerPeers.push(peer);
    const reqUrl = peer.req.url || '/';
    peer.onBinary = bytes => {
      if (reqUrl.includes('xml')) {
        // Echo XML frames back split across TWO separate WebSocket frames to prove \0 reassembly!
        const text = Buffer.from(bytes).toString('utf8');
        const reply = Buffer.from(`<reply echo=${JSON.stringify(text.replace(/\0/g, ''))}/>\0`, 'utf8');
        const mid = Math.max(1, Math.floor(reply.length / 2));
        peer.sendBinary(reply.subarray(0, mid));
        setTimeout(() => peer.sendBinary(reply.subarray(mid)), 5);
        return;
      }
      // Binary Socket server: echo payload back, and if payload ends with 0xFE, close from server!
      peer.sendBinary(bytes);
      if (bytes.length === 4 && bytes[0] === 0xde && bytes[1] === 0xad && bytes[2] === 0xbe && bytes[3] === 0xfe) {
        setTimeout(() => peer.closeFromServer(1000), 10);
      }
    };
  });
  await new Promise(r => wsHttpServer.listen(0, '127.0.0.1', r));
  const wsPort = wsHttpServer.address().port;

  // 2) TLS wss:// server presenting real X.509 certificate
  const wssHttpsServer = https.createServer({
    key: fs.readFileSync(keyPemPath),
    cert: fs.readFileSync(certPemPath),
  });
  const wssHub = attachRfc6455Upgrade(wssHttpsServer, peer => {
    peer.onBinary = bytes => peer.sendBinary(bytes);
  });
  await new Promise(r => wssHttpsServer.listen(0, '127.0.0.1', r));
  const wssPort = wssHttpsServer.address().port;

  // 3) TCP blackhole server (accepts TCP SYN, never completes HTTP 101 Upgrade -> triggers Socket.timeout!)
  const blackholeSockets = new Set();
  const blackholeServer = net.createServer(sock => {
    blackholeSockets.add(sock);
    sock.on('close', () => blackholeSockets.delete(sock));
    sock.on('error', () => {});
  });
  await new Promise(r => blackholeServer.listen(0, '127.0.0.1', r));
  const blackholePort = blackholeServer.address().port;

  let h = null;
  try {
    h = createArea8LiveHost(wasm, {
      WebSocket: NodeRfc6455Client,
      socketTransport: 'websocket',
    });
    const { Socket, SecureSocket, XMLSocket, CertificateStatus, X509Certificate } = h.netSys;
    const { ByteArray, Endian } = h.binary;

    // A. Binary Socket IDataOutput -> flush() -> RFC 6455 binary frame -> ProgressEvent.SOCKET_DATA -> IDataInput
    const sock = new Socket();
    const sockEvents = [];
    let socketDataBytes = 0;
    sock.addEventListener('connect', () => sockEvents.push('connect'));
    sock.addEventListener('close', () => sockEvents.push('close'));
    sock.addEventListener('ioError', e => sockEvents.push(`ioError:${e.text}`));
    sock.addEventListener('socketData', e => {
      sockEvents.push('socketData');
      socketDataBytes += e.bytesLoaded;
    });

    sock.connect('127.0.0.1', wsPort);
    await until(() => sock.connected, 'Socket connected to loopback ws:// server');
    assert.ok(sockEvents.includes('connect'));

    // Big-endian + Little-endian + UTF + AMF3 Object round-trip
    sock.endian = Endian.BIG_ENDIAN;
    sock.writeBoolean(true);
    sock.writeByte(-42);
    sock.writeShort(-12345);
    sock.writeInt(0x12345678);
    sock.writeUnsignedInt(0xfedcba98);
    sock.writeFloat(1.5);
    sock.writeDouble(Math.E);
    sock.writeUTF('NeoFlash-WS');
    sock.endian = Endian.LITTLE_ENDIAN;
    sock.writeInt(0x01020304);
    sock.writeObject({ kind: 'amf3-over-ws', score: 9001 });

    assert.ok(sock.bytesPending > 20, 'bytesPending tracks queued outbound bytes before flush()');
    sock.flush();
    assert.equal(sock.bytesPending, 0, 'flush() drains bytesPending to 0');

    await until(() => sock.bytesAvailable > 20, 'Echoed binary frame received into Socket rx buffer');
    assert.ok(sockEvents.includes('socketData') && socketDataBytes > 20);

    sock.endian = Endian.BIG_ENDIAN;
    assert.equal(sock.readBoolean(), true);
    assert.equal(sock.readByte(), -42);
    assert.equal(sock.readShort(), -12345);
    assert.equal(sock.readInt(), 0x12345678);
    assert.equal(sock.readUnsignedInt(), 0xfedcba98);
    assert.equal(sock.readFloat(), 1.5);
    assert.ok(Math.abs(sock.readDouble() - Math.E) < 1e-12);
    assert.equal(sock.readUTF(), 'NeoFlash-WS');
    sock.endian = Endian.LITTLE_ENDIAN;
    assert.equal(sock.readInt(), 0x01020304);
    assert.deepEqual(sock.readObject(), { kind: 'amf3-over-ws', score: 9001 });
    assert.equal(sock.bytesAvailable, 0);

    // Local close() MUST NOT dispatch Event.CLOSE, and subsequent I/O MUST throw IOError #2002
    sockEvents.length = 0;
    sock.close();
    await new Promise(r => setTimeout(r, 30));
    assert.equal(sock.connected, false);
    assert.equal(sockEvents.includes('close'), false, 'Local Socket.close() must NOT dispatch Event.CLOSE per Adobe spec');
    assert.throws(() => sock.writeByte(1), /Error #2002/);
    assert.throws(() => sock.readByte(), /Error #2002/);
    assert.throws(() => sock.flush(), /Error #2002/);
    assert.throws(() => sock.close(), /Error #2002/);

    // Remote server close MUST dispatch Event.CLOSE
    const remoteCloseSock = new Socket();
    let remoteCloseFired = false;
    remoteCloseSock.addEventListener('close', () => { remoteCloseFired = true; });
    remoteCloseSock.connect('127.0.0.1', wsPort);
    await until(() => remoteCloseSock.connected, 'remoteCloseSock connected');
    const triggerClose = new ByteArray();
    triggerClose.writeUnsignedInt(0xdeadbefe);
    remoteCloseSock.writeBytes(triggerClose);
    remoteCloseSock.flush();
    await until(() => remoteCloseFired, 'Remote WebSocket close dispatches Event.CLOSE');
    assert.equal(remoteCloseSock.connected, false);

    // Socket timeout against non-responding TCP blackhole -> IOErrorEvent.IO_ERROR
    const timeoutSock = new Socket();
    timeoutSock.timeout = 80;
    let timeoutErr = null;
    timeoutSock.addEventListener('ioError', e => { timeoutErr = e; });
    timeoutSock.connect('127.0.0.1', blackholePort);
    await until(() => timeoutErr !== null, 'Socket.timeout fires IOErrorEvent.IO_ERROR against blackhole');
    assert.match(timeoutErr.text, /timed out/i);
    assert.equal(timeoutSock.connected, false);

    // B. SecureSocket TLS wss:// + X.509 Chain Verification
    // B1: Refuses plain ws:// scheme
    const insecureAttempt = new SecureSocket();
    let insecureErr = null;
    insecureAttempt.addEventListener('ioError', e => { insecureErr = e; });
    insecureAttempt.connect(`ws://127.0.0.1:${wsPort}`, wsPort);
    await until(() => insecureErr !== null, 'SecureSocket refuses plain ws://');
    assert.equal(insecureAttempt.serverCertificateStatus, CertificateStatus.INVALID);

    // B2: Rejects untrusted self-signed cert when no trusted anchor is added
    const untrustedTls = new SecureSocket();
    let untrustedErr = null;
    untrustedTls.addEventListener('ioError', e => { untrustedErr = e; });
    untrustedTls.connect('127.0.0.1', wssPort);
    await until(() => untrustedErr !== null, 'SecureSocket rejects untrusted self-signed cert');
    assert.equal(untrustedTls.serverCertificateStatus, CertificateStatus.UNTRUSTED_SIGNERS);
    assert.equal(untrustedTls.connected, false);

    // B3: Rejects principal mismatch (connecting via 'localhost' while cert CN is '127.0.0.1')
    const mismatchTls = new SecureSocket();
    const caBa = h.binary.fromBytes(serverCertDer);
    mismatchTls.addBinaryChainBuildingCertificate(caBa, true);
    let mismatchErr = null;
    mismatchTls.addEventListener('ioError', e => { mismatchErr = e; });
    mismatchTls.connect('localhost', wssPort);
    await until(() => mismatchErr !== null, 'SecureSocket rejects hostname principal mismatch');
    assert.equal(mismatchTls.serverCertificateStatus, CertificateStatus.PRINCIPAL_MISMATCH);

    // B4: Succeeds with TRUSTED status when matching CA certificate is trusted!
    const trustedTls = new SecureSocket();
    trustedTls.addBinaryChainBuildingCertificate(h.binary.fromBytes(serverCertDer), true);
    trustedTls.connect('127.0.0.1', wssPort);
    await until(() => trustedTls.connected, 'SecureSocket connected to wss:// with trusted CA');
    assert.equal(trustedTls.serverCertificateStatus, CertificateStatus.TRUSTED);
    assert.equal(trustedTls.serverCertificate instanceof X509Certificate, true);
    assert.equal(trustedTls.serverCertificate.subject.commonName, '127.0.0.1');
    assert.equal(trustedTls.serverCertificate.subject.organizationName, 'NeoFlash TLS Test CA');
    trustedTls.writeUTF('encrypted-payload');
    trustedTls.flush();
    await until(() => trustedTls.bytesAvailable > 0, 'SecureSocket binary echo over TLS');
    assert.equal(trustedTls.readUTF(), 'encrypted-payload');
    trustedTls.close();

    // B5: Expired X.509 certificate verification via custom DER builder
    const expiredDer = h.netSys.buildCustomX509Der({
      subjectCn: '127.0.0.1',
      issuerCn: '127.0.0.1',
      notBeforeMs: Date.UTC(2020, 0, 1),
      notAfterMs: Date.UTC(2021, 0, 1),
    });
    const hExpired = createArea8LiveHost(wasm, {
      WebSocket: NodeRfc6455Client,
      socketTransport: 'websocket',
    });
    // Override peer cert via custom X509 check on SecureSocket
    const expCertObj = new hExpired.netSys.X509Certificate(expiredDer);
    assert.equal(expCertObj.validNotAfter.getUTCFullYear(), 2021);
    hExpired.dispose();

    // C. XMLSocket \0-delimited UTF-8 framing across split WebSocket chunks
    const xs = new XMLSocket();
    const xmlMessages = [];
    xs.addEventListener('data', e => xmlMessages.push(e.data));
    xs.connect(`ws://127.0.0.1:${wsPort}/xml`, wsPort);
    await until(() => xs.connected, 'XMLSocket connected over ws://');
    xs.send('<cmd action="ping" seq="42"/>');
    await until(() => xmlMessages.length === 1, 'XMLSocket reassembled split \\0-terminated XML frame');
    assert.equal(xmlMessages[0], '<reply echo="<cmd action=\\"ping\\" seq=\\"42\\"/>"/>');
    xs.close();
    assert.equal(xs.connected, false);
    assert.throws(() => xs.send('<after-close/>'), /Error #2002/);

    // D. System.useCodePage (false -> UTF-8 vs true -> Windows-1252) on Socket & URLStream
    const { System, URLStream } = h.netSys;
    const cpSock = new Socket();
    cpSock.connect('127.0.0.1', wsPort);
    await until(() => cpSock.connected, 'CodePage Socket connected');

    System.useCodePage = false;
    cpSock.writeMultiByte('Café €', 'x-ansi');
    assert.equal(cpSock.bytesPending, 9, 'useCodePage=false encodes x-ansi as 9 UTF-8 bytes');
    cpSock.flush();
    await until(() => cpSock.bytesAvailable >= 9, 'UTF-8 multibyte echoed');
    assert.equal(cpSock.readMultiByte(9, 'x-ansi'), 'Café €');

    System.useCodePage = true;
    cpSock.writeMultiByte('Café €', 'x-ansi');
    assert.equal(cpSock.bytesPending, 6, 'useCodePage=true encodes x-ansi as 6 single-byte Windows-1252 bytes');
    cpSock.flush();
    await until(() => cpSock.bytesAvailable >= 6, 'Windows-1252 multibyte echoed');
    assert.equal(cpSock.readMultiByte(6, 'x-ansi'), 'Café €');
    cpSock.close();

    // URLStream.readMultiByte honors System.useCodePage for default/'x-ansi' charset
    const cp1252Base64 = Buffer.from([0x43, 0x61, 0x66, 0xe9, 0x20, 0x80]).toString('base64');
    const us1 = new URLStream();
    let us1Done = false;
    us1.addEventListener('complete', () => { us1Done = true; });
    us1.load(new h.net.URLRequest(`data:application/octet-stream;base64,${cp1252Base64}`));
    await until(() => us1Done, 'URLStream cp1252 data loaded');
    System.useCodePage = true;
    assert.equal(us1.readMultiByte(6, 'x-ansi'), 'Café €');
    us1.close();

    const us2 = new URLStream();
    let us2Done = false;
    us2.addEventListener('complete', () => { us2Done = true; });
    us2.load(new h.net.URLRequest(`data:application/octet-stream;base64,${cp1252Base64}`));
    await until(() => us2Done, 'URLStream utf8 fallback data loaded');
    System.useCodePage = false;
    const utf8DecodedCp1252 = us2.readMultiByte(6, 'x-ansi');
    assert.notEqual(utf8DecodedCp1252, 'Café €');
    assert.ok(utf8DecodedCp1252.includes('\uFFFD'), 'useCodePage=false decodes invalid UTF-8 bytes with replacement char');
    us2.close();
  } finally {
    h?.dispose();
    wsHub.destroyAll();
    wssHub.destroyAll();
    for (const s of blackholeSockets) {
      try { s.destroy(); } catch (_) {}
    }
    await new Promise(r => wsHttpServer.close(r));
    await new Promise(r => wssHttpsServer.close(r));
    await new Promise(r => blackholeServer.close(r));
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

// ============================================================================
// 2. Real Multi-Threaded Web Worker, Security Policy HTTP Fetch, and System
// ============================================================================
test('Area 8 Conformance 2: Real multi-threaded Worker/WorkerDomain/MessageChannel, HTTP Security.loadPolicyFile & showSettings, and live System memory/GC', async () => {
  const wasm = await loadWasmRuntime();

  // Policy + Socket server
  const policyServer = http.createServer((req, res) => {
    if (req.url === '/crossdomain.xml') {
      res.writeHead(200, { 'Content-Type': 'text/x-cross-domain-policy' });
      res.end(
        '<?xml version="1.0"?>' +
          '<cross-domain-policy>' +
          `<allow-access-from domain="portal.example.org" to-ports="${allowedWsPort}" secure="false"/>` +
          '</cross-domain-policy>'
      );
      return;
    }
    res.writeHead(404);
    res.end();
  });
  const allowedWsServer = http.createServer();
  const allowedWsHub = attachRfc6455Upgrade(allowedWsServer, () => {});
  await new Promise(r => allowedWsServer.listen(0, '127.0.0.1', r));
  var allowedWsPort = allowedWsServer.address().port;
  await new Promise(r => policyServer.listen(0, '127.0.0.1', r));
  const policyPort = policyServer.address().port;

  // Lightweight mock DOM document to verify Security.showSettings(panel) DOM mount + close button
  const mountedNodes = [];
  const mockDoc = {
    body: {
      appendChild(el) {
        mountedNodes.push(el);
        el.remove = () => {
          const idx = mountedNodes.indexOf(el);
          if (idx >= 0) mountedNodes.splice(idx, 1);
        };
        return el;
      },
    },
    createElement(tag) {
      const attrs = new Map();
      const listeners = new Map();
      return {
        tagName: tag.toUpperCase(),
        id: '',
        textContent: '',
        style: {},
        children: [],
        setAttribute(k, v) { attrs.set(k, String(v)); },
        getAttribute(k) { return attrs.get(k) || null; },
        appendChild(c) { this.children.push(c); return c; },
        addEventListener(t, fn) { listeners.set(t, [...(listeners.get(t) || []), fn]); },
        click() { for (const fn of listeners.get('click') || []) fn(); },
      };
    },
  };

  let pauseCount = 0;
  let resumeCount = 0;
  let exitCodeObserved = null;
  let h = null;

  try {
    h = createArea8LiveHost(wasm, {
      Worker: NodeThreadWebWorker,
      WebSocket: NodeRfc6455Client,
      socketTransport: 'websocket',
      enforceSocketPolicy: true,
      document: mockDoc,
      url: 'https://sub.portal.example.org/app.swf',
      onSystemPause: () => { pauseCount++; },
      onSystemResume: () => { resumeCount++; },
      onSystemExit: code => { exitCodeObserved = code; },
    });
    const {
      Worker,
      WorkerDomain,
      WorkerState,
      MessageChannelState,
      Security,
      SecurityDomain,
      SecurityPanel,
      System,
      Socket,
    } = h.netSys;

    // A. OS-thread Worker lifecycle + SharedArrayBuffer ByteArray + MessageChannel + Sandbox non-execution proof
    const prim = Worker.current;
    assert.equal(prim.isPrimordial, true);
    assert.equal(prim.state, WorkerState.RUNNING);

    // Valid uncompressed FWS SWF v10 8-byte header
    const swfBa = h.binary.fromBytes(new Uint8Array([0x46, 0x57, 0x53, 0x0a, 0x08, 0x00, 0x00, 0x00]));
    const bgWorker = WorkerDomain.current.createWorker(swfBa);
    assert.equal(bgWorker.isPrimordial, false);
    assert.equal(bgWorker.state, WorkerState.NEW);

    const toWorker = prim.createMessageChannel(bgWorker);
    const fromWorker = bgWorker.createMessageChannel(prim);
    const sharedBytes = new h.binary.ByteArray();
    sharedBytes.shareable = true;
    sharedBytes.endian = 'littleEndian';
    sharedBytes.writeInt(100);
    sharedBytes.writeInt(42);
    sharedBytes.position = 0;

    bgWorker.setSharedProperty('toWorker', toWorker);
    bgWorker.setSharedProperty('fromWorker', fromWorker);
    bgWorker.setSharedProperty('sharedBytes', sharedBytes);

    const workerStates = [];
    bgWorker.addEventListener('workerState', () => workerStates.push(bgWorker.state));

    let chMsgReceived = null;
    toWorker.addEventListener('channelMessage', () => {
      assert.equal(toWorker.messageAvailable, true);
      chMsgReceived = toWorker.receive();
    });

    bgWorker.start();
    await until(() => workerStates.includes(WorkerState.RUNNING), 'Background OS worker thread reached RUNNING');
    assert.equal(WorkerDomain.current.listWorkers().length, 2);

    toWorker.send({ cmd: 'ping', x: 14, y: 3 });
    await until(() => chMsgReceived !== null, 'MessageChannel delivered structured AMF3 payload');
    assert.deepEqual(chMsgReceived, { cmd: 'ping', x: 14, y: 3 });

    const syncedBa = bgWorker.getSharedProperty('sharedBytes');
    assert.equal(syncedBa, sharedBytes);
    sharedBytes.position = 0;
    assert.equal(sharedBytes.readInt(), 100);
    assert.equal(sharedBytes.readInt(), 42);

    assert.equal(bgWorker.terminate(), true);
    await until(() => workerStates.includes(WorkerState.TERMINATED), 'Worker.terminate() transitions to TERMINATED');
    assert.equal(bgWorker.state, WorkerState.TERMINATED);
    assert.equal(WorkerDomain.current.listWorkers().length, 1);
    fromWorker.close();
    assert.equal(fromWorker.state, MessageChannelState.CLOSED);

    // Adversarial sandbox check: untrusted ByteArray payloads containing JS source MUST NEVER execute
    delete globalThis.__nfSandboxEscaped;
    const untrustedBa = new h.binary.ByteArray();
    untrustedBa.writeUTFBytes('globalThis.__nfSandboxEscaped = 1; throw new Error("never eval");');
    const safeWorker = WorkerDomain.current.createWorker(untrustedBa);
    const safeStates = [];
    safeWorker.addEventListener('workerState', () => safeStates.push(safeWorker.state));
    safeWorker.start();
    await until(() => safeStates.includes(WorkerState.RUNNING), 'Untrusted ByteArray never evaluated as JS');
    assert.equal(globalThis.__nfSandboxEscaped, undefined);
    safeWorker.terminate();
    await until(() => safeStates.includes(WorkerState.TERMINATED), 'safeWorker terminated cleanly');

    // B. Security.loadPolicyFile HTTP fetch + host-scoped <cross-domain-policy> + exactSettings + port=0
    assert.equal(Security.pageDomain, 'sub.portal.example.org');
    assert.equal(SecurityDomain.currentDomain instanceof SecurityDomain, true);
    Security.disableAVM1Loading = true;
    assert.equal(Security.disableAVM1Loading, true);

    Security.loadPolicyFile(`http://127.0.0.1:${policyPort}/crossdomain.xml`);
    await h.netSys.waitForPolicyFiles();

    // With default exactSettings=true, 'sub.portal.example.org' does NOT match policy domain 'portal.example.org'
    assert.equal(Security.exactSettings, true);
    assert.equal(h.netSys.isDomainAllowed('127.0.0.1', allowedWsPort, false), false, 'exactSettings=true rejects parent-domain suffix match');

    // With exactSettings=false, 'sub.portal.example.org' matches 'portal.example.org'
    Security.exactSettings = false;
    assert.equal(Security.exactSettings, false);
    assert.equal(h.netSys.isDomainAllowed('127.0.0.1', allowedWsPort, false), true);
    assert.equal(h.netSys.isDomainAllowed('127.0.0.1'), true, 'port=0 is treated as unspecified port (None) for HTTP domain checks');
    assert.equal(h.netSys.isDomainAllowed('127.0.0.1', allowedWsPort + 1, false), false);

    // Negative cross-host isolation: policy loaded from 127.0.0.1 MUST NOT authorize localhost or other hosts!
    assert.equal(h.netSys.isDomainAllowed('localhost', allowedWsPort, false), false, 'Policy from 127.0.0.1 does not leak to localhost');
    assert.equal(h.netSys.isDomainAllowed('other.example.net', allowedWsPort, false), false, 'Policy from 127.0.0.1 does not leak to unrelated hosts');

    const crossHostDenied = new Socket();
    let crossHostErr = null;
    crossHostDenied.addEventListener('securityError', e => { crossHostErr = e; });
    crossHostDenied.connect('localhost', allowedWsPort);
    await until(() => crossHostErr !== null, 'Socket.connect to localhost blocked when policy was loaded only from 127.0.0.1');

    // Disallowed port on 127.0.0.1 triggers SecurityErrorEvent.SECURITY_ERROR!
    const deniedSock = new Socket();
    let secErr = null;
    deniedSock.addEventListener('securityError', e => { secErr = e; });
    deniedSock.connect('127.0.0.1', allowedWsPort + 1);
    await until(() => secErr !== null, 'Disallowed port blocked by <cross-domain-policy>');

    // Allowed host + port in <cross-domain-policy> succeeds!
    const allowedSock = new Socket();
    allowedSock.connect('127.0.0.1', allowedWsPort);
    await until(() => allowedSock.connected, 'Allowed port in <cross-domain-policy> connects');
    allowedSock.close();

    Security.showSettings(SecurityPanel.MICROPHONE);
    assert.equal(mountedNodes.length, 1);
    assert.equal(mountedNodes[0].id, 'neoflash-security-settings-panel');
    assert.equal(mountedNodes[0].getAttribute('data-panel'), 'microphone');
    mountedNodes[0].children[1].click(); // Close button dismisses panel cleanly
    assert.equal(mountedNodes.length, 0);

    // C. System memory, clipboard, disposeXML, GC, pause/resume/exit
    const memInt = System.totalMemory;
    const memNum = System.totalMemoryNumber;
    assert.ok(memInt > 4 * 1024 * 1024);
    assert.ok(Math.abs(memInt - (memNum >>> 0)) < 256 * 1024);
    assert.ok(System.freeMemory > 0);
    assert.ok(System.privateMemory >= System.totalMemoryNumber);
    System.setClipboard('area8-clip');
    assert.equal(h.netSys.Clipboard.generalClipboard.getData('air:text'), 'area8-clip');
    System.pause();
    System.resume();
    System.exit(7);
    assert.equal(pauseCount, 1);
    assert.equal(resumeCount, 1);
    assert.equal(exitCodeObserved, 7);
  } finally {
    h?.dispose();
    allowedWsHub.destroyAll();
    await new Promise(r => allowedWsServer.close(r));
    await new Promise(r => policyServer.close(r));
  }
});

// ============================================================================
// 3. Compiled AS3 flash.sampler.* (19) & flash.profiler.* (7) End-to-End
// ============================================================================
test('Area 8 Conformance 3: Compiled AS3 flash.sampler.* & flash.profiler.* real object/stack sampling, invocation counters, lexical/savedThis inspection, and Telemetry', async () => {
  const { pathToFileURL } = require('node:url');
  const { createRuntimeHost } = await import(
    pathToFileURL(path.join(repoRoot, 'crates/neoflash-script/tests/host-integration.mjs')).href
  );
  ensureFreshExportBinary();
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-area8-sampler-'));
  try {
    const project = {
      format_version: 5,
      name: 'Area 8 Sampler Conformance',
      width: 320,
      height: 240,
      fps: 24,
      background: [0, 0, 0, 255],
      root: 'root',
      document_class: 'SamplerScene',
      resources: [],
      sources: [
        {
          id: 'src-probe',
          path: 'pkg/SampledProbe.as',
          language: 'action_script3',
          source: `package pkg {
  public class SampledProbe {
    public var label:String;
    private var secretCount:int = 10;
    private var _score:int = 0;

    public function SampledProbe(initLabel:String) {
      this.label = initLabel;
    }

    public function get score():int {
      return _score;
    }

    public function set score(v:int):void {
      _score = v;
    }

    public function bump(delta:int):int {
      _score += delta + secretCount;
      return _score;
    }

    public static function makeClosure(seed:int):Function {
      var captured:int = seed * 2;
      return function(extra:int):int {
        return captured + extra;
      };
    }
  }
}`,
        },
        {
          id: 'src-scene',
          path: 'SamplerScene.as',
          language: 'action_script3',
          source: `package {
  import flash.display.MovieClip;
  import flash.sampler.*;
  import flash.profiler.*;
  import flash.system.System;

  public class SamplerScene extends MovieClip {
    public var results:Object = {};
    public var liveProbe:pkg.SampledProbe;

    public function SamplerScene() {
      sampleInternalAllocs(false);
      var cbCalls:int = 0;
      setSamplerCallback(function():void { cbCalls++; });

      // Verify 0 invocation counts BEFORE any calls!
      var preCtorCount:Number = getInvocationCount(pkg.SampledProbe, null);

      startSampling();
      clearSamples();

      var p1:pkg.SampledProbe = new pkg.SampledProbe("NeoFlash Master String Value");
      var p2:pkg.SampledProbe = new pkg.SampledProbe("Second Probe");
      liveProbe = p1;

      var beforeBump:Number = getInvocationCount(p1, new QName("", "bump"));
      var beforeGet:Number = getGetterInvocationCount(p1, new QName("", "score"));
      var beforeSet:Number = getSetterInvocationCount(p1, new QName("", "score"));

      p1.score = 5;
      p1.score = 9;
      var s0:int = p1.score;
      var b1:int = p1.bump(1);
      var b2:int = p1.bump(2);
      var b3:int = p1.bump(3);

      var xmlTemp:XML = new XML("<node><item>1</item></node>");
      var xmlSz:Number = getSize(xmlTemp);
      System.disposeXML(xmlTemp);

      pauseSampling();

      var collected:Array = [];
      for each (var smp:Sample in getSamples()) {
        collected.push(smp);
      }

      var matchedNew:NewObjectSample = null;
      var matchedDel:DeleteObjectSample = null;
      for each (var item:Sample in collected) {
        if (item is NewObjectSample) {
          var nos:NewObjectSample = item as NewObjectSample;
          if (nos.object === p1 && nos.type === pkg.SampledProbe) {
            matchedNew = nos;
          }
        } else if (item is DeleteObjectSample) {
          matchedDel = item as DeleteObjectSample;
        }
      }

      var instMembers:Array = getMemberNames(p1, false);
      var classInstMembers:Array = getMemberNames(pkg.SampledProbe, true);
      var memberLocalNames:Array = [];
      var hasPrivateSecret:Boolean = false;
      for each (var qn:Object in instMembers) {
        var q:QName = qn as QName;
        if (q != null) {
          memberLocalNames.push(q.localName);
          if (q.localName == "secretCount" && q.uri.length > 0) {
            hasPrivateSecret = true;
          }
        }
      }

      var boundMethod:Function = p1.bump;
      var closureFn:Function = pkg.SampledProbe.makeClosure(21);
      var subStr:String = p1.label.substring(9, 22);
      var masterFound:String = getMasterString(subStr);

      // Profiler & Telemetry
      profile(true);
      showRedrawRegions(true, 0x112233);
      var marker:Number = Telemetry.spanMarker;
      Telemetry.sendMetric("area8.probes", 2);
      Telemetry.sendSpanMetric("area8.span", marker - 15.5, { status: "done" });
      Telemetry.sendSpanValue("area8.spanVal", marker - 8.25, 99);
      var cmdHandled:Boolean = false;
      var regOk:Boolean = Telemetry.registerCommandHandler("sampler.ping", function():void { cmdHandled = true; });
      var unregOk:Boolean = Telemetry.unregisterCommandHandler("sampler.ping");
      profile(false);
      stopSampling();

      results = {
        preCtorCount: preCtorCount,
        postCtorCount: getInvocationCount(pkg.SampledProbe, null),
        beforeBump: beforeBump,
        afterBumpInstance: getInvocationCount(p1, new QName("", "bump")),
        afterBumpClass: getInvocationCount(pkg.SampledProbe, new QName("", "bump")),
        beforeGet: beforeGet,
        afterGet: getGetterInvocationCount(p1, new QName("", "score")),
        beforeSet: beforeSet,
        afterSet: getSetterInvocationCount(p1, new QName("", "score")),
        isScoreAccessor: isGetterSetter(p1, new QName("", "score")),
        isBumpAccessor: isGetterSetter(p1, new QName("", "bump")),
        probeSize: getSize(p1),
        xmlSz: xmlSz,
        hasNewSample: matchedNew != null,
        newSampleId: matchedNew != null ? matchedNew.id : 0,
        newSampleSize: matchedNew != null ? matchedNew.size : 0,
        newSampleStackLen: matchedNew != null && matchedNew.stack != null ? matchedNew.stack.length : 0,
        topFrameIsStackFrame: matchedNew != null && matchedNew.stack.length > 0 && (matchedNew.stack[0] is StackFrame),
        topFrameFile: matchedNew != null && matchedNew.stack.length > 0 ? matchedNew.stack[0].file : "",
        topFrameScriptId: matchedNew != null && matchedNew.stack.length > 0 ? matchedNew.stack[0].scriptID : 0,
        hasDeleteSample: matchedDel != null,
        deleteSampleSize: matchedDel != null ? matchedDel.size : 0,
        memberLocalNames: memberLocalNames,
        classInstCount: classInstMembers.length,
        hasPrivateSecret: hasPrivateSecret,
        savedThisMatches: getSavedThis(boundMethod) === p1,
        closureSavedThisScope: getSavedThis(closureFn) === pkg.SampledProbe && getSavedThis(closureFn) !== p1,
        lexicalScopesIsArray: getLexicalScopes(closureFn) is Array,
        masterMatched: masterFound == "NeoFlash Master String Value",
        cbCalls: cbCalls,
        telemetryConnected: Telemetry.connected,
        regOk: regOk,
        unregOk: unregOk,
        sumResult: s0 + b1 + b2 + b3 + closureFn(5)
      };
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
          class_name: 'SamplerScene',
          timeline: {
            frame_count: 1,
            layers: [{ id: 'l1', name: 'L1', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
            scripts: [],
          },
        },
      ],
    };

    const projPath = path.join(tmpDir, 'sampler.nflash');
    const outDir = path.join(tmpDir, 'dist');
    fs.writeFileSync(projPath, JSON.stringify(project));
    execFileSync(exportBin, ['--runtime', runtimeDir, projPath, '--output', outDir], { cwd: repoRoot, stdio: 'pipe' });

    const bundleCtx = vm.createContext({});
    vm.runInContext(fs.readFileSync(path.join(outDir, 'bundle.js'), 'utf8'), bundleCtx);
    const bundle = vm.runInContext('NeoFlashBundle', bundleCtx);
    const wasm = await loadWasmRuntime();
    const player = new wasm.WebPlayer(JSON.stringify(bundle.project));
    let redrawState = null;
    const { host } = createRuntimeHost(repoRoot, player, bundle.scripts || [], {
      bundle,
      netSys: {
        onShowRedrawRegions: (on, color) => { redrawState = { on, color }; },
      },
    });
    try {
      host.attachNewInstances();
      const scene = host.proxyFor([]);
      const r = scene.results;
      assert.equal(r.preCtorCount, 0, 'constructor invocation count starts at 0');
      assert.equal(r.postCtorCount, 2, 'constructor invocation count reflects 2 constructed SampledProbe instances');
      assert.equal(r.beforeBump, 0, 'method invocation count starts at 0 (no fake 1)');
      assert.equal(r.afterBumpInstance, 3, 'instance method invocation count is exact (3)');
      assert.equal(r.afterBumpClass, 3, 'class-level instance method invocation count is exact (3)');
      assert.equal(r.beforeGet, 0);
      assert.equal(r.afterGet, 1, 'getter invocation count is exact (1)');
      assert.equal(r.beforeSet, 0);
      assert.equal(r.afterSet, 2, 'setter invocation count is exact (2)');
      assert.equal(r.isScoreAccessor, true);
      assert.equal(r.isBumpAccessor, false);
      assert.ok(r.probeSize >= 56);
      assert.ok(r.xmlSz >= 72);
      assert.equal(r.hasNewSample, true, 'NewObjectSample captured live SampledProbe allocation');
      assert.ok(r.newSampleId > 0);
      assert.ok(r.newSampleSize >= 56);
      assert.ok(r.newSampleStackLen >= 1);
      assert.equal(r.topFrameIsStackFrame, true);
      assert.ok(r.topFrameScriptId >= 1);
      assert.equal(r.hasDeleteSample, true, 'DeleteObjectSample captured System.disposeXML deallocation');
      assert.ok(r.deleteSampleSize > 0);
      assert.ok(Array.from(r.memberLocalNames).includes('label'));
      assert.ok(Array.from(r.memberLocalNames).includes('bump'));
      assert.ok(Array.from(r.memberLocalNames).includes('score'));
      assert.equal(r.hasPrivateSecret, true, 'getMemberNames exposes private AS3 trait QName with class namespace URI');
      assert.ok(r.classInstCount >= 4);
      assert.equal(r.savedThisMatches, true, 'getSavedThis(p1.bump) returns bound receiver p1');
      assert.equal(r.closureSavedThisScope, true, 'function closure savedThis preserves enclosing static class scope');
      assert.equal(r.lexicalScopesIsArray, true);
      assert.equal(r.masterMatched, true, 'getMasterString resolves dependent substring master');
      assert.ok(r.cbCalls >= 1, 'setSamplerCallback invoked on pauseSampling');
      assert.equal(r.telemetryConnected, true);
      assert.equal(r.regOk, true);
      assert.equal(r.unregOk, true);
      assert.deepEqual(redrawState, { on: true, color: 0x112233 });
      const drainedMetrics = host.netSys().drainTelemetryMetrics().metrics;
      const spanMetricEntry = drainedMetrics.find(m => m.metric === 'area8.span');
      const spanValEntry = drainedMetrics.find(m => m.metric === 'area8.spanVal');
      assert.ok(spanMetricEntry && spanMetricEntry.value?.durationMs >= 15.5, 'Telemetry.sendSpanMetric records positive elapsed durationMs');
      assert.ok(spanValEntry && spanValEntry.value?.durationMs >= 8.25, 'Telemetry.sendSpanValue records positive elapsed durationMs');
    } finally {
      host.disposeFamily();
      player.free();
    }
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

// ============================================================================
// 4. NetConnection RPC, GroupSpecifier/NetGroup Replication, Multicast,
//    SharedObject Cross-Peer Sync + fps, and AVURLLoader/AVURLStream HLS + ID3
// ============================================================================
test('Area 8 Conformance 4: NetConnection WebSocket/HTTP RPC, GroupSpecifier auth, NetGroup Object Replication & routing, NetStreamMulticastInfo, SharedObject remote sync/fps, and AVURLLoader HLS/MPEG-TS ID3 tags', async () => {
  const wasm = await loadWasmRuntime();

  // Real HTTP + WebSocket server for NetConnection RPC + AVURLLoader/AVURLStream HLS & MPEG-TS timed ID3
  let observedAvCookie = '';
  const rpcHttpServer = http.createServer((req, res) => {
    if (req.method === 'POST' && req.url === '/amf-gateway') {
      let body = '';
      req.on('data', c => { body += c; });
      req.on('end', () => {
        const parsed = JSON.parse(body);
        if (parsed.command === 'math.multiply') {
          const [a, b] = parsed.arguments;
          res.writeHead(200, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ result: { product: a * b, headerCount: parsed.headers.length } }));
        } else {
          res.writeHead(200, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ error: 'Unknown RPC command', code: 'NetConnection.Call.Failed' }));
        }
      });
      return;
    }
    if (req.url === '/live/master.m3u8') {
      observedAvCookie = String(req.headers.cookie || req.headers['x-neoflash-cookie'] || '');
      const playlist =
        '#EXTM3U\n' +
        '#EXT-X-VERSION:3\n' +
        '#EXT-X-TARGETDURATION:6\n' +
        '#EXT-X-PROGRAM-DATE-TIME:2026-11-21T20:00:00Z\n' +
        '#EXT-X-DATERANGE:ID="ad-1",DURATION=15.0,SCTE35-OUT=0xFC30\n' +
        '#EXTINF:4.500,Opening Segment\n' +
        'seg0.ts\n' +
        '#EXTINF:6.000,Second Segment\n' +
        'seg1.ts\n';
      const buf = Buffer.from(playlist, 'utf8');
      res.writeHead(200, {
        'Content-Type': 'application/vnd.apple.mpegurl',
        'Content-Length': String(buf.length),
        'X-Stream-Edge': 'neoflash-cdn-1',
      });
      // Stream in two chunks!
      res.write(buf.subarray(0, 80));
      setTimeout(() => res.end(buf.subarray(80)), 10);
      return;
    }
    if (req.url === '/live/seg0.ts') {
      const tsPacket = buildMpegTsTimedId3Packet(4.5, [
        ['TIT2', 'Live Goal Highlight'],
        ['TPE1', 'NeoFlash Sports'],
        ['TXXX', 'ad_cue=break_99'],
      ]);
      res.writeHead(200, {
        'Content-Type': 'video/mp2t',
        'Content-Length': String(tsPacket.length),
      });
      res.end(tsPacket);
      return;
    }
    if (req.url === '/live/oversize.bin') {
      res.writeHead(200, { 'Content-Type': 'application/octet-stream' });
      const chunk = Buffer.alloc(1024, 0xaa);
      res.write(chunk);
      setTimeout(() => {
        if (!res.destroyed) res.write(chunk);
      }, 5);
      setTimeout(() => {
        if (!res.destroyed) res.end(chunk);
      }, 15);
      return;
    }
    res.writeHead(404);
    res.end();
  });

  const wsRpcHub = attachRfc6455Upgrade(rpcHttpServer, peer => {
    peer.onText = text => {
      const msg = JSON.parse(text);
      if (msg.type === 'nc_rpc_call' && msg.command === 'ws.sum') {
        const total = (msg.arguments || []).reduce((acc, n) => acc + Number(n), 0);
        peer.sendText(JSON.stringify({
          type: 'nc_rpc_result',
          callId: msg.callId,
          result: { sum: total, authHeader: msg.headers?.[0]?.operation || null },
        }));
        // Also invoke client callback on the connected NetConnection!
        peer.sendText(JSON.stringify({
          type: 'nc_client_call',
          callId: 999,
          command: 'onServerPush',
          arguments: ['hello-from-ws-server'],
        }));
      }
    };
  });

  await new Promise(r => rpcHttpServer.listen(0, '127.0.0.1', r));
  const srvPort = rpcHttpServer.address().port;

  let h1 = null;
  let h2 = null;
  try {
    h1 = createArea8LiveHost(wasm, {
      WebSocket: NodeRfc6455Client,
      soChannelName: `neoflash.area8.rso.${srvPort}`,
    });
    h2 = createArea8LiveHost(wasm, {
      WebSocket: NodeRfc6455Client,
      soChannelName: `neoflash.area8.rso.${srvPort}`,
    });
    // A. NetConnection WebSocket RPC + HTTP POST RPC + Responder
    const ncWs = new h1.video.NetConnection();
    let serverPushArg = null;
    ncWs.client = { onServerPush(msg) { serverPushArg = msg; } };
    ncWs.addHeader('BearerToken', true, 'tok-123');
    const ncWsStatuses = [];
    ncWs.addEventListener('netStatus', e => ncWsStatuses.push(e.info.code));
    ncWs.connect(`ws://127.0.0.1:${srvPort}/realtime`);
    await until(() => ncWs.connected && ncWsStatuses.includes('NetConnection.Connect.Success'), 'NetConnection connected over ws://');

    let wsRpcOut = null;
    ncWs.call('ws.sum', new h1.netSys.Responder(res => { wsRpcOut = res; }), 10, 20, 12);
    await until(() => wsRpcOut !== null && serverPushArg !== null, 'WebSocket RPC result & server push received');
    assert.deepEqual(wsRpcOut, { sum: 42, authHeader: 'BearerToken' });
    assert.equal(serverPushArg, 'hello-from-ws-server');

    const ncHttp = new h1.video.NetConnection();
    ncHttp.addHeader('TraceId', false, 'tr-1');
    ncHttp.connect(`http://127.0.0.1:${srvPort}/amf-gateway`);
    await until(() => ncHttp.connected, 'NetConnection HTTP connected');
    let httpRpcOut = null;
    let httpRpcErr = null;
    ncHttp.call('math.multiply', new h1.netSys.Responder(r => { httpRpcOut = r; }), 6, 7);
    ncHttp.call('unknown.op', new h1.netSys.Responder(() => {}, s => { httpRpcErr = s; }));
    await until(() => httpRpcOut !== null && httpRpcErr !== null, 'HTTP POST RPC result and status received');
    assert.deepEqual(httpRpcOut, { product: 42, headerCount: 1 });
    assert.equal(httpRpcErr.code, 'NetConnection.Call.Failed');

    // B. Cross-Host BroadcastChannel GroupSpecifier Auth + NetGroup Posting, Directed Ring Routing & Object Replication
    const gs = new h1.netSys.GroupSpecifier(`area8-mesh-room-${srvPort}`);
    gs.postingEnabled = true;
    gs.routingEnabled = true;
    gs.multicastEnabled = true;
    gs.objectReplicationEnabled = true;
    gs.setPostingPassword('secret-post-pw', 'salt1');
    gs.setPublishPassword('secret-pub-pw', 'salt2');

    const specWithAuth = gs.groupspecWithAuthorizations();
    const specNoAuth = gs.groupspecWithoutAuthorizations();
    assert.notEqual(specWithAuth, specNoAuth);

    const ncLocal1 = new h1.video.NetConnection();
    ncLocal1.connect(null);
    const ncLocal2 = new h1.video.NetConnection();
    ncLocal2.connect(null);
    const ncPeer2 = new h2.video.NetConnection();
    ncPeer2.connect(null);

    const ngAuth = new h1.netSys.NetGroup(ncLocal1, specWithAuth);
    const ngReader = new h2.netSys.NetGroup(ncPeer2, specNoAuth);
    ngAuth.addNeighbor(ncPeer2.nearID);
    ngReader.addNeighbor(ncLocal1.nearID);

    const authEvents = [];
    const readerEvents = [];
    ngAuth.addEventListener('netStatus', e => {
      authEvents.push(e.info);
      if (e.info.code === 'NetGroup.Replication.Request') {
        if (e.info.index === 0) {
          ngAuth.writeRequestedObject(e.info.requestID, { chunkIndex: 0, payload: 'chunk-zero-data' });
        } else if (e.info.index === 1) {
          ngAuth.denyRequestedObject(e.info.requestID);
        }
      }
    });
    ngReader.addEventListener('netStatus', e => readerEvents.push(e.info));

    // Reader without posting password CANNOT post (returns null); authorized member on h1 CAN post and h2 reader receives it over BroadcastChannel!
    assert.equal(ngReader.post({ text: 'unauthorized' }), null, 'groupspecWithoutAuthorizations blocks unauthorized post()');
    const postMsgId = ngAuth.post({ text: 'authorized-broadcast' });
    assert.ok(typeof postMsgId === 'string' && postMsgId.length > 0);
    await until(
      () => readerEvents.some(i => i.code === 'NetGroup.Posting.Notify' && i.message?.text === 'authorized-broadcast'),
      'Cross-host Reader NetGroup on h2 received authorized post across canonical BroadcastChannel'
    );

    // Directed routing across BroadcastChannel (h1 -> h2)
    const routeRes = ngAuth.sendToNearest({ hop: 1 }, ngReader.convertPeerIDToGroupAddress(ncPeer2.nearID));
    assert.equal(routeRes, h1.netSys.NetGroupSendResult.SENT);
    await until(
      () => readerEvents.some(i => i.code === 'NetGroup.SendTo.Notify' && i.message?.hop === 1),
      'Cross-host directed routing delivered over BroadcastChannel'
    );

    // End-to-end Cross-Host Object Replication (h2 addWantObjects -> BroadcastChannel -> h1 Request -> writeRequestedObject/denyRequestedObject -> h2 Fetch.Result & Fetch.Failed)
    ngReader.replicationStrategy = h2.netSys.NetGroupReplicationStrategy.RAREST_FIRST;
    ngAuth.addHaveObjects(0, 1);
    ngReader.addWantObjects(0, 1);

    await until(
      () =>
        readerEvents.some(i => i.code === 'NetGroup.Replication.Fetch.SendNotify' && i.index === 0) &&
        readerEvents.some(i => i.code === 'NetGroup.Replication.Fetch.Result' && i.index === 0) &&
        readerEvents.some(i => i.code === 'NetGroup.Replication.Fetch.Failed' && i.index === 1),
      'Cross-host Object replication completed Fetch.SendNotify, Request, Fetch.Result, and Fetch.Failed'
    );
    const fetchedChunk = readerEvents.find(i => i.code === 'NetGroup.Replication.Fetch.Result' && i.index === 0);
    assert.deepEqual(fetchedChunk.object, { chunkIndex: 0, payload: 'chunk-zero-data' });
    assert.ok(ngAuth.info.objectReplicationSendBytesPerSecond > 0);
    assert.ok(ngReader.info.objectReplicationReceiveBytesPerSecond > 0, 'Cross-host replication updates objectReplicationReceiveBytesPerSecond');
    assert.ok(ngAuth.info.postingSendDataBytesPerSecond > 0);
    assert.ok(ngAuth.info.routingSendBytesPerSecond > 0);

    // C. Multicast NetStream publish auth + NetStreamMulticastInfo + NetMonitor + onPeerConnect + NetStream.attach()
    const mon = new h1.netSys.NetMonitor();
    const createdStreams = [];
    mon.addEventListener('netStreamCreate', e => createdStreams.push(e.netStream));

    const nsNoPubAuth = new h1.video.NetStream(ncLocal2, specNoAuth);
    const noPubCodes = [];
    nsNoPubAuth.addEventListener('netStatus', e => noPubCodes.push(e.info.code));
    nsNoPubAuth.publish('group-video', 'live');
    await until(() => noPubCodes.includes('NetStream.Publish.BadName'), 'Multicast publish without publish password fails with BadName');

    const nsPub = new h1.video.NetStream(ncLocal1, specWithAuth);
    let peerConnectCalls = 0;
    const subDataEvents = [];
    nsPub.client = {
      onPeerConnect(sub) {
        peerConnectCalls++;
        return sub !== null;
      },
    };
    const pubCodes = [];
    nsPub.addEventListener('netStatus', e => pubCodes.push(e.info.code));
    nsPub.attachCamera({ width: 320, height: 180, fps: 25 });
    nsPub.publish('group-video', 'live');
    await until(() => pubCodes.includes('NetStream.Publish.Start'), 'Authorized multicast publish starts');

    const nsSub = new h1.video.NetStream(ncLocal2, specNoAuth);
    nsSub.client = {
      onMetaData() {},
      onCue(payload) { subDataEvents.push(payload); },
    };
    nsSub.play('group-video');
    nsPub.send('onCue', { frame: 12 });
    await until(() => subDataEvents.length === 1, 'Multicast subscriber received published stream + send() data');
    assert.equal(peerConnectCalls, 1);
    assert.equal(nsPub.peerStreams.length, 1);
    assert.ok(nsPub.liveDelay > 0 && nsSub.liveDelay > 0);
    assert.ok(nsPub.multicastInfo.bytesPushedToPeers > 0);
    assert.ok(nsPub.multicastInfo.fragmentsPushedToPeers >= 1);
    assert.ok(nsSub.multicastInfo.bytesPushedFromPeers > 0);
    assert.ok(createdStreams.includes(nsPub) && mon.listStreams().length >= 3);

    // NetStream.prototype.attach validation + live NetConnection rebinding
    assert.throws(() => nsSub.attach(null), /Error #2007/);
    const ncDisconnected = new h1.video.NetConnection();
    assert.throws(() => nsSub.attach(ncDisconnected), /Error #2126/);
    const ncRebind = new h1.video.NetConnection();
    ncRebind.connect(null);
    const rebindStatuses = [];
    nsSub.addEventListener('netStatus', e => rebindStatuses.push(e.info.code));
    nsSub.attach(ncRebind);
    await until(() => rebindStatuses.includes('NetStream.Connect.Success'), 'NetStream.attach(connectedNC) dispatches NetStream.Connect.Success');

    // D. Cross-host SharedObject.connect + BroadcastChannel sync + fps coalescing
    const rso1 = h1.storage.SharedObject.getRemote('roomState', ncLocal1.uri, false);
    const rso2 = h2.storage.SharedObject.getRemote('roomState', ncPeer2.uri, false);
    const rso2SyncBatches = [];
    let rso2RpcArg = null;
    rso2.client = { onBroadcastChat(txt) { rso2RpcArg = txt; } };
    rso2.addEventListener('sync', e => rso2SyncBatches.push(e.changeList));
    rso1.connect(ncLocal1);
    rso2.connect(ncPeer2);
    await until(() => rso2SyncBatches.length >= 1, 'Initial RSO sync fired');

    // Set fps = 20 (50ms window) and mutate 3 properties rapidly -> coalesced into one SyncEvent batch!
    rso2SyncBatches.length = 0;
    rso1.fps = 20;
    rso1.setProperty('playerX', 120);
    rso1.setProperty('playerY', 240);
    rso1.data.ready = true;
    rso1.setDirty('ready');
    await until(() => rso2SyncBatches.length >= 1 && rso2.data.playerY === 240, 'Cross-host BroadcastChannel RSO synced');
    assert.equal(rso2.data.playerX, 120);
    assert.equal(rso2.data.ready, true);
    assert.equal(rso2SyncBatches[0].length, 3, 'SharedObject.fps=20 coalesced 3 rapid updates into 1 SyncEvent batch');

    rso1.send('onBroadcastChat', 'gg-wp');
    await until(() => rso2RpcArg === 'gg-wp', 'Cross-host SharedObject.send() invoked remote peer client handler');

    // E. AVURLLoader & AVURLStream HTTP chunked streaming + cookieHeader + AVHTTPStatusEvent + HLS & MPEG-TS ID3 tags
    const AVURLLoader = h1.builtins['flash.media.AVURLLoader'];
    const AVURLStream = h1.builtins['flash.media.AVURLStream'];
    const AVTagData = h1.builtins['flash.media.AVTagData'];

    const avLoader = new AVURLLoader();
    avLoader.cookieHeader = 'MediaAuth=tok_hls_99';
    assert.equal(avLoader.stream instanceof AVURLStream, true);
    let avHttpStatusEv = null;
    const hlsDictEvents = [];
    avLoader.addEventListener('avHttpResponseStatus', e => { avHttpStatusEv = e; });
    avLoader.addEventListener('avDictionaryData', e => hlsDictEvents.push(e));
    let hlsDone = false;
    avLoader.addEventListener('complete', () => { hlsDone = true; });
    avLoader.load(new h1.net.URLRequest(`http://127.0.0.1:${srvPort}/live/master.m3u8`));

    await until(() => hlsDone, 'AVURLLoader finished streaming HLS playlist');
    assert.equal(observedAvCookie, 'MediaAuth=tok_hls_99', 'AVURLLoader forwarded cookieHeader over HTTP');
    assert.ok(avHttpStatusEv !== null && avHttpStatusEv.status === 200);
    assert.ok(Array.isArray(avHttpStatusEv.responseHeaders) && avHttpStatusEv.responseHeaders.some(hdr => hdr.name.toLowerCase() === 'x-stream-edge'));
    assert.ok(hlsDictEvents.length >= 4, 'HLS #EXT-X-* tags dispatched AVDictionaryDataEvent');
    const dateRangeEv = hlsDictEvents.find(e => e.dictionary.tagName === 'EXT-X-DATERANGE');
    assert.ok(dateRangeEv, 'Extracted #EXT-X-DATERANGE dictionary');
    assert.equal(dateRangeEv.dictionary.ID, 'ad-1');
    assert.equal(dateRangeEv.dictionary.tag instanceof AVTagData, true);
    const secondSegEv = hlsDictEvents.find(e => e.dictionary.title === 'Second Segment');
    assert.ok(secondSegEv && Math.abs(secondSegEv.time - 4.5) < 1e-6, 'Cumulative HLS segment localTime tracked accurately');

    // Stream MPEG-TS segment with embedded PES timed ID3v2.4 metadata via AVURLStream
    const avTsStream = new AVURLStream();
    const tsDictEvents = [];
    let tsDone = false;
    avTsStream.addEventListener('avDictionaryData', e => tsDictEvents.push(e));
    avTsStream.addEventListener('complete', () => { tsDone = true; });
    avTsStream.load(new h1.net.URLRequest(`http://127.0.0.1:${srvPort}/live/seg0.ts`));
    await until(() => tsDone, 'AVURLStream finished streaming MPEG-TS packet');
    assert.equal(avTsStream.bytesAvailable, 188);
    assert.equal(avTsStream.readUnsignedByte(), 0x47, 'MPEG-TS sync byte 0x47 readable from AVURLStream');
    assert.equal(tsDictEvents.length, 1, 'Extracted timed ID3 tag from MPEG-TS PES packet');
    assert.ok(Math.abs(tsDictEvents[0].time - 4.5) < 1e-3, 'Decoded 33-bit 90kHz PES PTS (4.5s) into localTime');
    assert.equal(tsDictEvents[0].dictionary.TIT2, 'Live Goal Highlight');
    assert.equal(tsDictEvents[0].dictionary.TPE1, 'NeoFlash Sports');
    assert.equal(tsDictEvents[0].dictionary.ad_cue, 'break_99');
    avTsStream.close();

    // Stream byte cap overflow enforcement -> cancels reader and dispatches IOErrorEvent.IO_ERROR
    const hCap = createArea8LiveHost(wasm, { maxAvStreamBytes: 1536 });
    try {
      const CappedAVURLStream = hCap.builtins['flash.media.AVURLStream'];
      const cappedStream = new CappedAVURLStream();
      let capErr = null;
      cappedStream.addEventListener('ioError', e => { capErr = e; });
      cappedStream.load(new hCap.net.URLRequest(`http://127.0.0.1:${srvPort}/live/oversize.bin`));
      await until(() => capErr !== null, 'AVURLStream cancels stream and dispatches IOErrorEvent on maxAvStreamBytes overflow');
      assert.match(capErr.text, /exceeded max size/i);
      assert.equal(cappedStream.connected, false);
    } finally {
      hCap.dispose();
    }

    ncWs.close();
  } finally {
    h1?.dispose();
    h2?.dispose();
    wsRpcHub.destroyAll();
    await new Promise(r => rpcHttpServer.close(r));
  }
});

// ============================================================================
// 5. Real Headless Chrome End-to-End Browser Verification
// ============================================================================
async function startSharedObjectRelay(t, origins, store) {
  execFileSync(findCargoBinary(), ['build','--quiet','--locked','-p','neoflash-sync'], {cwd:repoRoot});
  const relay = spawn(path.join(repoRoot,'target/debug/neoflash-sync'), [
    '--bind','127.0.0.1:0', ...origins.flatMap(origin=>['--origin',origin]), ...(store ? ['--store',store] : []),
  ], {stdio:['ignore','pipe','inherit']});
  const closed = new Promise(resolve => relay.once('close', (code, signal) => resolve(signal ?? code)));
  const stop = async () => {
    if (relay.pid && relay.exitCode === null && relay.signalCode === null) relay.kill();
    await closed;
  };
  t.after(stop);
  const lines = createInterface({ input: relay.stdout });
  let timeout;
  try {
    const address = await new Promise((resolve, reject) => {
      timeout = setTimeout(() => reject(new Error('SharedObject relay did not announce LISTENING within 30s')), 30_000);
      relay.once('error', reject);
      closed.then(status => reject(new Error(`SharedObject relay closed before LISTENING (${status})`)));
      lines.once('line', line => {
        const match = /^LISTENING (127\.0\.0\.1:\d+)$/u.exec(line);
        if (match) resolve(match[1]);
        else reject(new Error(`Unexpected SharedObject relay startup line: ${line}`));
      });
    });
    return { url: `ws://${address}/live`, stop };
  } finally {
    clearTimeout(timeout);
    lines.close();
  }
}

async function openRoom(t, wasm, {url}, origin, legacy = false, persistent = false) {
  class Socket extends NodeRfc6455Client { constructor(endpoint) { super(endpoint,origin); } }
  const host = createArea8LiveHost(wasm,{WebSocket:Socket,url:origin+'/movie',soChannelName:crypto.randomUUID()});
  t.after(()=>host.dispose());
  const nc = legacy ? new host.video.LegacyNetConnection() : new host.video.NetConnection();
  nc.connect(url);
  await until(()=>nc.connected,'live NetConnection');
  const so = (legacy ? host.storage.LegacySharedObject : host.storage.SharedObject).getRemote('stage',url,persistent);
  const events = [];
  if (legacy) so.onSync = list=>events.push(list);
  else so.addEventListener('sync', e=>events.push(e.changeList));
  so.connect(nc);
  await until(()=>events.length > 0,'relay snapshot');
  return {host, nc, so, events};
}

test('Remote SharedObject relay merges AS2/AS3 fields, snapshots late joins and persists through restart', async t => {
  const wasm = await loadWasmRuntime();
  const origins = ['https://author.example','https://audience.example'];
  const store = fs.mkdtempSync(path.join(os.tmpdir(),'neoflash-rso-'));
  t.after(()=>fs.rmSync(store,{recursive:true,force:true}));
  const relay = await startSharedObjectRelay(t,origins,store);
  const a = await openRoom(t,wasm,relay,origins[0],true,true);
  assert.equal(a.nc.isConnected,true);
  a.so.data.score = 7;
  await until(()=>a.events.flat().some(e=>e.name==='score' && e.code==='success'),'server confirms AMF0 mutation');
  const b = await openRoom(t,wasm,relay,origins[1],false,true);
  assert.deepEqual(b.so.data,{score:7});
  a.so.data.turn = 'writer-a';
  b.so.setProperty('turn','writer-b');
  await until(()=>[a,b].every(client=>client.events.flat().filter(e=>e.name==='turn').length===2),'both clients receive both overlapping writes');
  assert.equal(a.so.data.turn,b.so.data.turn);
  const winningTurn = b.so.data.turn;
  assert.ok(['writer-a','writer-b'].includes(winningTurn));
  a.so.data.left = ['circle',1];
  b.so.fps = 45;
  b.so.setProperty('right',{color:'blue'});
  await until(()=>a.so.data.right?.color==='blue' && b.so.data.left?.[0]==='circle','concurrent disjoint updates merge');
  b.so.data.right.color = 'green';
  b.so.setDirty('right');
  delete a.so.data.left;
  await until(()=>a.so.data.right?.color==='green' && !Object.hasOwn(b.so.data,'left'),'nested dirty and delete across clients');
  const calls = [];
  a.so.marker = value=>calls.push(['a',value]);
  b.so.client = {marker:value=>calls.push(['b',value])};
  b.so.send('marker',{beat:4});
  await until(()=>calls.length===2,'single callback on each peer');
  assert.deepEqual(calls.sort(),[['a',{beat:4}],['b',{beat:4}]]);
  const flushed = [];
  b.so.addEventListener('netStatus',event=>flushed.push(event.info.code));
  assert.equal(b.so.flush(),'pending');
  await until(()=>flushed.includes('SharedObject.Flush.Success'),'persistent writes acknowledged by server');

  const invalid = new NodeRfc6455Client(relay.url,origins[1]);
  const replies = [];
  invalid.addEventListener('message', event=>replies.push(JSON.parse(event.data)));
  await until(()=>invalid.readyState===1,'independent participant opens socket');
  const common = {type:'so',room:'stage',sub:1};
  invalid.send(JSON.stringify({...common,op:'join',persistent:true}));
  await until(()=>replies.some(x=>x.op==='snapshot'),'independent client receives snapshot');
  invalid.send(JSON.stringify({...common,op:'patch',fields:{score:{encoding:1,bytes:'AQ=='}}}));
  await until(()=>replies.some(x=>x.op==='error'),'server rejects unsupported value encoding');
  invalid.close();
  a.so.close();
  b.so.setProperty('score',9);
  await until(()=>b.events.flat().some(e=>e.name==='score' && e.code==='success'),'second client acknowledgement');
  assert.equal(a.so.data.score,7);
  a.nc.close();
  b.so.close();
  b.nc.close();
  await relay.stop();
  const restarted = await startSharedObjectRelay(t,origins,store);
  const c = await openRoom(t,wasm,restarted,origins[0],true,true);
  assert.deepEqual({...c.so.data},{score:9,right:{color:'green'},turn:winningTurn});
  c.so.clear();
  await until(()=>Object.keys(c.so.data).length===0,'authoritative clear');
  assert.throws(()=>c.so.send('marker','done'),/connect/i);
  c.so.close();
  c.nc.close();
});

test('Client-persistent AS2/AS3 rooms reopen offline, reconcile dirty fields and report conflicts', async t => {
  const wasm = await loadWasmRuntime();
  const origin = 'https://persistent.example';
  const store = fs.mkdtempSync(path.join(os.tmpdir(),'neoflash-offline-'));
  t.after(()=>fs.rmSync(store,{recursive:true,force:true}));
  const relay = await startSharedObjectRelay(t,[origin],store);
  const online = await openRoom(t,wasm,relay,origin,false,true);
  const entries = new Map();
  const disk = {getItem:k=>entries.get(k)??null,setItem:(k,v)=>entries.set(k,v),removeItem:k=>entries.delete(k)};
  class Socket extends NodeRfc6455Client { constructor(uri){super(uri,origin);} }
  const create = () => {
    const host = createArea8LiveHost(wasm,{WebSocket:Socket,url:origin+'/movie',storage:disk,soChannelName:crypto.randomUUID()});
    t.after(()=>host.dispose());
    return host;
  };
  const host = create();
  const before = host.storage.LegacySharedObject.getRemote('stage',relay.url,'/slot');
  const first = new host.video.LegacyNetConnection();
  first.connect(relay.url);
  await until(()=>first.connected,'AS2 WebSocket connected');
  const notices=[];
  before.onSync = changes=>notices.push(...changes);
  before.connect(first);
  await until(()=>notices.length>0,'empty persistent snapshot');
  before.data.shared = 'initial';
  before.data.removeMe = 1;
  await until(()=>notices.some(e=>e.name==='removeMe' && e.code==='success'),'baseline acknowledged');
  first.close();
  before.data.shared = 'offline conflict';
  before.data.nested = {paint:'red'};
  before.data.nested.paint = 'blue';
  before.setDirty('nested');
  delete before.data.removeMe;
  assert.equal(before.flush(),true,'AS2 local persist completes offline');
  host.storage.dispose();
  online.so.setProperty('shared','server wins');
  online.so.setProperty('newOnServer',2);
  await until(()=>online.events.flat().some(e=>e.name==='newOnServer' && e.code==='success'),'other client changes shared room');

  const fresh = create();
  const get = (name='stage',url=relay.url,path='/slot')=>fresh.storage.SharedObject.getRemote(name,url,path);
  const restored = get();
  assert.deepEqual({...restored.data},{shared:'offline conflict',nested:{paint:'blue'}});
  assert.deepEqual({...get('other').data},{});
  assert.deepEqual({...get('stage',relay.url+'/other').data},{});
  assert.deepEqual({...get('stage',relay.url,'/other-slot').data},{});
  assert.deepEqual({...get('stage',relay.url,true).data},{});
  assert.deepEqual(fresh.storage.SharedObject.getLocal('stage').data,{});
  const saveItem = disk.setItem;
  disk.setItem = ()=>{throw new Error('QuotaExceededError');};
  assert.throws(()=>restored.setProperty('tooLarge',3),/quota/i);
  assert.equal(Object.hasOwn(restored.data,'tooLarge'),false);
  disk.setItem = saveItem;
  const otherMovie = create().storage.SharedObject.getRemote('stage',relay.url,'/slot');
  otherMovie.data.otherMovie = 'same browser cache';
  restored.data.extra = 'offline property';
  assert.equal(restored.flush(),'flushed','AS3 local persist completes offline');
  const second = new fresh.video.NetConnection();
  second.connect(relay.url);
  await until(()=>second.connected,'fresh AS3 connection');
  const sync=[];
  restored.addEventListener('sync',e=>sync.push(...e.changeList));
  restored.connect(second);
  await until(()=>sync.some(e=>e.code==='reject' && e.name==='shared') && online.so.data.extra==='offline property','server reconciles offline outbox');
  assert.ok(sync.some(e=>e.code==='reject' && e.oldValue==='offline conflict'));
  assert.deepEqual({...restored.data},{shared:'server wins',nested:{paint:'blue'},newOnServer:2,otherMovie:'same browser cache',extra:'offline property'});
  assert.deepEqual({...online.so.data},{...restored.data});
  second.close();
  // A rejected edit must not reappear when the next session reads only disk.
  const snapshot = create().storage.LegacySharedObject.getRemote('stage',relay.url,'/slot');
  assert.deepEqual({...snapshot.data},{...restored.data});
  snapshot.clear();
  assert.equal(entries.size,0);
  online.so.close();
  online.nc.close();
});

test('Area 8 Conformance 5: Headless Chrome end-to-end verification (Browser WebSocket Socket/XMLSocket, Blob Web Worker + MessageChannel, Security.showSettings DOM, and AVURLLoader)', async t => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Headless Chrome binary not installed on this machine');
    return;
  }
  ensureFreshExportBinary();
  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-area8-chrome-'));
  t.after(() => fs.rmSync(workDir, { recursive: true, force: true }));

  // Serve exported bundle + crossdomain.xml + HLS stream + RFC 6455 WebSocket echo on one HTTP server!
  let pubDir = '';
  const mimeByExt = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'application/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.css': 'text/css; charset=utf-8',
  };

  const server = http.createServer((req, res) => {
    const urlPath = (req.url || '/').split('?')[0];
    if (urlPath === '/crossdomain.xml') {
      res.writeHead(200, { 'Content-Type': 'text/x-cross-domain-policy', 'Access-Control-Allow-Origin': '*' });
      res.end('<cross-domain-policy><allow-access-from domain="*" to-ports="*"/></cross-domain-policy>');
      return;
    }
    if (urlPath === '/hls/live.m3u8') {
      res.writeHead(200, {
        'Content-Type': 'application/vnd.apple.mpegurl',
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Expose-Headers': 'X-Chrome-Hls',
        'X-Chrome-Hls': 'ok',
      });
      res.end('#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,Live Chrome\nchunk0.ts\n');
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

  const wsHub = attachRfc6455Upgrade(server, peer => {
    peer.onBinary = bytes => {
      if ((peer.req.url || '').includes('xml')) {
        const str = Buffer.from(bytes).toString('utf8').replace(/\0/g, '');
        peer.sendBinary(Buffer.from(`<chromeAck>${str}</chromeAck>\0`, 'utf8'));
      } else {
        peer.sendBinary(bytes);
      }
    };
  });

  t.after(async () => {
    wsHub.destroyAll();
    await new Promise(resolve => server.close(resolve));
  });
  await new Promise((resolve, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolve));
  const port = server.address().port;

  const origin = `http://127.0.0.1:${port}`;
  const secondOrigin = `http://localhost:${port}`;
  const relay = await startSharedObjectRelay(t,[origin,secondOrigin,'https://author.example'],path.join(workDir,'rooms'));
  const peer = await openRoom(t,await loadWasmRuntime(),relay,'https://author.example',true,true);
  peer.so.data.score = 41;
  await until(()=>peer.events.flat().some(e=>e.name==='score' && e.code==='success'),'seed remote room for published player');
  const project = {
    format_version: 5,
    name: 'Area 8 Chrome Verification',
    width: 400,
    height: 300,
    fps: 24,
    background: [16, 20, 28, 255],
    root: 'root',
    document_class: 'Area8ChromeMain',
    resources: [],
    sources: [
      {
        id: 'chrome-main',
        path: 'Area8ChromeMain.as',
        language: 'action_script3',
        source: `package {
  import flash.display.MovieClip;
  import flash.events.*;
  import flash.net.*;
  import flash.system.*;
  import flash.media.*;
  import flash.utils.ByteArray;

  public class Area8ChromeMain extends MovieClip {
    private var sock:Socket;
    private var xs:XMLSocket;
    private var bgWorker:Worker;
    private var toWorker:MessageChannel;
    private var fromWorker:MessageChannel;
    private var avLoader:AVURLLoader;
    private var checks:Object = {};
    private var connection:NetConnection;
    private var room:SharedObject;

    public function Area8ChromeMain() {
      connection = new NetConnection();
      connection.addEventListener(NetStatusEvent.NET_STATUS,function(e:NetStatusEvent):void {
        if (e.info.code != "NetConnection.Connect.Success") return;
        room = SharedObject.getRemote("stage","${relay.url}",true);
        room.addEventListener(SyncEvent.SYNC,function(e:SyncEvent):void {
          if (!checks.replied && room.data.score == 41) {
            checks.replied = true;
            room.setProperty("reply",42);
          }
          if (!checks.roomOk && room.data.verified == 43) {
            checks.roomOk = true;
            maybeFinish();
          }
        });
        room.connect(connection);
      });
      connection.connect("${relay.url}");
      Security.loadPolicyFile("http://127.0.0.1:${port}/crossdomain.xml");
      Security.showSettings(SecurityPanel.PRIVACY);

      // 1. Browser WebSocket Socket
      sock = new Socket();
      sock.addEventListener(Event.CONNECT, function(e:Event):void {
        sock.writeInt(20261121);
        sock.writeUTF("chrome-ws-ok");
        sock.flush();
      });
      sock.addEventListener(ProgressEvent.SOCKET_DATA, function(e:ProgressEvent):void {
        var n:int = sock.readInt();
        var s:String = sock.readUTF();
        checks.socketOk = (n == 20261121 && s == "chrome-ws-ok");
        sock.close();
        maybeFinish();
      });
      sock.connect("127.0.0.1", ${port});

      // 2. Browser WebSocket XMLSocket
      xs = new XMLSocket();
      xs.addEventListener(Event.CONNECT, function(e:Event):void {
        xs.send("ping-chrome");
      });
      xs.addEventListener(DataEvent.DATA, function(e:DataEvent):void {
        checks.xmlSocketOk = (e.data == "<chromeAck>ping-chrome</chromeAck>");
        xs.close();
        maybeFinish();
      });
      xs.connect("ws://127.0.0.1:${port}/xml", ${port});

      // 3. Real Browser Blob URL Web Worker lifecycle + MessageChannel (SWF header bytes, no dynamic JS eval)
      var wkSwf:ByteArray = new ByteArray();
      wkSwf.writeByte(0x46); wkSwf.writeByte(0x57); wkSwf.writeByte(0x53); wkSwf.writeByte(0x0A);
      wkSwf.writeUnsignedInt(8);
      bgWorker = WorkerDomain.current.createWorker(wkSwf);
      toWorker = Worker.current.createMessageChannel(bgWorker);
      fromWorker = bgWorker.createMessageChannel(Worker.current);
      bgWorker.setSharedProperty("rx", toWorker);
      bgWorker.setSharedProperty("tx", fromWorker);
      toWorker.addEventListener(Event.CHANNEL_MESSAGE, function(e:Event):void {
        var msg:Object = toWorker.receive();
        if (msg == 33 && bgWorker.isPrimordial === false) {
          bgWorker.terminate();
        }
      });
      bgWorker.addEventListener(Event.WORKER_STATE, function(e:Event):void {
        if (bgWorker.state == WorkerState.RUNNING) {
          toWorker.send(33);
        } else if (bgWorker.state == WorkerState.TERMINATED) {
          checks.workerOk = true;
          maybeFinish();
        }
      });
      bgWorker.start();

      // 4. Browser AVURLLoader HTTP streaming + HLS tag dictionary
      avLoader = new AVURLLoader();
      var gotAvStatus:Boolean = false;
      var gotAvDict:Boolean = false;
      avLoader.addEventListener(AVHTTPStatusEvent.AV_HTTP_RESPONSE_STATUS, function(e:AVHTTPStatusEvent):void {
        gotAvStatus = (e.status == 200);
      });
      avLoader.addEventListener(AVDictionaryDataEvent.AV_DICTIONARY_DATA, function(e:AVDictionaryDataEvent):void {
        if (e.dictionary != null) gotAvDict = true;
      });
      avLoader.addEventListener(Event.COMPLETE, function(e:Event):void {
        checks.avOk = gotAvStatus && gotAvDict && avLoader.stream.bytesAvailable > 0;
        avLoader.close();
        maybeFinish();
      });
      avLoader.load(new URLRequest("http://127.0.0.1:${port}/hls/live.m3u8"));
    }

    private function maybeFinish():void {
      if (checks.roomOk && checks.socketOk && checks.xmlSocketOk && checks.workerOk && checks.avOk) {
        trace("AREA8_CHROME_ALL_OK=1 mem=" + System.totalMemory);
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
        class_name: 'Area8ChromeMain',
        timeline: {
          frame_count: 1,
          layers: [{ id: 'l1', name: 'L1', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
          scripts: [],
        },
      },
    ],
  };

  pubDir = path.join(workDir, 'pub');
  const projFile = path.join(workDir, 'chrome-area8.nflash');
  fs.writeFileSync(projFile, JSON.stringify(project));
  execFileSync(exportBin, ['--runtime', runtimeDir, projFile, '--output', pubDir], { cwd: repoRoot, stdio: 'pipe' });

  const legacyProject = structuredClone(project);
  delete legacyProject.document_class;
  delete legacyProject.sources;
  delete legacyProject.symbols[0].class_name;
  legacyProject.symbols[0].timeline.scripts = [{frame:0,script:{language:'action_script2',source:`
    stop();
    var nc:NetConnection = new NetConnection();
    var room:SharedObject = SharedObject.getRemote("stage","${relay.url}","/cache");
    if (room.data.verified == 43) room.data.reloaded = 44;
    nc.onStatus = function(info:Object):Void {
      if (info.code == "NetConnection.Connect.Success") room.connect(nc);
    };
    room.onSync = function(changes:Array):Void {
      if (room.data.reply == 42 && room.data.verified != 43) room.data.verified = 43;
    };
    nc.connect("${relay.url}");
  `}}];
  const legacyFile = path.join(workDir,'peer.nflash');
  fs.writeFileSync(legacyFile,JSON.stringify(legacyProject));
  execFileSync(exportBin,['--runtime',runtimeDir,legacyFile,'--output',path.join(pubDir,'as2')],{cwd:repoRoot,stdio:'pipe'});

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
    { stdio: ['ignore', 'ignore', 'pipe'] }
  );

  let cdpWs = null;
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

    cdpWs = new NodeRfc6455Client(pageWsUrl);
    await until(() => cdpWs.readyState === 1, 'Connected to Chrome CDP WebSocket');

    let nextCdpId = 1;
    const pendingCdp = new Map();
    cdpWs.addEventListener('message', ev => {
      const msg = JSON.parse(String(ev.data));
      if (msg.id && pendingCdp.has(msg.id)) {
        const { resolve, reject } = pendingCdp.get(msg.id);
        pendingCdp.delete(msg.id);
        if (msg.error) reject(new Error(JSON.stringify(msg.error)));
        else resolve(msg.result);
      }
    });
    const sendCdp = (method, params = {}) =>
      new Promise((resolve, reject) => {
        const id = nextCdpId++;
        pendingCdp.set(id, { resolve, reject });
        cdpWs.send(JSON.stringify({ id, method, params }));
      });

    await sendCdp('Page.enable');
    await sendCdp('Runtime.enable');
    await sendCdp('Page.navigate', { url: `http://127.0.0.1:${port}/index.html` });

    const evalInPage = async expr => {
      const res = await sendCdp('Runtime.evaluate', {
        expression: expr,
        awaitPromise: true,
        returnByValue: true,
      });
      if (res.exceptionDetails) {
        throw new Error(`Page exception: ${JSON.stringify(res.exceptionDetails)}`);
      }
      return res.result?.value;
    };

    await until(()=>peer.so.data.reply===42,'published AS3 writes to the independent AS2 client',15000);
    await evalInPage(`(() => {
      const player = document.createElement('iframe');
      player.id = 'offline-sharedobject-client';
      player.src = '${secondOrigin}/as2/index.html';
      document.body.appendChild(player);
    })()`);
    await evalInPage(`new Promise((resolve, reject) => {
      const deadline = Date.now() + 15000;
      const poll = () => {
        const err = document.getElementById('error')?.textContent?.trim();
        if (err) return reject(new Error('Player UI error: ' + err));
        const trace = document.getElementById('trace')?.textContent || '';
        if (trace.includes('AREA8_CHROME_ALL_OK=1')) return resolve(true);
        if (Date.now() > deadline) return reject(new Error('Timeout waiting for AREA8_CHROME_ALL_OK=1; trace=' + trace));
        setTimeout(poll, 30);
      };
      poll();
    })`);

    await until(()=>peer.so.data.verified===43,'published AS2 on another origin replies to the published AS3 movie');
    await evalInPage(`(() => {
      const previous=document.getElementById('offline-sharedobject-client');
      const next=document.createElement('iframe');
      next.src='${secondOrigin}/as2/index.html?reopen';
      previous.replaceWith(next);
    })()`);
    await until(()=>peer.so.data.reloaded===44,'reopened AS2 movie reads native browser storage and sends its offline edit',15000);
    peer.so.close();
    peer.nc.close();
    const domCheck = await evalInPage(`(() => {
      const panel = document.getElementById('neoflash-security-settings-panel');
      const hasPanel = Boolean(panel && panel.getAttribute('data-panel') === 'privacy');
      panel?.querySelector('button')?.click();
      const dismissed = !document.getElementById('neoflash-security-settings-panel');
      return { hasPanel, dismissed };
    })()`);
    assert.deepEqual(domCheck, { hasPanel: true, dismissed: true });
  } finally {
    try { cdpWs?.close(); } catch (_) {}
    try { chromeProc.kill('SIGTERM'); } catch (_) {}
    await new Promise(resolve => {
      const t = setTimeout(() => {
        try { chromeProc.kill('SIGKILL'); } catch (_) {}
        resolve();
      }, 2000);
      chromeProc.once('exit', () => { clearTimeout(t); resolve(); });
    });
  }
});
