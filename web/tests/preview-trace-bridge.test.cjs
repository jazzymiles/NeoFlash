'use strict';

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

function buildOneFrameGameProject(dialect) {
  const isAs2 = dialect === 'as2';
  const lang = isAs2 ? 'action_script2' : 'action_script3';
  const frameSource = isAs2
    ? [
        'stop();',
        'trace("AS2_INIT");',
        'var ticks:Number = 0;',
        'this.onEnterFrame = function():Void {',
        '  ticks = ticks + 1;',
        '  if (ticks <= 3) {',
        '    trace("AS2_TICK:" + ticks);',
        '  }',
        '};',
        'var clicks:Number = 0;',
        'this.fireBtn.onRelease = function():Void {',
        '  clicks = clicks + 1;',
        '  trace("AS2_BTN:" + clicks);',
        '  if (clicks >= 2) {',
        '    throw new Error("AS2_BOOM_AT_BTN");',
        '  }',
        '};',
      ].join('\n')
    : [
        'import flash.events.Event;',
        'import flash.events.MouseEvent;',
        'stop();',
        'trace("AS3_INIT");',
        'var ticks:int = 0;',
        'addEventListener(Event.ENTER_FRAME, function(e:Event):void {',
        '  ticks += 1;',
        '  if (ticks <= 3) {',
        '    trace("AS3_TICK:" + ticks);',
        '  }',
        '});',
        'var clicks:int = 0;',
        'fireBtn.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {',
        '  clicks += 1;',
        '  trace("AS3_BTN:" + clicks);',
        '  if (clicks >= 2) {',
        '    throw new Error("AS3_BOOM_AT_BTN");',
        '  }',
        '});',
      ].join('\n');

  const btnSymbol = {
    id: 'sym-fire-btn',
    name: 'FireButton',
    kind: 'button',
    timeline: {
      frame_count: 4,
      layers: [
        {
          id: 'btn-layer',
          name: 'Art',
          visible: true,
          locked: false,
          keyframes: [0, 1, 2, 3].map(f => ({
            frame: f,
            label: '',
            tween: null,
            elements: [
              {
                id: `btn-rect-${f}`,
                name: '',
                transform: makeTransform(),
                kind: {
                  type: 'rectangle',
                  width: 120,
                  height: 60,
                  fill: [220, 70, 50, 255],
                  stroke: [255, 255, 255, 255],
                  stroke_width: 2,
                },
                visible: true,
                alpha: 1,
                scripts: [],
              },
            ],
          })),
        },
      ],
      scripts: [],
    },
  };

  const rootSymbol = {
    id: 'root',
    name: 'Scene 1',
    kind: 'movie_clip',
    timeline: {
      frame_count: 1,
      layers: [
        {
          id: 'layer-main',
          name: 'Game',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'el-fire-btn',
                  name: 'fireBtn',
                  transform: makeTransform({ x: 100, y: 80 }),
                  kind: {
                    type: 'instance',
                    symbol_id: 'sym-fire-btn',
                    playback: 'independent',
                  },
                  visible: true,
                  alpha: 1,
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
            language: lang,
            source: frameSource,
          },
        },
      ],
    },
  };

  return {
    format_version: 6,
    name: `OneFrame_${dialect.toUpperCase()}`,
    width: 400,
    height: 240,
    fps: 30,
    background: [24, 28, 36, 255],
    script_language: lang,
    root: 'root',
    symbols: [rootSymbol, btnSymbol],
    sources: [],
    resources: [],
  };
}

function buildBurstProject() {
  const rootSymbol = {
    id: 'root',
    name: 'Scene 1',
    kind: 'movie_clip',
    timeline: {
      frame_count: 1,
      layers: [
        {
          id: 'layer-burst',
          name: 'Burst',
          visible: true,
          locked: false,
          keyframes: [{ frame: 0, label: '', tween: null, elements: [] }],
        },
      ],
      scripts: [
        {
          frame: 0,
          script: {
            language: 'action_script3',
            source: [
              'stop();',
              'for (var i:int = 0; i < 5250; i++) {',
              '  trace("BURST:" + i);',
              '}',
              'setTimeout(function():void { trace("AFTER_BURST"); }, 1100);',
            ].join('\n'),
          },
        },
      ],
    },
  };
  return {
    format_version: 6,
    name: 'BurstCapCheck',
    width: 320,
    height: 200,
    fps: 24,
    background: [20, 20, 20, 255],
    script_language: 'action_script3',
    root: 'root',
    symbols: [rootSymbol],
    sources: [],
    resources: [],
  };
}

function startPreviewCaptureServer(rootDir) {
  const mimeByExt = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.map': 'application/json; charset=utf-8',
    '.txt': 'text/plain; charset=utf-8',
    '.png': 'image/png',
  };
  const state = {
    outputRequests: 0,
    events: [],
    rejectOutput: false,
    clear() {
      this.outputRequests = 0;
      this.events.length = 0;
    },
  };

  const server = http.createServer((req, res) => {
    const cleanUrl = decodeURIComponent((req.url || '/').split('?')[0]);
    if (cleanUrl === '/__neoflash/output') {
      state.outputRequests += 1;
      if (req.method === 'OPTIONS') {
        res.writeHead(204, { Allow: 'POST, OPTIONS', 'Cache-Control': 'no-store' });
        res.end();
        return;
      }
      if (state.rejectOutput) {
        req.socket.destroy();
        return;
      }
      const chunks = [];
      req.on('data', chunk => chunks.push(chunk));
      req.on('end', () => {
        try {
          const parsed = JSON.parse(Buffer.concat(chunks).toString('utf8'));
          if (Array.isArray(parsed.events)) {
            for (const ev of parsed.events) {
              state.events.push({ ...ev, session: ev.session || parsed.session || null });
            }
          }
          res.writeHead(204, { 'Cache-Control': 'no-store, no-cache, must-revalidate' });
          res.end();
        } catch (_) {
          res.writeHead(400, { 'Content-Type': 'text/plain; charset=utf-8' });
          res.end('400 Bad Request\n');
        }
      });
      return;
    }

    const relPath = cleanUrl === '/' ? 'index.html' : cleanUrl.replace(/^\/+/, '');
    const candidate = path.resolve(rootDir, relPath);
    if (
      !candidate.startsWith(path.resolve(rootDir) + path.sep) ||
      !fs.existsSync(candidate) ||
      fs.statSync(candidate).isDirectory()
    ) {
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

  return new Promise(resolve => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port, state }));
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
    { stdio: ['ignore', 'pipe', 'pipe'] },
  );

  const wsUrl = await new Promise((resolve, reject) => {
    let stderrBuf = '';
    const timer = setTimeout(() => reject(new Error(`Chrome DevTools timeout: ${stderrBuf}`)), 15000);
    chromeProc.stderr.on('data', async chunk => {
      stderrBuf += chunk.toString();
      const match = stderrBuf.match(/DevTools listening on (ws:\/\/\S+)/);
      if (match) {
        clearTimeout(timer);
        try {
          const devOrigin = match[1].replace(/^ws:/, 'http:').replace(/\/devtools\/.*/, '');
          const resp = await fetch(`${devOrigin}/json/list`);
          const targets = await resp.json();
          const page = targets.find(t => t.type === 'page' && t.webSocketDebuggerUrl);
          if (!page) reject(new Error('No page CDP target'));
          else resolve(page.webSocketDebuggerUrl);
        } catch (err) {
          reject(err);
        }
      }
    });
    chromeProc.once('exit', code => {
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
  const consoleErrors = [];
  const uncaughtExceptions = [];
  let loadResolve = null;

  ws.addEventListener('message', event => {
    const msg = JSON.parse(String(event.data));
    if (msg.method === 'Page.loadEventFired' && loadResolve) {
      const fn = loadResolve;
      loadResolve = null;
      fn();
    } else if (msg.method === 'Runtime.exceptionThrown') {
      uncaughtExceptions.push(JSON.stringify(msg.params.exceptionDetails));
    } else if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
      const rendered = (msg.params.args || [])
        .map(a => a.description || a.value || JSON.stringify(a))
        .join(' ');
      consoleErrors.push(rendered);
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
  await send('Emulation.setDeviceMetricsOverride', {
    width: 800,
    height: 600,
    deviceScaleFactor: 1,
    mobile: false,
  });

  const navigateAndWait = async url => {
    const loaded = new Promise(resolve => {
      loadResolve = resolve;
    });
    await send('Page.navigate', { url });
    await loaded;
  };

  const evalJson = async expression => {
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

  const dispatchClickAt = async (x, y) => {
    await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'none' });
    await send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
    await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
  };

  return {
    chromeProc,
    ws,
    send,
    navigateAndWait,
    evalJson,
    dispatchClickAt,
    consoleErrors,
    uncaughtExceptions,
  };
}

async function waitForCondition(predicate, description, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  throw new Error(`Timed out waiting for: ${description}`);
}

test('T-356: preview HTML relays Frame-1 trace, onEnterFrame, button click, mapped runtime errors, and Restart while normal Publish emits 0 output requests', async t => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Google Chrome / Chromium binary not installed');
    return;
  }
  assert.ok(
    fs.existsSync(path.join(runtimeDir, 'neoflash_web_bg.wasm')),
    'target/web-runtime/neoflash_web_bg.wasm missing — run ./scripts/build-web.sh first',
  );
  const bin = ensureFreshExportBinary();

  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-t356-trace-bridge-'));
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-t356-chrome-'));
  let serverInfo = null;
  let cdp = null;

  try {
    for (const dialect of ['as2', 'as3']) {
      const projPath = path.join(tempRoot, `game-${dialect}.nflash`);
      fs.writeFileSync(projPath, JSON.stringify(buildOneFrameGameProject(dialect), null, 2));

      // 1) Preview export (`--preview` == `PublishPrefs::preview_html_config()`).
      const previewOut = path.join(tempRoot, `preview-${dialect}`);
      execFileSync(bin, ['--runtime', runtimeDir, projPath, '--output', previewOut, '--preview'], {
        cwd: repoRoot,
        encoding: 'utf8',
      });
      const previewHtml = fs.readFileSync(path.join(previewOut, 'index.html'), 'utf8');
      assert.match(
        previewHtml,
        /data-neoflash-output-endpoint="\/__neoflash\/output"/,
        `${dialect} preview HTML must carry data-neoflash-output-endpoint`,
      );

      // 2) Ordinary Publish exports (default and `--clean`).
      const publishOut = path.join(tempRoot, `publish-${dialect}`);
      execFileSync(bin, ['--runtime', runtimeDir, projPath, '--output', publishOut], {
        cwd: repoRoot,
        encoding: 'utf8',
      });
      const publishHtml = fs.readFileSync(path.join(publishOut, 'index.html'), 'utf8');
      assert.doesNotMatch(
        publishHtml,
        /data-neoflash-output-endpoint/,
        `${dialect} published HTML must NOT carry preview telemetry attributes`,
      );

      const cleanOut = path.join(tempRoot, `clean-${dialect}`);
      execFileSync(bin, ['--runtime', runtimeDir, projPath, '--output', cleanOut, '--clean'], {
        cwd: repoRoot,
        encoding: 'utf8',
      });
      const cleanHtml = fs.readFileSync(path.join(cleanOut, 'index.html'), 'utf8');
      assert.doesNotMatch(cleanHtml, /data-neoflash-output-endpoint/);
    }

    const burstPath = path.join(tempRoot, 'burst.nflash');
    fs.writeFileSync(burstPath, JSON.stringify(buildBurstProject(), null, 2));
    execFileSync(bin, ['--runtime', runtimeDir, burstPath, '--output', path.join(tempRoot, 'preview-burst'), '--preview'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });

    serverInfo = await startPreviewCaptureServer(tempRoot);
    cdp = await launchCdpSession(chromeBin, userDataDir);

    // =========================================================================
    // A. Non-preview Publish packages make ZERO `/__neoflash/output` requests!
    // =========================================================================
    for (const variant of ['publish-as2', 'publish-as3', 'clean-as2', 'clean-as3']) {
      serverInfo.state.clear();
      cdp.consoleErrors.length = 0;
      cdp.uncaughtExceptions.length = 0;

      await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/${variant}/index.html`);
      await waitForCondition(
        async () => {
          const ok = await cdp.evalJson('Boolean(globalThis.NeoFlashPlayer)');
          return ok;
        },
        `${variant} booted`,
      );
      await new Promise(resolve => setTimeout(resolve, 150));

      assert.equal(
        serverInfo.state.outputRequests,
        0,
        `${variant} must emit 0 /__neoflash/output requests`,
      );
      assert.deepEqual(cdp.consoleErrors, [], `${variant} console errors`);
      assert.deepEqual(cdp.uncaughtExceptions, [], `${variant} uncaught exceptions`);
    }

    // =========================================================================
    // B. Preview 1-frame AS2 & AS3 games stream Frame-1 trace, onEnterFrame,
    //    button click, mapped runtime errors, and Restart in exact order!
    // =========================================================================
    for (const dialect of ['as2', 'as3']) {
      const upper = dialect.toUpperCase();
      const expectedErrorLine = dialect === 'as2' ? 15 : 17;

      serverInfo.state.clear();
      serverInfo.state.rejectOutput = false;
      cdp.consoleErrors.length = 0;
      cdp.uncaughtExceptions.length = 0;

      await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-${dialect}/index.html`);

      // Wait for Frame 1 trace + all 3 onEnterFrame traces to arrive on the HTTP server.
      await waitForCondition(
        () => serverInfo.state.events.length >= 4,
        `${upper} Frame-1 + 3 onEnterFrame traces (${ JSON.stringify(serverInfo.state.events) })`,
      );

      assert.deepEqual(
        serverInfo.state.events.slice(0, 4).map(e => ({ kind: e.kind, message: e.message })),
        [
          { kind: 'trace', message: `${upper}_INIT` },
          { kind: 'trace', message: `${upper}_TICK:1` },
          { kind: 'trace', message: `${upper}_TICK:2` },
          { kind: 'trace', message: `${upper}_TICK:3` },
        ],
        `${upper}: synchronous Frame-1 trace must arrive first followed by exact onEnterFrame ticks`,
      );
      assert.equal(serverInfo.state.events[0].sourcePath, 'Scene 1 / frame 1');
      assert.equal(serverInfo.state.events[0].line, dialect === 'as2' ? 2 : 4);
      assert.ok(serverInfo.state.events[0].column >= 1);

      // Compute stage center of `fireBtn` (`x=100..220, y=80..140` on `400x240` stage).
      const btnPt = await cdp.evalJson(`(() => {
        const r = document.getElementById('stage').getBoundingClientRect();
        return {
          x: r.left + (160 / 400) * r.width,
          y: r.top + (110 / 240) * r.height,
        };
      })()`);

      // Click #1: emits `${upper}_BTN:1` with 0 console errors!
      await cdp.dispatchClickAt(btnPt.x, btnPt.y);
      await waitForCondition(
        () => serverInfo.state.events.some(e => e.message === `${upper}_BTN:1`),
        `${upper} button click #1 trace`,
      );
      assert.deepEqual(cdp.consoleErrors, [], `${upper}: 0 console errors before intentional throw`);
      assert.deepEqual(cdp.uncaughtExceptions, [], `${upper}: 0 uncaught exceptions`);

      // Click #2: emits `${upper}_BTN:2`, then throws `${upper}_BOOM_AT_BTN`!
      // (`fail()` intentionally logs the caught error once to `console.error`; clear afterward.)
      await cdp.dispatchClickAt(btnPt.x, btnPt.y);
      await waitForCondition(
        () => serverInfo.state.events.some(e => e.kind === 'error'),
        `${upper} mapped runtime error`,
      );

      const errEvent = serverInfo.state.events.find(e => e.kind === 'error');
      assert.ok(errEvent, `${upper} error event received`);
      assert.match(errEvent.message, new RegExp(`${upper}_BOOM_AT_BTN`));
      assert.equal(errEvent.sourcePath, 'Scene 1 / frame 1');
      assert.equal(errEvent.line, expectedErrorLine, `${upper} mapped runtime error line`);
      assert.ok(errEvent.column >= 1, `${upper} mapped runtime error column`);
      assert.deepEqual(cdp.uncaughtExceptions, [], `${upper}: 0 uncaught exceptions on runtime error`);

      // Verify monotonic sequence numbers across the entire stream.
      for (let i = 1; i < serverInfo.state.events.length; i++) {
        assert.equal(
          serverInfo.state.events[i].seq,
          serverInfo.state.events[i - 1].seq + 1,
          `${upper}: sequence numbers must be strictly consecutive`,
        );
      }

      // Click `#restart`: emits `restart` boundary followed by fresh `${upper}_INIT`!
      cdp.consoleErrors.length = 0;
      const countBeforeRestart = serverInfo.state.events.length;
      await cdp.evalJson(`document.getElementById('restart').click(); true`);
      await waitForCondition(
        () =>
          serverInfo.state.events
            .slice(countBeforeRestart)
            .some(e => e.kind === 'trace' && e.message === `${upper}_INIT`),
        `${upper} restart + fresh Frame-1 trace`,
      );
      const afterRestart = serverInfo.state.events.slice(countBeforeRestart);
      assert.equal(afterRestart[0].kind, 'restart');
      assert.equal(afterRestart[1].kind, 'trace');
      assert.equal(afterRestart[1].message, `${upper}_INIT`);
      assert.deepEqual(cdp.consoleErrors, [], `${upper}: 0 console errors after Restart`);
      assert.deepEqual(cdp.uncaughtExceptions, []);
    }

    // =========================================================================
    // C. Burst >5,000 lines/sec caps gracefully with `[trace] ... suppressed N lines ...`
    //    AND closing `/__neoflash/output` causes 0 console/page errors!
    // =========================================================================
    serverInfo.state.clear();
    serverInfo.state.rejectOutput = false;
    cdp.consoleErrors.length = 0;
    cdp.uncaughtExceptions.length = 0;

    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-burst/index.html`);
    await waitForCondition(
      () => serverInfo.state.events.some(e => e.message === 'AFTER_BURST'),
      'burst cap + AFTER_BURST recovery',
      10000,
    );
    const suppressedNotice = serverInfo.state.events.find(e =>
      /^\[trace\] \.\.\. suppressed \d+ lines \.\.\.$/.test(e.message),
    );
    assert.ok(suppressedNotice, 'runaway >5,000 lines/sec loop must emit suppression notice');
    assert.equal(serverInfo.state.events.at(-1).message, 'AFTER_BURST');
    assert.deepEqual(cdp.consoleErrors, []);
    assert.deepEqual(cdp.uncaughtExceptions, []);

    // Simulate editor closing `/__neoflash/output` while browser keeps emitting traces:
    serverInfo.state.rejectOutput = true;
    await cdp.evalJson(`document.getElementById('restart').click(); true`);
    await new Promise(resolve => setTimeout(resolve, 200));
    assert.deepEqual(
      cdp.consoleErrors,
      [],
      'closed /__neoflash/output endpoint must NEVER emit console.error',
    );
    assert.deepEqual(cdp.uncaughtExceptions, []);
  } finally {
    if (cdp) {
      try {
        cdp.ws.close();
      } catch (_) {}
      cdp.chromeProc.kill('SIGKILL');
    }
    if (serverInfo) {
      await new Promise(resolve => serverInfo.server.close(resolve));
    }
    fs.rmSync(tempRoot, { recursive: true, force: true });
    fs.rmSync(userDataDir, { recursive: true, force: true });
  }
});

test('T-368: formatTraceValue and formatTraceArgs preserve structure for 2D/3D/sparse/associative arrays, shared DAGs, cycles, plain/null-proto objects, Map, Set, TypedArray, and custom toString without invoking getters', () => {
  const { formatTraceValue, formatTraceArgs } = require('../runtime.js');

  assert.equal(formatTraceValue([0, 1, 0]), '[0, 1, 0]');

  const tFigure = [
    [0, 1, 0],
    [1, 1, 1],
    [0, 0, 0],
  ];
  assert.equal(
    formatTraceValue(tFigure),
    '[\n  [0, 1, 0],\n  [1, 1, 1],\n  [0, 0, 0]\n]',
  );

  const allFigures = [tFigure, [[1, 1, 1, 1]]];
  assert.equal(
    formatTraceValue(allFigures),
    '[\n  [\n    [0, 1, 0],\n    [1, 1, 1],\n    [0, 0, 0]\n  ],\n  [\n    [1, 1, 1, 1]\n  ]\n]',
  );

  // Shared non-circular DAG sub-array must expand in full both times, while true self-cycle prints [Circular].
  const sharedRow = [1, 2];
  assert.equal(formatTraceValue([sharedRow, sharedRow]), '[\n  [1, 2],\n  [1, 2]\n]');
  const cyc = [1];
  cyc.push(cyc);
  assert.equal(formatTraceValue(cyc), '[\n  1,\n  [Circular]\n]');

  // Sparse holes + AS2-style associative string keys on an Array.
  const sparseAssoc = [1, , 3];
  sparseAssoc.piece = 'T';
  assert.equal(formatTraceValue(sparseAssoc), '[1, <empty>, 3, piece: "T"]');

  // Object.create(null) + nested objects + side-effect-free [Getter] safety.
  let getterCalled = 0;
  const nullProto = Object.create(null);
  nullProto.kind = 'grid';
  nullProto.cells = [[1, 0], [0, 1]];
  Object.defineProperty(nullProto, 'boom', {
    enumerable: true,
    get() {
      getterCalled += 1;
      throw new Error('must never invoke getter');
    },
  });
  assert.equal(
    formatTraceValue(nullProto),
    '{\n  kind: "grid",\n  cells: [\n    [1, 0],\n    [0, 1]\n  ],\n  boom: [Getter]\n}',
  );
  assert.equal(getterCalled, 0);

  // Map, Set, TypedArray, and custom toString preservation.
  const map = new Map([['hp', 100], ['pos', [3, 4]]]);
  assert.equal(formatTraceValue(map), 'Map(2) {\n  "hp" => 100,\n  "pos" => [3, 4]\n}');
  assert.equal(formatTraceValue(new Set([1, 2, 3])), 'Set(3) { 1, 2, 3 }');
  assert.equal(formatTraceValue(new Int32Array([4, 5, 6])), 'Int32Array(3) [4, 5, 6]');

  class CustomBadge {
    toString() {
      return '<Badge#7>';
    }
  }
  assert.equal(formatTraceArgs(['Label:', new CustomBadge(), [1, 2]]), 'Label: <Badge#7> [1, 2]');
});

test('T-368: Test Movie relays structured AS2 2D/3D arrays & classes, AS3 Vector/Dictionary/classes, and JS console.log/warn objects with mapped locations while keeping Array/Object.toString intact', async t => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Google Chrome / Chromium binary not installed');
    return;
  }
  assert.ok(
    fs.existsSync(path.join(runtimeDir, 'neoflash_web_bg.wasm')),
    'target/web-runtime/neoflash_web_bg.wasm missing — run ./scripts/build-web.sh first',
  );
  const bin = ensureFreshExportBinary();
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-t368-structured-'));
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-t368-chrome-'));
  let serverInfo = null;
  let cdp = null;

  const makeStructuredProject = (name, scriptLanguage, frameSource, sources = []) => ({
    format_version: 6,
    name,
    width: 320,
    height: 200,
    fps: 24,
    background: [20, 24, 30, 255],
    script_language: scriptLanguage,
    root: 'root',
    symbols: [
      {
        id: 'root',
        name: 'Scene 1',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'layer-1',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [{ frame: 0, label: '', tween: null, elements: [] }],
            },
          ],
          scripts: [
            {
              frame: 0,
              script: { language: scriptLanguage, source: frameSource },
            },
          ],
        },
      },
    ],
    sources,
    resources: [],
  });

  try {
    // 1) AS2 project: Tetris 2D/3D arrays, plain object, AS2 class without toString, AS2 class with custom toString, and ECMA String/+ conformance.
    const as2Proj = makeStructuredProject(
      'Structured_AS2',
      'action_script2',
      [
        'stop();',
        'var TFigure = [[0, 1, 0], [1, 1, 1], [0, 0, 0]];',
        'var AllFigures = new Array(TFigure, [[1, 1, 1, 1]]);',
        'trace(TFigure);',
        'trace(AllFigures);',
        'trace({ name: "T", cells: [[1, 0], [1, 1]] });',
        'trace(new Piece("L", [[1, 0], [1, 1]]));',
        'trace(new NamedBadge("gold"));',
        'trace("COERCE:" + [1, 2] + "|" + ({ a: 1 }));',
      ].join('\n'),
      [
        {
          id: 'src-piece',
          path: 'Piece.as',
          language: 'action_script2',
          source: [
            'class Piece {',
            '  var kind:String;',
            '  var matrix:Array;',
            '  function Piece(k:String, m:Array) {',
            '    this.kind = k;',
            '    this.matrix = m;',
            '  }',
            '}',
          ].join('\n'),
        },
        {
          id: 'src-badge',
          path: 'NamedBadge.as',
          language: 'action_script2',
          source: [
            'class NamedBadge {',
            '  var tier:String;',
            '  function NamedBadge(t:String) { this.tier = t; }',
            '  function toString():String { return "Badge(" + this.tier + ")"; }',
            '}',
          ].join('\n'),
        },
      ],
    );

    // 2) AS3 project: nested Vector.<Vector.<int>>, Dictionary, Point, XML, AS3 class with public fields + getter safety, and String() conformance.
    const as3Proj = {
      ...makeStructuredProject('Structured_AS3', 'action_script3', '', [
        {
          id: 'src-main',
          path: 'app/Main.as',
          language: 'action_script3',
          source: [
            'package app {',
            '  import flash.display.MovieClip;',
            '  import flash.utils.Dictionary;',
            '  import flash.geom.Point;',
            '  public class Main extends MovieClip {',
            '    public function Main() {',
            '      super();',
            '      stop();',
            '      var row0:Vector.<int> = new Vector.<int>();',
            '      row0.push(0, 1, 0);',
            '      var row1:Vector.<int> = new Vector.<int>();',
            '      row1.push(1, 1, 1);',
            '      var grid:Vector.<Vector.<int>> = new Vector.<Vector.<int>>();',
            '      grid.push(row0, row1);',
            '      trace(grid);',
            '      var dict:Dictionary = new Dictionary();',
            '      dict["hp"] = 100;',
            '      trace(dict);',
            '      trace(new Point(3, 4));',
            '      trace(new XML("<item id=\\"7\\"/>"));',
            '      var bs:BoardState = new BoardState();',
            '      trace(bs, bs.getterCalls);',
            '      trace("AS3_COERCE:" + String([1, 2]) + "|" + String({ a: 1 }));',
            '    }',
            '  }',
            '}',
          ].join('\n'),
        },
        {
          id: 'src-board-state',
          path: 'app/BoardState.as',
          language: 'action_script3',
          source: [
            'package app {',
            '  public class BoardState {',
            '    public var level:int = 3;',
            '    public var rows:Array = [[1, 0], [0, 1]];',
            '    public var getterCalls:int = 0;',
            '    public function get computed():int {',
            '      getterCalls += 1;',
            '      return 99;',
            '    }',
            '  }',
            '}',
          ].join('\n'),
        },
      ]),
      document_class: 'app.Main',
    };

    // 3) JavaScript project: console.log & console.warn with nested arrays/objects and mapped source locations.
    const jsProj = makeStructuredProject(
      'Structured_JS',
      'java_script',
      [
        'stop();',
        'console.log("JS_MAT", [[1, 2], [3, 4]]);',
        'console.warn({ warnCode: 7, nested: { ok: true } });',
      ].join('\n'),
    );

    for (const [slug, proj] of [['as2', as2Proj], ['as3', as3Proj], ['js', jsProj]]) {
      const pPath = path.join(tempRoot, `${slug}.nflash`);
      fs.writeFileSync(pPath, JSON.stringify(proj, null, 2));
      execFileSync(bin, ['--runtime', runtimeDir, pPath, '--output', path.join(tempRoot, `preview-${slug}`), '--preview'], {
        cwd: repoRoot,
        encoding: 'utf8',
      });
    }

    serverInfo = await startPreviewCaptureServer(tempRoot);
    cdp = await launchCdpSession(chromeBin, userDataDir);

    // Verify AS2 structured output:
    serverInfo.state.clear();
    cdp.consoleErrors.length = 0;
    cdp.uncaughtExceptions.length = 0;
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-as2/index.html`);
    await waitForCondition(() => serverInfo.state.events.length >= 6, 'AS2 structured traces');
    assert.deepEqual(
      serverInfo.state.events.map(e => e.message),
      [
        '[\n  [0, 1, 0],\n  [1, 1, 1],\n  [0, 0, 0]\n]',
        '[\n  [\n    [0, 1, 0],\n    [1, 1, 1],\n    [0, 0, 0]\n  ],\n  [\n    [1, 1, 1, 1]\n  ]\n]',
        '{\n  name: "T",\n  cells: [\n    [1, 0],\n    [1, 1]\n  ]\n}',
        'Piece {\n  kind: "L",\n  matrix: [\n    [1, 0],\n    [1, 1]\n  ]\n}',
        'Badge(gold)',
        'COERCE:1,2|[object Object]',
      ],
    );
    assert.deepEqual(cdp.consoleErrors, []);
    assert.deepEqual(cdp.uncaughtExceptions, []);

    // Verify AS3 structured output:
    serverInfo.state.clear();
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-as3/index.html`);
    await waitForCondition(() => serverInfo.state.events.length >= 6, 'AS3 structured traces');
    assert.deepEqual(
      serverInfo.state.events.map(e => e.message),
      [
        'Vector.<Vector.<int>>[\n  Vector.<int>[0, 1, 0],\n  Vector.<int>[1, 1, 1]\n]',
        'Dictionary { "hp" => 100 }',
        '(x=3, y=4)',
        '<item id="7"/>',
        'app.BoardState {\n  level: 3,\n  rows: [\n    [1, 0],\n    [0, 1]\n  ],\n  getterCalls: 0,\n  computed: [Getter]\n} 0',
        'AS3_COERCE:1,2|[object Object]',
      ],
    );
    assert.deepEqual(cdp.consoleErrors, []);
    assert.deepEqual(cdp.uncaughtExceptions, []);

    // Verify JS console.log / console.warn structured output + mapped source locations:
    serverInfo.state.clear();
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-js/index.html`);
    await waitForCondition(() => serverInfo.state.events.length >= 2, 'JS console structured events');
    assert.deepEqual(
      serverInfo.state.events.map(e => ({ kind: e.kind, message: e.message, sourcePath: e.sourcePath, line: e.line })),
      [
        {
          kind: 'trace',
          message: 'JS_MAT [\n  [1, 2],\n  [3, 4]\n]',
          sourcePath: 'Scene 1 / frame 1',
          line: 2,
        },
        {
          kind: 'warning',
          message: '{\n  warnCode: 7,\n  nested: { ok: true }\n}',
          sourcePath: 'Scene 1 / frame 1',
          line: 3,
        },
      ],
    );
    assert.deepEqual(cdp.uncaughtExceptions, []);
  } finally {
    if (cdp) {
      try {
        cdp.ws.close();
      } catch (_) {}
      cdp.chromeProc.kill('SIGKILL');
    }
    if (serverInfo) {
      await new Promise(resolve => serverInfo.server.close(resolve));
    }
    fs.rmSync(tempRoot, { recursive: true, force: true });
    fs.rmSync(userDataDir, { recursive: true, force: true });
  }
});
