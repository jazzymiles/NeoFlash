// End-to-end unit, VM, and headless Chrome integration tests for T-016:
// Loadable HTML5 publication modules (`NeoFlash.mount(container, options)`,
// generated `module.mjs` ES module entry, shared WASM compilation, per-instance
// isolation, `loadPublish('child/module.mjs')`, and deterministic `destroy()`).
const assert = require('node:assert/strict');
const test = require('node:test');
const crypto = require('node:crypto');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const { execFileSync, spawn } = require('node:child_process');
const { pathToFileURL } = require('node:url');

const repoRoot = path.resolve(__dirname, '../..');
const runtimeDir = path.join(repoRoot, 'target/web-runtime');
const exportBin = path.join(repoRoot, 'target/debug/neoflash-export');

const eventsMod = require('../events.js');
const byteArrayMod = require('../byte-array.js');
const netMod = require('../net.js');
const loaderMod = require('../loader.js');

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
  execFileSync('cargo', ['build', '-p', 'neoflash-export', '--bin', 'neoflash-export'], {
    cwd: repoRoot,
    stdio: 'inherit',
  });
  return exportBin;
}

function makeTransform(x = 0, y = 0) {
  return { x, y, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 };
}

function buildModuleProject({
  name,
  width = 320,
  height = 220,
  fps = 24,
  frameCount = 4,
  language = 'action_script2',
  scripts = [],
  fill = [40, 140, 240, 255],
  publishSettingsJson = '',
}) {
  return {
    format_version: publishSettingsJson ? 10 : 3,
    name,
    width,
    height,
    fps,
    background: [20, 24, 34, 255],
    root: 'root',
    publish_settings_json: publishSettingsJson || undefined,
    resources: [],
    symbols: [
      {
        id: 'root',
        name: 'Scene 1',
        kind: 'movie_clip',
        timeline: {
          frame_count: frameCount,
          layers: [
            {
              id: 'layer_0',
              name: 'Art',
              visible: true,
              locked: false,
              keyframes: Array.from({ length: frameCount }, (_, idx) => ({
                frame: idx,
                label: idx === 0 ? 'start' : `f${idx + 1}`,
                tween: null,
                elements: [
                  {
                    id: `box_${idx}`,
                    name: 'box',
                    transform: makeTransform(16 + idx * 24, 24),
                    kind: {
                      type: 'rectangle',
                      width: 64,
                      height: 48,
                      fill,
                      stroke: [255, 255, 255, 255],
                      stroke_width: 2,
                    },
                    scripts: [],
                  },
                ],
              })),
            },
          ],
          scripts: scripts.map(({ frame, source }) => ({
            frame,
            script: { language, source },
          })),
        },
      },
    ],
  };
}

function startStaticServer(rootDir) {
  const mimeByExt = {
    '.html': 'text/html; charset=utf-8',
    '.css': 'text/css; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.mjs': 'text/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.png': 'image/png',
    '.txt': 'text/plain; charset=utf-8',
  };
  const sockets = new Set();
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
    res.writeHead(200, {
      'Content-Type': mimeByExt[ext] || 'application/octet-stream',
      'Cache-Control': 'no-store',
    });
    fs.createReadStream(candidate).pipe(res);
  });
  server.on('connection', (socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
  });
  server.on('upgrade', (req, socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    const key = req.headers['sec-websocket-key'] || '';
    const accept = crypto
      .createHash('sha1')
      .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
      .digest('base64');
    socket.write(
      `HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port, sockets }));
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
    { stdio: ['ignore', 'pipe', 'pipe'] },
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

  const close = async () => {
    try {
      ws.close();
    } catch (_) {}
    chromeProc.kill('SIGKILL');
    await new Promise((resolve) => chromeProc.once('exit', resolve));
  };

  return { navigateAndWait, evalJson, errors, close };
}

test('T-016: loader.js redirects child/module.mjs to sibling child/bundle.js and supports rootParameters', async () => {
  const childProject = buildModuleProject({
    name: 'ChildModule',
    frameCount: 2,
    scripts: [{ frame: 0, source: 'stop();' }],
  });
  const childBundleJs = `globalThis.NeoFlashBundle = ${JSON.stringify({ project: childProject, scripts: [], warnings: [] })};\n`;
  const childModuleMjs = [
    'export const manifest = Object.freeze({ name: "ChildModule", bundle: "bundle.js" });',
    'export async function loadBundle() {}',
    'export async function ensureRuntime() {}',
    'export async function mount(container, options = {}) {}',
    'export default mount;',
  ].join('\n');

  const requestedUrls = [];
  const origFetch = globalThis.fetch;
  globalThis.fetch = async (url) => {
    const key = String(url);
    requestedUrls.push(key);
    if (key.endsWith('child/module.mjs')) {
      return new Response(childModuleMjs, { status: 200, headers: { 'content-type': 'text/javascript' } });
    }
    if (key.endsWith('child/bundle.js')) {
      return new Response(childBundleJs, { status: 200, headers: { 'content-type': 'text/javascript' } });
    }
    return new Response('not found', { status: 404 });
  };

  try {
    const wasmCtx = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'), 'utf8'), wasmCtx);
    const wasm = vm.runInContext('NeoFlashWasm', wasmCtx);
    wasmCtx.wasmBytes = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm'));
    await vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', wasmCtx);

    const parentProject = buildModuleProject({ name: 'HostApp', frameCount: 2 });
    const player = new wasm.WebPlayer(JSON.stringify(parentProject));
    const binary = byteArrayMod.create({ wasm });
    const net = netMod.create({
      events: eventsMod,
      binary,
      resources: [],
      onError: (err) => {
        throw err;
      },
    });

    let mountedBundleName = null;
    const rootDisplay = { alive: true };
    const refs = new WeakMap();
    refs.set(rootDisplay, { objectRef: { Authored: [] }, key: JSON.stringify({ Authored: [] }) });

    const displayStub = {
      nodes: () => [{ object_ref: { Authored: [] }, authored_path: [] }],
      alive: (obj) => Boolean(obj && refs.has(obj)),
      descriptor: () => ({ owner: 'movie_clip' }),
    };

    const loaderHost = loaderMod.create({
      player,
      events: eventsMod,
      net,
      binary,
      project: parentProject,
      rootParameters: { slotId: 'alpha-slot', count: 7 },
      rootUrl: 'https://app.example/widgets/host/module.mjs',
      display: displayStub,
      proxyForRef: (ref) => {
        const proxy = { ref };
        refs.set(proxy, { objectRef: ref, key: JSON.stringify(ref) });
        return proxy;
      },
      referenceOf: (target) => refs.get(target),
      getRoot: () => rootDisplay,
      getRootDisplay: () => rootDisplay,
      getFrameRate: () => 24,
      getStageSize: () => ({ width: 320, height: 220 }),
      mountPublishedBundle: (record, bundle) => {
        mountedBundleName = bundle.project?.name || null;
        return {
          contentRef: record.ref,
          events: [],
          width: bundle.project.width,
          height: bundle.project.height,
          frameRate: bundle.project.fps,
        };
      },
      unloadPublishedBundle: () => ({ events: [] }),
    });

    const rootInfo = loaderHost.infoForDisplay(rootDisplay);
    assert.ok(rootInfo, 'root LoaderInfo available');
    assert.deepEqual(
      { ...rootInfo.parameters },
      { slotId: 'alpha-slot', count: '7' },
      'rootParameters populate root.loaderInfo.parameters',
    );
    assert.equal(rootInfo.url, 'https://app.example/widgets/host/module.mjs');

    await loaderHost.loadPublish('https://app.example/widgets/child/module.mjs', new loaderHost.Loader());
    assert.equal(mountedBundleName, 'ChildModule');
    assert.deepEqual(requestedUrls, [
      'https://app.example/widgets/child/module.mjs',
      'https://app.example/widgets/child/bundle.js',
    ]);

    loaderHost.dispose();
    net.dispose?.();
    player.free?.();
  } finally {
    globalThis.fetch = origFetch;
  }
});

test('T-016: generated module.mjs exports manifest and loadBundle() in Node without touching globalThis.NeoFlashBundle', async () => {
  const bin = ensureExportBinary();
  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-mod-esm-'));
  try {
    const projPath = path.join(tmpRoot, 'widget.nflash');
    const outDir = path.join(tmpRoot, 'dist');
    const project = buildModuleProject({
      name: 'ESM Card',
      width: 480,
      height: 270,
      fps: 30,
      frameCount: 3,
      scripts: [{ frame: 0, source: 'trace("esm ready"); stop();' }],
    });
    fs.writeFileSync(projPath, JSON.stringify(project, null, 2));

    execFileSync(bin, [projPath, '--output', outDir, '--size-level', 'compact', '--clean'], {
      cwd: repoRoot,
      stdio: 'pipe',
    });

    const moduleMjsPath = path.join(outDir, 'module.mjs');
    assert.ok(fs.existsSync(moduleMjsPath), 'module.mjs generated alongside index.html and bundle.js');

    const sentinel = { untouched: true };
    globalThis.NeoFlashBundle = sentinel;
    try {
      const mod = await import(pathToFileURL(moduleMjsPath).href);
      assert.equal(typeof mod.mount, 'function');
      assert.equal(typeof mod.ensureRuntime, 'function');
      assert.equal(typeof mod.loadBundle, 'function');
      assert.equal(mod.default, mod.mount);
      assert.equal(mod.manifest.name, 'ESM Card');
      assert.equal(mod.manifest.width, 480);
      assert.equal(mod.manifest.height, 270);
      assert.equal(mod.manifest.fps, 30);
      assert.equal(mod.manifest.renderMode, 'canvas');
      assert.equal(mod.manifest.runtimeLevel, 'compact');
      assert.equal(mod.manifest.bundle, 'bundle.js');
      assert.equal(mod.manifest.wasmUrl, 'neoflash_web_bg.wasm');

      const bundle = await mod.loadBundle();
      assert.equal(bundle.project.name, 'ESM Card');
      assert.equal(bundle.project.width, 480);
      assert.equal(
        globalThis.NeoFlashBundle,
        sentinel,
        'loadBundle() must parse bundle.js without mutating globalThis.NeoFlashBundle',
      );
    } finally {
      delete globalThis.NeoFlashBundle;
    }
  } finally {
    fs.rmSync(tmpRoot, { recursive: true, force: true });
  }
});

test('T-016: headless Chrome mounts multiple independent HTML5 modules (AS2 + AS3 + DOM + Shared runtime) on one page, compiles WASM once, supports loadPublish("child/module.mjs"), and destroys cleanly', async (t) => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Chrome/Chromium binary not available on this machine');
    return;
  }

  const bin = ensureExportBinary();
  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-chrome-modules-'));
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-chrome-profile-'));

  let serverHandle = null;
  let cdp = null;

  try {
    // 1. Export shared runtime once into `shared-rt/`.
    const sharedRtDir = path.join(tmpRoot, 'shared-rt');
    execFileSync(bin, ['--shared-runtime', sharedRtDir], { cwd: repoRoot, stdio: 'pipe' });

    // 2. Export Module A: interactive AS2 4-frame module using Shared runtime (`../shared-rt`).
    const projAPath = path.join(tmpRoot, 'mod-a.nflash');
    const projA = buildModuleProject({
      name: 'ModuleA_AS2',
      width: 300,
      height: 180,
      fps: 20,
      frameCount: 4,
      language: 'action_script2',
      fill: [235, 85, 65, 255],
      scripts: [
        { frame: 0, source: 'this.visits = (this.visits || 0) + 1; trace("A_F1:" + _root.greeting + ":" + this.visits);' },
        { frame: 1, source: 'trace("A_F2");' },
        { frame: 2, source: 'trace("A_F3");' },
        { frame: 3, source: 'trace("A_F4");' },
      ],
    });
    fs.writeFileSync(projAPath, JSON.stringify(projA, null, 2));
    execFileSync(
      bin,
      [projAPath, '--output', path.join(tmpRoot, 'a'), '--size-level', 'shared', '--runtime-url', '../shared-rt', '--clean'],
      { cwd: repoRoot, stdio: 'pipe' },
    );

    // 3. Export Module B: interactive AS3 5-frame module using Shared runtime (`../shared-rt`).
    const projBPath = path.join(tmpRoot, 'mod-b.nflash');
    const projB = buildModuleProject({
      name: 'ModuleB_AS3',
      width: 340,
      height: 200,
      fps: 24,
      frameCount: 5,
      language: 'action_script3',
      fill: [65, 175, 245, 255],
      scripts: [
        {
          frame: 0,
          source: [
            'var self:* = this;',
            'self.counter = (self.counter || 0) + 1;',
            'var badge:String = String(self.loaderInfo.parameters.badge || "none");',
            'trace("B_F1:" + badge + ":" + self.counter);',
          ].join('\n'),
        },
        { frame: 1, source: 'trace("B_F2");' },
        { frame: 2, source: 'trace("B_F3");' },
        { frame: 3, source: 'trace("B_F4");' },
        { frame: 4, source: 'trace("B_F5");' },
      ],
    });
    fs.writeFileSync(projBPath, JSON.stringify(projB, null, 2));
    execFileSync(
      bin,
      [projBPath, '--output', path.join(tmpRoot, 'b'), '--size-level', 'shared', '--runtime-url', '../shared-rt', '--clean'],
      { cwd: repoRoot, stdio: 'pipe' },
    );

    // 4. Export Module C: DOM render mode module using Shared runtime (`../shared-rt`).
    const projCPath = path.join(tmpRoot, 'mod-c-dom.nflash');
    const projC = buildModuleProject({
      name: 'ModuleC_DOM',
      width: 280,
      height: 160,
      fps: 15,
      frameCount: 3,
      language: 'action_script3',
      fill: [90, 210, 130, 255],
      publishSettingsJson: JSON.stringify({ render_mode: 'dom', breakpoints: [] }),
      scripts: [{ frame: 0, source: 'trace("C_DOM_F1"); stop();' }],
    });
    fs.writeFileSync(projCPath, JSON.stringify(projC, null, 2));
    execFileSync(
      bin,
      [
        projCPath,
        '--output',
        path.join(tmpRoot, 'c-dom'),
        '--render-mode',
        'dom',
        '--size-level',
        'shared',
        '--runtime-url',
        '../shared-rt',
        '--clean',
      ],
      { cwd: repoRoot, stdio: 'pipe' },
    );

    // 5. Export Child publication (`child/module.mjs` + `child/bundle.js`) for `loadPublish('child/module.mjs')`.
    const childPath = path.join(tmpRoot, 'child.nflash');
    const childProj = buildModuleProject({
      name: 'ChildLoadedPub',
      width: 120,
      height: 90,
      fps: 24,
      frameCount: 2,
      language: 'action_script2',
      fill: [250, 200, 50, 255],
      scripts: [{ frame: 0, source: 'trace("CHILD_OK:" + this._currentframe); stop();' }],
    });
    fs.writeFileSync(childPath, JSON.stringify(childProj, null, 2));
    execFileSync(bin, [childPath, '--output', path.join(tmpRoot, 'child'), '--size-level', 'full', '--clean'], {
      cwd: repoRoot,
      stdio: 'pipe',
    });

    // 6. Export a standalone `Full` publication to verify 100% backward-compatible `index.html` auto-boot.
    const standalonePath = path.join(tmpRoot, 'standalone.nflash');
    const standaloneProj = buildModuleProject({
      name: 'StandaloneCompat',
      width: 320,
      height: 200,
      fps: 24,
      frameCount: 2,
      language: 'action_script2',
      scripts: [{ frame: 0, source: 'trace("STANDALONE_BOOT_OK"); stop();' }],
    });
    fs.writeFileSync(standalonePath, JSON.stringify(standaloneProj, null, 2));
    execFileSync(bin, [standalonePath, '--output', path.join(tmpRoot, 'standalone'), '--size-level', 'full'], {
      cwd: repoRoot,
      stdio: 'pipe',
    });

    // 7. Write multi-module host page with NO `#stage`, `#viewport`, `#loading`, or `#error` IDs!
    const hostHtml = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>NeoFlash Multi-Module Host</title>
  <style>
    body { margin: 0; display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 12px; padding: 12px; background: #111; }
    .slot { height: 240px; border: 1px solid #333; position: relative; }
  </style>
</head>
<body>
  <div id="slot-a" class="slot"></div>
  <div id="slot-b" class="slot"></div>
  <div id="slot-c" class="slot"></div>
  <script type="module">
    import { mount as mountA, manifest as manifestA } from './a/module.mjs';
    import { mount as mountB, manifest as manifestB } from './b/module.mjs';
    import { mount as mountC, manifest as manifestC } from './c-dom/module.mjs';

    window.__testState = {
      tracesA: [],
      tracesB: [],
      tracesC: [],
      errors: [],
      manifests: { a: manifestA, b: manifestB, c: manifestC },
      mountA,
      ready: false,
    };

    try {
      const [appA, appB, appC] = await Promise.all([
        mountA('#slot-a', {
          autoplay: true,
          clean: true,
          parameters: { greeting: 'hello-from-a' },
          onTrace: (msg) => window.__testState.tracesA.push(msg),
          onError: (err) => window.__testState.errors.push('A:' + String(err?.message || err)),
        }),
        mountB(document.getElementById('slot-b'), {
          autoplay: true,
          clean: false,
          parameters: { badge: 'hero-b' },
          onTrace: (msg) => window.__testState.tracesB.push(msg),
          onError: (err) => window.__testState.errors.push('B:' + String(err?.message || err)),
        }),
        mountC('#slot-c', {
          autoplay: true,
          clean: true,
          onTrace: (msg) => window.__testState.tracesC.push(msg),
          onError: (err) => window.__testState.errors.push('C:' + String(err?.message || err)),
        }),
      ]);
      window.__testState.appA = appA;
      window.__testState.appB = appB;
      window.__testState.appC = appC;
      window.__testState.ready = true;
    } catch (err) {
      window.__testState.bootFailure = String(err?.stack || err);
    }
  </script>
</body>
</html>`;
    fs.writeFileSync(path.join(tmpRoot, 'host.html'), hostHtml);

    serverHandle = await startStaticServer(tmpRoot);
    cdp = await launchCdpSession(chromeBin, userDataDir);

    await cdp.navigateAndWait(`http://127.0.0.1:${serverHandle.port}/host.html`);

    const initial = await cdp.evalJson(`
      new Promise((resolve, reject) => {
        const deadline = Date.now() + 15000;
        const poll = () => {
          const st = window.__testState;
          if (st?.bootFailure) {
            reject(new Error('Mount failed: ' + st.bootFailure));
            return;
          }
          if (st?.ready && st.tracesA.length >= 2 && st.tracesB.length >= 2 && st.tracesC.includes('C_DOM_F1')) {
            resolve({
              hasGlobalBundle: typeof window.NeoFlashBundle !== 'undefined',
              hasGlobalPlayer: typeof window.NeoFlashPlayer !== 'undefined',
              runtimeLoaderSize: window.__neoflashRuntimeLoader?.size || 0,
              sharedWasmSize: window.NeoFlash?.sharedWasmModules?.size || 0,
              scriptTagCount: document.querySelectorAll('script[data-neoflash-runtime]').length,
              tracesA: st.tracesA.slice(),
              tracesB: st.tracesB.slice(),
              tracesC: st.tracesC.slice(),
              errors: st.errors.slice(),
              slotACanvas: Boolean(document.querySelector('#slot-a canvas')),
              slotAHudAbsent: document.querySelector('#slot-a [data-neoflash-role="footer"]') === null,
              slotBCanvas: Boolean(document.querySelector('#slot-b canvas')),
              slotBHudVisible: Boolean(document.querySelector('#slot-b [data-neoflash-role="footer"]')) && !document.querySelector('#slot-b [data-neoflash-role="footer"]').hidden,
              slotCDomStage: Boolean(document.querySelector('#slot-c neoflash-stage[data-render-mode="dom"]')),
              aTotal: st.appA.totalFrames,
              bTotal: st.appB.totalFrames,
              cTotal: st.appC.totalFrames,
              aTargetFps: st.appA.targetFps,
              bTargetFps: st.appB.targetFps,
              aFrozen: Object.isFrozen(st.appA),
            });
            return;
          }
          if (Date.now() > deadline) {
            reject(new Error('Timed out waiting for multi-module mount: ' + JSON.stringify(st)));
            return;
          }
          setTimeout(poll, 30);
        };
        poll();
      })
    `);

    assert.equal(cdp.errors.length, 0, `unexpected browser errors: ${cdp.errors.join('\n')}`);
    assert.deepEqual(initial.errors, []);
    assert.equal(initial.hasGlobalBundle, false, 'module.mjs must not pollute globalThis.NeoFlashBundle');
    assert.equal(initial.hasGlobalPlayer, false, 'NeoFlash.mount must not overwrite globalThis.NeoFlashPlayer');
    assert.equal(initial.runtimeLoaderSize, 1, 'Shared runtime URL loaded once in __neoflashRuntimeLoader');
    assert.equal(initial.sharedWasmSize, 1, 'WebAssembly.Module compiled once and shared across all 3 instances');
    assert.equal(initial.slotACanvas, true);
    assert.equal(initial.slotAHudAbsent, true, 'clean: true omits HUD footer inside #slot-a');
    assert.equal(initial.slotBCanvas, true);
    assert.equal(initial.slotBHudVisible, true, 'clean: false renders scoped HUD inside #slot-b');
    assert.equal(initial.slotCDomStage, true, 'DOM render mode mounts scoped neoflash-stage inside #slot-c');
    assert.equal(initial.aTotal, 4);
    assert.equal(initial.bTotal, 5);
    assert.equal(initial.cTotal, 3);
    assert.equal(initial.aTargetFps, 20);
    assert.equal(initial.bTargetFps, 24);
    assert.equal(initial.aFrozen, true);
    assert.ok(
      initial.tracesA.some((line) => line.startsWith('A_F1:hello-from-a:')),
      `expected AS2 FlashVars greeting in tracesA: ${JSON.stringify(initial.tracesA)}`,
    );
    assert.ok(
      initial.tracesB.some((line) => line.startsWith('B_F1:hero-b:')),
      `expected AS3 loaderInfo.parameters.badge in tracesB: ${JSON.stringify(initial.tracesB)}`,
    );
    assert.ok(!initial.tracesA.some((line) => line.startsWith('B_')), 'tracesA isolated from Module B');
    assert.ok(!initial.tracesB.some((line) => line.startsWith('A_')), 'tracesB isolated from Module A');

    // 8. Verify independent playback controls (`gotoAndStop`, `pause`, `resume`, `restart`) + `loadPublish('child/module.mjs')`.
    const controlResult = await cdp.evalJson(`
      (async () => {
        const { appA, appB, appC, tracesA, tracesB } = window.__testState;
        appA.gotoAndStop(3);
        appB.gotoAndStop(4);
        const pausedSnap = {
          aFrame: appA.currentFrame,
          aPlaying: appA.isPlaying,
          bFrame: appB.currentFrame,
          bPlaying: appB.isPlaying,
          cFrame: appC.currentFrame,
        };

        await appA.loadPublish('child/module.mjs');

        appB.play();
        await new Promise((r) => setTimeout(r, 140));
        const afterBPlay = {
          aFrameStill3: appA.currentFrame,
          aStillPaused: !appA.isPlaying,
          bPlayingNow: appB.isPlaying,
          childTraceInA: tracesA.includes('CHILD_OK:1'),
          childTraceInB: tracesB.includes('CHILD_OK:1'),
        };

        appB.restart();
        return {
          pausedSnap,
          afterBPlay,
          bPlayingAfterRestart: appB.isPlaying,
          bFrameAfterRestart: appB.currentFrame,
          errorsAfterRestart: window.__testState.errors.slice(),
        };
      })()
    `);

    assert.deepEqual(controlResult.pausedSnap, {
      aFrame: 3,
      aPlaying: false,
      bFrame: 4,
      bPlaying: false,
      cFrame: 1,
    });
    assert.equal(controlResult.afterBPlay.aFrameStill3, 3, 'paused Module A stays on frame 3 while Module B plays');
    assert.equal(controlResult.afterBPlay.aStillPaused, true);
    assert.equal(controlResult.afterBPlay.bPlayingNow, true);
    assert.equal(controlResult.afterBPlay.childTraceInA, true, 'loadPublish("child/module.mjs") mounted child in Module A');
    assert.equal(controlResult.afterBPlay.childTraceInB, false, 'child mount did not leak into Module B');
    assert.deepEqual(controlResult.errorsAfterRestart, []);
    assert.equal(controlResult.bPlayingAfterRestart, true, JSON.stringify(controlResult));

    // 9. Verify deterministic `appA.destroy()` cleans up `#slot-a` idempotently while `appB` & `appC` keep running,
    //    and `#slot-a` can be cleanly re-mounted afterwards!
    const destroyResult = await cdp.evalJson(`
      (async () => {
        const st = window.__testState;
        const tracesACountBefore = st.tracesA.length;
        const tracesBCountBefore = st.tracesB.length;

        st.appA.destroy();
        st.appA.destroy(); // idempotent second call

        const slotAChildrenAfterDestroy = document.getElementById('slot-a').children.length;
        const aDisposed = st.appA.disposed;
        const aRootAfterDestroy = st.appA.root;
        let postDestroyControlThrows = false;
        try {
          st.appA.play();
        } catch (_) {
          postDestroyControlThrows = true;
        }

        await new Promise((r) => setTimeout(r, 180));
        const tracesACountAfter = st.tracesA.length;
        const tracesBCountAfter = st.tracesB.length;

        // Re-mount a fresh instance into #slot-a to verify container WeakMap lock was released!
        const remountedA = await st.mountA('#slot-a', {
          autoplay: false,
          clean: true,
          parameters: { greeting: 'remounted' },
        });
        const remountOk = !remountedA.disposed && remountedA.totalFrames === 4 && Boolean(document.querySelector('#slot-a canvas'));
        remountedA.destroy();

        return {
          slotAChildrenAfterDestroy,
          aDisposed,
          aRootAfterDestroy,
          postDestroyControlThrows,
          aSilentAfterDestroy: tracesACountAfter === tracesACountBefore,
          bStillAdvancing: !st.appB.disposed && st.appB.isPlaying && tracesBCountAfter > tracesBCountBefore,
          cStillAlive: !st.appC.disposed && Boolean(document.querySelector('#slot-c neoflash-stage[data-render-mode="dom"]')),
          sharedWasmStillCached: window.NeoFlash.sharedWasmModules.size === 1,
          remountOk,
          slotACleanFinal: document.getElementById('slot-a').children.length === 0,
        };
      })()
    `);

    assert.equal(cdp.errors.length, 0, `unexpected browser errors during destroy: ${cdp.errors.join('\n')}`);
    assert.deepEqual(destroyResult, {
      slotAChildrenAfterDestroy: 0,
      aDisposed: true,
      aRootAfterDestroy: null,
      postDestroyControlThrows: true,
      aSilentAfterDestroy: true,
      bStillAdvancing: true,
      cStillAlive: true,
      sharedWasmStillCached: true,
      remountOk: true,
      slotACleanFinal: true,
    });

    // 10. Verify standalone `index.html` auto-boot backward compatibility in Chrome!
    await cdp.navigateAndWait(`http://127.0.0.1:${serverHandle.port}/standalone/index.html`);
    const standaloneResult = await cdp.evalJson(`
      new Promise((resolve, reject) => {
        const deadline = Date.now() + 10000;
        const poll = () => {
          const traceText = document.getElementById('trace')?.textContent || '';
          const loading = document.getElementById('loading');
          if (window.NeoFlashPlayer && loading?.style.display === 'none' && traceText.includes('STANDALONE_BOOT_OK')) {
            resolve({
              hasPlayer: Boolean(window.NeoFlashPlayer),
              targetFps: window.NeoFlashPlayer.targetFps,
              hasMountApi: typeof window.NeoFlash?.mount === 'function',
            });
            return;
          }
          if (Date.now() > deadline) {
            reject(new Error('Standalone index.html failed to auto-boot: ' + traceText));
            return;
          }
          setTimeout(poll, 25);
        };
        poll();
      })
    `);
    assert.deepEqual(standaloneResult, {
      hasPlayer: true,
      targetFps: 24,
      hasMountApi: true,
    });
    assert.equal(cdp.errors.length, 0, `unexpected standalone errors: ${cdp.errors.join('\n')}`);
  } finally {
    if (cdp) await cdp.close();
    if (serverHandle) {
      for (const s of serverHandle.sockets) s.destroy();
      await new Promise((resolve) => serverHandle.server.close(resolve));
    }
    fs.rmSync(tmpRoot, { recursive: true, force: true });
    fs.rmSync(userDataDir, { recursive: true, force: true });
  }
});
