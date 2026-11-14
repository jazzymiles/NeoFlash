const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { execFileSync, spawn } = require('node:child_process');
const { pathToFileURL } = require('node:url');

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
    size: 15,
    color: 0xf1f5f9,
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
  if (fieldOptions) {
    fmt.fieldOptions = fieldOptions;
  }
  return fmt;
}

function makeButtonSymbol(id, name, labelText, fillUp, fillOver, fillDown) {
  const makeStateElements = (stateKey, fill) => [
    {
      id: `${id}-bg-${stateKey}`,
      name: '',
      transform: makeTransform(),
      kind: {
        type: 'rectangle',
        width: 148,
        height: 36,
        fill,
        stroke: [148, 163, 184, 255],
        stroke_width: 1,
      },
      scripts: [],
    },
    {
      id: `${id}-lbl-${stateKey}`,
      name: '',
      transform: makeTransform({ x: 14, y: 9 }),
      kind: {
        type: 'text',
        text: labelText,
        font_family: 'Arial',
        font_size: 14,
        color: [255, 255, 255, 255],
      },
      scripts: [],
    },
  ];
  return {
    id,
    name,
    kind: 'button',
    timeline: {
      frame_count: 4,
      layers: [
        {
          id: `${id}-layer`,
          name: 'Button States',
          visible: true,
          locked: false,
          keyframes: [
            { frame: 0, label: 'Up', tween: null, elements: makeStateElements('up', fillUp) },
            { frame: 1, label: 'Over', tween: null, elements: makeStateElements('over', fillOver) },
            { frame: 2, label: 'Down', tween: null, elements: makeStateElements('down', fillDown) },
            {
              frame: 3,
              label: 'Hit',
              tween: null,
              elements: [
                {
                  id: `${id}-hit`,
                  name: '',
                  transform: makeTransform(),
                  kind: {
                    type: 'rectangle',
                    width: 148,
                    height: 36,
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
      ],
      scripts: [],
    },
  };
}

function buildInteractiveAs3Project() {
  const publishSettingsJson = JSON.stringify({
    render_mode: 'dom',
    breakpoints: [
      {
        name: 'mobile',
        max_width: 599,
        scale_mode: 'exactFit',
        stage_width: 360,
        stage_height: 540,
      },
      {
        name: 'tablet',
        min_width: 600,
        max_width: 999,
        scale_mode: 'showAll',
        stage_width: 640,
        stage_height: 400,
      },
      {
        name: 'desktop',
        min_width: 1000,
        scale_mode: 'showAll',
        stage_width: 800,
        stage_height: 500,
      },
    ],
  });

  const frame0Script = [
    'import flash.events.Event;',
    'import flash.events.MouseEvent;',
    'stop();',
    'var self:* = this;',
    'var nameInput:* = self.getChildByName("nameInput");',
    'var statusField:* = self.getChildByName("statusField");',
    'var nextBtn:* = self.getChildByName("nextBtn");',
    'var presetBtn:* = self.getChildByName("presetBtn");',
    'nameInput.tabIndex = 1;',
    'nextBtn.tabIndex = 2;',
    'presetBtn.tabIndex = 3;',
    'if (!self.savedName) {',
    '  self.savedName = "Initial";',
    '  nameInput.text = "Initial";',
    '} else {',
    '  nameInput.text = String(self.savedName);',
    '}',
    'statusField.htmlText = "<b>Ready:</b> <a href=\\"https://neoflash.example/docs\\">Portal Guide</a> (" + nameInput.text + ")";',
    'nameInput.addEventListener(Event.CHANGE, function(e:Event):void {',
    '  self.savedName = nameInput.text;',
    '  statusField.text = "Typed: " + nameInput.text;',
    '  trace("AS3_CHANGE=" + nameInput.text);',
    '});',
    'presetBtn.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {',
    '  self.savedName = "Ada Lovelace";',
    '  nameInput.text = "Ada Lovelace";',
    '  statusField.text = "Preset: " + nameInput.text;',
    '  trace("AS3_PRESET=" + nameInput.text);',
    '});',
    'nextBtn.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {',
    '  self.savedName = nameInput.text;',
    '  trace("AS3_NEXT=" + self.savedName);',
    '  self.gotoAndStop(2);',
    '});',
    'trace("BOOT_F1=" + self.currentFrame);',
  ].join('\n');

  const frame1Script = [
    'import flash.events.MouseEvent;',
    'stop();',
    'var self:* = this;',
    'var summaryField:* = self.getChildByName("summaryField");',
    'var backBtn:* = self.getChildByName("backBtn");',
    'backBtn.tabIndex = 1;',
    'summaryField.text = "Submitted for: " + String(self.savedName || "");',
    'backBtn.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {',
    '  trace("AS3_BACK=1");',
    '  self.gotoAndStop(1);',
    '});',
    'trace("VIEW_F2=" + summaryField.text);',
  ].join('\n');

  const cardSymbol = {
    id: 'sym-card',
    name: 'HeroCard',
    kind: 'movie_clip',
    timeline: {
      frame_count: 1,
      layers: [
        {
          id: 'card-layer',
          name: 'Vectors',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'card-linear-bg',
                  name: '',
                  transform: makeTransform({ x: 0, y: 0 }),
                  kind: {
                    type: 'gradient_path',
                    commands: [
                      { op: 'move_to', x: 0, y: 0 },
                      { op: 'line_to', x: 180, y: 0 },
                      { op: 'line_to', x: 180, y: 110 },
                      { op: 'line_to', x: 0, y: 110 },
                      { op: 'close' },
                    ],
                    fill_rule: 'nonzero',
                    fill: [0, 0, 0, 0],
                    stroke: [96, 165, 250, 255],
                    stroke_width: 2,
                    fill_gradient: {
                      kind: 'linear',
                      matrix: [0.11, 0, 0, 0.067, 90, 55],
                      focal_point_ratio: 0,
                      stops: [
                        { ratio: 0, color: [30, 64, 175, 255] },
                        { ratio: 255, color: [14, 165, 233, 255] },
                      ],
                    },
                    stroke_gradient: null,
                  },
                  scripts: [],
                },
                {
                  id: 'card-radial-orb',
                  name: '',
                  transform: makeTransform({ x: 110, y: 25 }),
                  kind: {
                    type: 'gradient_path',
                    commands: [
                      { op: 'move_to', x: 0, y: 0 },
                      { op: 'line_to', x: 52, y: 0 },
                      { op: 'line_to', x: 52, y: 52 },
                      { op: 'line_to', x: 0, y: 52 },
                      { op: 'close' },
                    ],
                    fill_rule: 'nonzero',
                    fill: [0, 0, 0, 0],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0,
                    fill_gradient: {
                      kind: 'radial',
                      matrix: [0.032, 0, 0, 0.032, 26, 26],
                      focal_point_ratio: 0,
                      stops: [
                        { ratio: 0, color: [250, 204, 21, 255] },
                        { ratio: 255, color: [236, 72, 153, 255] },
                      ],
                    },
                    stroke_gradient: null,
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

  return {
    format_version: 7,
    name: 'DomPortalApp',
    width: 640,
    height: 400,
    fps: 24,
    background: [15, 23, 42, 255],
    root: 'root',
    resources: [
      {
        id: 'res-publish-settings',
        path: 'publish/settings.json',
        media_type: 'application/json',
        data_base64: Buffer.from(publishSettingsJson, 'utf8').toString('base64'),
      },
    ],
    symbols: [
      makeButtonSymbol('sym-next-btn', 'NextButton', 'Submit & Next', [37, 99, 235, 255], [59, 130, 246, 255], [29, 78, 216, 255]),
      makeButtonSymbol('sym-preset-btn', 'PresetButton', 'Preset Ada', [51, 65, 85, 255], [71, 85, 105, 255], [30, 41, 59, 255]),
      makeButtonSymbol('sym-back-btn', 'BackButton', 'Back to Form', [16, 185, 129, 255], [52, 211, 153, 255], [5, 150, 105, 255]),
      cardSymbol,
      {
        id: 'root',
        name: 'MainScene',
        kind: 'movie_clip',
        timeline: {
          frame_count: 2,
          layers: [
            {
              id: 'layer-ui',
              name: 'Portal UI',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  label: 'form_view',
                  tween: null,
                  elements: [
                    {
                      id: 'el-header',
                      name: '',
                      transform: makeTransform({ x: 28, y: 24 }),
                      kind: {
                        type: 'text',
                        text: 'NeoFlash Web Portal',
                        font_family: 'Arial',
                        font_size: 24,
                        color: [248, 250, 252, 255],
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-status',
                      name: 'statusField',
                      transform: makeTransform({ x: 28, y: 68 }),
                      kind: {
                        type: 'text_box',
                        text: 'Booting...',
                        width: 320,
                        height: 36,
                        format: makeTextFormat(
                          { size: 14, color: 0x93c5fd },
                          { field_type: 'dynamic', selectable: true, html: true }
                        ),
                        word_wrap: true,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-name-input',
                      name: 'nameInput',
                      transform: makeTransform({ x: 28, y: 122 }),
                      kind: {
                        type: 'text_box',
                        text: 'Initial',
                        width: 260,
                        height: 34,
                        format: makeTextFormat(
                          { size: 16, color: 0xf8fafc },
                          {
                            field_type: 'input',
                            selectable: true,
                            border: true,
                            border_color: 0x60a5fa,
                            background: true,
                            background_color: 0x1e293b,
                          }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-next-btn',
                      name: 'nextBtn',
                      transform: makeTransform({ x: 28, y: 178 }),
                      accessibility: {
                        name: 'Submit & Next',
                        description: 'Submit applicant name and open summary',
                        role: 'button',
                        tab_order: 2,
                        disabled: false,
                      },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-next-btn',
                        playback: 'independent',
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-preset-btn',
                      name: 'presetBtn',
                      transform: makeTransform({ x: 190, y: 178 }),
                      accessibility: {
                        name: 'Preset Ada',
                        description: 'Fill Ada Lovelace preset',
                        role: 'button',
                        tab_order: 3,
                        disabled: false,
                      },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-preset-btn',
                        playback: 'independent',
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-card-clip',
                      name: 'cardClip',
                      transform: makeTransform({
                        x: 410,
                        y: 64,
                        scale_x: 1.08,
                        scale_y: 1.08,
                        rotation: 6,
                      }),
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-card',
                        playback: 'independent',
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-glow-island',
                      name: 'glowBadge',
                      blend_mode: 'add',
                      filters: [{ type: 'blur', blur_x: 6, blur_y: 6, quality: 1 }],
                      transform: makeTransform({ x: 410, y: 248 }),
                      kind: {
                        type: 'rectangle',
                        width: 140,
                        height: 54,
                        fill: [56, 189, 248, 255],
                        stroke: [244, 114, 182, 255],
                        stroke_width: 3,
                      },
                      scripts: [],
                    },
                  ],
                },
                {
                  frame: 1,
                  label: 'summary_view',
                  tween: null,
                  elements: [
                    {
                      id: 'el-summary-header',
                      name: '',
                      transform: makeTransform({ x: 28, y: 28 }),
                      kind: {
                        type: 'text',
                        text: 'Application Submitted',
                        font_family: 'Arial',
                        font_size: 24,
                        color: [110, 231, 183, 255],
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-summary-field',
                      name: 'summaryField',
                      transform: makeTransform({ x: 28, y: 78 }),
                      kind: {
                        type: 'text_box',
                        text: '',
                        width: 420,
                        height: 40,
                        format: makeTextFormat(
                          { size: 16, color: 0xf8fafc },
                          { field_type: 'dynamic', selectable: true }
                        ),
                        word_wrap: true,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-back-btn',
                      name: 'backBtn',
                      transform: makeTransform({ x: 28, y: 144 }),
                      accessibility: {
                        name: 'Back to Form',
                        description: 'Return to form view',
                        role: 'button',
                        tab_order: 1,
                        disabled: false,
                      },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-back-btn',
                        playback: 'independent',
                      },
                      scripts: [],
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [
            { frame: 0, script: { language: 'action_script3', source: frame0Script } },
            { frame: 1, script: { language: 'action_script3', source: frame1Script } },
          ],
        },
      },
    ],
  };
}

function buildCanvasBaselineProject(sourceProject) {
  const clone = JSON.parse(JSON.stringify(sourceProject));
  clone.resources = [];
  return clone;
}

function buildInteractiveAs2Project() {
  const frame0As2 = [
    'stop();',
    'userHandle = "guest";',
    'as2Out.text = "Handle: " + userHandle;',
    'as2Input.onChanged = function(tf) {',
    '  as2Out.text = "AS2Changed: " + _root.userHandle;',
    '  trace("AS2_CHANGED=" + _root.userHandle);',
    '};',
    'as2SubmitBtn.onRelease = function() {',
    '  as2Out.text = "AS2Submitted: " + _root.userHandle;',
    '  trace("AS2_SUBMIT=" + _root.userHandle);',
    '};',
    'trace("AS2_READY=1");',
  ].join('\n');

  return {
    format_version: 3,
    name: 'DomAs2Form',
    width: 480,
    height: 300,
    fps: 24,
    background: [20, 24, 36, 255],
    root: 'root',
    resources: [],
    symbols: [
      makeButtonSymbol('sym-as2-btn', 'As2SubmitButton', 'Apply Handle', [79, 70, 229, 255], [99, 102, 241, 255], [67, 56, 202, 255]),
      {
        id: 'root',
        name: 'MainAs2',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'as2-layer',
              name: 'Form',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  label: '',
                  tween: null,
                  elements: [
                    {
                      id: 'as2-in',
                      name: 'as2Input',
                      transform: makeTransform({ x: 24, y: 36 }),
                      kind: {
                        type: 'text_box',
                        text: 'guest',
                        width: 220,
                        height: 32,
                        format: makeTextFormat(
                          { size: 15, color: 0xffffff },
                          {
                            field_type: 'input',
                            variable: 'userHandle',
                            border: true,
                            border_color: 0x818cf8,
                            background: true,
                            background_color: 0x1e1b4b,
                          }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'as2-out',
                      name: 'as2Out',
                      transform: makeTransform({ x: 24, y: 88 }),
                      kind: {
                        type: 'text_box',
                        text: '',
                        width: 320,
                        height: 32,
                        format: makeTextFormat(
                          { size: 15, color: 0xa5b4fc },
                          { field_type: 'dynamic', selectable: true }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'as2-btn',
                      name: 'as2SubmitBtn',
                      transform: makeTransform({ x: 24, y: 140 }),
                      accessibility: {
                        name: 'Apply Handle',
                        description: 'Submit AS2 handle',
                        role: 'button',
                        tab_order: 2,
                        disabled: false,
                      },
                      kind: {
                        type: 'instance',
                        symbol_id: 'sym-as2-btn',
                        playback: 'independent',
                      },
                      scripts: [],
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [{ frame: 0, script: { language: 'action_script2', source: frame0As2 } }],
        },
      },
    ],
  };
}

function startStaticServer(rootDir) {
  const mimeByExt = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.png': 'image/png',
    '.txt': 'text/plain; charset=utf-8',
  };
  const server = http.createServer((req, res) => {
    const rawPath = decodeURIComponent((req.url || '/').split('?')[0]);
    const rel = rawPath.replace(/^\/+/, '');
    const candidate = path.resolve(rootDir, rel);
    if (!candidate.startsWith(rootDir) || !fs.existsSync(candidate) || fs.statSync(candidate).isDirectory()) {
      res.writeHead(404);
      res.end('Not found');
      return;
    }
    const ext = path.extname(candidate).toLowerCase();
    res.writeHead(200, { 'Content-Type': mimeByExt[ext] || 'application/octet-stream' });
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
      '--allow-file-access-from-files',
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

  const waitBooted = async (traceMarker) => {
    await evalJson(`
      new Promise((resolve, reject) => {
        const deadline = Date.now() + 15000;
        const tick = () => {
          const errEl = document.getElementById('error');
          if (errEl && errEl.textContent.trim()) {
            reject(new Error('Runtime error UI: ' + errEl.textContent));
            return;
          }
          const loadingEl = document.getElementById('loading');
          const traceText = document.getElementById('trace')?.textContent || '';
          if (
            loadingEl &&
            loadingEl.style.display === 'none' &&
            window.NeoFlashPlayer &&
            (${JSON.stringify(traceMarker)} === '' || traceText.includes(${JSON.stringify(traceMarker)}))
          ) {
            resolve(true);
            return;
          }
          if (Date.now() > deadline) {
            reject(new Error('Timeout waiting for boot (' + ${JSON.stringify(traceMarker)} + '): trace=' + traceText));
            return;
          }
          setTimeout(tick, 30);
        };
        tick();
      })
    `);
  };

  const pressKey = async (key, code, windowsVirtualKeyCode) => {
    const text = key === 'Enter' ? '\r' : key.length === 1 ? key : undefined;
    await send('Input.dispatchKeyEvent', {
      type: 'keyDown',
      key,
      code,
      windowsVirtualKeyCode,
      nativeVirtualKeyCode: windowsVirtualKeyCode,
      ...(text ? { text, unmodifiedText: text } : {}),
    });
    await send('Input.dispatchKeyEvent', {
      type: 'keyUp',
      key,
      code,
      windowsVirtualKeyCode,
      nativeVirtualKeyCode: windowsVirtualKeyCode,
    });
  };

  return { chromeProc, ws, send, navigateAndWait, evalJson, waitBooted, pressKey, errors };
}

test('DOM publish mode (T-307): Full/Compact/Shared across HTTP & file://, forms, Tab order, breakpoints, Canvas islands, lifecycle, AS2, and Canvas baseline', async (t) => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Google Chrome / Chromium binary not available');
    return;
  }
  assert.ok(
    fs.existsSync(path.join(runtimeDir, 'neoflash_web_bg.wasm')),
    'target/web-runtime must be built before running dom-publish-mode.test.cjs'
  );
  const bin = ensureExportBinary();

  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-dom-publish-307-'));
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-chrome-307-'));
  let serverInfo = null;
  let cdp = null;

  try {
    const as3Project = buildInteractiveAs3Project();
    const canvasProject = buildCanvasBaselineProject(as3Project);
    const as2Project = buildInteractiveAs2Project();

    const as3Path = path.join(tempRoot, 'dom-portal.nflash');
    const canvasPath = path.join(tempRoot, 'canvas-portal.nflash');
    const as2Path = path.join(tempRoot, 'dom-as2.nflash');
    fs.writeFileSync(as3Path, JSON.stringify(as3Project, null, 2));
    fs.writeFileSync(canvasPath, JSON.stringify(canvasProject, null, 2));
    fs.writeFileSync(as2Path, JSON.stringify(as2Project, null, 2));

    const outDomFull = path.join(tempRoot, 'dom-full');
    const outDomCompact = path.join(tempRoot, 'dom-compact');
    const outSharedRuntime = path.join(tempRoot, 'shared_runtime');
    const outDomShared = path.join(tempRoot, 'dom-shared');
    const outCanvasDefault = path.join(tempRoot, 'canvas-default');
    const outDomAs2 = path.join(tempRoot, 'dom-as2');

    const runExport = (args) =>
      execFileSync(bin, ['--runtime', runtimeDir, ...args], {
        cwd: repoRoot,
        encoding: 'utf8',
      });

    runExport([as3Path, '--output', outDomFull, '--render-mode', 'dom', '--size-level', 'full']);
    runExport([as3Path, '--output', outDomCompact, '--render-mode', 'dom', '--size-level', 'compact']);
    runExport(['--shared-runtime', outSharedRuntime]);
    runExport([
      as3Path,
      '--output',
      outDomShared,
      '--render-mode',
      'dom',
      '--size-level',
      'shared',
      '--runtime-url',
      '../shared_runtime',
    ]);
    runExport([canvasPath, '--output', outCanvasDefault]);
    runExport([as2Path, '--output', outDomAs2, '--render-mode', 'dom', '--size-level', 'full']);

    serverInfo = await startStaticServer(tempRoot);
    cdp = await launchCdpSession(chromeBin, userDataDir);

    // -------------------------------------------------------------------------
    // 1. Deep Interactive Verification on DOM Full over HTTP
    // -------------------------------------------------------------------------
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: 900,
      height: 700,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/dom-full/index.html`);
    await cdp.waitBooted('BOOT_F1=1');

    const initialReport = await cdp.evalJson(`(() => {
      const stageEl = document.getElementById('stage-dom-root');
      const texts = Array.from(stageEl.querySelectorAll('.nf-dom-text')).map((el) => ({
        text: el.textContent,
        userSelect: getComputedStyle(el).userSelect,
        links: Array.from(el.querySelectorAll('a')).map((a) => a.getAttribute('href')),
        bolds: Array.from(el.querySelectorAll('b, span')).filter((s) => getComputedStyle(s).fontWeight === '700' || s.style.fontWeight === 'bold').length,
      }));
      const cardContainer = stageEl.querySelector('.nf-dom-container[data-nf-name="cardClip"]');
      const svgs = cardContainer ? Array.from(cardContainer.querySelectorAll('svg.nf-dom-svg')) : [];
      const hasLinearGrad = svgs.some((s) => Boolean(s.querySelector('linearGradient')));
      const hasRadialGrad = svgs.some((s) => Boolean(s.querySelector('radialGradient')));
      const island = stageEl.querySelector('canvas.nf-dom-canvas-island');
      let islandLitPixels = 0;
      if (island) {
        const ictx = island.getContext('2d');
        const data = ictx.getImageData(0, 0, island.width, island.height).data;
        for (let i = 3; i < data.length; i += 4) {
          if (data[i] > 16) islandLitPixels++;
        }
      }
      const inputEl = stageEl.querySelector('input.nf-dom-input');
      const buttons = Array.from(stageEl.querySelectorAll('button.nf-dom-button')).map((b) => ({
        label: b.getAttribute('aria-label'),
        tabIndex: b.tabIndex,
      }));
      return {
        bodyRenderMode: document.body.dataset.renderMode,
        stageTag: stageEl.tagName.toLowerCase(),
        mountCount: stageEl.mountCount,
        disposeCount: stageEl.disposeCount,
        breakpoint: stageEl.dataset.breakpoint,
        logicalWidth: stageEl.dataset.logicalWidth,
        logicalHeight: stageEl.dataset.logicalHeight,
        bundleWarnings: window.NeoFlashBundle?.warnings || [],
        texts,
        cardTransform: cardContainer?.style?.transform || '',
        svgCount: svgs.length,
        hasLinearGrad,
        hasRadialGrad,
        islandReason: island?.getAttribute('data-nf-island-reason') || null,
        islandLitPixels,
        inputValue: inputEl?.value ?? null,
        inputTabIndex: inputEl?.tabIndex ?? null,
        buttons,
        duplicateSemanticTargets: document.querySelectorAll('.semantic-target').length,
        a11yOverlayChildren: document.getElementById('stage-a11y-overlay')?.children?.length || 0,
      };
    })()`);

    assert.equal(initialReport.bodyRenderMode, 'dom');
    assert.equal(initialReport.stageTag, 'neoflash-stage');
    assert.equal(initialReport.mountCount, 1);
    assert.equal(initialReport.disposeCount, 0);
    assert.equal(initialReport.breakpoint, 'tablet');
    assert.equal(initialReport.logicalWidth, '640');
    assert.equal(initialReport.logicalHeight, '400');
    assert.ok(
      initialReport.bundleWarnings.some((w) => w.includes('scoped <canvas> islands')),
      `Expected DOM island diagnostic in warnings, got ${JSON.stringify(initialReport.bundleWarnings)}`
    );

    const headerText = initialReport.texts.find((t) => t.text === 'NeoFlash Web Portal');
    assert.ok(headerText && headerText.userSelect === 'text', 'Static text must be semantic & selectable');
    const richStatus = initialReport.texts.find((t) => t.text.includes('Portal Guide'));
    assert.ok(
      richStatus && richStatus.links.includes('https://neoflash.example/docs') && richStatus.bolds >= 1,
      `Dynamic htmlText must render semantic <a> and bold formatting: ${JSON.stringify(initialReport.texts)}`
    );

    assert.ok(
      initialReport.cardTransform.startsWith('matrix(') && initialReport.svgCount === 2,
      `Nested MovieClip container must apply CSS matrix and mount 2 SVG paths: ${JSON.stringify(initialReport)}`
    );
    assert.equal(initialReport.hasLinearGrad, true, 'SVG linearGradient must mount');
    assert.equal(initialReport.hasRadialGrad, true, 'SVG radialGradient must mount');
    assert.equal(initialReport.islandReason, 'filters', 'Filtered/add-blend artwork must mount as <canvas> island');
    assert.ok(initialReport.islandLitPixels > 25, `Canvas island must paint lit pixels, got ${initialReport.islandLitPixels}`);

    // Keyboard focus & tab order (accessibility.js) — zero duplicate overlay stops
    assert.equal(initialReport.duplicateSemanticTargets, 0);
    assert.equal(initialReport.a11yOverlayChildren, 0);
    assert.equal(initialReport.inputTabIndex, 1);
    assert.deepEqual(
      initialReport.buttons.sort((a, b) => a.tabIndex - b.tabIndex),
      [
        { label: 'Submit & Next', tabIndex: 2 },
        { label: 'Preset Ada', tabIndex: 3 },
      ]
    );

    await cdp.evalJson(`(() => {
      const inp = document.querySelector('#stage-dom-root input.nf-dom-input');
      window.__nfSavedInput = inp;
      inp.focus();
      inp.select();
      return document.activeElement === inp;
    })()`);

    // Type via real browser input -> updates AS3 state + dynamic status text while keeping the exact same DOM node!
    await cdp.send('Input.insertText', { text: 'Grace Hopper' });
    const afterTyping = await cdp.evalJson(`new Promise((resolve) => {
      setTimeout(() => {
        const inp = document.querySelector('#stage-dom-root input.nf-dom-input');
        const texts = Array.from(document.querySelectorAll('#stage-dom-root .nf-dom-text')).map((e) => e.textContent);
        resolve({
          sameNodeAcrossTicks: inp === window.__nfSavedInput,
          stillFocused: document.activeElement === window.__nfSavedInput,
          inputValue: inp.value,
          texts,
          trace: document.getElementById('trace')?.textContent || '',
        });
      }, 120);
    })`);
    assert.equal(afterTyping.sameNodeAcrossTicks, true, 'Keyed reconciliation must preserve the live <input> node across ticks');
    assert.equal(afterTyping.stillFocused, true, 'Focused <input> must retain focus across animation ticks');
    assert.equal(afterTyping.inputValue, 'Grace Hopper');
    assert.ok(afterTyping.texts.includes('Typed: Grace Hopper'), `Dynamic text must reflect typed input: ${JSON.stringify(afterTyping.texts)}`);
    assert.ok(afterTyping.trace.includes('AS3_CHANGE=Grace Hopper'), `Expected AS3 Event.CHANGE trace, got ${afterTyping.trace}`);

    // Real Tab traversal: tabIndex 1 (input) -> tabIndex 2 (Submit & Next) -> tabIndex 3 (Preset Ada)
    await cdp.pressKey('Tab', 'Tab', 9);
    const tabStop2 = await cdp.evalJson(`document.activeElement?.getAttribute('aria-label')`);
    assert.equal(tabStop2, 'Submit & Next');

    await cdp.pressKey('Tab', 'Tab', 9);
    const tabStop3 = await cdp.evalJson(`document.activeElement?.getAttribute('aria-label')`);
    assert.equal(tabStop3, 'Preset Ada');

    // Activate "Preset Ada" via keyboard Space/Enter or click -> AS3 script mutates nameInput.text -> syncs back into <input>.value!
    await cdp.pressKey('Enter', 'Enter', 13);
    const afterPreset = await cdp.evalJson(`(() => ({
      inputValue: document.querySelector('#stage-dom-root input.nf-dom-input')?.value,
      texts: Array.from(document.querySelectorAll('#stage-dom-root .nf-dom-text')).map((e) => e.textContent),
    }))()`);
    assert.equal(afterPreset.inputValue, 'Ada Lovelace');
    assert.ok(afterPreset.texts.includes('Preset: Ada Lovelace'));

    // Responsive breakpoints: resize across mobile (480x760) -> desktop (1280x800) -> tablet (900x700)
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: 480,
      height: 760,
      deviceScaleFactor: 1,
      mobile: true,
    });
    const mobileBp = await cdp.evalJson(`new Promise((resolve) => setTimeout(() => {
      const s = document.getElementById('stage-dom-root');
      const surf = document.getElementById('stage-surface');
      resolve({
        breakpoint: s.dataset.breakpoint,
        logicalWidth: s.dataset.logicalWidth,
        logicalHeight: s.dataset.logicalHeight,
        surfaceWidth: parseFloat(surf.style.width),
      });
    }, 80))`);
    assert.equal(mobileBp.breakpoint, 'mobile');
    assert.equal(mobileBp.logicalWidth, '360');
    assert.equal(mobileBp.logicalHeight, '540');
    assert.ok(mobileBp.surfaceWidth > 0);

    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: 1280,
      height: 800,
      deviceScaleFactor: 1,
      mobile: false,
    });
    const desktopBp = await cdp.evalJson(`new Promise((resolve) => setTimeout(() => {
      const s = document.getElementById('stage-dom-root');
      resolve({
        breakpoint: s.dataset.breakpoint,
        logicalWidth: s.dataset.logicalWidth,
        logicalHeight: s.dataset.logicalHeight,
      });
    }, 80))`);
    assert.equal(desktopBp.breakpoint, 'desktop');
    assert.equal(desktopBp.logicalWidth, '800');
    assert.equal(desktopBp.logicalHeight, '500');

    // Timeline navigation: focus "Submit & Next" and press Enter -> advances to Frame 2 (Summary view)
    await cdp.evalJson(`(() => {
      const btn = document.querySelector('#stage-dom-root button[aria-label="Submit & Next"]');
      btn.focus();
      return document.activeElement === btn;
    })()`);
    await cdp.pressKey('Enter', 'Enter', 13);
    const frame2State = await cdp.evalJson(`new Promise((resolve) => setTimeout(() => {
      const s = document.getElementById('stage-dom-root');
      resolve({
        status: document.getElementById('status')?.textContent || '',
        inputCount: s.querySelectorAll('input.nf-dom-input').length,
        islandCount: s.querySelectorAll('canvas.nf-dom-canvas-island').length,
        texts: Array.from(s.querySelectorAll('.nf-dom-text')).map((e) => e.textContent),
        backBtnLabel: s.querySelector('button.nf-dom-button')?.getAttribute('aria-label') || null,
      });
    }, 60))`);
    assert.ok(frame2State.status.startsWith('2 / 2'), `Expected frame 2 / 2, got ${frame2State.status}`);
    assert.equal(frame2State.inputCount, 0, 'Frame 1 <input> must unmount cleanly on Frame 2');
    assert.equal(frame2State.islandCount, 0, 'Frame 1 <canvas> island must unmount cleanly on Frame 2');
    assert.ok(frame2State.texts.includes('Application Submitted'));
    assert.ok(frame2State.texts.includes('Submitted for: Ada Lovelace'));
    assert.equal(frame2State.backBtnLabel, 'Back to Form');

    // Click "Back to Form" -> returns to Frame 1
    await cdp.evalJson(`(() => {
      document.querySelector('#stage-dom-root button[aria-label="Back to Form"]').click();
      return true;
    })()`);
    const backToFrame1 = await cdp.evalJson(`(() => ({
      status: document.getElementById('status')?.textContent || '',
      inputValue: document.querySelector('#stage-dom-root input.nf-dom-input')?.value || '',
    }))()`);
    assert.ok(backToFrame1.status.startsWith('1 / 2'));
    assert.equal(backToFrame1.inputValue, 'Ada Lovelace');

    // Restart lifecycle: Web Component stays mounted once (mountCount === 1, disposeCount === 0), zero leaked nodes!
    const afterRestart = await cdp.evalJson(`(() => {
      document.getElementById('restart').click();
      const s = document.getElementById('stage-dom-root');
      return {
        mountCount: s.mountCount,
        disposeCount: s.disposeCount,
        isMounted: s.isMounted,
        sceneCount: s.querySelectorAll('.nf-dom-scene').length,
        inputCount: s.querySelectorAll('input.nf-dom-input').length,
        inputValue: s.querySelector('input.nf-dom-input')?.value || '',
      };
    })()`);
    assert.equal(afterRestart.mountCount, 1, '<neoflash-stage> must mount only once across Restart');
    assert.equal(afterRestart.disposeCount, 0, 'Restart must reset scene without disposing <neoflash-stage>');
    assert.equal(afterRestart.isMounted, true);
    assert.equal(afterRestart.sceneCount, 1);
    assert.equal(afterRestart.inputCount, 1);
    assert.equal(afterRestart.inputValue, 'Initial');

    // Unload / pagehide lifecycle: opt out of BFCache via an active Web Lock so Chrome executes
    // a real document unload (persisted === false + unload) and verify <neoflash-stage> disposes
    // once with zero leftover children!
    await cdp.evalJson(`new Promise((resolve) => {
      const recordState = () => {
        const s = document.getElementById('stage-dom-root');
        sessionStorage.setItem('nf_unload_lifecycle', JSON.stringify({
          mountCount: s.mountCount,
          disposeCount: s.disposeCount,
          isMounted: s.isMounted,
          remainingChildren: s.children.length,
        }));
      };
      window.addEventListener('pagehide', recordState);
      window.addEventListener('unload', recordState);
      if (navigator.locks?.request) {
        navigator.locks.request('nf_force_real_unload', () => {
          resolve(true);
          return new Promise(() => {});
        });
      } else {
        resolve(true);
      }
    })`);
    // Navigate within same HTTP origin so we can read back sessionStorage proof of pagehide disposal!
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/dom-compact/index.html`);
    await cdp.waitBooted('BOOT_F1=1');
    const unloadProof = await cdp.evalJson(`JSON.parse(sessionStorage.getItem('nf_unload_lifecycle'))`);
    assert.deepEqual(unloadProof, {
      mountCount: 1,
      disposeCount: 1,
      isMounted: false,
      remainingChildren: 0,
    });

    // -------------------------------------------------------------------------
    // 2. Verify DOM Compact & DOM Shared over HTTP, plus Full/Compact/Shared over file://
    // -------------------------------------------------------------------------
    const verifyDomModeBundle = async (label, url) => {
      await cdp.navigateAndWait(url);
      await cdp.waitBooted('BOOT_F1=1');
      await cdp.evalJson(`(() => {
        const inp = document.querySelector('#stage-dom-root input.nf-dom-input');
        inp.focus();
        inp.select();
        return true;
      })()`);
      await cdp.send('Input.insertText', { text: `User_${label}` });
      const check = await cdp.evalJson(`new Promise((resolve) => setTimeout(() => {
        const s = document.getElementById('stage-dom-root');
        const typedTrace = document.getElementById('trace')?.textContent || '';
        s.querySelector('button[aria-label="Submit & Next"]').click();
        const frame2Texts = Array.from(s.querySelectorAll('.nf-dom-text')).map((e) => e.textContent);
        document.getElementById('restart').click();
        resolve({
          mountCount: s.mountCount,
          disposeCount: s.disposeCount,
          typedTrace,
          frame2Texts,
          postRestartInput: s.querySelector('input.nf-dom-input')?.value || '',
        });
      }, 60))`);
      assert.equal(check.mountCount, 1, `${label}: mountCount must be 1`);
      assert.equal(check.disposeCount, 0, `${label}: disposeCount must stay 0 after Restart`);
      assert.ok(check.typedTrace.includes(`AS3_CHANGE=User_${label}`), `${label}: input change trace missing: ${check.typedTrace}`);
      assert.ok(
        check.frame2Texts.includes(`Submitted for: User_${label}`),
        `${label}: frame 2 summary mismatch: ${JSON.stringify(check.frame2Texts)}`
      );
      assert.equal(check.postRestartInput, 'Initial', `${label}: Restart must restore Initial input`);
    };

    await verifyDomModeBundle('http_compact', `http://127.0.0.1:${serverInfo.port}/dom-compact/index.html`);
    await verifyDomModeBundle('http_shared', `http://127.0.0.1:${serverInfo.port}/dom-shared/index.html`);
    await verifyDomModeBundle('file_full', pathToFileURL(path.join(outDomFull, 'index.html')).href);
    await verifyDomModeBundle('file_compact', pathToFileURL(path.join(outDomCompact, 'index.html')).href);
    await verifyDomModeBundle('file_shared', pathToFileURL(path.join(outDomShared, 'index.html')).href);

    // -------------------------------------------------------------------------
    // 3. Verify AS2 Interactive Form & Button in DOM Mode
    // -------------------------------------------------------------------------
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/dom-as2/index.html`);
    await cdp.waitBooted('AS2_READY=1');
    await cdp.evalJson(`(() => {
      const inp = document.querySelector('#stage-dom-root input.nf-dom-input');
      inp.focus();
      inp.select();
      return true;
    })()`);
    await cdp.send('Input.insertText', { text: 'miles_as2' });
    const as2Result = await cdp.evalJson(`new Promise((resolve) => setTimeout(() => {
      const s = document.getElementById('stage-dom-root');
      const afterType = Array.from(s.querySelectorAll('.nf-dom-text')).map((e) => e.textContent);
      s.querySelector('button[aria-label="Apply Handle"]').click();
      const afterClick = Array.from(s.querySelectorAll('.nf-dom-text')).map((e) => e.textContent);
      resolve({ afterType, afterClick, trace: document.getElementById('trace')?.textContent || '' });
    }, 60))`);
    assert.ok(as2Result.afterType.includes('AS2Changed: miles_as2'), `AS2 onChanged + variable sync failed: ${JSON.stringify(as2Result)}`);
    assert.ok(as2Result.afterClick.includes('AS2Submitted: miles_as2'), `AS2 button onRelease failed: ${JSON.stringify(as2Result)}`);

    // -------------------------------------------------------------------------
    // 4. Verify Default Canvas Baseline Remains Completely Unchanged
    // -------------------------------------------------------------------------
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/canvas-default/index.html`);
    await cdp.waitBooted('BOOT_F1=1');
    const canvasCheck = await cdp.evalJson(`(() => {
      const canvas = document.getElementById('stage');
      const ctx = canvas.getContext('2d');
      const img = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      let nonBg = 0;
      for (let i = 0; i < img.length; i += 16) {
        if (Math.abs(img[i] - 15) > 12 || Math.abs(img[i + 1] - 23) > 12 || Math.abs(img[i + 2] - 42) > 12) {
          nonBg++;
        }
      }
      return {
        bodyRenderMode: document.body.dataset.renderMode || null,
        domStageCount: document.querySelectorAll('neoflash-stage, #stage-dom-root').length,
        nonBg,
      };
    })()`);
    assert.equal(canvasCheck.bodyRenderMode, null, 'Default Canvas export must not set data-render-mode');
    assert.equal(canvasCheck.domStageCount, 0, 'Default Canvas export must not mount <neoflash-stage>');
    assert.ok(canvasCheck.nonBg > 50, `Default Canvas export must paint stage artwork onto <canvas>, got ${canvasCheck.nonBg}`);

    // Final navigate to about:blank and assert zero uncaught exceptions or console.errors across all 8 navigations!
    await cdp.navigateAndWait('about:blank');
    assert.deepEqual(cdp.errors, [], `Uncaught browser errors during DOM publish suite: ${cdp.errors.join('\n')}`);
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
