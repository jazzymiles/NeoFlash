const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { execFileSync, spawn } = require('node:child_process');

const repoRoot = path.resolve(__dirname, '../..');
const runtimeDir = path.join(repoRoot, 'target/web-runtime');
const exportBin = path.join(repoRoot, 'target/debug/neoflash-export');

function findChromeBinary() {
  const candidates = [
    process.env.CHROME_BIN,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    'google-chrome',
    'chromium',
  ].filter(Boolean);
  for (const bin of candidates) {
    if (bin.startsWith('/') && fs.existsSync(bin)) return bin;
  }
  return null;
}

function ensureExportBinary() {
  if (fs.existsSync(exportBin)) return exportBin;
  execFileSync('cargo', ['build', '--locked', '-p', 'neoflash-export', '--bin', 'neoflash-export'], {
    cwd: repoRoot,
    stdio: 'inherit',
  });
  return exportBin;
}

function makeTinyWavBase64() {
  const sampleRate = 8000;
  const numSamples = 400;
  const dataBytes = numSamples * 2;
  const buf = Buffer.alloc(44 + dataBytes);
  buf.write('RIFF', 0);
  buf.writeUInt32LE(36 + dataBytes, 4);
  buf.write('WAVEfmt ', 8);
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(1, 22);
  buf.writeUInt32LE(sampleRate, 24);
  buf.writeUInt32LE(sampleRate * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write('data', 36);
  buf.writeUInt32LE(dataBytes, 40);
  for (let i = 0; i < numSamples; i++) {
    const sample = Math.round(Math.sin(i * 0.25) * 6000);
    buf.writeInt16LE(sample, 44 + i * 2);
  }
  return buf.toString('base64');
}

function makeTransform(overrides = {}) {
  return {
    x: 0,
    y: 0,
    scale_x: 1,
    scale_y: 1,
    rotation: 0,
    alpha: 1,
    ...overrides,
  };
}

function makeTextFormat(overrides = {}, fieldOptions = null) {
  const fmt = {
    font: 'Arial',
    size: 14,
    color: 0xf8fafc,
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
    kerning: null,
    bullet: false,
    tabStops: [],
    url: '',
    target: '',
    ...overrides,
  };
  if (fieldOptions) fmt.fieldOptions = fieldOptions;
  return fmt;
}

function startNoCacheServer(rootDir) {
  const mimeByExt = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.mjs': 'text/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.wav': 'audio/wav',
    '.svg': 'image/svg+xml',
    '.png': 'image/png',
  };
  const server = http.createServer((req, res) => {
    const cleanUrl = decodeURIComponent((req.url || '/').split('?')[0]);
    const relPath = cleanUrl === '/' ? 'index.html' : cleanUrl.replace(/^\/+/, '');
    const candidate = path.resolve(rootDir, relPath);
    if (!candidate.startsWith(path.resolve(rootDir) + path.sep) || !fs.existsSync(candidate) || fs.statSync(candidate).isDirectory()) {
      res.writeHead(404, { 'Cache-Control': 'no-store, no-cache, must-revalidate' });
      res.end('Not found');
      return;
    }
    const ext = path.extname(candidate).toLowerCase();
    res.writeHead(200, {
      'Content-Type': mimeByExt[ext] || 'application/octet-stream',
      'Cache-Control': 'no-store, no-cache, must-revalidate',
    });
    fs.createReadStream(candidate).pipe(res);
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port }));
  });
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

  const navigateAndWait = async (url) => {
    const loaded = new Promise((resolve) => {
      loadResolve = resolve;
    });
    await send('Page.navigate', { url });
    await loaded;
  };

  const evalJson = async (expression) => {
    const res = await send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (res.exceptionDetails) {
      throw new Error(`Eval failed: ${JSON.stringify(res.exceptionDetails)}`);
    }
    return res.result?.value;
  };

  const waitTrace = async (marker, timeoutMs = 15000) => {
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
          const traceText = document.getElementById('trace')?.textContent || '';
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

  const dispatchClickAt = async (x, y) => {
    await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'none' });
    await send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
    await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
  };

  return { chromeProc, ws, send, navigateAndWait, evalJson, waitTrace, dispatchClickAt, errors };
}

function buildMultiSceneAndSymbolsProject(renderMode) {
  const graphicSym = {
    id: 'sym-graphic',
    name: 'FourStepGraphic',
    kind: 'graphic',
    timeline: {
      frame_count: 4,
      layers: [
        {
          id: 'g-layer',
          name: 'Boxes',
          visible: true,
          locked: false,
          keyframes: [0, 1, 2, 3].map((f) => ({
            frame: f,
            label: '',
            tween: null,
            elements: [
              {
                id: `g-box-${f}`,
                name: '',
                transform: makeTransform({ x: f * 20 }),
                kind: {
                  type: 'rectangle',
                  width: 14,
                  height: 14,
                  fill: [40 + f * 50, 180, 220 - f * 40, 255],
                  stroke: [0, 0, 0, 0],
                  stroke_width: 0,
                },
                scripts: [],
              },
            ],
          })),
        },
      ],
      scripts: [],
    },
  };

  const indepMcSym = {
    id: 'sym-indep-mc',
    name: 'IndepTickerClip',
    kind: 'movie_clip',
    timeline: {
      frame_count: 3,
      layers: [
        {
          id: 'mc-layer',
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
                  id: 'mc-dot',
                  name: '',
                  transform: makeTransform(),
                  kind: {
                    type: 'rectangle',
                    width: 12,
                    height: 12,
                    fill: [250, 204, 21, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
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
  };

  // Button with geometry ONLY in Hit frame 3 (non-rectangular right triangle: (0,0)->(80,0)->(0,80))!
  const hitOnlyBtnSym = {
    id: 'sym-hit-only-btn',
    name: 'InvisibleHotspotBtn',
    kind: 'button',
    timeline: {
      frame_count: 4,
      layers: [
        {
          id: 'hob-layer',
          name: 'Hotspot',
          visible: true,
          locked: false,
          keyframes: [
            { frame: 0, label: 'Up', tween: null, elements: [] },
            { frame: 1, label: 'Over', tween: null, elements: [] },
            { frame: 2, label: 'Down', tween: null, elements: [] },
            {
              frame: 3,
              label: 'Hit',
              tween: null,
              elements: [
                {
                  id: 'hob-tri',
                  name: '',
                  transform: makeTransform(),
                  kind: {
                    type: 'path',
                    commands: [
                      { op: 'move_to', x: 0, y: 0 },
                      { op: 'line_to', x: 80, y: 0 },
                      { op: 'line_to', x: 0, y: 80 },
                      { op: 'close' },
                    ],
                    fill_rule: 'nonzero',
                    fill: [255, 0, 255, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
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
  };

  // Button with visible Up state, but an authoritative EMPTY Hit frame 3 (must NOT be clickable!).
  const emptyHitBtnSym = {
    id: 'sym-empty-hit-btn',
    name: 'EmptyHitBtn',
    kind: 'button',
    timeline: {
      frame_count: 4,
      layers: [
        {
          id: 'ehb-layer',
          name: 'States',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: 'Up',
              tween: null,
              elements: [
                {
                  id: 'ehb-up',
                  name: '',
                  transform: makeTransform(),
                  kind: {
                    type: 'rectangle',
                    width: 60,
                    height: 30,
                    fill: [100, 116, 139, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
                  },
                  scripts: [],
                },
              ],
            },
            { frame: 3, label: 'Hit', tween: null, elements: [] },
          ],
        },
      ],
      scripts: [],
    },
  };

  const scene1Elements = (frameIdx) => [
    {
      id: 'g-single',
      name: '',
      transform: makeTransform({ x: 20 + frameIdx * 10, y: 20 }),
      kind: { type: 'instance', symbol_id: 'sym-graphic', playback: { single_frame: 2 } },
      scripts: [],
    },
    {
      id: 'g-once',
      name: '',
      transform: makeTransform({ x: 20, y: 45 }),
      kind: { type: 'instance', symbol_id: 'sym-graphic', playback: { play_once_from: 1 } },
      scripts: [],
    },
    {
      id: 'g-loop',
      name: '',
      transform: makeTransform({ x: 20, y: 70 }),
      kind: { type: 'instance', symbol_id: 'sym-graphic', playback: { loop_from: 2 } },
      scripts: [],
    },
    {
      id: 'mc-persist',
      name: 'persistMc',
      transform: makeTransform({ x: 20, y: 100 }),
      kind: { type: 'instance', symbol_id: 'sym-indep-mc', playback: 'independent' },
      scripts: [],
    },
  ];

  const scene1Sym = {
    id: 'scene-1',
    name: 'Scene 1',
    kind: 'movie_clip',
    timeline: {
      frame_count: 2,
      layers: [
        {
          id: 's1-layer',
          name: 'Main',
          visible: true,
          locked: false,
          keyframes: [
            { frame: 0, label: 's1_f1', tween: 'linear', elements: scene1Elements(0) },
            { frame: 1, label: 's1_f2', tween: null, elements: scene1Elements(1) },
          ],
        },
      ],
      scripts: [
        {
          frame: 0,
          script: {
            language: 'action_script3',
            source: [
              'var self:* = this;',
              'var mc:* = self.getChildByName("persistMc");',
              'self.s1Visits = (self.s1Visits || 0) + 1;',
              'self.f1McFrame = mc ? mc.currentFrame : -1;',
              'trace("S1_F1_VISITS=" + self.s1Visits + ",MC=" + self.f1McFrame);',
            ].join('\n'),
          },
        },
        {
          frame: 1,
          script: {
            language: 'action_script3',
            source: [
              'var self:* = this;',
              'var mc:* = self.getChildByName("persistMc");',
              'self.f2McFrame = mc ? mc.currentFrame : -1;',
              'trace("S1_F2_MC=" + self.f2McFrame);',
            ].join('\n'),
          },
        },
      ],
    },
  };

  const scene2Sym = {
    id: 'scene-2',
    name: 'Scene 2',
    kind: 'movie_clip',
    timeline: {
      frame_count: 2,
      layers: [
        {
          id: 's2-layer',
          name: 'Controls',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: 's2_start',
              tween: null,
              sound: {
                resource: 'res-beep',
                sync: 'event',
                loops: 1,
                loop_forever: false,
                effect: 'none',
              },
              elements: [
                {
                  id: 's2-input',
                  name: 'userField',
                  transform: makeTransform({ x: 30, y: 150 }),
                  kind: {
                    type: 'text_box',
                    text: 'DefaultInput',
                    width: 180,
                    height: 28,
                    format: makeTextFormat(
                      {},
                      {
                        field_type: 'input',
                        selectable: true,
                        border: true,
                        background: true,
                        border_color: 0x64748b,
                        background_color: 0x1e293b,
                      }
                    ),
                    word_wrap: false,
                    multiline: false,
                  },
                  scripts: [],
                },
                {
                  id: 's2-hit-only',
                  name: 'hitOnlyBtn',
                  transform: makeTransform({ x: 240, y: 30 }),
                  kind: { type: 'instance', symbol_id: 'sym-hit-only-btn', playback: 'independent' },
                  scripts: [],
                },
                {
                  id: 's2-empty-hit',
                  name: 'emptyHitBtn',
                  transform: makeTransform({ x: 350, y: 30 }),
                  kind: { type: 'instance', symbol_id: 'sym-empty-hit-btn', playback: 'independent' },
                  scripts: [],
                },
              ],
            },
            {
              frame: 1,
              label: 's2_end',
              tween: null,
              elements: [
                {
                  id: 's2-end-banner',
                  name: '',
                  transform: makeTransform({ x: 30, y: 30 }),
                  kind: {
                    type: 'text',
                    text: 'Reached Scene 2 End',
                    font_family: 'Arial',
                    font_size: 16,
                    color: [248, 250, 252, 255],
                  },
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
            language: 'action_script3',
            source: [
              'import flash.display.Sprite;',
              'import flash.events.MouseEvent;',
              'import flash.utils.setInterval;',
              'stop();',
              'var self:* = this;',
              'self.hitOnlyClicks = self.hitOnlyClicks || 0;',
              'self.emptyHitClicks = self.emptyHitClicks || 0;',
              'self.timerCount = self.timerCount || 0;',
              'if (!self.getChildByName("dynChild")) {',
              '  var sp:Sprite = new Sprite();',
              '  sp.name = "dynChild";',
              '  sp.graphics.beginFill(0x22c55e, 1);',
              '  sp.graphics.drawRect(30, 200, 32, 20);',
              '  sp.graphics.endFill();',
              '  self.addChild(sp);',
              '}',
              'if (!self.intervalInstalled) {',
              '  self.intervalInstalled = true;',
              '  setInterval(function():void { self.timerCount = (self.timerCount || 0) + 1; }, 25);',
              '}',
              'var hob:* = self.getChildByName("hitOnlyBtn");',
              'if (hob && !self.hobWired) {',
              '  self.hobWired = true;',
              '  hob.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {',
              '    self.hitOnlyClicks++;',
              '    trace("HIT_ONLY_CLICKED=" + self.hitOnlyClicks);',
              '    if (self.hitOnlyClicks === 2) {',
              '      self.gotoAndStop("s2_end");',
              '    }',
              '  });',
              '}',
              'var ehb:* = self.getChildByName("emptyHitBtn");',
              'if (ehb && !self.ehbWired) {',
              '  self.ehbWired = true;',
              '  ehb.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {',
              '    self.emptyHitClicks++;',
              '    trace("EMPTY_HIT_CLICKED=" + self.emptyHitClicks);',
              '  });',
              '}',
              'self.stage.scaleMode = "exactFit";',
              'trace("S2_READY_VISITS=" + self.s1Visits + "_MC_F1=" + self.f1McFrame + "_F2=" + self.f2McFrame + "_DYN=" + Boolean(self.getChildByName("dynChild")));',
            ].join('\n'),
          },
        },
        {
          frame: 1,
          script: {
            language: 'action_script3',
            source: 'stop(); trace("AT_S2_END_LABEL=1");',
          },
        },
      ],
    },
  };

  return {
    format_version: 10,
    name: `ParityMultiScene_${renderMode}`,
    width: 480,
    height: 300,
    fps: 24,
    background: [15, 23, 42, 255],
    root: 'scene-1',
    scenes: ['scene-1', 'scene-2'],
    script_language: 'action_script3',
    symbols: [scene1Sym, scene2Sym, graphicSym, indepMcSym, hitOnlyBtnSym, emptyHitBtnSym],
    resources: [
      {
        id: 'res-beep',
        path: 'beep.wav',
        media_type: 'audio/wav',
        data_base64: makeTinyWavBase64(),
      },
      {
        id: 'publish-settings',
        path: 'publish/settings.json',
        media_type: 'application/json',
        data_base64: Buffer.from(JSON.stringify({ render_mode: renderMode }), 'utf8').toString('base64'),
      },
    ],
    sources: [],
  };
}

function buildVisualCameraMasksProject(renderMode) {
  const boxClip = (id, name, fill) => ({
    id,
    name,
    kind: 'movie_clip',
    timeline: {
      frame_count: 1,
      layers: [
        {
          id: `${id}-l`,
          name: 'Shape',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: `${id}-r`,
                  name: '',
                  transform: makeTransform(),
                  kind: {
                    type: 'rectangle',
                    width: 80,
                    height: 50,
                    fill,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
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
  });

  const layerBlendSym = {
    id: 'sym-layer-erase',
    name: 'LayerEraseClip',
    kind: 'movie_clip',
    timeline: {
      frame_count: 1,
      layers: [
        {
          id: 'le-punch',
          name: 'ErasePunch',
          visible: true,
          locked: false,
          properties: { role: 'normal', blend_mode: 'erase' },
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'le-hole',
                  name: '',
                  transform: makeTransform({ x: 15, y: 10 }),
                  kind: {
                    type: 'rectangle',
                    width: 30,
                    height: 30,
                    fill: [0, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
                  },
                  scripts: [],
                },
              ],
            },
          ],
        },
        {
          id: 'le-base',
          name: 'Base',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'le-bg',
                  name: '',
                  transform: makeTransform(),
                  kind: {
                    type: 'rectangle',
                    width: 60,
                    height: 50,
                    fill: [59, 130, 246, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
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
  };

  const mainSym = {
    id: 'visual-root',
    name: 'VisualScene',
    kind: 'movie_clip',
    timeline: {
      frame_count: 1,
      layers: [
        // Camera layer: pan (+40, 0), zoom 2x around stage center (200, 150)
        {
          id: 'cam-layer',
          name: 'Camera',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              camera: {
                x: 40,
                y: 0,
                zoom: 2,
                rotation: 0,
              },
              elements: [],
            },
          ],
        },
        // Pinned HUD layer (`attach_to_camera: true`)
        {
          id: 'hud-layer',
          name: 'HUD',
          visible: true,
          locked: false,
          properties: { role: 'normal', attach_to_camera: true },
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'hud-inst',
                  name: 'hudClip',
                  transform: makeTransform({ x: 12, y: 14 }),
                  kind: { type: 'instance', symbol_id: 'sym-hud-box', playback: 'independent' },
                  scripts: [],
                },
              ],
            },
          ],
        },
        // Alpha mask layer + masked content layer
        {
          id: 'alpha-mask-layer',
          name: 'AlphaMask',
          visible: true,
          locked: false,
          properties: { role: 'mask', mask_mode: 'alpha' },
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'alpha-mask-shape',
                  name: '',
                  transform: makeTransform({ x: 180, y: 120 }),
                  kind: {
                    type: 'rectangle',
                    width: 50,
                    height: 40,
                    fill: [255, 255, 255, 160],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
                  },
                  scripts: [],
                },
              ],
            },
          ],
        },
        {
          id: 'alpha-masked-content',
          name: 'AlphaTarget',
          visible: true,
          locked: false,
          properties: { role: 'normal', mask: 'alpha-mask-layer' },
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'alpha-target-rect',
                  name: '',
                  transform: makeTransform({ x: 170, y: 110 }),
                  kind: {
                    type: 'rectangle',
                    width: 70,
                    height: 60,
                    fill: [236, 72, 153, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
                  },
                  scripts: [],
                },
              ],
            },
          ],
        },
        // Layer blend + erase punch-out + world + scrollRect container
        {
          id: 'world-layer',
          name: 'World',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'world-inst',
                  name: 'worldClip',
                  transform: makeTransform({ x: 200, y: 150 }),
                  kind: { type: 'instance', symbol_id: 'sym-world-box', playback: 'independent' },
                  scripts: [],
                },
                {
                  id: 'scroll-inst',
                  name: 'scrollClip',
                  transform: makeTransform({ x: 160, y: 60 }),
                  kind: { type: 'instance', symbol_id: 'sym-scroll-box', playback: 'independent' },
                  scripts: [],
                },
                {
                  id: 'layer-erase-inst',
                  name: 'layerEraseClip',
                  transform: makeTransform({ x: 260, y: 150 }),
                  blend_mode: 'layer',
                  kind: { type: 'instance', symbol_id: 'sym-layer-erase', playback: 'independent' },
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
            language: 'action_script3',
            source: [
              'import flash.geom.Rectangle;',
              'stop();',
              'var self:* = this;',
              'var sc:* = self.getChildByName("scrollClip");',
              'if (sc) sc.scrollRect = new Rectangle(10, 5, 40, 25);',
              'trace("VISUAL_READY=1");',
            ].join('\n'),
          },
        },
      ],
    },
  };

  return {
    format_version: 10,
    name: `ParityVisual_${renderMode}`,
    width: 400,
    height: 300,
    fps: 24,
    background: [15, 23, 42, 255],
    root: 'visual-root',
    scenes: [],
    script_language: 'action_script3',
    symbols: [
      mainSym,
      boxClip('sym-hud-box', 'HudBox', [16, 185, 129, 255]),
      boxClip('sym-world-box', 'WorldBox', [249, 115, 22, 255]),
      boxClip('sym-scroll-box', 'ScrollBox', [168, 85, 247, 255]),
      layerBlendSym,
    ],
    resources: [
      {
        id: 'publish-settings',
        path: 'publish/settings.json',
        media_type: 'application/json',
        data_base64: Buffer.from(JSON.stringify({ render_mode: renderMode }), 'utf8').toString('base64'),
      },
    ],
    sources: [],
  };
}

function buildFailureRecoveryProject(renderMode) {
  return {
    format_version: 10,
    name: `FailureRecovery_${renderMode}`,
    width: 320,
    height: 240,
    fps: 24,
    background: [15, 23, 42, 255],
    root: 'failure-root',
    scenes: [],
    script_language: 'java_script',
    symbols: [{
      id: 'failure-root',
      name: 'Failure Recovery',
      kind: 'movie_clip',
      timeline: {
        frame_count: 2,
        layers: [{
          id: 'failure-layer', name: 'Main', visible: true, locked: false,
          keyframes: [{ frame: 0, label: '', tween: null, elements: [] }],
        }],
        scripts: [{
          frame: 0,
          script: {
            language: 'java_script',
            source: [
              'const probe = globalThis.__failureProbe;',
              'probe.starts++;',
              'if (!probe.recover) {',
              '  probe.original = new Error("T340 authored startup failure");',
              '  probe.cleanupPending = true;',
              '  throw probe.original;',
              '}',
              'probe.external.marshallExceptions = true;',
              'probe.external.addCallback("t340Throw", function () {',
              '  probe.callbackOriginal = new Error("T340 authored external failure");',
              '  probe.renderPending = true;',
              '  throw probe.callbackOriginal;',
              '});',
              'trace("T340_RECOVERED");',
            ].join('\n'),
          },
        }, {
          frame: 1,
          script: {
            language: 'java_script',
            source: 'globalThis.__failureProbe.ticks++; trace("T340_TICK=" + globalThis.__failureProbe.ticks);',
          },
        }],
      },
    }],
    resources: [],
    sources: [],
  };
}

test('Canvas + DOM publish parity: scenes, symbols, Restart recovery with authored errors, masks, camera and DPR resize', async (t) => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Google Chrome / Chromium binary not available');
    return;
  }
  assert.ok(
    fs.existsSync(path.join(runtimeDir, 'neoflash_web_bg.wasm')),
    'target/web-runtime must be built before running publish-parity-polish.test.cjs'
  );
  const bin = ensureExportBinary();
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-parity-320-'));
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-chrome-320-'));
  let serverInfo = null;
  let cdp = null;

  try {
    for (const mode of ['dom', 'canvas']) {
      const multiPath = path.join(tempRoot, `multi-${mode}.nflash`);
      const visPath = path.join(tempRoot, `vis-${mode}.nflash`);
      const failurePath = path.join(tempRoot, `failure-${mode}.nflash`);
      fs.writeFileSync(multiPath, JSON.stringify(buildMultiSceneAndSymbolsProject(mode), null, 2));
      fs.writeFileSync(visPath, JSON.stringify(buildVisualCameraMasksProject(mode), null, 2));
      fs.writeFileSync(failurePath, JSON.stringify(buildFailureRecoveryProject(mode), null, 2));

      execFileSync(
        bin,
        ['--runtime', runtimeDir, multiPath, '--output', path.join(tempRoot, `multi-${mode}`), '--render-mode', mode],
        { cwd: repoRoot, encoding: 'utf8' }
      );
      execFileSync(
        bin,
        ['--runtime', runtimeDir, visPath, '--output', path.join(tempRoot, `vis-${mode}`), '--render-mode', mode],
        { cwd: repoRoot, encoding: 'utf8' }
      );
      execFileSync(
        bin,
        ['--runtime', runtimeDir, failurePath, '--output', path.join(tempRoot, `failure-${mode}`), '--render-mode', mode],
        { cwd: repoRoot, encoding: 'utf8' }
      );
    }

    serverInfo = await startNoCacheServer(tempRoot);
    cdp = await launchCdpSession(chromeBin, userDataDir);

    // =========================================================================
    // 1. Multi-Scene Auto-Advance, Hit-Only Button, Graphic/MC Modes & Restart
    // =========================================================================
    for (const mode of ['dom', 'canvas']) {
      await cdp.send('Emulation.setDeviceMetricsOverride', {
        width: 800,
        height: 600,
        deviceScaleFactor: 1,
        mobile: false,
      });
      await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/multi-${mode}/index.html`);

      // Scene 1 (2 frames, unstopped) must auto-advance into Scene 2 frame 1 (where stop() halts)!
      // Also proves `persistMc` advanced from frame 1 on S1:F1 to frame 2 on S1:F2 across parent keyframes!
      await cdp.waitTrace('S2_READY_VISITS=1_MC_F1=1_F2=2_DYN=true');

      let hitInsidePt = null;
      let hitOutsidePt = null;
      let emptyCenterPt = null;

      if (mode === 'dom') {
        const domScene2Check = await cdp.evalJson(`(() => {
          const stageEl = document.getElementById('stage-dom-root');
          const hitContainer = stageEl.querySelector('.nf-dom-container[data-nf-name="hitOnlyBtn"]');
          const hitBtn = hitContainer?.querySelector('button.nf-dom-button') || null;
          const emptyContainer = stageEl.querySelector('.nf-dom-container[data-nf-name="emptyHitBtn"]');
          const emptyBtn = emptyContainer?.querySelector('button.nf-dom-button') || null;
          const inputEl = stageEl.querySelector('input.nf-dom-input');
          // Verify hitOnlyBtn has 80x80 Hit bounds yet renders ZERO visible SVG/Canvas/img/text elements inside its container!
          const hitRect = hitBtn ? hitBtn.getBoundingClientRect() : null;
          const emptyRect = emptyContainer ? emptyContainer.getBoundingClientRect() : null;
          return {
            hasHitBtn: Boolean(hitBtn),
            hitBtnWidth: parseFloat(hitBtn?.style?.width || '0'),
            hitBtnHeight: parseFloat(hitBtn?.style?.height || '0'),
            hitBtnVisualChildren: hitContainer ? hitContainer.querySelectorAll('svg, canvas, img, .nf-dom-text').length : -1,
            hasEmptyHitBtn: Boolean(emptyBtn),
            inputValue: inputEl?.value || '',
            hitScreenInside: hitRect ? { x: hitRect.left + hitRect.width * 0.2, y: hitRect.top + hitRect.height * 0.2 } : null,
            hitScreenOutsideCorner: hitRect ? { x: hitRect.left + hitRect.width * 0.85, y: hitRect.top + hitRect.height * 0.85 } : null,
            emptyCenter: emptyRect ? { x: emptyRect.left + emptyRect.width * 0.5, y: emptyRect.top + emptyRect.height * 0.5 } : null,
          };
        })()`);

        assert.equal(domScene2Check.hasHitBtn, true, 'DOM mode must mount <button> for Hit-only Button');
        assert.equal(domScene2Check.hitBtnWidth, 80, 'Hit-only Button width must match Hit frame geometry');
        assert.equal(domScene2Check.hitBtnHeight, 80, 'Hit-only Button height must match Hit frame geometry');
        assert.equal(domScene2Check.hitBtnVisualChildren, 0, 'Hit-only Button must NEVER render visible Hit artwork');
        assert.equal(domScene2Check.hasEmptyHitBtn, false, 'Authoritative empty Hit frame must NOT mount clickable <button>');
        assert.equal(domScene2Check.inputValue, 'DefaultInput');
        hitInsidePt = domScene2Check.hitScreenInside;
        hitOutsidePt = domScene2Check.hitScreenOutsideCorner;
        emptyCenterPt = domScene2Check.emptyCenter;

        // Mutate DOM <input> before Restart to prove Restart resets it cleanly!
        await cdp.evalJson(`(() => {
          const inp = document.querySelector('#stage-dom-root input.nf-dom-input');
          inp.focus();
          inp.select();
          return true;
        })()`);
        await cdp.send('Input.insertText', { text: 'DirtyBeforeRestart' });
      } else {
        const canvasPts = await cdp.evalJson(`(() => {
          const r = document.getElementById('stage').getBoundingClientRect();
          const mapPt = (sx, sy) => ({ x: r.left + (sx / 480) * r.width, y: r.top + (sy / 300) * r.height });
          return {
            hitScreenInside: mapPt(240 + 16, 30 + 16),
            hitScreenOutsideCorner: mapPt(240 + 68, 30 + 68),
            emptyCenter: mapPt(350 + 40, 30 + 20),
          };
        })()`);
        hitInsidePt = canvasPts.hitScreenInside;
        hitOutsidePt = canvasPts.hitScreenOutsideCorner;
        emptyCenterPt = canvasPts.emptyCenter;
      }

      // Click 1 INSIDE the (0,0)-(80,0)-(0,80) Hit triangle -> triggers HIT_ONLY_CLICKED=1!
      await cdp.dispatchClickAt(hitInsidePt.x, hitInsidePt.y);
      await cdp.waitTrace('HIT_ONLY_CLICKED=1');

      // Click inside the 80x80 bounding box but OUTSIDE the Hit triangle -> must NOT trigger CLICK!
      await cdp.dispatchClickAt(hitOutsidePt.x, hitOutsidePt.y);
      // Click on emptyHitBtn's visible Up artwork -> must NOT trigger CLICK!
      if (emptyCenterPt) {
        await cdp.dispatchClickAt(emptyCenterPt.x, emptyCenterPt.y);
      }
      const clickCounts = await cdp.evalJson(`document.getElementById('trace')?.textContent || ''`);
      assert.ok(
        clickCounts.includes('HIT_ONLY_CLICKED=1')
          && !clickCounts.includes('HIT_ONLY_CLICKED=2')
          && !clickCounts.includes('EMPTY_HIT_CLICKED'),
        `${mode}: Corner outside triangle and empty-Hit button must not fire click, got: ${clickCounts}`
      );

      // Click 2 INSIDE Hit triangle -> calls gotoAndStop("s2_end") -> reaches Scene 2 frame 2!
      await cdp.dispatchClickAt(hitInsidePt.x, hitInsidePt.y);
      await cdp.waitTrace('AT_S2_END_LABEL=1');

      // Wait 80ms so setInterval ticks at least once, then click Restart and verify full cleanup!
      const restartReport = await cdp.evalJson(`new Promise((resolve) => {
        setTimeout(() => {
          document.getElementById('restart').click();
          const immediateStatus = document.getElementById('status')?.textContent || '';
          resolve({ immediateStatus });
        }, 80);
      })`);
      assert.ok(
        restartReport.immediateStatus.startsWith('1 / 4'),
        `${mode}: Restart must immediately reset flattened playhead back to Scene 1 frame 1, got ${restartReport.immediateStatus}`
      );

      // After Restart, Scene 1 auto-advances into Scene 2 again with fresh state (s1Visits === 1, NOT 2, and input restored to DefaultInput)!
      await cdp.waitTrace('S2_READY_VISITS=1_MC_F1=1_F2=2_DYN=true');
      if (mode === 'dom') {
        const postRestartInput = await cdp.evalJson(
          `document.querySelector('#stage-dom-root input.nf-dom-input')?.value || ''`
        );
        assert.equal(postRestartInput, 'DefaultInput', 'Restart must reset dynamic DOM <input> back to authored text');
      }
    }

    // =========================================================================
    // 2. Visual & Responsive Parity: Camera + HUD + scrollRect + Alpha Mask +
    //    BlendMode.LAYER/ERASE + DPR 2.5 Resize in BOTH DOM & Canvas Modes
    // =========================================================================
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: 760,
      height: 560,
      deviceScaleFactor: 2.5,
      mobile: false,
    });

    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/vis-dom/index.html`);
    await cdp.waitTrace('VISUAL_READY=1');

    const domVis = await cdp.evalJson(`(() => {
      const stageEl = document.getElementById('stage-dom-root');
      const hudEl = stageEl.querySelector('.nf-dom-container[data-nf-name="hudClip"]');
      const worldEl = stageEl.querySelector('.nf-dom-container[data-nf-name="worldClip"]');
      const scrollEl = stageEl.querySelector('.nf-dom-container[data-nf-name="scrollClip"]');
      const scrollRectEl = scrollEl?.querySelector('.nf-dom-scroll-rect');
      const scrollInner = scrollEl?.querySelector('.nf-dom-scroll-inner');
      const islands = Array.from(stageEl.querySelectorAll('canvas.nf-dom-canvas-island')).map((c) => {
        const ctx = c.getContext('2d');
        const img = ctx.getImageData(0, 0, c.width, c.height).data;
        let lit = 0;
        let transparent = 0;
        for (let i = 3; i < img.length; i += 4) {
          if (img[i] > 16) lit++;
          else transparent++;
        }
        return {
          reason: c.getAttribute('data-nf-island-reason'),
          width: c.width,
          height: c.height,
          lit,
          transparent,
        };
      });
      return {
        dpr: stageEl.dataset.dpr,
        hudTransform: hudEl?.style?.transform || '',
        worldTransform: worldEl?.style?.transform || '',
        scrollTransform: scrollEl?.style?.transform || '',
        scrollOverflow: scrollRectEl?.style?.overflow || '',
        scrollInnerTransform: scrollInner?.style?.transform || '',
        islands,
      };
    })()`);

    assert.equal(domVis.dpr, '2.5', 'DOM stage must track devicePixelRatio = 2.5');
    assert.equal(
      domVis.hudTransform,
      'matrix(1, 0, 0, 1, 12, 14)',
      `HUD attach_to_camera layer must remain pinned at (12, 14), got ${domVis.hudTransform}`
    );
    assert.equal(
      domVis.worldTransform,
      'matrix(2, 0, 0, 2, 120, 150)',
      `World clip at (200, 150) under camera (+40, 0, zoom 2) must land at matrix(2, 0, 0, 2, 120, 150), got ${domVis.worldTransform}`
    );
    assert.equal(domVis.scrollOverflow, 'hidden', 'scrollRect container must clip overflow');
    assert.equal(
      domVis.scrollInnerTransform,
      'translate(-10px, -5px)',
      'scrollRect inner wrapper must shift by (-scroll_rect.x, -scroll_rect.y)'
    );
    assert.equal(
      domVis.scrollTransform,
      'matrix(2, 0, 0, 2, 40, -30)',
      `scrollRect outer container must NOT double-shift scrollRect (expected matrix(2, 0, 0, 2, 40, -30), got ${domVis.scrollTransform})`
    );
    assert.ok(
      domVis.islands.some((isl) => isl.reason === 'mask' && isl.lit > 20),
      `Alpha mask must mount as a painted Canvas island in DOM mode: ${JSON.stringify(domVis.islands)}`
    );
    assert.ok(
      domVis.islands.some((isl) => String(isl.reason || '').startsWith('blend') && isl.lit > 20 && isl.transparent > 20),
      `BlendMode.LAYER + erase punch-out must mount as a Canvas island with both lit and punched-out transparent pixels: ${JSON.stringify(domVis.islands)}`
    );

    // Verify Canvas mode with the exact same visual + Camera + DPR 2.5 project!
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/vis-canvas/index.html`);
    await cdp.waitTrace('VISUAL_READY=1');

    const canvasVis = await cdp.evalJson(`(() => {
      const c = document.getElementById('stage');
      const ctx = c.getContext('2d');
      const data = ctx.getImageData(0, 0, c.width, c.height).data;
      let nonBg = 0;
      for (let i = 0; i < data.length; i += 16) {
        if (Math.abs(data[i] - 15) > 12 || Math.abs(data[i + 1] - 23) > 12 || Math.abs(data[i + 2] - 42) > 12) {
          nonBg++;
        }
      }
      return {
        canvasWidth: c.width,
        canvasHeight: c.height,
        cssWidth: parseFloat(c.style.width),
        nonBg,
      };
    })()`);
    assert.ok(
      canvasVis.canvasWidth > canvasVis.cssWidth * 2,
      `Canvas backing store must scale with DPR 2.5 (backing=${canvasVis.canvasWidth}, css=${canvasVis.cssWidth})`
    );
    assert.ok(canvasVis.nonBg > 100, `Canvas mode must paint camera/masked/blended stage content (${canvasVis.nonBg})`);

    await cdp.navigateAndWait('about:blank');
    assert.deepEqual(cdp.errors, [], `Uncaught browser errors during T-320 parity suite: ${cdp.errors.join('\n')}`);

    // Keep the actual exported WASM, adapters, renderer and frame loop. Inject
    // faults at host boundaries, retaining the thrown objects for identity checks.
    const preload = await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: `
      const probe = globalThis.__failureProbe = {
        starts: 0, ticks: 0, resets: 0, errors: [], warnings: [], messages: [],
        cleanupError: new Error('T340 secondary cleanup failure'),
        renderError: new Error('T340 secondary renderer failure'),
      };
      for (const [method, values] of [['error', probe.errors], ['warn', probe.warnings]]) {
        const original = console[method];
        console[method] = (...args) => { values.push(args); original.apply(console, args); };
      }
      addEventListener('neoflash-script-message', event => {
        if (event.detail.kind === 'error') probe.messages.push(event.detail);
      });
      Object.defineProperty(globalThis, 'NeoFlashExternal', {
        configurable: true,
        set(api) {
          Object.defineProperty(globalThis, 'NeoFlashExternal', { configurable: true, value: {
            ...api,
            create(options) {
              const adapter = api.create(options);
              probe.external = adapter.ExternalInterface;
              return { ...adapter, reset() {
                probe.resets++;
                adapter.reset();
                if (probe.cleanupPending) {
                  probe.cleanupPending = false;
                  throw probe.cleanupError;
                }
              } };
            },
          } });
        },
      });
      const clearRect = CanvasRenderingContext2D.prototype.clearRect;
      CanvasRenderingContext2D.prototype.clearRect = function (...args) {
        if (this.canvas.id === 'stage' && probe.renderPending) {
          probe.renderPending = false;
          throw probe.renderError;
        }
        return clearRect.apply(this, args);
      };
    ` });
    for (const mode of ['dom', 'canvas']) {
      const firstExpectedError = cdp.errors.length;
      await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/failure-${mode}/index.html`);
      const failure = await cdp.evalJson(`new Promise((resolve, reject) => {
        const deadline = Date.now() + 10000;
        const poll = () => {
          const probe = globalThis.__failureProbe;
          if (globalThis.NeoFlashPlayer && probe.errors.length >= 2) {
            const error = document.getElementById('error');
            resolve({
              messages: probe.messages, text: error.textContent, visible: error.style.display,
              original: probe.errors[0][0].cause === probe.original,
              secondary: probe.errors[1][0] === probe.cleanupError,
              errors: probe.errors.length, starts: probe.starts, ticks: probe.ticks,
            });
          } else if (Date.now() > deadline) reject(new Error('Startup failure did not leave a restartable player'));
          else setTimeout(poll, 25);
        };
        poll();
      })`);
      assert.equal(failure.original, true, `${mode}: runtime wrapper must retain the authored Error object`);
      assert.equal(failure.secondary, true, `${mode}: cleanup must report its own original error separately`);
      assert.equal(failure.errors, 2);
      assert.equal(failure.messages.length, 1, `${mode}: a secondary error must not replace the first diagnostic`);
      assert.equal(failure.messages[0].location.source, 'Failure Recovery / frame 1');
      assert.equal(failure.messages[0].location.line, 4);
      assert.ok(failure.messages[0].location.column > 0);
      assert.match(failure.messages[0].message, /T340 authored startup failure/);
      assert.match(failure.text, /Failure Recovery \/ frame 1:4:/);
      assert.match(failure.text, /T340 authored startup failure/);
      assert.doesNotMatch(failure.text, /secondary cleanup failure/);
      assert.equal(failure.visible, 'block');
      assert.equal(failure.starts, 1);
      assert.equal(failure.ticks, 0);

      const recovered = await cdp.evalJson(`new Promise((resolve, reject) => {
        const probe = globalThis.__failureProbe;
        probe.recover = true;
        NeoFlashPlayer.restart();
        const deadline = Date.now() + 10000;
        const poll = () => {
          if (probe.ticks >= 2) resolve({
            ticks: probe.ticks, starts: probe.starts, resets: probe.resets,
            visible: document.getElementById('error').style.display,
            errors: probe.errors.length, messages: probe.messages.length,
          });
          else if (Date.now() > deadline) reject(new Error('Restart did not resume timeline ticks'));
          else setTimeout(poll, 25);
        };
        poll();
      })`);
      assert.ok(recovered.starts >= 2 && recovered.ticks >= 2, `${mode}: Restart must execute scripts and keep ticking`);
      assert.ok(recovered.resets >= 2, `${mode}: both failure and Restart must reset adapters`);
      assert.equal(recovered.visible, 'none');
      assert.equal(recovered.errors, 2);
      assert.equal(recovered.messages, 1);

      if (mode === 'canvas') {
        const callback = await cdp.evalJson(`new Promise((resolve, reject) => {
          const probe = globalThis.__failureProbe, before = probe.ticks;
          let caught, threw = false;
          try { document.getElementById('stage').t340Throw(); }
          catch (error) { caught = error; threw = true; }
          const result = {
            original: threw && caught === probe.callbackOriginal,
            stack: caught?.stack || '',
            renderer: probe.warnings.length === 1 && probe.warnings[0][1] === probe.renderError,
            renderPending: probe.renderPending,
          };
          const deadline = Date.now() + 10000;
          const poll = () => {
            if (probe.ticks > before) resolve({ ...result, messages: probe.messages.length,
              errors: probe.errors.length, visible: document.getElementById('error').style.display });
            else if (Date.now() > deadline) reject(new Error('External callback failure stopped playback'));
            else setTimeout(poll, 25);
          };
          poll();
        })`);
        assert.equal(callback.original, true, 'ExternalInterface must return the authored Error despite a render failure');
        assert.match(callback.stack, /neoflash-script-0\.js:/);
        assert.equal(callback.renderer, true, 'Renderer error must be diagnosed separately by identity');
        assert.equal(callback.renderPending, false, 'Real stage renderer must have reached the injected failure');
        assert.equal(callback.messages, 1);
        assert.equal(callback.errors, 2);
        assert.equal(callback.visible, 'none');
      }
      // These two intentional console errors were verified by object identity;
      // uncaught exceptions and any extra console failures still fail the batch.
      const expectedErrors = cdp.errors.slice(firstExpectedError);
      assert.equal(expectedErrors.length, 2, `${mode}: unexpected browser errors: ${expectedErrors.join('\n')}`);
      assert.ok(expectedErrors.every(error => error.startsWith('console.error:')));
      assert.match(expectedErrors[0], /T340 authored startup failure/);
      assert.match(expectedErrors[1], /T340 secondary cleanup failure/);
    }
    await cdp.send('Page.removeScriptToEvaluateOnNewDocument', { identifier: preload.identifier });
    const expectedErrorCount = cdp.errors.length;
    await cdp.navigateAndWait('about:blank');
    assert.equal(cdp.errors.length, expectedErrorCount, 'Failure recovery teardown must not add browser errors');
  } finally {
    try {
      cdp?.ws?.close();
    } catch {}
    if (cdp?.chromeProc && cdp.chromeProc.exitCode === null) {
      await new Promise((resolve) => {
        cdp.chromeProc.once('exit', resolve);
        cdp.chromeProc.kill('SIGKILL');
        setTimeout(resolve, 1500);
      });
    }
    if (serverInfo?.server) {
      await new Promise((resolve) => serverInfo.server.close(resolve));
    }
    for (const dir of [tempRoot, userDataDir]) {
      try {
        fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 50 });
      } catch {}
    }
  }
});
