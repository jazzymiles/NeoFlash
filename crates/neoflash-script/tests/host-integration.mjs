// Execute the real host proxy/dispatch implementation with a small Player stub.
// Console, host trace and on-page feedback have independent observations so a
// compiler that bypasses host trace cannot accidentally pass this regression.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url);

export function createRuntimeHost(root, player, compiledScripts, options = {}) {
  const path = resolve(root, 'web/runtime.js');
  const runtime = readFileSync(path, 'utf8');
  const { sourceKey, oneBasedFrame, as2Frame, createAs2Methods, describeThrown, unwrapScriptThrow } = require(path);
  const start = runtime.indexOf('    const propertyMap =');
  const end = runtime.indexOf('    function resize()', start);
  assert.ok(start >= 0 && end > start, 'host implementation boundaries changed');
  const scriptConsole = [], hostConsole = [], traceCalls = [];
  const bindings = ['event', '_root', '_parent', 'trace', 'play', 'stop', 'gotoAndPlay', 'gotoAndStop', 'nextFrame', 'prevFrame', '__nfAs2Method', '__nfAs2Value', '__nfProgram', '__nfDisplay'];
  const functions = new Map(compiledScripts.map(({ script, code, location }) => {
    const fn = new Function('console', `return function(${bindings.join(',')}) { "use strict"; return (function(){\n${code}\n}).call(this); }`)({ log: (...args) => scriptConsole.push(args) });
    return [sourceKey(script), { fn, location }];
  }));
  const globals = { NeoFlashDisplay:require(resolve(root,'web/display.js')), NeoFlashEvents:require(resolve(root,'web/events.js')),
    NeoFlashStage:require(resolve(root,'web/stage.js')), NeoFlashAccessibility:require(resolve(root,'web/accessibility.js')) };
  const host = new Function('player', 'functions', 'console', 'sourceKey', 'oneBasedFrame', 'as2Frame', 'createAs2Methods', 'describeThrown', 'unwrapScriptThrow', 'traceSpy', 'global', 'bundle', 'project', 'context', 'font',
    'const proxies = new Map(); const render = () => {}; let lastTrace = "";\n' + runtime.slice(start, end) +
    '\nconst originalTrace = trace; trace = (...args) => { traceSpy(...args); return originalTrace(...args); };\n' +
    'initializeFamily(); return { proxyFor, dispatchScripts, broadcastFrame, attachNewInstances, resetProject, display:()=>display, stage:()=>stageHost, program:()=>program, lastTrace: () => lastTrace };'
  )(player, functions, { log: (...args) => hostConsole.push(args) }, sourceKey, oneBasedFrame, as2Frame, createAs2Methods, describeThrown, unwrapScriptThrow, (...args) => traceCalls.push(args),
    globals, options.bundle, options.project || options.bundle?.project, options.context, kind=>`${kind.font_size}px ${JSON.stringify(kind.font_family)}, sans-serif`);
  return { host, traceCalls, hostConsole, scriptConsole };
}

function createHostHarness(root, { language, source }, code) {
  const script = { language, source };
  const lifetimes = new Map([['[]', 1], ['["blue"]', 2], ['["red"]', 3]]);
  let queue = [];
  const player = {
    lifetime_id: key => lifetimes.get(key) || 0,
    child_path: (key, name) => JSON.stringify(key === '[]' && lifetimes.has(JSON.stringify([name])) ? [name] : null),
    clip_frame: () => 0, clip_frame_count: () => 1,
    property: () => 0, set_property: () => true, control: () => true, resolve_frame: () => -1,
    scripts_json: () => { const events = queue; queue = []; return JSON.stringify(events); },
    is_dispatch_current: raw => { const item = JSON.parse(raw); return lifetimes.get(JSON.stringify(item.target_path)) === item.target_lifetime; },
    script_overflowed: () => false,
  };
  const result = createRuntimeHost(root, player, [{ script, code, location: 'AS3 keyboard integration' }]);
  const { host } = result;
  function click(name) {
    queue.push({ target_path: [name], target_lifetime: lifetimes.get(JSON.stringify([name])), event: 'click', script });
    host.dispatchScripts([name]);
  }
  return { ...result, click, lifetimes, enqueue: item => queue.push(item), script };
}

export function verifyHostIntegration(root, scriptFixture, code) {
  const { host, click, lifetimes, enqueue, script, traceCalls, hostConsole, scriptConsole } = createHostHarness(root, scriptFixture, code);
  click('blue');
  const blue = host.proxyFor(['blue']), savedIncrement = blue.increment;
  assert.equal(host.lastTrace(), 'Blue 1');
  click('blue');
  assert.equal(host.lastTrace(), 'Blue 2');
  click('red');
  const red = host.proxyFor(['red']);
  assert.equal(blue.count, 2); assert.equal(red.count, 1);
  assert.equal(savedIncrement.call(red), 3); assert.equal(red.count, 1);
  lifetimes.set('["blue"]', 4);
  const replacement = host.proxyFor(['blue']);
  assert.notEqual(blue, replacement); assert.equal(replacement.count, undefined);
  click('blue'); assert.equal(replacement.count, 1);
  enqueue({ target_path: ['blue'], target_lifetime: 2, event: 'click', script });
  host.dispatchScripts(['blue']); assert.equal(replacement.count, 1);
  assert.equal(savedIncrement(), 4); assert.equal(replacement.count, 1);
  assert.throws(() => blue.x, /removed or replaced/);
  assert.deepEqual(traceCalls, [['Blue', 1], ['Blue', 2], ['Blue', 1], ['Blue', 1]]);
  assert.deepEqual(hostConsole, traceCalls.map(args => ['[NeoFlash]', ...args]));
  assert.deepEqual(scriptConsole, [], 'AS must not call console directly');
  assert.equal(host.lastTrace(), 'Blue 1');
}

export function verifyHostFailureFormatting(root) {
  const hostile = Object.defineProperty({ toString() { throw new Error('secondary conversion failure'); } }, 'message', {
    get() { throw new Error('secondary getter failure'); },
  });
  const revoked = Proxy.revocable({}, {}); revoked.revoke();
  const fixtures = [
    ['reason', 'reason'], [17, '17'], [null, 'null'], [undefined, 'undefined'],
    [false, 'false'], ['', ''], [Symbol('reason'), 'Symbol(reason)'], [3n, '3'],
    [{ detail: 1 }, '[object Object]'], [new Error('original error'), 'original error'],
    [hostile, '<unprintable thrown value>'], [revoked.proxy, '<unprintable thrown value>'],
    [Object.create(null), '<unprintable thrown value>'],
  ];
  for (const [value, description] of fixtures) for (const branded of [false, true]) {
    const { host, click, traceCalls } = createHostHarness(root,
      { language: 'java_script', source: 'throw this.payload;' }, 'throw this.payload;');
    host.proxyFor(['blue']).payload = branded ? { [Symbol.for('neoflash.as2.thrown.v1')]: true, value } : value;
    assert.throws(() => click('blue'), error => {
      assert.equal(error.message, `AS3 keyboard integration: ${description}`);
      assert.equal(error.cause, value, 'diagnostics must retain the exact thrown value');
      return true;
    });
    assert.deepEqual(traceCalls, [], 'failed dispatch must not invent a trace');
  }
}
