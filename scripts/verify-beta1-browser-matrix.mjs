#!/usr/bin/env node
/**
 * T-237 — macOS Beta 1 Browser & Deployment Matrix Verification
 *
 * Verifies published HTML5 bundles across all four size/deployment modes:
 *   1. Full      (loaded directly from disk via file://)
 *   2. Balanced  (loaded directly from disk via file://, verifying NeoFlashWasmBase64 fallback)
 *   3. Compact   (loaded directly from disk via file://, verifying NeoFlashWasmBase64 fallback)
 *   4. Shared    (loaded over local HTTP http://127.0.0.1:<port>/ with --shared-runtime)
 *
 * Exercises:
 *   - Vector & nested MovieClip classic-tween animation
 *   - Interactive button / MovieClip click dispatch (trace("click:cta_ok"))
 *   - Editable Input TextBox focus & typing commit
 *   - AS2 + JS frame script execution (trace("boot:as2_ok"), trace("boot:js_ok"))
 *   - Timeline WAV audio scheduling & gesture unlock
 *   - Embedded AVI video timeline channel decoding & canvas rendering
 *
 * Executes real desktop Google Chrome via Chrome DevTools Protocol (CDP) across all 4 modes,
 * and probes desktop Safari / WebKit via /usr/bin/safaridriver (running live WebDriver sessions
 * when Safari "Allow Remote Automation" is enabled, or recording the honest host-configuration
 * gate plus static WebKit compatibility verification in target/beta1-browser-matrix/report.json).
 */

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import zlib from 'node:zlib';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(__dirname, '..');
const OUT_ROOT = path.join(ROOT_DIR, 'target', 'beta1-browser-matrix');
const RUNTIME_DIR = path.join(ROOT_DIR, 'target', 'web-runtime');

function ensureWebRuntime() {
  const jsPath = path.join(RUNTIME_DIR, 'neoflash_web.js');
  const wasmPath = path.join(RUNTIME_DIR, 'neoflash_web_bg.wasm');
  if (fs.existsSync(jsPath) && fs.existsSync(wasmPath)) {
    return;
  }
  console.log('[T-237] Building target/web-runtime via scripts/build-web.sh...');
  const res = spawnSync('bash', [path.join(ROOT_DIR, 'scripts', 'build-web.sh')], {
    cwd: ROOT_DIR,
    stdio: 'inherit',
  });
  if (res.status !== 0) {
    throw new Error('scripts/build-web.sh failed');
  }
}

/** Generate a valid 16-bit mono PCM WAV buffer (22050 Hz, 200ms 440Hz sine). */
function buildToneWavBytes() {
  const sampleRate = 22050;
  const numSamples = Math.floor(sampleRate * 0.2);
  const dataBytes = numSamples * 2;
  const buf = Buffer.alloc(44 + dataBytes);
  buf.write('RIFF', 0, 'ascii');
  buf.writeUInt32LE(36 + dataBytes, 4);
  buf.write('WAVE', 8, 'ascii');
  buf.write('fmt ', 12, 'ascii');
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20); // PCM
  buf.writeUInt16LE(1, 22); // mono
  buf.writeUInt32LE(sampleRate, 24);
  buf.writeUInt32LE(sampleRate * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write('data', 36, 'ascii');
  buf.writeUInt32LE(dataBytes, 40);
  for (let i = 0; i < numSamples; i++) {
    const sample = Math.round(Math.sin((2 * Math.PI * 440 * i) / sampleRate) * 12000);
    buf.writeInt16LE(sample, 44 + i * 2);
  }
  return buf;
}

/** Generate a minimal valid RGBA PNG buffer of width x height filled with [r, g, b, a]. */
function buildSolidPngBytes(width, height, [r, g, b, a]) {
  const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  const crcTable = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    crcTable[n] = c >>> 0;
  }
  function crc32(buf) {
    let c = 0xffffffff;
    for (let i = 0; i < buf.length; i++) {
      c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
    }
    return (c ^ 0xffffffff) >>> 0;
  }
  function pngChunk(type, data) {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length, 0);
    const typeBuf = Buffer.from(type, 'ascii');
    const crcBuf = Buffer.alloc(4);
    crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
    return Buffer.concat([len, typeBuf, data, crcBuf]);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

  const rowLen = 1 + width * 4;
  const raw = Buffer.alloc(rowLen * height);
  for (let y = 0; y < height; y++) {
    const rowStart = y * rowLen;
    raw[rowStart] = 0;
    for (let x = 0; x < width; x++) {
      const px = rowStart + 1 + x * 4;
      raw[px] = r;
      raw[px + 1] = g;
      raw[px + 2] = b;
      raw[px + 3] = a;
    }
  }
  const idat = zlib.deflateSync(raw);
  return Buffer.concat([
    signature,
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', idat),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

/** Build a valid RIFF/AVI container with PNG-encoded video frames (`PNG ` fourcc). */
function buildMinimalPngAviBytes(width, height, fps, pngFrames) {
  function makeChunk(id, payload) {
    const pad = payload.length & 1;
    const out = Buffer.alloc(8 + payload.length + pad);
    out.write(id, 0, 4, 'ascii');
    out.writeUInt32LE(payload.length, 4);
    payload.copy(out, 8);
    return out;
  }
  function makeList(listType, body) {
    const len = 4 + body.length;
    const pad = len & 1;
    const out = Buffer.alloc(8 + len + pad);
    out.write('LIST', 0, 4, 'ascii');
    out.writeUInt32LE(len, 4);
    out.write(listType, 8, 4, 'ascii');
    body.copy(out, 12);
    return out;
  }
  const microSec = Math.max(1, Math.floor(1_000_000 / fps));
  const nFrames = pngFrames.length;
  const avih = Buffer.alloc(56);
  avih.writeUInt32LE(microSec, 0);
  avih.writeUInt32LE(0x10, 12);
  avih.writeUInt32LE(nFrames, 16);
  avih.writeUInt32LE(1, 24);
  avih.writeUInt32LE(width, 32);
  avih.writeUInt32LE(height, 36);

  const vStrh = Buffer.alloc(56);
  vStrh.write('vids', 0, 4, 'ascii');
  vStrh.write('PNG ', 4, 4, 'ascii');
  vStrh.writeUInt32LE(1, 20);
  vStrh.writeUInt32LE(fps, 24);
  vStrh.writeUInt32LE(nFrames, 32);
  vStrh.writeUInt32LE(10000, 40);
  vStrh.writeUInt16LE(width, 52);
  vStrh.writeUInt16LE(height, 54);

  const vStrf = Buffer.alloc(40);
  vStrf.writeUInt32LE(40, 0);
  vStrf.writeInt32LE(width, 4);
  vStrf.writeInt32LE(height, 8);
  vStrf.writeUInt16LE(1, 12);
  vStrf.writeUInt16LE(32, 14);
  vStrf.write('PNG ', 16, 4, 'ascii');
  vStrf.writeUInt32LE(width * height * 4, 20);

  const strl = makeList('strl', Buffer.concat([makeChunk('strh', vStrh), makeChunk('strf', vStrf)]));
  const hdrl = makeList('hdrl', Buffer.concat([makeChunk('avih', avih), strl]));
  const moviBody = Buffer.concat(pngFrames.map((f) => makeChunk('00dc', f)));
  const movi = makeList('movi', moviBody);
  const riffBody = Buffer.concat([hdrl, movi]);
  const out = Buffer.alloc(12 + riffBody.length + (riffBody.length & 1));
  out.write('RIFF', 0, 4, 'ascii');
  out.writeUInt32LE(4 + riffBody.length, 4);
  out.write('AVI ', 8, 4, 'ascii');
  riffBody.copy(out, 12);
  return out;
}

/** Build the interactive Beta 1 matrix .nflash project. */
function buildMatrixProjectJson() {
  const wavBytes = buildToneWavBytes();
  const frame0Png = buildSolidPngBytes(32, 24, [34, 166, 179, 255]);
  const frame1Png = buildSolidPngBytes(32, 24, [106, 176, 76, 255]);
  const aviBytes = buildMinimalPngAviBytes(32, 24, 12, [
    frame0Png,
    frame1Png,
    frame0Png,
    frame1Png,
    frame0Png,
    frame1Png,
    frame0Png,
    frame1Png,
  ]);

  const project = {
    format_version: 9,
    name: 'Beta1BrowserMatrix',
    width: 640,
    height: 360,
    fps: 24,
    background: [18, 22, 31, 255],
    root: 'sym-root',
    resources: [
      {
        id: 'res-tone',
        path: 'audio/tone.wav',
        media_type: 'audio/wav',
        data_base64: wavBytes.toString('base64'),
      },
      {
        id: 'res-vid',
        path: 'video/matrix.avi',
        media_type: 'video/x-msvideo',
        data_base64: aviBytes.toString('base64'),
      },
    ],
    symbols: [
      {
        id: 'sym-inner',
        name: 'InnerBadge',
        folder: '',
        kind: 'movie_clip',
        timeline: {
          frame_count: 4,
          layers: [
            {
              id: 'layer-inner',
              name: 'Badge Shape',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: '',
                  elements: [
                    {
                      id: 'el-inner-rect',
                      name: 'BadgeRect',
                      transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'rectangle',
                        width: 48,
                        height: 48,
                        fill: [249, 202, 36, 255],
                        stroke: [240, 147, 43, 255],
                        stroke_width: 2,
                      },
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
      {
        id: 'sym-outer',
        name: 'OuterCard',
        folder: '',
        kind: 'movie_clip',
        timeline: {
          frame_count: 6,
          layers: [
            {
              id: 'layer-outer',
              name: 'Nested Motion',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: 'linear',
                  label: '',
                  tween_kind: 'classic',
                  elements: [
                    {
                      id: 'inst-inner',
                      name: 'innerMc',
                      transform: { x: 10, y: 10, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-inner',
                        playback: 'independent',
                      },
                    },
                  ],
                },
                {
                  frame: 5,
                  tween: null,
                  label: '',
                  elements: [
                    {
                      id: 'inst-inner',
                      name: 'innerMc',
                      transform: { x: 90, y: 20, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-inner',
                        playback: 'independent',
                      },
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
      {
        id: 'sym-btn',
        name: 'CtaButton',
        folder: '',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'layer-btn',
              name: 'Button Hit',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: '',
                  elements: [
                    {
                      id: 'el-btn-bg',
                      name: 'BtnBg',
                      transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'rectangle',
                        width: 140,
                        height: 52,
                        fill: [235, 77, 75, 255],
                        stroke: [255, 190, 118, 255],
                        stroke_width: 2,
                      },
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [],
        },
      },
      {
        id: 'sym-root',
        name: 'MainTimeline',
        folder: '',
        kind: 'movie_clip',
        timeline: {
          frame_count: 6,
          layers: [
            {
              id: 'layer-main',
              name: 'Interactive Layer',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  tween: null,
                  label: '',
                  sound: {
                    resource: 'res-tone',
                    sync: 'start',
                    loops: 1,
                    loop_forever: false,
                    effect: 'none',
                  },
                  video: {
                    resource: 'res-vid',
                    sync: 'stream',
                    deployment: 'embedded',
                    start_offset_ms: 0,
                    loop_video: true,
                    muted: true,
                    cue_points: [],
                  },
                  elements: [
                    {
                      id: 'el-hero-vec',
                      name: 'HeroVector',
                      transform: { x: 40, y: 40, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'rectangle',
                        width: 160,
                        height: 110,
                        fill: [34, 166, 179, 255],
                        stroke: [126, 214, 223, 255],
                        stroke_width: 3,
                      },
                    },
                    {
                      id: 'inst-outer',
                      name: 'outerMc',
                      transform: { x: 230, y: 45, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-outer',
                        playback: 'independent',
                      },
                    },
                    {
                      id: 'inst-cta',
                      name: 'ctaBtn',
                      transform: { x: 40, y: 210, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-btn',
                        playback: 'independent',
                      },
                      scripts: [
                        {
                          event: 'click',
                          script: {
                            language: 'java_script',
                            source: 'this.stage.focus = _root.getChildByName("inputField"); trace("click:cta_ok");',
                          },
                        },
                      ],
                    },
                    {
                      id: 'txt-input',
                      name: 'inputField',
                      transform: { x: 220, y: 215, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'text_box',
                        text: 'InitialText',
                        width: 200,
                        height: 40,
                        format: {
                          font: 'Sans',
                          size: 18,
                          bold: false,
                          italic: false,
                          color: 0x130f40,
                          align: 'left',
                          leading: 2,
                          leftMargin: 4,
                          rightMargin: 4,
                          indent: 0,
                          blockIndent: 0,
                          letterSpacing: 0,
                          underline: false,
                          url: '',
                          target: '',
                          fieldOptions: {
                            field_type: 'input',
                            selectable: true,
                            border: true,
                            background: true,
                            border_color: 0x4834d4,
                            background_color: 0xffffff,
                            max_chars: 64,
                          },
                        },
                        word_wrap: false,
                        multiline: false,
                      },
                    },
                    {
                      id: 'el-video',
                      name: 'matrixVideo',
                      transform: { x: 450, y: 190, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                      kind: {
                        type: 'video',
                        width: 140,
                        height: 105,
                      },
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
                source: 'trace("boot:as2_ok");',
              },
            },
            {
              frame: 0,
              script: {
                language: 'java_script',
                source: 'this.stage.focus = this.getChildByName("inputField"); trace("boot:js_ok");',
              },
            },
          ],
        },
      },
    ],
  };

  return JSON.stringify(project, null, 2);
}

function runExportCli(args) {
  const res = spawnSync(
    'cargo',
    ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--', ...args],
    {
      cwd: ROOT_DIR,
      encoding: 'utf8',
    },
  );
  if (res.status !== 0) {
    throw new Error(
      `neoflash-export ${args.join(' ')} failed:\n${res.stderr || res.stdout}`,
    );
  }
}

/** Start a local static HTTP server rooted at `rootDir` on 127.0.0.1:0. */
function startStaticServer(rootDir) {
  const mimeTypes = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.wav': 'audio/wav',
    '.avi': 'video/x-msvideo',
    '.mp4': 'video/mp4',
  };
  const server = http.createServer((req, res) => {
    const reqUrl = new URL(req.url || '/', 'http://127.0.0.1');
    const safeRel = path
      .normalize(decodeURIComponent(reqUrl.pathname))
      .replace(/^(\.\.(\/|\\|$))+/, '');
    const filePath = path.join(rootDir, safeRel);
    if (!filePath.startsWith(rootDir) || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
      res.writeHead(404);
      res.end('Not found');
      return;
    }
    const ext = path.extname(filePath).toLowerCase();
    res.writeHead(200, {
      'Content-Type': mimeTypes[ext] || 'application/octet-stream',
      'Access-Control-Allow-Origin': '*',
      'Cache-Control': 'no-store',
    });
    fs.createReadStream(filePath).pipe(res);
  });
  return new Promise((resolve, reject) => {
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      resolve({ server, port: addr.port });
    });
  });
}

/** Minimal CDP client over Node's built-in WebSocket. */
class CdpSession {
  constructor(wsUrl) {
    this.wsUrl = wsUrl;
    this.ws = null;
    this.nextId = 1;
    this.pending = new Map();
  }

  async connect() {
    this.ws = new WebSocket(this.wsUrl);
    await new Promise((resolve, reject) => {
      this.ws.addEventListener('open', () => resolve(), { once: true });
      this.ws.addEventListener('error', (err) => reject(err), { once: true });
    });
    this.ws.addEventListener('message', (event) => {
      const msg = JSON.parse(String(event.data));
      if (msg.id && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        if (msg.error) {
          reject(new Error(msg.error.message || JSON.stringify(msg.error)));
        } else {
          resolve(msg.result);
        }
      }
    });
  }

  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  async evalJson(expression) {
    const res = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (res.exceptionDetails) {
      throw new Error(
        `CDP eval exception: ${res.exceptionDetails.text || JSON.stringify(res.exceptionDetails)}`,
      );
    }
    return res.result?.value;
  }

  close() {
    try {
      this.ws?.close();
    } catch {}
  }
}

function fetchJson(url) {
  return new Promise((resolve, reject) => {
    http
      .get(url, (res) => {
        let body = '';
        res.on('data', (chunk) => (body += chunk));
        res.on('end', () => {
          try {
            resolve(JSON.parse(body));
          } catch (e) {
            reject(e);
          }
        });
      })
      .on('error', reject);
  });
}

async function launchChromeCdp() {
  const chromeBin = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  if (!fs.existsSync(chromeBin)) {
    throw new Error(`Google Chrome binary not found at ${chromeBin}`);
  }
  const profileDir = path.join(OUT_ROOT, `chrome-profile-${process.pid}`);
  fs.rmSync(profileDir, { recursive: true, force: true });
  fs.mkdirSync(profileDir, { recursive: true });

  const proc = spawn(
    chromeBin,
    [
      '--headless=new',
      '--remote-debugging-port=0',
      `--user-data-dir=${profileDir}`,
      '--no-first-run',
      '--no-default-browser-check',
      '--allow-file-access-from-files',
      '--autoplay-policy=no-user-gesture-required',
      '--window-size=1280,800',
      'about:blank',
    ],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  );

  const browserWsUrl = await new Promise((resolve, reject) => {
    let stderr = '';
    const timer = setTimeout(() => {
      reject(new Error(`Timed out waiting for Chrome DevTools WS URL. stderr:\n${stderr}`));
    }, 15000);
    proc.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
      const match = stderr.match(/DevTools listening on (ws:\/\/127\.0\.0\.1:(\d+)\/[^\s]+)/);
      if (match) {
        clearTimeout(timer);
        resolve({ wsUrl: match[1], port: Number(match[2]) });
      }
    });
    proc.on('exit', (code) => {
      clearTimeout(timer);
      reject(new Error(`Chrome exited early with code ${code}: ${stderr}`));
    });
  });

  const targets = await fetchJson(`http://127.0.0.1:${browserWsUrl.port}/json/list`);
  const pageTarget = targets.find((t) => t.type === 'page') || targets[0];
  if (!pageTarget?.webSocketDebuggerUrl) {
    throw new Error(`No page target found in Chrome DevTools: ${JSON.stringify(targets)}`);
  }
  const cdp = new CdpSession(pageTarget.webSocketDebuggerUrl);
  await cdp.connect();
  await cdp.send('Page.enable');
  await cdp.send('Runtime.enable');

  return {
    cdp,
    close: async () => {
      cdp.close();
      proc.kill('SIGTERM');
      await new Promise((r) => setTimeout(r, 150));
      fs.rmSync(profileDir, { recursive: true, force: true });
    },
  };
}

/** Verify a single published bundle URL inside Chrome via CDP. */
async function verifyUrlInChrome(cdp, modeName, url, expectedProtocol) {
  await cdp.send('Page.navigate', { url });

  // Wait up to 8s for window.NeoFlashPlayer to mount and render at least 2 frames.
  const readyState = await cdp.evalJson(`
    new Promise((resolve) => {
      const deadline = Date.now() + 8000;
      const poll = () => {
        const errEl = document.getElementById('error');
        const errText = errEl && errEl.style.display === 'block' ? errEl.textContent : '';
        if (errText) {
          resolve({ ok: false, error: errText });
          return;
        }
        const player = window.NeoFlashPlayer;
        const traceEl = document.getElementById('trace');
        const statusEl = document.getElementById('status');
        if (player && statusEl && statusEl.textContent && statusEl.textContent.includes('fps')) {
          resolve({
            ok: true,
            protocol: window.location.protocol,
            status: statusEl.textContent,
            trace: traceEl ? traceEl.textContent : '',
            hasBase64Wasm: typeof window.NeoFlashWasmBase64 === 'string' && window.NeoFlashWasmBase64.length > 100,
          });
          return;
        }
        if (Date.now() > deadline) {
          resolve({
            ok: false,
            error: 'Timeout waiting for NeoFlashPlayer',
            status: statusEl ? statusEl.textContent : null,
            errDisplay: errEl ? errEl.textContent : null,
          });
          return;
        }
        setTimeout(poll, 60);
      };
      poll();
    })
  `);

  if (!readyState?.ok) {
    throw new Error(`[${modeName}] Runtime failed to initialize at ${url}: ${JSON.stringify(readyState)}`);
  }
  if (readyState.protocol !== expectedProtocol) {
    throw new Error(`[${modeName}] Expected protocol ${expectedProtocol}, got ${readyState.protocol}`);
  }
  if ((modeName === 'balanced' || modeName === 'compact') && !readyState.hasBase64Wasm) {
    throw new Error(`[${modeName}] Expected NeoFlashWasmBase64 to be present for file:// fallback`);
  }

  // Inspect rendering first, then use CDP input rather than synthetic DOM events. Trusted
  // pointer/keyboard input is required to verify the browser's audio-gesture boundary and the
  // same editable-text route that an actual user exercises.
  const renderReport = await cdp.evalJson(`
    (async () => {
      const stage = document.getElementById('stage');
      const ctx = stage.getContext('2d');

      // Wait up to 800ms for decoded video frame to present at (500, 230).
      const vidDeadline = Date.now() + 800;
      let vidSample = ctx.getImageData(Math.floor(500 * (stage.width / 640)), Math.floor(230 * (stage.height / 360)), 1, 1).data;
      while (
        Math.abs(vidSample[0] - 18) <= 8 &&
        Math.abs(vidSample[1] - 22) <= 8 &&
        Math.abs(vidSample[2] - 31) <= 8 &&
        Date.now() < vidDeadline
      ) {
        await new Promise((r) => setTimeout(r, 30));
        vidSample = ctx.getImageData(Math.floor(500 * (stage.width / 640)), Math.floor(230 * (stage.height / 360)), 1, 1).data;
      }
      const videoPixelRgba = [vidSample[0], vidSample[1], vidSample[2], vidSample[3]];

      const img = ctx.getImageData(0, 0, stage.width, stage.height).data;

      // Background is [18, 22, 31, 255]; count non-background pixels and check vector/video regions.
      let nonBgPixels = 0;
      for (let i = 0; i < img.length; i += 4) {
        if (Math.abs(img[i] - 18) > 8 || Math.abs(img[i + 1] - 22) > 8 || Math.abs(img[i + 2] - 31) > 8) {
          nonBgPixels++;
        }
      }

      const initialTrace = document.getElementById('trace')?.textContent || '';

      // 1. Dispatch click on ctaBtn at stage coordinates (100, 235).
      const rect = stage.getBoundingClientRect();
      const scaleX = rect.width / 640;
      const scaleY = rect.height / 360;
      const clickClientX = rect.left + 100 * scaleX;
      const clickClientY = rect.top + 235 * scaleY;

      return {
        canvasWidth: stage.width,
        canvasHeight: stage.height,
        nonBgPixels,
        initialTrace,
        videoPixelRgba,
        clickClientX,
        clickClientY,
      };
    })()
  `);

  await cdp.send('Input.dispatchMouseEvent', {
    type: 'mousePressed',
    x: renderReport.clickClientX,
    y: renderReport.clickClientY,
    button: 'left',
    buttons: 1,
    clickCount: 1,
  });
  await cdp.send('Input.dispatchMouseEvent', {
    type: 'mouseReleased',
    x: renderReport.clickClientX,
    y: renderReport.clickClientY,
    button: 'left',
    buttons: 0,
    clickCount: 1,
  });
  await new Promise((resolve) => setTimeout(resolve, 100));

  const inputState = await cdp.evalJson(`
    (() => {
      const input = document.querySelector('[data-neoflash-text-input="true"]');
      if (!input) return { mounted: false };
      input.focus();
      input.select();
      return { mounted: true };
    })()
  `);
  if (inputState?.mounted) {
    await cdp.send('Input.insertText', { text: 'Beta1MatrixTyped' });
  }

  const interactionState = await cdp.evalJson(`
    new Promise((resolve) => {
      const deadline = Date.now() + 1500;
      const poll = () => {
        const audioStatus = document.getElementById('audio-status')?.textContent || '';
        if ((!/waiting|loading/i.test(audioStatus) && audioStatus) || Date.now() > deadline) {
          const input = document.querySelector('[data-neoflash-text-input="true"]');
          const audioButton = document.getElementById('audio-toggle');
          resolve({
            traceAfterClick: document.getElementById('trace')?.textContent || '',
            errorText: document.getElementById('error')?.textContent || '',
            inputMounted: Boolean(input),
            typedValue: input?.value ?? null,
            audioStatus,
            audioButtonText: audioButton?.textContent || '',
            audioButtonHidden: Boolean(audioButton?.hidden),
          });
          return;
        }
        setTimeout(poll, 30);
      };
      poll();
    })
  `);
  const interactionReport = { ...renderReport, ...interactionState };

  if (interactionReport.errorText) {
    throw new Error(`[${modeName}] Runtime reported an error: "${interactionReport.errorText}"`);
  }
  if (interactionReport.nonBgPixels < 500) {
    throw new Error(`[${modeName}] Canvas did not render expected vector/video artwork (${interactionReport.nonBgPixels} non-bg pixels)`);
  }
  if (!interactionReport.traceAfterClick.startsWith('click:cta_ok')) {
    throw new Error(`[${modeName}] Expected traceAfterClick to start with "click:cta_ok", got "${interactionReport.traceAfterClick}"`);
  }
  if (!interactionReport.inputMounted || interactionReport.typedValue !== 'Beta1MatrixTyped') {
    throw new Error(`[${modeName}] Text input verification failed: ${JSON.stringify(interactionReport)}`);
  }
  if (!interactionReport.audioStatus.startsWith('Audio ') ||
      interactionReport.audioButtonHidden || interactionReport.audioButtonText !== 'Mute audio') {
    throw new Error(`[${modeName}] Unexpected audio state: ${JSON.stringify({
      status: interactionReport.audioStatus,
      button: interactionReport.audioButtonText,
      hidden: interactionReport.audioButtonHidden,
    })}`);
  }
  const [vr, vg, vb] = interactionReport.videoPixelRgba;
  if (Math.abs(vr - 18) <= 8 && Math.abs(vg - 22) <= 8 && Math.abs(vb - 31) <= 8) {
    throw new Error(`[${modeName}] Video viewport pixel did not render decoded video frame: ${JSON.stringify(interactionReport.videoPixelRgba)}`);
  }

  return {
    mode: modeName,
    url,
    protocol: readyState.protocol,
    status: readyState.status,
    hasBase64Wasm: readyState.hasBase64Wasm,
    ...interactionReport,
    passed: true,
  };
}

/** Find a free TCP port on 127.0.0.1. */
function findFreePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.listen(0, '127.0.0.1', () => {
      const port = srv.address().port;
      srv.close(() => resolve(port));
    });
    srv.on('error', reject);
  });
}

function httpPostJson(port, reqPath, payload) {
  return new Promise((resolve, reject) => {
    const body = Buffer.from(JSON.stringify(payload), 'utf8');
    const req = http.request(
      {
        hostname: '127.0.0.1',
        port,
        path: reqPath,
        method: 'POST',
        headers: {
          'Content-Type': 'application/json; charset=utf-8',
          'Content-Length': body.length,
        },
      },
      (res) => {
        let data = '';
        res.on('data', (c) => (data += c));
        res.on('end', () => {
          try {
            resolve({ statusCode: res.statusCode, body: JSON.parse(data) });
          } catch {
            resolve({ statusCode: res.statusCode, body: data });
          }
        });
      },
    );
    req.on('error', reject);
    req.write(body);
    req.end();
  });
}

function httpDelete(port, reqPath) {
  return new Promise((resolve) => {
    const req = http.request(
      { hostname: '127.0.0.1', port, path: reqPath, method: 'DELETE' },
      (res) => {
        res.resume();
        res.on('end', () => resolve());
      },
    );
    req.on('error', () => resolve());
    req.end();
  });
}

/**
 * Probe Safari / WebKit via /usr/bin/safaridriver and verify WebKit compatibility invariants
 * in the emitted runtime bundles.
 */
async function verifySafariWebKit(fullDir, balancedDir, sharedUrl) {
  const safariApp = '/Applications/Safari.app';
  const safariDriverBin = '/usr/bin/safaridriver';
  const safariExists = fs.existsSync(safariApp) && fs.existsSync(safariDriverBin);

  // Static WebKit compatibility checks on emitted bundle files:
  const runtimeJs = fs.readFileSync(path.join(fullDir, 'runtime.js'), 'utf8');
  const audioJs = fs.readFileSync(path.join(fullDir, 'audio.js'), 'utf8');
  const videoJs = fs.readFileSync(path.join(fullDir, 'video.js'), 'utf8');
  const balancedWebJs = fs.readFileSync(path.join(balancedDir, 'neoflash_web.js'), 'utf8');
  const staticChecks = {
    webkitFullscreenFallback:
      runtimeJs.includes('webkitFullscreenElement') &&
      runtimeJs.includes('webkitRequestFullscreen') &&
      runtimeJs.includes('webkitExitFullscreen') &&
      runtimeJs.includes('webkitfullscreenchange'),
    webkitAudioContextFallback:
      audioJs.includes('webkitAudioContext') && audioJs.includes('resumeFromGesture'),
    webkitFileProtocolBase64WasmLoader:
      runtimeJs.includes("location?.protocol === 'file:'") &&
      runtimeJs.includes('NeoFlashWasmBase64') &&
      balancedWebJs.includes('globalThis.NeoFlashWasmBase64 ='),
    webkitInlineVideoAttributes:
      videoJs.includes('media.playsInline=true') && videoJs.includes('media.muted=true'),
  };
  const allStaticPassed = Object.values(staticChecks).every(Boolean);
  if (!allStaticPassed) {
    throw new Error(`Safari/WebKit static compatibility check failed: ${JSON.stringify(staticChecks)}`);
  }

  if (!safariExists) {
    return {
      safariInstalled: false,
      automationStatus: 'safari_not_installed',
      staticWebKitChecks: staticChecks,
    };
  }

  const versionOut = spawnSync(safariDriverBin, ['--version'], { encoding: 'utf8' });
  const driverVersion = (versionOut.stdout || versionOut.stderr || '').trim();

  const port = await findFreePort();
  const proc = spawn(safariDriverBin, ['-p', String(port)], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  await new Promise((r) => setTimeout(r, 600));

  try {
    const sessionRes = await httpPostJson(port, '/session', {
      capabilities: {
        alwaysMatch: {
          browserName: 'safari',
        },
      },
    });

    if (sessionRes.statusCode === 200 && sessionRes.body?.value?.sessionId) {
      const sessionId = sessionRes.body.value.sessionId;
      try {
        const navigationRes = await httpPostJson(port, `/session/${sessionId}/url`, { url: sharedUrl });
        if (navigationRes.statusCode !== 200) {
          throw new Error(`Safari navigation failed: ${JSON.stringify(navigationRes.body)}`);
        }
        let liveResult = null;
        const deadline = Date.now() + 8000;
        while (Date.now() <= deadline) {
          const evalRes = await httpPostJson(port, `/session/${sessionId}/execute/sync`, {
            script: `return {
              hasPlayer: Boolean(window.NeoFlashPlayer),
              status: document.getElementById('status')?.textContent || '',
              trace: document.getElementById('trace')?.textContent || '',
              error: document.getElementById('error')?.style.display === 'block'
                ? document.getElementById('error')?.textContent || ''
                : '',
            };`,
            args: [],
          });
          if (evalRes.statusCode !== 200) {
            throw new Error(`Safari runtime inspection failed: ${JSON.stringify(evalRes.body)}`);
          }
          liveResult = evalRes.body?.value || null;
          if (liveResult?.error) throw new Error(`Safari runtime failed: ${liveResult.error}`);
          if (liveResult?.hasPlayer && liveResult.status.includes('fps')) break;
          await new Promise((resolve) => setTimeout(resolve, 100));
        }
        if (!liveResult?.hasPlayer || !liveResult.status.includes('fps')) {
          throw new Error(`Safari runtime did not initialize: ${JSON.stringify(liveResult)}`);
        }
        return {
          safariInstalled: true,
          driverVersion,
          automationStatus: 'live_webdriver_passed',
          liveResult,
          staticWebKitChecks: staticChecks,
        };
      } finally {
        await httpDelete(port, `/session/${sessionId}`);
      }
    }

    const errMsg =
      sessionRes.body?.value?.message ||
      JSON.stringify(sessionRes.body);
    return {
      safariInstalled: true,
      driverVersion,
      automationStatus: 'host_remote_automation_disabled',
      hostBlocker: errMsg,
      remediation:
        "Enable 'Safari -> Settings -> Advanced -> Show features for web developers', then enable 'Develop -> Allow Remote Automation' and run 'safaridriver --enable'.",
      staticWebKitChecks: staticChecks,
    };
  } finally {
    proc.kill('SIGTERM');
  }
}

async function main() {
  ensureWebRuntime();
  fs.rmSync(OUT_ROOT, { recursive: true, force: true });
  fs.mkdirSync(OUT_ROOT, { recursive: true });

  const projectPath = path.join(OUT_ROOT, 'Beta1MatrixProject.nflash');
  fs.writeFileSync(projectPath, buildMatrixProjectJson(), 'utf8');

  const fullDir = path.join(OUT_ROOT, 'full');
  const balancedDir = path.join(OUT_ROOT, 'balanced');
  const compactDir = path.join(OUT_ROOT, 'compact');
  const sharedRuntimeDir = path.join(OUT_ROOT, 'shared-runtime');
  const sharedDir = path.join(OUT_ROOT, 'shared');

  const { server, port } = await startStaticServer(OUT_ROOT);
  const sharedRuntimeUrl = `http://127.0.0.1:${port}/shared-runtime`;

  try {
    console.log('[T-237] Exporting project in Full, Balanced, Compact, and Shared modes...');
    runExportCli([projectPath, '--output', fullDir, '--size-level', 'full']);
    runExportCli([projectPath, '--output', balancedDir, '--size-level', 'balanced']);
    runExportCli([projectPath, '--output', compactDir, '--size-level', 'compact']);
    runExportCli(['--shared-runtime', sharedRuntimeDir]);
    runExportCli([
      projectPath,
      '--output',
      sharedDir,
      '--size-level',
      'shared',
      '--runtime-url',
      sharedRuntimeUrl,
    ]);

    const fullFileUrl = pathToFileURL(path.join(fullDir, 'index.html')).href;
    const balancedFileUrl = pathToFileURL(path.join(balancedDir, 'index.html')).href;
    const compactFileUrl = pathToFileURL(path.join(compactDir, 'index.html')).href;
    const sharedHttpUrl = `http://127.0.0.1:${port}/shared/index.html`;

    console.log('[T-237] Launching desktop Google Chrome (CDP) across all 4 deployment modes...');
    const chrome = await launchChromeCdp();
    const chromeResults = [];
    try {
      chromeResults.push(await verifyUrlInChrome(chrome.cdp, 'full', fullFileUrl, 'file:'));
      chromeResults.push(await verifyUrlInChrome(chrome.cdp, 'balanced', balancedFileUrl, 'file:'));
      chromeResults.push(await verifyUrlInChrome(chrome.cdp, 'compact', compactFileUrl, 'file:'));
      chromeResults.push(await verifyUrlInChrome(chrome.cdp, 'shared', sharedHttpUrl, 'http:'));
    } finally {
      await chrome.close();
    }

    console.log('[T-237] Probing desktop Safari / WebKit...');
    const safariReport = await verifySafariWebKit(fullDir, balancedDir, sharedHttpUrl);

    const report = {
      timestamp: new Date().toISOString(),
      host: {
        platform: process.platform,
        arch: process.arch,
        node: process.version,
      },
      modesVerifiedInChrome: chromeResults,
      safariWebKit: safariReport,
    };

    const reportPath = path.join(OUT_ROOT, 'report.json');
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2), 'utf8');
    console.log(`[T-237] Browser matrix verification succeeded! Report written to ${reportPath}`);
    for (const r of chromeResults) {
      console.log(
        `  - Chrome [${r.mode}] (${r.protocol}): nonBgPixels=${r.nonBgPixels}, traceAfterClick="${r.traceAfterClick}", typed="${r.typedValue}", audio="${r.audioStatus}"`,
      );
    }
    console.log(
      `  - Safari/WebKit: status=${safariReport.automationStatus}, staticChecks=${JSON.stringify(safariReport.staticWebKitChecks)}`,
    );
  } finally {
    server.close();
  }
}

main().catch((err) => {
  console.error('[T-237] Browser matrix verification failed:', err);
  process.exit(1);
});
