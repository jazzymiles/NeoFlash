#!/usr/bin/env node
/**
 * Permanent executable HTML5 AS2/AS3 publish-correctness gate.
 *
 * By default exports all 18 authored AS2/AS3 examples + focused feature/diagnostic projects
 * across Full, Balanced, Compact, and Shared runtime size levels via the real
 * neoflash-export pipeline, executes them inside headless Chromium (CDP) over both
 * HTTP and file:// (excluding origin-restricted SharedObject & parent-relative
 * cross-origin Shared checks on file://, while explicitly verifying the HTTP(S)
 * origin error for SharedObject on file://), drives real UI interactions and
 * Restart resets, checks SafariDriver availability cleanly without hanging, and
 * writes a structured JSON report. check.sh passes its fresh Rust fixtures via --input-dir.
 */

import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '..');
const DEFAULT_GATE_DIR = path.join(REPO_ROOT, 'target', 'publish-gate');
const WEB_RUNTIME_DIR = path.join(REPO_ROOT, 'target', 'web-runtime');

const MIME_TYPES = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.htm', 'text/html; charset=utf-8'],
  ['.js', 'application/javascript; charset=utf-8'],
  ['.mjs', 'application/javascript; charset=utf-8'],
  ['.wasm', 'application/wasm'],
  ['.json', 'application/json; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.svg', 'image/svg+xml'],
  ['.png', 'image/png'],
  ['.jpg', 'image/jpeg'],
  ['.jpeg', 'image/jpeg'],
  ['.gif', 'image/gif'],
  ['.webp', 'image/webp'],
  ['.wav', 'audio/wav'],
  ['.mp3', 'audio/mpeg'],
  ['.ogg', 'audio/ogg'],
  ['.mp4', 'video/mp4'],
  ['.webm', 'video/webm'],
  ['.txt', 'text/plain; charset=utf-8'],
  ['.xml', 'application/xml; charset=utf-8'],
  ['.ttf', 'font/ttf'],
  ['.otf', 'font/otf'],
  ['.woff2', 'font/woff2'],
]);

function parseArgs(argv) {
  const opts = {
    skipExport: false,
    inputDir: null,
    filter: null,
    httpOnly: false,
    fileOnly: false,
    quietProgress: false,
  };
  for (const arg of argv) {
    if (arg === '--skip-export') {
      opts.skipExport = true;
    } else if (arg.startsWith('--input-dir=')) {
      const dir = arg.slice('--input-dir='.length);
      if (!dir) throw new Error('--input-dir requires a directory');
      opts.inputDir = path.resolve(dir);
      opts.skipExport = true;
    } else if (arg === '--http-only') {
      opts.httpOnly = true;
    } else if (arg === '--file-only') {
      opts.fileOnly = true;
    } else if (arg === '--quiet') {
      opts.quietProgress = true;
    } else if (arg.startsWith('--filter=')) {
      opts.filter = arg.slice('--filter='.length);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  return opts;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function getFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      const port = typeof addr === 'object' && addr ? addr.port : 0;
      server.close((err) => {
        if (err) reject(err);
        else resolve(port);
      });
    });
  });
}

function findChromeBinary() {
  const override = process.env.NEOFLASH_GATE_CHROME;
  if (override) {
    if (!fs.existsSync(override)) {
      throw new Error(`NEOFLASH_GATE_CHROME not found: ${override}`);
    }
    return override;
  }

  const staticCandidates = [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/snap/bin/chromium',
  ];
  for (const candidate of staticCandidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  const pathDirs = (process.env.PATH || '').split(path.delimiter).filter(Boolean);
  for (const dir of pathDirs) {
    for (const exe of ['google-chrome', 'chromium', 'chromium-browser', 'chrome']) {
      const candidate = path.join(dir, exe);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }

  const pwCaches = [
    path.join(os.homedir(), 'Library', 'Caches', 'ms-playwright'),
    path.join(os.homedir(), '.cache', 'ms-playwright'),
  ];
  for (const cacheDir of pwCaches) {
    if (!fs.existsSync(cacheDir)) continue;
    for (const entry of fs.readdirSync(cacheDir).sort().reverse()) {
      if (!entry.startsWith('chromium')) continue;
      const macBin = path.join(
        cacheDir,
        entry,
        'chrome-mac',
        'Google Chrome for Testing.app',
        'Contents',
        'MacOS',
        'Google Chrome for Testing'
      );
      const macArmBin = path.join(
        cacheDir,
        entry,
        'chrome-mac-arm64',
        'Google Chrome for Testing.app',
        'Contents',
        'MacOS',
        'Google Chrome for Testing'
      );
      const linuxBin = path.join(cacheDir, entry, 'chrome-linux', 'chrome');
      const linux64Bin = path.join(cacheDir, entry, 'chrome-linux64', 'chrome');
      for (const candidate of [macArmBin, macBin, linux64Bin, linuxBin]) {
        if (fs.existsSync(candidate)) return candidate;
      }
    }
  }

  throw new Error(
    'No Chromium/Chrome executable found for mandatory publish gate. Install Google Chrome or set NEOFLASH_GATE_CHROME.'
  );
}

function startStaticServer(rootDir) {
  return new Promise((resolve, reject) => {
    const absRoot = path.resolve(rootDir);
    const server = http.createServer((req, res) => {
      const reqUrl = new URL(req.url || '/', 'http://127.0.0.1');
      if (reqUrl.pathname === '/favicon.ico') {
        res.writeHead(204);
        res.end();
        return;
      }
      let relPath;
      try {
        relPath = decodeURIComponent(reqUrl.pathname).replace(/^\/+/, '');
      } catch {
        res.writeHead(400);
        res.end('Bad URL');
        return;
      }
      let targetPath = path.resolve(absRoot, relPath);
      if (!targetPath.startsWith(absRoot + path.sep) && targetPath !== absRoot) {
        res.writeHead(403);
        res.end('Forbidden');
        return;
      }
      if (!fs.existsSync(targetPath)) {
        res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
        res.end('Not found');
        return;
      }
      const stat = fs.statSync(targetPath);
      if (stat.isDirectory()) {
        targetPath = path.join(targetPath, 'index.html');
        if (!fs.existsSync(targetPath)) {
          res.writeHead(404);
          res.end('Not found');
          return;
        }
      }
      const fileStat = fs.statSync(targetPath);
      const ext = path.extname(targetPath).toLowerCase();
      const mime = MIME_TYPES.get(ext) || 'application/octet-stream';
      const rangeHeader = req.headers.range;
      if (rangeHeader && /^bytes=(\d*)-(\d*)$/.test(rangeHeader)) {
        const [, startStr, endStr] = rangeHeader.match(/^bytes=(\d*)-(\d*)$/);
        const total = fileStat.size;
        const start = startStr ? Number(startStr) : 0;
        const end = endStr ? Math.min(Number(endStr), total - 1) : total - 1;
        if (Number.isNaN(start) || Number.isNaN(end) || start > end || start >= total) {
          res.writeHead(416, { 'Content-Range': `bytes */${total}` });
          res.end();
          return;
        }
        res.writeHead(206, {
          'Content-Type': mime,
          'Content-Length': end - start + 1,
          'Content-Range': `bytes ${start}-${end}/${total}`,
          'Accept-Ranges': 'bytes',
          'Cache-Control': 'no-store',
        });
        fs.createReadStream(targetPath, { start, end }).pipe(res);
        return;
      }
      res.writeHead(200, {
        'Content-Type': mime,
        'Content-Length': fileStat.size,
        'Accept-Ranges': 'bytes',
        'Cache-Control': 'no-store',
      });
      fs.createReadStream(targetPath).pipe(res);
    });
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      resolve({
        server,
        baseUrl: `http://127.0.0.1:${addr.port}`,
      });
    });
  });
}

class CdpClient {
  constructor(wsUrl) {
    this.ws = new WebSocket(wsUrl);
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Set();
    this.closed = false;

    this.ready = new Promise((resolve, reject) => {
      const openTimer = setTimeout(() => {
        cleanup();
        reject(new Error('CDP WebSocket open timed out after 10000ms'));
      }, 10000);
      openTimer.unref?.();
      const onOpen = () => {
        cleanup();
        resolve();
      };
      const onError = (ev) => {
        cleanup();
        reject(new Error(`CDP WebSocket error: ${ev?.message || 'connection failed'}`));
      };
      const cleanup = () => {
        clearTimeout(openTimer);
        this.ws.removeEventListener('open', onOpen);
        this.ws.removeEventListener('error', onError);
      };
      this.ws.addEventListener('open', onOpen);
      this.ws.addEventListener('error', onError);
    });

    this.ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(String(ev.data));
      if (typeof msg.id === 'number') {
        const entry = this.pending.get(msg.id);
        if (!entry) return;
        this.pending.delete(msg.id);
        clearTimeout(entry.timer);
        if (msg.error) {
          entry.reject(new Error(`CDP ${entry.method}: ${msg.error.message}`));
        } else {
          entry.resolve(msg.result);
        }
      } else if (msg.method) {
        for (const fn of this.listeners) {
          fn(msg);
        }
      }
    });

    this.ws.addEventListener('close', () => {
      this.closed = true;
      for (const [, entry] of this.pending) {
        clearTimeout(entry.timer);
        entry.reject(new Error(`CDP socket closed during ${entry.method}`));
      }
      this.pending.clear();
    });
  }

  send(method, params = {}, sessionId = undefined, timeoutMs = 30000) {
    if (this.closed) {
      return Promise.reject(new Error(`CDP socket already closed for ${method}`));
    }
    const id = this.nextId++;
    const payload = sessionId ? { id, method, params, sessionId } : { id, method, params };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending.delete(id)) {
          reject(new Error(`CDP ${method} timed out after ${timeoutMs}ms`));
        }
      }, timeoutMs);
      timer.unref?.();
      this.pending.set(id, { method, resolve, reject, timer });
      try {
        this.ws.send(JSON.stringify(payload));
      } catch (err) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(err);
      }
    });
  }

  onEvent(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  close() {
    if (!this.closed) {
      this.ws.close();
    }
  }
}

async function launchChrome(chromeBin) {
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'neoflash-pubgate-chrome-'));
  const args = [
    '--headless=new',
    '--remote-debugging-port=0',
    `--user-data-dir=${userDataDir}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-gpu',
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--autoplay-policy=no-user-gesture-required',
    '--window-size=1280,820',
    'about:blank',
  ];
  const proc = spawn(chromeBin, args, {
    stdio: ['ignore', 'ignore', 'pipe'],
  });

  let stderrBuf = '';
  const wsUrl = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`Timed out waiting for Chrome DevTools WS URL. stderr:\n${stderrBuf}`));
    }, 15000);
    proc.stderr.on('data', (chunk) => {
      stderrBuf += chunk.toString('utf8');
      const match = stderrBuf.match(/DevTools listening on (ws:\/\/[^\s]+)/);
      if (match) {
        clearTimeout(timer);
        resolve(match[1]);
      }
    });
    proc.once('exit', (code, sig) => {
      clearTimeout(timer);
      reject(new Error(`Chrome exited prematurely code=${code} sig=${sig}. stderr:\n${stderrBuf}`));
    });
  });

  const cdp = new CdpClient(wsUrl);
  await cdp.ready;
  const versionInfo = await cdp.send('Browser.getVersion');

  return {
    proc,
    cdp,
    userDataDir,
    productVersion: versionInfo.product || 'unknown',
    async shutdown() {
      cdp.close();
      proc.stderr?.destroy();
      if (proc.exitCode === null) {
        proc.kill('SIGTERM');
        await Promise.race([
          new Promise((r) => proc.once('exit', r)),
          sleep(2000).then(() => {
            try {
              proc.kill('SIGKILL');
            } catch {}
          }),
        ]);
      }
      fs.rmSync(userDataDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    },
  };
}

function keyMeta(keyName) {
  const map = {
    ArrowLeft: { key: 'ArrowLeft', code: 'ArrowLeft', vk: 37 },
    ArrowUp: { key: 'ArrowUp', code: 'ArrowUp', vk: 38 },
    ArrowRight: { key: 'ArrowRight', code: 'ArrowRight', vk: 39 },
    ArrowDown: { key: 'ArrowDown', code: 'ArrowDown', vk: 40 },
    Space: { key: ' ', code: 'Space', vk: 32, text: ' ' },
    Enter: { key: 'Enter', code: 'Enter', vk: 13, text: '\r' },
    KeyR: { key: 'r', code: 'KeyR', vk: 82, text: 'r' },
  };
  if (map[keyName]) return map[keyName];
  if (keyName.length === 1) {
    const upper = keyName.toUpperCase();
    return {
      key: keyName,
      code: /^[A-Z]$/.test(upper) ? `Key${upper}` : /^[0-9]$/.test(keyName) ? `Digit${keyName}` : '',
      vk: upper.charCodeAt(0),
      text: keyName,
    };
  }
  return { key: keyName, code: keyName, vk: 0 };
}

function formatRemoteArg(arg) {
  if (!arg) return '';
  if (Object.prototype.hasOwnProperty.call(arg, 'value')) {
    return String(arg.value);
  }
  if (typeof arg.unserializableValue === 'string') {
    return arg.unserializableValue;
  }
  if (typeof arg.description === 'string') {
    return arg.description;
  }
  return '';
}

async function runSinglePageCheck(cdp, url, caseSpec, opts = {}) {
  const expectOriginError = opts.expectOriginError || '';
  const { browserContextId } = await cdp.send('Target.createBrowserContext', {
    disposeOnDetach: true,
  });
  const { targetId } = await cdp.send('Target.createTarget', {
    url: 'about:blank',
    browserContextId,
  });
  const { sessionId } = await cdp.send('Target.attachToTarget', {
    targetId,
    flatten: true,
  });

  const logs = [];
  const consoleErrors = [];
  const uncaughtExceptions = [];
  const httpErrors = [];
  const networkFailures = [];
  const requestUrls = new Map();

  const removeListener = cdp.onEvent((ev) => {
    if (ev.sessionId !== sessionId) return;
    if (ev.method === 'Runtime.consoleAPICalled') {
      const p = ev.params;
      const line = (p.args || []).map(formatRemoteArg).join(' ');
      logs.push({ type: p.type, text: line });
      if (p.type === 'error') {
        consoleErrors.push(line);
      }
    } else if (ev.method === 'Runtime.exceptionThrown') {
      const d = ev.params.exceptionDetails || {};
      const desc = d.exception?.description || d.exception?.value || d.text || 'uncaught exception';
      uncaughtExceptions.push(String(desc));
    } else if (ev.method === 'Log.entryAdded') {
      const entry = ev.params.entry || {};
      if (entry.level === 'error') {
        consoleErrors.push(`${entry.source || 'log'}: ${entry.text || ''}`);
      }
    } else if (ev.method === 'Network.requestWillBeSent') {
      requestUrls.set(ev.params.requestId, ev.params.request?.url || '');
    } else if (ev.method === 'Network.responseReceived') {
      const r = ev.params.response || {};
      if (r.status >= 400) {
        httpErrors.push(`${r.status} ${r.url}`);
      }
    } else if (ev.method === 'Network.loadingFailed') {
      if (!ev.params.canceled) {
        const reqUrl = requestUrls.get(ev.params.requestId) || ev.params.requestId;
        networkFailures.push(`${ev.params.errorText || 'failed'} ${reqUrl}`);
      }
    }
  });

  async function evalDom() {
    const res = await cdp.send(
      'Runtime.evaluate',
      {
        expression: `(() => {
          const err = document.getElementById('error');
          const errStyle = err ? getComputedStyle(err) : null;
          const errText = err ? err.textContent.trim() : '';
          const errVisible = Boolean(err && errStyle && errStyle.display !== 'none' && errText.length > 0);
          const isVisible = (el) => {
            if (!el) return false;
            if (typeof el.checkVisibility === 'function') return el.checkVisibility();
            if (el.closest('[hidden]')) return false;
            const st = getComputedStyle(el);
            return st.display !== 'none' && st.visibility !== 'hidden';
          };
          const status = document.getElementById('status');
          const statusText = status ? status.textContent.trim() : '';
          const statusVisible = isVisible(status) && statusText.length > 0;
          const restart = document.getElementById('restart');
          const restartVisible = isVisible(restart);
          const restartRect = restart ? restart.getBoundingClientRect() : null;
          const canvas = document.querySelector('#stage canvas, canvas');
          const canvasRect = canvas ? canvas.getBoundingClientRect() : null;
          return {
            errVisible,
            errText,
            statusText,
            statusVisible,
            hasRestart: Boolean(restart),
            restartVisible,
            restartRect: restartRect ? { x: restartRect.x, y: restartRect.y, width: restartRect.width, height: restartRect.height } : null,
            canvasRect: canvasRect ? { x: canvasRect.x, y: canvasRect.y, width: canvasRect.width, height: canvasRect.height } : null,
          };
        })()`,
        returnByValue: true,
      },
      sessionId
    );
    return res.result?.value || {};
  }

  function mapStagePoint(dom, stageX, stageY) {
    const r = dom.canvasRect;
    if (!r || r.width <= 0 || r.height <= 0) {
      throw new Error('Stage canvas not rendered or zero-sized');
    }
    return {
      x: r.x + (stageX / caseSpec.stageWidth) * r.width,
      y: r.y + (stageY / caseSpec.stageHeight) * r.height,
    };
  }

  async function waitUntil(pred, timeoutMs, label) {
    const deadline = Date.now() + timeoutMs;
    let lastDom = null;
    while (Date.now() < deadline) {
      lastDom = await evalDom();
      const hit = pred(lastDom);
      if (hit) return { hit, dom: lastDom };
      await sleep(50);
    }
    return { hit: null, dom: lastDom, timedOutLabel: label };
  }

  try {
    await cdp.send('Page.enable', {}, sessionId);
    await cdp.send('Runtime.enable', {}, sessionId);
    await cdp.send('Log.enable', {}, sessionId);
    await cdp.send('Network.enable', {}, sessionId);
    await cdp.send(
      'Emulation.setDeviceMetricsOverride',
      {
        width: 1280,
        height: 820,
        deviceScaleFactor: 1,
        mobile: false,
      },
      sessionId
    );

    await cdp.send('Page.navigate', { url }, sessionId);

    const isExpectedDiag = Boolean(expectOriginError) || caseSpec.expectedErrorSubstrings.length > 0;
    if (isExpectedDiag) {
      const expectedList = expectOriginError ? [expectOriginError] : caseSpec.expectedErrorSubstrings;
      const waited = await waitUntil(
        (dom) => {
          const logTexts = logs.map((l) => l.text);
          const combined = [dom.errText, ...consoleErrors, ...uncaughtExceptions, ...logTexts].join('\n');
          return dom.errVisible || expectedList.every((sub) => combined.includes(sub));
        },
        7000,
        'expected diagnostic error'
      );
      // Allow a brief extra settle tick so async runtime error text renders into #error if thrown just now.
      await sleep(120);
      const dom = await evalDom();
      const logTexts = logs.map((l) => l.text);
      const combined = [dom.errText, ...consoleErrors, ...uncaughtExceptions, ...logTexts].join('\n');
      for (const sub of expectedList) {
        if (!combined.includes(sub)) {
          throw new Error(
            `Expected diagnostic substring ${JSON.stringify(sub)} not found.\n#error=${JSON.stringify(
              dom.errText
            )}\nconsoleErrors=${JSON.stringify(consoleErrors)}\nexceptions=${JSON.stringify(
              uncaughtExceptions
            )}\nlogs=${JSON.stringify(logTexts)}`
          );
        }
      }
      if (!expectOriginError && !dom.errVisible) {
        throw new Error(`Expected #error banner visible for diagnostic case, got hidden (#error=${JSON.stringify(dom.errText)})`);
      }
      if (httpErrors.length > 0 || networkFailures.length > 0) {
        throw new Error(`Unexpected network failure in diagnostic case: http=${httpErrors.join('; ')} net=${networkFailures.join('; ')}`);
      }
      return { ok: true, logs: logs.map((l) => l.text), errorText: dom.errText };
    }

    // Normal runtime pass wait
    const bootWait = await waitUntil(
      (dom) => {
        if (dom.errVisible || uncaughtExceptions.length > 0 || httpErrors.length > 0 || networkFailures.length > 0) {
          return 'fail-fast';
        }
        return logs.some((l) => l.text.includes(caseSpec.expectedPass)) ? 'pass' : null;
      },
      9000,
      `expected trace ${caseSpec.expectedPass}`
    );

    if (bootWait.hit !== 'pass') {
      const dom = bootWait.dom || (await evalDom());
      throw new Error(
        `Did not observe ${JSON.stringify(caseSpec.expectedPass)}.\n` +
          `errVisible=${dom.errVisible} errText=${JSON.stringify(dom.errText)}\n` +
          `consoleErrors=${JSON.stringify(consoleErrors)}\n` +
          `exceptions=${JSON.stringify(uncaughtExceptions)}\n` +
          `httpErrors=${JSON.stringify(httpErrors)}\n` +
          `networkFailures=${JSON.stringify(networkFailures)}\n` +
          `logs=${JSON.stringify(logs.map((l) => l.text))}`
      );
    }

    const initialDom = bootWait.dom;
    if (caseSpec.cleanChrome) {
      if (initialDom.hasRestart && initialDom.restartVisible) {
        throw new Error('cleanChrome case unexpectedly rendered visible #restart button');
      }
      if (initialDom.statusVisible) {
        throw new Error(`cleanChrome case unexpectedly rendered visible #status text: ${initialDom.statusText}`);
      }
    }

    // Execute scripted interactions if any.
    let stageFocused = false;
    for (const step of caseSpec.interactions || []) {
      const logStartIdx = logs.length;
      const dom = await evalDom();

      if (step.kind === 'click') {
        const pt = mapStagePoint(dom, step.x, step.y);
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: pt.x, y: pt.y }, sessionId);
        await cdp.send(
          'Input.dispatchMouseEvent',
          { type: 'mousePressed', x: pt.x, y: pt.y, button: 'left', clickCount: 1 },
          sessionId
        );
        await sleep(30);
        await cdp.send(
          'Input.dispatchMouseEvent',
          { type: 'mouseReleased', x: pt.x, y: pt.y, button: 'left', clickCount: 1 },
          sessionId
        );
      } else if (step.kind === 'drag') {
        const from = mapStagePoint(dom, step.x, step.y);
        const to = mapStagePoint(dom, step.toX, step.toY);
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: from.x, y: from.y }, sessionId);
        await cdp.send(
          'Input.dispatchMouseEvent',
          { type: 'mousePressed', x: from.x, y: from.y, button: 'left', clickCount: 1 },
          sessionId
        );
        for (let i = 1; i <= 5; i++) {
          const ix = from.x + ((to.x - from.x) * i) / 5;
          const iy = from.y + ((to.y - from.y) * i) / 5;
          await cdp.send(
            'Input.dispatchMouseEvent',
            { type: 'mouseMoved', x: ix, y: iy, button: 'left', buttons: 1 },
            sessionId
          );
          await sleep(20);
        }
        await cdp.send(
          'Input.dispatchMouseEvent',
          { type: 'mouseReleased', x: to.x, y: to.y, button: 'left', clickCount: 1 },
          sessionId
        );
      } else if (step.kind === 'text_input') {
        const pt = mapStagePoint(dom, step.x, step.y);
        await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: pt.x, y: pt.y }, sessionId);
        await cdp.send(
          'Input.dispatchMouseEvent',
          { type: 'mousePressed', x: pt.x, y: pt.y, button: 'left', clickCount: 1 },
          sessionId
        );
        await sleep(30);
        await cdp.send(
          'Input.dispatchMouseEvent',
          { type: 'mouseReleased', x: pt.x, y: pt.y, button: 'left', clickCount: 1 },
          sessionId
        );
        await sleep(60);
        for (const ch of step.text || '') {
          const meta = keyMeta(ch);
          await cdp.send(
            'Input.dispatchKeyEvent',
            { type: 'keyDown', key: meta.key, code: meta.code, windowsVirtualKeyCode: meta.vk, text: meta.text },
            sessionId
          );
          await cdp.send(
            'Input.dispatchKeyEvent',
            { type: 'keyUp', key: meta.key, code: meta.code, windowsVirtualKeyCode: meta.vk },
            sessionId
          );
          await sleep(20);
        }
      } else if (step.kind === 'key') {
        if (!stageFocused) {
          const center = mapStagePoint(dom, caseSpec.stageWidth / 2, caseSpec.stageHeight / 2);
          await cdp.send(
            'Input.dispatchMouseEvent',
            { type: 'mousePressed', x: center.x, y: center.y, button: 'left', clickCount: 1 },
            sessionId
          );
          await cdp.send(
            'Input.dispatchMouseEvent',
            { type: 'mouseReleased', x: center.x, y: center.y, button: 'left', clickCount: 1 },
            sessionId
          );
          await sleep(40);
          stageFocused = true;
        }
        const meta = keyMeta(step.key);
        await cdp.send(
          'Input.dispatchKeyEvent',
          { type: 'keyDown', key: meta.key, code: meta.code, windowsVirtualKeyCode: meta.vk, text: meta.text },
          sessionId
        );
        await sleep(30);
        await cdp.send(
          'Input.dispatchKeyEvent',
          { type: 'keyUp', key: meta.key, code: meta.code, windowsVirtualKeyCode: meta.vk },
          sessionId
        );
      } else {
        throw new Error(`Unsupported interaction kind: ${step.kind}`);
      }

      if (step.expectTrace) {
        const evWait = await waitUntil(
          () => logs.slice(logStartIdx).some((l) => l.text.includes(step.expectTrace)),
          4000,
          `interaction trace ${step.expectTrace}`
        );
        if (!evWait.hit) {
          throw new Error(
            `Interaction ${step.kind} did not produce ${JSON.stringify(step.expectTrace)}.\nPost-step logs: ${JSON.stringify(
              logs.slice(logStartIdx).map((l) => l.text)
            )}\nErrors: ${JSON.stringify(consoleErrors)}\nExceptions: ${JSON.stringify(uncaughtExceptions)}`
          );
        }
      } else {
        await sleep(160);
      }
    }

    if (caseSpec.id === 'focus-t304-display-graphics') {
      await sleep(60);
      const pixelAudit = await cdp.send(
        'Runtime.evaluate',
        {
          expression: `(() => {
            const canvas = document.querySelector('#stage canvas, canvas');
            if (!canvas) return { ok: false, reason: 'missing canvas' };
            const ctx = canvas.getContext('2d');
            if (!ctx) return { ok: false, reason: 'missing 2d context' };
            const sw = ${caseSpec.stageWidth}, sh = ${caseSpec.stageHeight};
            const sample = (sx, sy) => {
              const cx = Math.min(canvas.width - 1, Math.max(0, Math.round((sx / sw) * canvas.width)));
              const cy = Math.min(canvas.height - 1, Math.max(0, Math.round((sy / sh) * canvas.height)));
              return Array.from(ctx.getImageData(cx, cy, 1, 1).data);
            };
            const close = (px, exp, tol = 14) => Math.abs(px[0] - exp[0]) <= tol && Math.abs(px[1] - exp[1]) <= tol && Math.abs(px[2] - exp[2]) <= tol;
            const countRedRows = (sx, y0, y1) => {
              const cx = Math.round((sx / sw) * canvas.width);
              const cy0 = Math.max(0, Math.floor((y0 / sh) * canvas.height));
              const cy1 = Math.min(canvas.height - 1, Math.ceil((y1 / sh) * canvas.height));
              let rows = 0;
              for (let cy = cy0; cy <= cy1; cy++) {
                const d = ctx.getImageData(cx, cy, 1, 1).data;
                if (d[0] > 140 && d[1] < 90) rows++;
              }
              return rows;
            };
            const dynOver = sample(290, 65);
            const hairRows = countRedRows(260, 196, 212);
            const normRows = countRedRows(430, 196, 212);
            const gradMid1 = sample(70, 300);
            const gradMid2 = sample(130, 300);
            const gradPeak = sample(99, 300);
            const rep1 = sample(55, 380);
            const rep2 = sample(115, 380);
            const subPx = sample(290, 310);
            const invPx = sample(350, 310);
            const layerErase = sample(450, 310);
            const layerKeep = sample(510, 310);
            const layerMaskOut = sample(570, 310);
            const checks = {
              dynOverSkyBlue: close(dynOver, [56, 189, 248]),
              hairlineStaysThin: hairRows >= 1 && hairRows <= 3 && normRows >= 6 && normRows > hairRows * 2,
              reflectLinearRgb: gradMid1[0] > 165 && gradMid1[1] > 165 && gradMid1[2] < 25 && close(gradMid1, gradMid2, 16) && gradPeak[1] > 210 && gradPeak[0] < 90,
              repeatPeriodic: rep1[2] > 150 && close(rep1, rep2, 18),
              subtractBlend: close(subPx, [150, 80, 20]),
              invertBlend: close(invPx, [55, 155, 205]),
              layerEraseHole: close(layerErase, [200, 100, 50]),
              layerAlphaKeep: close(layerKeep, [0, 220, 100]),
              layerAlphaMaskOut: close(layerMaskOut, [200, 100, 50]),
            };
            const ok = Object.values(checks).every(Boolean);
            return { ok, checks, samples: { dynOver, hairRows, normRows, gradMid1, gradMid2, gradPeak, rep1, rep2, subPx, invPx, layerErase, layerKeep, layerMaskOut } };
          })()`,
          returnByValue: true,
        },
        sessionId
      );
      const val = pixelAudit.result?.value;
      if (!val || !val.ok) {
        throw new Error(`T-304 live Canvas pixel verification failed: ${JSON.stringify(val)}`);
      }
    }

    // Verify Restart reset when requested.
    if (caseSpec.verifyRestart) {
      const dom = await evalDom();
      if (!dom.hasRestart || !dom.restartVisible || !dom.restartRect) {
        throw new Error('verifyRestart enabled, but #restart button is missing or hidden');
      }
      const restartStartIdx = logs.length;
      const rx = dom.restartRect.x + dom.restartRect.width / 2;
      const ry = dom.restartRect.y + dom.restartRect.height / 2;
      await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: rx, y: ry }, sessionId);
      await cdp.send(
        'Input.dispatchMouseEvent',
        { type: 'mousePressed', x: rx, y: ry, button: 'left', clickCount: 1 },
        sessionId
      );
      await sleep(25);
      await cdp.send(
        'Input.dispatchMouseEvent',
        { type: 'mouseReleased', x: rx, y: ry, button: 'left', clickCount: 1 },
        sessionId
      );

      const restartWait = await waitUntil(
        (d) => {
          if (d.errVisible || uncaughtExceptions.length > 0) return 'fail-fast';
          return logs.slice(restartStartIdx).some((l) => l.text.includes(caseSpec.expectedPass)) ? 'pass' : null;
        },
        6000,
        `restart pass ${caseSpec.expectedPass}`
      );
      if (restartWait.hit !== 'pass') {
        const afterDom = restartWait.dom || (await evalDom());
        throw new Error(
          `Restart did not cleanly re-emit ${JSON.stringify(caseSpec.expectedPass)}.\n` +
            `errText=${JSON.stringify(afterDom.errText)}\npostRestartLogs=${JSON.stringify(
              logs.slice(restartStartIdx).map((l) => l.text)
            )}\nerrors=${JSON.stringify(consoleErrors)}\nexceptions=${JSON.stringify(uncaughtExceptions)}`
        );
      }
    }

    // Final invariant check: zero uncaught errors, zero HTTP >= 400, zero network failures.
    await sleep(60);
    const finalDom = await evalDom();
    if (finalDom.errVisible || consoleErrors.length > 0 || uncaughtExceptions.length > 0 || httpErrors.length > 0 || networkFailures.length > 0) {
      throw new Error(
        `Runtime errors after execution:\n#error=${JSON.stringify(finalDom.errText)}\nconsoleErrors=${JSON.stringify(
          consoleErrors
        )}\nexceptions=${JSON.stringify(uncaughtExceptions)}\nhttpErrors=${JSON.stringify(
          httpErrors
        )}\nnetworkFailures=${JSON.stringify(networkFailures)}`
      );
    }

    return { ok: true, logs: logs.map((l) => l.text) };
  } finally {
    removeListener();
    try {
      await cdp.send('Target.closeTarget', { targetId });
    } catch {}
    try {
      await cdp.send('Target.disposeBrowserContext', { browserContextId });
    } catch {}
  }
}

async function probeAndRunSafari(baseUrl, smokeCases) {
  const safariBin = '/usr/bin/safaridriver';
  if (process.platform !== 'darwin' || !fs.existsSync(safariBin)) {
    return { status: 'unavailable', reason: 'not macOS or /usr/bin/safaridriver missing', runs: [] };
  }

  const port = await getFreePort();
  const proc = spawn(safariBin, ['-p', String(port)], {
    stdio: 'ignore',
  });

  try {
    const endpoint = `http://127.0.0.1:${port}`;
    const readyDeadline = Date.now() + 4000;
    let ready = false;
    while (Date.now() < readyDeadline) {
      try {
        const res = await fetch(`${endpoint}/status`, { signal: AbortSignal.timeout(800) });
        if (res.ok || res.status === 500 || res.status === 404) {
          ready = true;
          break;
        }
      } catch {}
      await sleep(100);
    }
    if (!ready) {
      return { status: 'unavailable', reason: 'safaridriver did not answer /status within 4s', runs: [] };
    }

    const sessionRes = await fetch(`${endpoint}/session`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ capabilities: { alwaysMatch: { browserName: 'safari' } } }),
      signal: AbortSignal.timeout(4000),
    });
    const sessionJson = await sessionRes.json();
    if (!sessionRes.ok || !sessionJson?.value?.sessionId) {
      const msg = sessionJson?.value?.message || `HTTP ${sessionRes.status}`;
      return {
        status: 'unavailable',
        reason: msg,
        runs: [],
      };
    }

    const sessionId = sessionJson.value.sessionId;
    const runs = [];
    try {
      for (const item of smokeCases) {
        const url = `${baseUrl}/${item.group}/${item.id}/full/index.html`;
        await fetch(`${endpoint}/session/${sessionId}/url`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ url }),
          signal: AbortSignal.timeout(15000),
        });
        const deadline = Date.now() + 15000;
        let passed = false;
        let lastErr = '';
        while (Date.now() < deadline) {
          const evalRes = await fetch(`${endpoint}/session/${sessionId}/execute/sync`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              script: `return {
                err: (document.getElementById('error')?.textContent || '').trim(),
                status: (document.getElementById('status')?.textContent || '').trim(),
                hasCanvas: Boolean(document.querySelector('#stage canvas, canvas'))
              };`,
              args: [],
            }),
            signal: AbortSignal.timeout(3000),
          });
          const evalData = await evalRes.json();
          const val = evalData?.value || {};
          if (val.err) {
            lastErr = val.err;
            break;
          }
          if (val.hasCanvas && !val.err) {
            passed = true;
            break;
          }
          await sleep(150);
        }
        runs.push({ id: item.id, ok: passed, error: lastErr || (passed ? '' : 'timeout waiting for canvas') });
      }
      return {
        status: runs.every((r) => r.ok) ? 'passed' : 'failed',
        reason: 'Remote Automation active',
        runs,
      };
    } finally {
      try {
        await fetch(`${endpoint}/session/${sessionId}`, {
          method: 'DELETE',
          signal: AbortSignal.timeout(2000),
        });
      } catch {}
    }
  } catch (err) {
    return {
      status: 'unavailable',
      reason: err instanceof Error ? err.message : String(err),
      runs: [],
    };
  } finally {
    if (proc.exitCode === null) {
      proc.kill('SIGTERM');
      await Promise.race([
        new Promise((r) => proc.once('exit', r)),
        sleep(1000).then(() => {
          try {
            proc.kill('SIGKILL');
          } catch {}
        }),
      ]);
    }
  }
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const startMs = Date.now();
  const gateDir = opts.inputDir ?? DEFAULT_GATE_DIR;
  const manifestPath = path.join(gateDir, 'manifest.json');
  const reportPath = path.join(gateDir, 'report.json');

  if (!fs.existsSync(path.join(WEB_RUNTIME_DIR, 'neoflash_web.js'))) {
    throw new Error(`Missing built WASM web runtime at ${WEB_RUNTIME_DIR}. Run ./scripts/build-web.sh first.`);
  }

  if (!opts.skipExport) {
    console.log('[gate] Exporting HTML5 publish matrix via cargo test publish_gate_exports_and_runtime_contracts...');
    fs.rmSync(gateDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    fs.mkdirSync(gateDir, { recursive: true });
    const env = {
      ...process.env,
      NEOFLASH_PUBLISH_GATE_DIR: gateDir,
      NEOFLASH_CHECK_TIER: 'release',
      PATH: `${path.join(os.homedir(), '.cargo', 'bin')}:${process.env.PATH || ''}`,
    };
    execFileSync(
      'cargo',
      [
        'test',
        '--locked',
        '-p',
        'neoflash-export',
        '--test',
        'publish_correctness_gate',
        'publish_gate_exports_and_runtime_contracts',
        '--',
        '--exact',
        '--nocapture',
      ],
      { cwd: REPO_ROOT, env, stdio: 'inherit' }
    );
  }

  if (!fs.existsSync(manifestPath)) {
    throw new Error(`Missing gate manifest at ${manifestPath}`);
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  console.log(`[gate] Running ${manifest.tier ?? 'existing'} matrix from ${manifestPath}`);
  const chromeBin = findChromeBinary();

  const filteredCases = opts.filter
    ? manifest.cases.filter((c) => c.id.includes(opts.filter) || c.group.includes(opts.filter))
    : manifest.cases;
  if (filteredCases.length === 0) {
    throw new Error(`No cases matched filter ${JSON.stringify(opts.filter)}`);
  }

  const httpSrv = await startStaticServer(gateDir);
  const chrome = await launchChrome(chromeBin);
  const results = [];
  let passedRuns = 0;
  let failedRuns = 0;

  try {
    console.log(`[gate] Browser: ${chrome.productVersion} (${chromeBin})`);
    console.log(`[gate] HTTP root: ${httpSrv.baseUrl}`);

    const jobs = [];
    for (const c of filteredCases) {
      for (const level of c.levels) {
        if (!opts.fileOnly) {
          jobs.push({
            c,
            level,
            protocol: 'http',
            url: `${httpSrv.baseUrl}/${c.group}/${c.id}/${level}/index.html`,
            expectOriginError: '',
          });
        }
        // Shared uses cross-directory ../../../_shared_runtime/, blocked by default browser file://
        // origin isolation; test Full/Balanced/Compact directly over file:// when supported, and
        // explicitly verify SharedObject's HTTP(S) origin guard on file:// for 02-oop-xml-collections.
        const shouldTestFile =
          !opts.httpOnly &&
          level !== 'shared' &&
          (c.allowFileProtocol || c.id.includes('02-oop-xml-collections'));
        if (shouldTestFile) {
          const indexFile = path.join(gateDir, c.group, c.id, level, 'index.html');
          const expectOriginError = !c.allowFileProtocol
            ? 'SharedObject persistence requires an HTTP(S) origin'
            : '';
          jobs.push({
            c,
            level,
            protocol: expectOriginError ? 'file-origin-policy' : 'file',
            url: pathToFileURL(indexFile).href,
            expectOriginError,
          });
        }
      }
    }

    const concurrency = Math.max(1, Number(process.env.NEOFLASH_GATE_CONCURRENCY || 4));
    const orderedResults = new Array(jobs.length);
    let nextJobIdx = 0;

    async function worker() {
      while (true) {
        const idx = nextJobIdx++;
        if (idx >= jobs.length) break;
        const job = jobs[idx];
        const runStart = Date.now();
        try {
          await runSinglePageCheck(chrome.cdp, job.url, job.c, {
            expectOriginError: job.expectOriginError,
          });
          passedRuns++;
          orderedResults[idx] = {
            id: job.c.id,
            group: job.c.group,
            language: job.c.language,
            level: job.level,
            protocol: job.protocol,
            ok: true,
            ms: Date.now() - runStart,
          };
          if (!opts.quietProgress) {
            console.log(`  [PASS] ${job.c.id} [${job.level} ${job.protocol}] (${Date.now() - runStart}ms)`);
          }
        } catch (err) {
          failedRuns++;
          const message = err instanceof Error ? err.message : String(err);
          orderedResults[idx] = {
            id: job.c.id,
            group: job.c.group,
            language: job.c.language,
            level: job.level,
            protocol: job.protocol,
            ok: false,
            error: message,
            ms: Date.now() - runStart,
          };
          console.error(`  [FAIL] ${job.c.id} [${job.level} ${job.protocol}]: ${message}`);
        }
      }
    }

    await Promise.all(Array.from({ length: Math.min(concurrency, jobs.length) }, () => worker()));
    results.push(...orderedResults);

    const safariSmokeIds = new Set([
      'as2-01-math-geometry',
      'as3-01-math-geometry',
      'focus-as2-object-handlers',
      'focus-as3-object-handlers',
    ]);
    const safariCases = filteredCases.filter((c) => safariSmokeIds.has(c.id));
    const safari = opts.fileOnly ? { status: 'skipped', reason: '--file-only', runs: [] } : await probeAndRunSafari(httpSrv.baseUrl, safariCases);
    console.log(`[gate] Safari status: ${safari.status} (${safari.reason})`);
    if (safari.status === 'failed') {
      failedRuns++;
    }

    const report = {
      generatedAt: new Date().toISOString(),
    tier: manifest.tier,
      durationMs: Date.now() - startMs,
      chrome: {
        binary: chromeBin,
        version: chrome.productVersion,
      },
      safari,
      totals: {
        cases: filteredCases.length,
        passedRuns,
        failedRuns,
      },
      results,
    };
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(`[gate] Summary: ${passedRuns} passed, ${failedRuns} failed in ${((Date.now() - startMs) / 1000).toFixed(1)}s -> ${reportPath}`);

    if (failedRuns > 0) {
      process.exitCode = 1;
    }
  } finally {
    await chrome.shutdown();
    httpSrv.server.closeAllConnections?.();
    httpSrv.server.close();
  }
}

main().catch((err) => {
  console.error('[gate] Fatal error:', err);
  process.exit(1);
});
