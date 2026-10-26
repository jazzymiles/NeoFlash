// Execute the real host proxy/dispatch implementation with a small Player stub.
// Console, host trace and on-page feedback have independent observations so a
// compiler that bypasses host trace cannot accidentally pass this regression.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url);

export function verifyHostIntegration(root, { language, source }, code) {
  const path = resolve(root, 'web/runtime.js');
  const runtime = readFileSync(path, 'utf8');
  const { sourceKey, oneBasedFrame } = require(path);
  const start = runtime.indexOf('    const propertyMap =');
  const end = runtime.indexOf('    function resize()', start);
  assert.ok(start >= 0 && end > start, 'host implementation boundaries changed');
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
  const scriptConsole = [], hostConsole = [], traceCalls = [];
  const bindings = ['event', '_root', '_parent', 'trace', 'play', 'stop', 'gotoAndPlay', 'gotoAndStop', 'nextFrame', 'prevFrame'];
  const fn = new Function('console', `return function(${bindings.join(',')}) { "use strict"; return (function(){\n${code}\n}).call(this); }`)({ log: (...args) => scriptConsole.push(args) });
  const functions = new Map([[sourceKey(script), { fn, location: 'AS3 keyboard integration' }]]);
  const host = new Function('player', 'functions', 'console', 'sourceKey', 'oneBasedFrame', 'traceSpy',
    'const proxies = new Map(); let lastTrace = "";\n' + runtime.slice(start, end) +
    '\nconst originalTrace = trace; trace = (...args) => { traceSpy(...args); return originalTrace(...args); };\n' +
    'return { proxyFor, dispatchScripts, lastTrace: () => lastTrace };'
  )(player, functions, { log: (...args) => hostConsole.push(args) }, sourceKey, oneBasedFrame, (...args) => traceCalls.push(args));
  function click(name) {
    queue.push({ target_path: [name], target_lifetime: lifetimes.get(JSON.stringify([name])), event: 'click', script });
    host.dispatchScripts([name]);
  }
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
  queue.push({ target_path: ['blue'], target_lifetime: 2, event: 'click', script });
  host.dispatchScripts(['blue']); assert.equal(replacement.count, 1);
  assert.equal(savedIncrement(), 4); assert.equal(replacement.count, 1);
  assert.throws(() => blue.x, /removed or replaced/);
  assert.deepEqual(traceCalls, [['Blue', 1], ['Blue', 2], ['Blue', 1], ['Blue', 1]]);
  assert.deepEqual(hostConsole, traceCalls.map(args => ['[NeoFlash]', ...args]));
  assert.deepEqual(scriptConsole, [], 'AS must not call console directly');
  assert.equal(host.lastTrace(), 'Blue 1');
}
