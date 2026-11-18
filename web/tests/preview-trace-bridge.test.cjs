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
    staleRejections: 0,
    activeRunId: null,
    events: [],
    rejectOutput: false,
    clear() {
      this.outputRequests = 0;
      this.staleRejections = 0;
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
          if (state.activeRunId && parsed.runId !== state.activeRunId) {
            state.staleRejections += 1;
            res.writeHead(200, {
              'Content-Type': 'application/json; charset=utf-8',
              'Cache-Control': 'no-store, no-cache, must-revalidate',
              'X-NeoFlash-Session-Stale': '1',
            });
            res.end('{"stale":true}\n');
            return;
          }
          if (Array.isArray(parsed.events)) {
            for (const ev of parsed.events) {
              state.events.push({
                ...ev,
                session: ev.session || parsed.session || null,
                runId: ev.runId || parsed.runId || null,
              });
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
        'var lFigure = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);',
        'var f = lFigure.join("");',
        'trace(f);',
        'set("dynSlot", 19); var sIdx = 0; var bArr = [1, 20]; bArr[sIdx++] |= 6; var delOk = delete bArr[1];',
        'trace(("" + [undefined, null, 1]) + "|" + ("10" lt "2") + "|" + eval("dyn" + "Slot") + "|" + String(eval("a..b")) + "|" + bArr[0] + "|" + delOk + "|" + (1 in bArr) + "|" + (-1 >>> 0) + "|" + (4294967297 | 0) + "|" + (~0));',
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
            '      trace(("" + [bs, null]) + "|" + ((-3 >>> 1) == 2147483646) + "|" + (([1] + [2]) == "12"));',
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
    await waitForCondition(() => serverInfo.state.events.length >= 8, 'AS2 structured traces');
    assert.deepEqual(
      serverInfo.state.events.map(e => e.message),
      [
        '[\n  [0, 1, 0],\n  [1, 1, 1],\n  [0, 0, 0]\n]',
        '[\n  [\n    [0, 1, 0],\n    [1, 1, 1],\n    [0, 0, 0]\n  ],\n  [\n    [1, 1, 1, 1]\n  ]\n]',
        '{\n  name: "T",\n  cells: [\n    [1, 0],\n    [1, 1]\n  ]\n}',
        'Piece {\n  kind: "L",\n  matrix: [\n    [1, 0],\n    [1, 1]\n  ]\n}',
        'Badge(gold)',
        'COERCE:1,2|[object Object]',
        '0,1,00,1,00,1,00,1,0',
        'undefined,null,1|true|19|undefined|7|true|false|4294967295|1|-1',
      ],
    );
    assert.deepEqual(cdp.consoleErrors, []);
    assert.deepEqual(cdp.uncaughtExceptions, []);

    // Verify AS3 structured output:
    serverInfo.state.clear();
    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-as3/index.html`);
    await waitForCondition(() => serverInfo.state.events.length >= 7, 'AS3 structured traces');
    assert.deepEqual(
      serverInfo.state.events.map(e => e.message),
      [
        'Vector.<Vector.<int>>[\n  Vector.<int>[0, 1, 0],\n  Vector.<int>[1, 1, 1]\n]',
        'Dictionary { "hp" => 100 }',
        '(x=3, y=4)',
        '<item id="7"/>',
        'app.BoardState {\n  level: 3,\n  rows: [\n    [1, 0],\n    [0, 1]\n  ],\n  getterCalls: 0,\n  computed: [Getter]\n} 0',
        'AS3_COERCE:1,2|[object Object]',
        '[object BoardState],|true|true',
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

test('T-382: Safari/WebKit, Chrome/V8, Firefox stack & ErrorEvent SourceMap resolution skips internal host frames and humanizes .length() / __nf errors', () => {
  const vm = require('node:vm');
  const previewApi = require('../preview.js');

  const bin = ensureFreshExportBinary();
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-t382-sourcemap-'));
  try {
    // Export a real multi-layer tetris-like AS2 + class SourceUnit project to extract genuine SourceMaps!
    const figuresLines = Array.from({ length: 17 }, (_, i) => `// setup line ${i + 1}`);
    figuresLines.push('function rotateFigure(figure) {'); // line 18
    figuresLines.push('  var n = figure.length();'); // line 19
    figuresLines.push('  return n;'); // line 20
    figuresLines.push('}'); // line 21
    figuresLines.push('rotateFigure([[0, 1], [1, 1]]);'); // line 22

    const proj = {
      format_version: 13,
      name: 'TetrisSafariMap',
      width: 320,
      height: 240,
      fps: 24,
      background: [16, 18, 24, 255],
      script_language: 'action_script2',
      root: 'root',
      symbols: [
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [
              { id: 'layer-field', name: 'field', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] },
              { id: 'layer-figures', name: 'figures', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] },
            ],
            scripts: [
              { layer: 'layer-field', frame: 0, script: { language: 'action_script2', source: 'var cols = 10;\nvar rows = 20;\n' } },
              { layer: 'layer-figures', frame: 0, script: { language: 'action_script2', source: figuresLines.join('\n') } },
            ],
          },
        },
      ],
      sources: [
        {
          id: 'src-rotator',
          path: 'Rotator.as',
          language: 'action_script2',
          source: [
            'class Rotator {',
            '  static function spin(matrix:Array):Number {',
            '    return matrix.length();',
            '  }',
            '}',
          ].join('\n'),
        },
      ],
      resources: [],
    };
    const pPath = path.join(tempRoot, 'tetris.nflash');
    const outDir = path.join(tempRoot, 'preview');
    fs.writeFileSync(pPath, JSON.stringify(proj, null, 2));
    execFileSync(bin, ['--runtime', runtimeDir, pPath, '--output', outDir, '--preview'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });

    const bundleCode = fs.readFileSync(path.join(outDir, 'bundle.js'), 'utf8');
    const sandbox = { globalThis: {} };
    sandbox.globalThis = sandbox;
    vm.runInNewContext(bundleCode, sandbox);
    const bundle = sandbox.NeoFlashBundle;
    assert.ok(bundle && Array.isArray(bundle.scripts) && bundle.scripts.length >= 2);
    assert.ok(bundle.program && bundle.program.source_map, 'exported bundle includes real SourceMap v3');

    // Decode the real SourceMap v3 VLQ mappings from `bundle.program.source_map`:
    const programLocs = previewApi.decodeSourceMapLocations(bundle.program.source_map);
    const rotatorLine3 = programLocs.find(l => l.source === 'Rotator.as' && l.line === 3);
    assert.ok(rotatorLine3, 'VLQ SourceMap decoder must locate Rotator.as Line 3');

    const figuresUnit = bundle.scripts[1];
    figuresUnit.__domBodyStartLine = 260;
    figuresUnit.__domBodyEndLine = 260 + figuresUnit.code.split('\n').length;
    const figLine19Loc = figuresUnit.source_locations.find(l => l.line === 19);
    assert.ok(figLine19Loc, 'figures script unit maps authored Line 19');
    const safariDomLine = figuresUnit.__domBodyStartLine + figLine19Loc.generated_line - 1;

    // 1. Safari / WebKit stack frames (`func@url:line:col`, `global code@url:line:col`, `[native code]`):
    //    Top frames are inside `runtime.js` and `preview.js` — resolver MUST skip them and pick
    //    `rotateFigure@http://127.0.0.1:54321/m1/index.html?nf_run=nf-run-9:LINE:COL`!
    const safariErr = new TypeError("figure.length is not a function. (In 'figure.length()', '__nfContainerMethod' is undefined)");
    Object.defineProperty(safariErr, 'stack', {
      value: [
        '__nfContainerMethod@http://127.0.0.1:54321/m1/runtime.js:1420:22',
        'record@http://127.0.0.1:54321/m1/preview.js:582:11',
        '[native code]',
        `rotateFigure@http://127.0.0.1:54321/m1/index.html?nf_run=nf-run-9:${safariDomLine}:${figLine19Loc.generated_column + 2}`,
        `global code@http://127.0.0.1:54321/m1/index.html?nf_run=nf-run-9:${safariDomLine + 3}:5`,
      ].join('\n'),
    });
    const safariResolved = previewApi.resolveErrorLocation(safariErr, {
      bundle,
      pageUrl: 'http://127.0.0.1:54321/m1/index.html?nf_run=nf-run-9',
    });
    assert.equal(safariResolved.source, 'Scene 1 / frame 1');
    assert.equal(safariResolved.line, 19);
    assert.equal(safariResolved.sourceLine, 'var n = figure.length();');

    // 2. Safari `bundle.js` / `module code@` stack mapped strictly via SourceMap v3 VLQ:
    const safariBundleErr = new TypeError('matrix.length is not a function');
    Object.defineProperty(safariBundleErr, 'stack', {
      value: [
        '__nfContainerMethod@http://127.0.0.1:54321/m1/runtime.js:1420:22',
        `spin@http://127.0.0.1:54321/m1/bundle.js:${rotatorLine3.generated_line}:${rotatorLine3.generated_column}`,
        'module code@http://127.0.0.1:54321/m1/bundle.js:1:1',
      ].join('\n'),
    });
    const bundleResolved = previewApi.resolveErrorLocation(safariBundleErr, {
      sourceUnits: [],
      sourceMap: bundle.program.source_map,
    });
    assert.equal(bundleResolved.source, 'Rotator.as');
    assert.equal(bundleResolved.line, 3);
    assert.equal(bundleResolved.sourceLine, 'return matrix.length();');

    // 3. Chrome / V8 stack frames (`at fn (url:line:col)`): skips `runtime.js`, maps `neoflash-script-1.js`:
    delete figuresUnit.__domBodyStartLine;
    delete figuresUnit.__domBodyEndLine;
    const chromeErr = new TypeError('figure.length is not a function');
    Object.defineProperty(chromeErr, 'stack', {
      value: [
        'TypeError: figure.length is not a function',
        '    at __nfContainerMethod (http://127.0.0.1:54321/m1/runtime.js:1420:22)',
        `    at rotateFigure (neoflash-script-1.js:${figLine19Loc.generated_line}:${figLine19Loc.generated_column})`,
      ].join('\n'),
    });
    const chromeResolved = previewApi.resolveErrorLocation(chromeErr, { bundle });
    assert.equal(chromeResolved.source, 'Scene 1 / frame 1');
    assert.equal(chromeResolved.line, 19);
    assert.equal(chromeResolved.sourceLine, 'var n = figure.length();');

    // 4. `ErrorEvent` (`filename` / `lineno` / `colno`) fallback when `event.error` has no stack (e.g. Safari parse SyntaxError):
    const parseErrEvent = {
      message: "SyntaxError: Unexpected token ';'",
      filename: 'http://127.0.0.1:54321/m1/bundle.js',
      lineno: rotatorLine3.generated_line,
      colno: rotatorLine3.generated_column,
      error: null,
    };
    const eventResolved = previewApi.resolveErrorLocation(parseErrEvent, {
      sourceUnits: [],
      sourceMap: bundle.program.source_map,
    });
    assert.equal(eventResolved.source, 'Rotator.as');
    assert.equal(eventResolved.line, 3);

    // 5. Humanization cleans internal `__nf...`, Safari `Reflect.apply`, and V8 `Function.prototype.apply` leaks:
    const humanized = previewApi.humanizeRuntimeErrorMessage(
      "TypeError: figure.length is not a function. (In 'figure.length()', '__nfContainerMethod' is undefined)",
    );
    assert.ok(!humanized.includes('__nf'), `internal __nf must be stripped: ${humanized}`);
    assert.match(humanized, /without '\(\)'/);

    const safariReflectLength = previewApi.humanizeRuntimeErrorMessage(
      'Scene 1 / frame 1: TypeError: Reflect.apply requires the first argument be a function',
      'var k = "abc".length();',
    );
    assert.ok(!safariReflectLength.includes('Reflect.apply'), safariReflectLength);
    assert.ok(safariReflectLength.includes("'length' is a property — use '.length' without '()'"), safariReflectLength);

    const v8ApplyLength = previewApi.humanizeRuntimeErrorMessage(
      'TypeError: Function.prototype.apply was called on 3, which is a number and not a function',
      'var k = ({ length: 3 }).length();',
    );
    assert.ok(!v8ApplyLength.includes('Function.prototype.apply'), v8ApplyLength);
    assert.ok(v8ApplyLength.includes("'length' is a property — use '.length' without '()'"), v8ApplyLength);

    const safariReflectNonFn = previewApi.humanizeRuntimeErrorMessage(
      'TypeError: Reflect.apply requires the first argument be a function',
      'mc.foo();',
    );
    assert.ok(!safariReflectNonFn.includes('Reflect.apply'), safariReflectNonFn);
    assert.equal(safariReflectNonFn, "TypeError: 'foo' is not a function (attempted to call a non-function value)");
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test('T-379 & T-382: Test Movie relays figure.length() line + sourceLine, catches uncaught error & unhandledrejection, and halts stale nf_run tabs on X-NeoFlash-Session-Stale', async t => {
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
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-t379-t382-'));
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-t379-chrome-'));
  let serverInfo = null;
  let cdp = null;

  try {
    // 1) Multi-layer AS2 project (`field` + `figures`) where `figures` Line 19 calls `figure.length()`:
    const figuresSource = [
      ...Array.from({ length: 17 }, (_, i) => `// figures header ${i + 1}`),
      'function rotateFigure(figure) {',
      '  var n = figure.length();',
      '  return n;',
      '}',
      'rotateFigure([[0, 1, 0], [1, 1, 1]]);',
    ].join('\n');

    const tetrisProj = {
      format_version: 13,
      name: 'TetrisLengthBug',
      width: 320,
      height: 240,
      fps: 24,
      background: [20, 22, 28, 255],
      script_language: 'action_script2',
      root: 'root',
      symbols: [
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [
              { id: 'layer-field', name: 'field', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] },
              { id: 'layer-figures', name: 'figures', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] },
            ],
            scripts: [
              { layer: 'layer-field', frame: 0, script: { language: 'action_script2', source: 'stop();\ntrace("FIELD_READY");\n' } },
              { layer: 'layer-figures', frame: 0, script: { language: 'action_script2', source: figuresSource } },
            ],
          },
        },
      ],
      sources: [],
      resources: [],
    };

    // 2) Continuous 1-frame loop project to verify `nf_run` stale-session isolation & player pause:
    const loopProj = {
      format_version: 6,
      name: 'RunIsolationLoop',
      width: 320,
      height: 200,
      fps: 30,
      background: [20, 24, 30, 255],
      script_language: 'java_script',
      root: 'root',
      symbols: [
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [
              { id: 'layer-1', name: 'Main', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] },
            ],
            scripts: [
              {
                frame: 0,
                script: {
                  language: 'java_script',
                  source: [
                    'stop();',
                    'trace("LOOP_BOOT");',
                    'Promise.reject(new Error("ASYNC_REJECT_LINE_3"));',
                    'let tick = 0;',
                    'setInterval(() => {',
                    '  tick += 1;',
                    '  trace("LOOP_TICK:" + tick);',
                    '}, 25);',
                  ].join('\n'),
                },
              },
            ],
          },
        },
      ],
      sources: [],
      resources: [],
    };

    // 3) JS strict-mode check (`"use strict";` preserved in DOM `<script>` compilation so undeclared `tFigure = [[1]]` throws on Line 4):
    const strictJsProj = {
      format_version: 6,
      name: 'StrictModeDomCheck',
      width: 320,
      height: 200,
      fps: 24,
      background: [20, 24, 30, 255],
      script_language: 'java_script',
      root: 'root',
      symbols: [
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [{ id: 'layer-1', name: 'Main', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
            scripts: [
              {
                frame: 0,
                script: {
                  language: 'java_script',
                  source: [
                    'stop();',
                    'let TFigure = [[0, 1, 0]];',
                    'trace("STRICT_BOOT");',
                    'tFigure = [[1]];',
                  ].join('\n'),
                },
              },
            ],
          },
        },
      ],
      sources: [],
      resources: [],
    };

    // 4) AS2 non-Array `.length()` (`"abc".length()` invoking `__nfStringApply` / `Reflect.apply`) humanization check:
    const strLenProj = {
      format_version: 6,
      name: 'StringLengthCallBug',
      width: 320,
      height: 200,
      fps: 24,
      background: [20, 24, 30, 255],
      script_language: 'action_script2',
      root: 'root',
      symbols: [
        {
          id: 'root',
          name: 'Scene 1',
          kind: 'movie_clip',
          timeline: {
            frame_count: 1,
            layers: [{ id: 'layer-1', name: 'Main', visible: true, locked: false, keyframes: [{ frame: 0, label: '', tween: null, elements: [] }] }],
            scripts: [
              {
                frame: 0,
                script: {
                  language: 'action_script2',
                  source: [
                    'stop();',
                    'var label = "abc";',
                    'var count = label.length();',
                  ].join('\n'),
                },
              },
            ],
          },
        },
      ],
      sources: [],
      resources: [],
    };

    for (const [slug, proj] of [['tetris', tetrisProj], ['loop', loopProj], ['strict-js', strictJsProj], ['str-len', strLenProj]]) {
      const pPath = path.join(tempRoot, `${slug}.nflash`);
      fs.writeFileSync(pPath, JSON.stringify(proj, null, 2));
      execFileSync(bin, ['--runtime', runtimeDir, pPath, '--output', path.join(tempRoot, `preview-${slug}`), '--preview'], {
        cwd: repoRoot,
        encoding: 'utf8',
      });
    }

    serverInfo = await startPreviewCaptureServer(tempRoot);
    cdp = await launchCdpSession(chromeBin, userDataDir);

    // A. Launch `preview-tetris` with `?nf_run=nf-run-101` -> `figure.length()` maps to exact Line 19 + `sourceLine`!
    serverInfo.state.clear();
    serverInfo.state.activeRunId = 'nf-run-101';
    cdp.consoleErrors.length = 0;
    cdp.uncaughtExceptions.length = 0;

    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-tetris/index.html?nf_run=nf-run-101`);
    await waitForCondition(
      () => serverInfo.state.events.some(e => e.kind === 'error'),
      'tetris figure.length() runtime error relayed',
    );
    const errEvents = serverInfo.state.events.filter(e => e.kind === 'error');
    assert.equal(errEvents.length, 1, 'runtime error must be reported exactly once (no window.onerror duplicate)');
    const tetrisErr = errEvents[0];
    assert.equal(tetrisErr.runId, 'nf-run-101');
    assert.equal(tetrisErr.sourcePath, 'Scene 1 / frame 1');
    assert.equal(tetrisErr.line, 19, 'must map figure.length() to exact Line 19');
    assert.equal(tetrisErr.sourceLine, 'var n = figure.length();');
    assert.match(tetrisErr.message, /without '\(\)'/);
    assert.ok(!tetrisErr.message.includes('__nf'), `must not leak __nf helper names: ${tetrisErr.message}`);
    const leakedGlobals = await cdp.evalJson(
      `({ hasSlot: Object.prototype.hasOwnProperty.call(window, '__nfCompiledSlot'), hasSuppress: Object.prototype.hasOwnProperty.call(window, '__neoflashSuppressWindowError') })`,
    );
    assert.equal(leakedGlobals.hasSlot, false, '__nfCompiledSlot must be deleted after DOM script compilation');
    assert.equal(leakedGlobals.hasSuppress, false, '__neoflashSuppressWindowError must be deleted after DOM script compilation');

    // A2. Launch `preview-strict-js` -> `"use strict"` catches misspelled undeclared `tFigure = [[1]];` on exact Line 4!
    serverInfo.state.clear();
    serverInfo.state.activeRunId = 'nf-run-102';
    cdp.consoleErrors.length = 0;
    cdp.uncaughtExceptions.length = 0;

    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-strict-js/index.html?nf_run=nf-run-102`);
    await waitForCondition(
      () => serverInfo.state.events.some(e => e.kind === 'error'),
      'strict-mode ReferenceError on tFigure = [[1]] relayed',
    );
    const strictErr = serverInfo.state.events.find(e => e.kind === 'error');
    assert.equal(strictErr.sourcePath, 'Scene 1 / frame 1');
    assert.equal(strictErr.line, 4, 'must map strict-mode undeclared assignment to exact Line 4');
    assert.equal(strictErr.sourceLine, 'tFigure = [[1]];');
    assert.match(strictErr.message, /tFigure is not defined/);
    const pollutedTFigure = await cdp.evalJson(`Object.prototype.hasOwnProperty.call(window, 'tFigure')`);
    assert.equal(pollutedTFigure, false, 'strict mode must prevent window.tFigure pollution');

    // A3. Launch `preview-str-len` -> `"abc".length()` (`__nfStringApply` / `Reflect.apply`) relays humanized `.length` message on Line 3!
    serverInfo.state.clear();
    serverInfo.state.activeRunId = 'nf-run-103';
    cdp.consoleErrors.length = 0;
    cdp.uncaughtExceptions.length = 0;

    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-str-len/index.html?nf_run=nf-run-103`);
    await waitForCondition(
      () => serverInfo.state.events.some(e => e.kind === 'error'),
      'non-Array "abc".length() runtime error relayed',
    );
    const strLenErr = serverInfo.state.events.find(e => e.kind === 'error');
    assert.equal(strLenErr.sourcePath, 'Scene 1 / frame 1');
    assert.equal(strLenErr.line, 3);
    assert.equal(strLenErr.sourceLine, 'var count = label.length();');
    assert.ok(!strLenErr.message.includes('Reflect.apply'), strLenErr.message);
    assert.ok(!strLenErr.message.includes('Function.prototype.apply'), strLenErr.message);
    assert.ok(strLenErr.message.includes("'length' is a property — use '.length' without '()'"), strLenErr.message);

    // B. Launch `preview-loop` with `?nf_run=nf-run-201`:
    //    1) Relays `unhandledrejection` mapped to Line 3 (`Promise.reject(new Error("ASYNC_REJECT_LINE_3"));`);
    //    2) When server advances `activeRunId` to `nf-run-202`, responds with `X-NeoFlash-Session-Stale: 1`,
    //       causing the stale tab to disable its relay AND pause `NeoFlashPlayer`!
    serverInfo.state.clear();
    serverInfo.state.activeRunId = 'nf-run-201';
    cdp.consoleErrors.length = 0;
    cdp.uncaughtExceptions.length = 0;

    await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/preview-loop/index.html?nf_run=nf-run-201`);
    await waitForCondition(
      () =>
        serverInfo.state.events.some(e => e.kind === 'error' && e.message.includes('ASYNC_REJECT_LINE_3')) &&
        serverInfo.state.events.some(e => e.message.startsWith('LOOP_TICK:')),
      'unhandledrejection + onEnterFrame loop running',
    );
    const rejectEv = serverInfo.state.events.find(e => e.kind === 'error' && e.message.includes('ASYNC_REJECT_LINE_3'));
    assert.equal(rejectEv.sourcePath, 'Scene 1 / frame 1');
    assert.equal(rejectEv.line, 3);

    // Advance server to `nf-run-202` while `nf-run-201` tab is still open and ticking `onEnterFrame`!
    serverInfo.state.activeRunId = 'nf-run-202';
    await waitForCondition(
      () => serverInfo.state.staleRejections >= 1,
      'server rejects stale nf-run-201 post with X-NeoFlash-Session-Stale: 1',
    );
    await waitForCondition(
      async () => {
        const relayDisabled = await cdp.evalJson('Boolean(window.__neoflashOutputRelay && window.__neoflashOutputRelay.disabled)');
        return relayDisabled;
      },
      'stale preview tab disables output relay immediately',
    );
    const requestsAfterDisable = serverInfo.state.outputRequests;
    await new Promise(resolve => setTimeout(resolve, 180));
    assert.equal(
      serverInfo.state.outputRequests,
      requestsAfterDisable,
      'stale browser tab must emit ZERO further /__neoflash/output requests once marked stale',
    );
    assert.equal(
      serverInfo.state.events.filter(e => e.runId === 'nf-run-202').length,
      0,
    );
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
