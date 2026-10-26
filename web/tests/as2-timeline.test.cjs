const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const { resolve } = require('node:path');
const { homedir } = require('node:os');
const { spawnSync } = require('node:child_process');
const { as2Frame, createAs2Methods } = require('../runtime.js');
const root = resolve(__dirname, '../..');

const scripts = [
  { language: 'action_script2', source: 'gotoAndStop("missing"); trace(_currentframe);' },
  { language: 'action_script2', source: 'stop(); gotoAndPlay("missing"); trace(_currentframe);' },
  { language: 'action_script2', source: 'gotoAndStop("END"); gotoAndPlay("BADÀ"); trace(_currentframe);' },
  { language: 'action_script2', source: 'clip.run = function() { gotoAndStop("END"); trace(_currentframe); };' },
  { language: 'action_script2', source: 'run();' },
  { language: 'java_script', source: 'this.run();' },
  { language: 'action_script3', source: 'var callback:Function = run; callback();' },
  { language: 'action_script2', source: 'missing();' },
  { language: 'java_script', source: 'this.gotoAndStop("END");' },
  { language: 'action_script3', source: 'gotoAndStop("END");' },
  { language: 'action_script2', source: 'child.gotoAndStop("END");' },
  { language: 'action_script2', source: 'function call(gotoAndStop) { var jump = gotoAndStop; jump("END"); } call(function(value) { trace(value); });' },
  { language: 'action_script2', source: 'var jump = clip["gotoAndStop"]; jump("END");' },
  { language: 'action_script2', source: 'function call() { var jump = clip["gotoAndStop"]; jump("END"); } call();' },
  { language: 'action_script2', source: 'run = clip["gotoAndStop"]; run("END");' },
  { language: 'action_script2', source: 'var tools = {jump:function() {}}; tools.jump = clip["gotoAndStop"]; tools.jump("END");' },
];
let compiled;
function compiledScripts() {
  if (compiled) return compiled;
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const result = spawnSync(process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo'),
    ['run', '--quiet', '--locked', '-p', 'neoflash-script', '--example', 'compile_batch'],
    { cwd: root, encoding: 'utf8', input: JSON.stringify(scripts), maxBuffer: 8 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const outputs = JSON.parse(result.stdout);
  compiled = scripts.map((script, index) => {
    assert.ok(outputs[index].Ok, JSON.stringify(outputs[index]));
    return { script, code: outputs[index].Ok, location: `timeline fixture ${index}` };
  });
  return compiled;
}
async function harness() {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const frames = new Map([['[]', 0], ['["clip"]', 0]]);
  const playing = new Map([['[]', true], ['["clip"]', true]]);
  const controls = [];
  const labels = [[4, 'end'], [5, 'SomeFrame'], [2, 'SomeFrame'], [6, 'BADà']];
  const ascii = text => text.replace(/[A-Z]/g, c => c.toLowerCase());
  let queue = [];
  const player = {
    lifetime_id: key => frames.has(key) ? (key === '[]' ? 1 : 2) : 0,
    child_path: (key, name) => JSON.stringify(key === '[]' && name === 'clip' ? ['clip'] : null),
    clip_frame: key => frames.get(key), clip_frame_count: () => 7,
    control(key, command, frame) {
      controls.push([key, command, frame]);
      if (command.startsWith('goto_')) frames.set(key, frame);
      playing.set(key, command === 'play' || command === 'goto_and_play');
      return true;
    },
    resolve_frame: (_key, label) => labels.find(item => item[1] === label)?.[0] ?? -1,
    resolve_frame_as2: (_key, label) => labels.filter(item => ascii(item[1]) === ascii(label)).map(item => item[0]).sort((a, b) => a - b)[0] ?? -1,
    scripts_json() { const result = queue; queue = []; return JSON.stringify(result); },
    is_dispatch_current: () => true, script_overflowed: () => false,
  };
  const result = createRuntimeHost(root, player, compiledScripts());
  function dispatch(index, path = []) {
    queue.push({ target_path: path, target_lifetime: path.length ? 2 : 1, event: { frame: frames.get(JSON.stringify(path)) }, script: scripts[index] });
    result.host.dispatchScripts();
  }
  return { ...result, dispatch, frames, playing, controls };
}

test('AS2 missing labels preserve playing and stopped timelines', async () => {
  const h = await harness();
  h.dispatch(0);
  assert.equal(h.frames.get('[]'), 0); assert.equal(h.playing.get('[]'), true);
  assert.deepEqual(h.controls, []);
  h.dispatch(1);
  assert.equal(h.frames.get('[]'), 0); assert.equal(h.playing.get('[]'), false);
  assert.deepEqual(h.controls, [['[]', 'stop', 0]]);
  h.dispatch(2);
  assert.equal(h.frames.get('[]'), 4); assert.equal(h.playing.get('[]'), false);
  assert.deepEqual(h.traceCalls, [[1], [1], [5]]);
});

test('AS2 closure keeps its root timeline and language across AS2, JS and AS3 callers', async () => {
  for (const caller of [4, 5, 6]) {
    const h = await harness();
    h.dispatch(3);
    h.dispatch(caller, ['clip']);
    assert.equal(h.frames.get('[]'), 4);
    assert.equal(h.frames.get('["clip"]'), 0);
    assert.deepEqual(h.traceCalls, [[5]]);
  }
});

test('unknown dynamic AS2 calls fail with their source location, while JS/AS3 labels remain strict', async () => {
  const h = await harness();
  assert.throws(() => h.dispatch(7), /timeline fixture 7: AS2 timeline function 'missing'.*Define it/);
  for (const caller of [8, 9]) assert.throws(() => h.dispatch(caller), /Frame label 'END' does not exist/);
  h.host.proxyFor([]).missing = 12;
  assert.throws(() => h.dispatch(7), /is missing or is not callable/);
});

test('computed and renamed AS2 goto aliases fail explicitly without changing the target', async () => {
  for (const caller of [12, 13, 14, 15]) {
    const h = await harness();
    assert.throws(() => h.dispatch(caller), /goto method aliases need receiver semantics/);
    assert.deepEqual(h.controls, []);
  }
});

test('AS2 call bridge preserves user methods and getter/argument evaluation order', async () => {
  const h = await harness();
  const calls = [];
  const target = { gotoAndStop(label) { calls.push([this, label]); } };
  h.host.proxyFor([]).child = target;
  h.dispatch(10);
  assert.deepEqual(calls, [[target, 'END']]);
  const bridge = createAs2Methods();
  const order = [];
  const receiver = { get run() { order.push('get'); return function(value) { order.push('call'); assert.equal(this, receiver); return value; }; } };
  assert.equal(bridge.method(receiver, 'run')((order.push('argument'), 17)), 17);
  assert.deepEqual(order, ['get', 'argument', 'call']);
  let evaluated = false;
  assert.throws(() => bridge.method({}, 'missing')((evaluated = true)), /missing/);
  assert.equal(evaluated, true);
  assert.equal(as2Frame('missing', 7, () => -1), null);
  assert.equal(as2Frame('SomeFrame', 7, () => 2), 2);
  assert.equal(as2Frame('3', 7, () => 5), 2, 'numeric strings take precedence over labels');
  assert.equal(as2Frame('3.0', 7, () => 5), 2);
  for (const value of [0, -1, 8, 1.5, Infinity, '0', '8', '4294967299', '1.5']) {
    assert.throws(() => as2Frame(value, 7, () => 5), /legacy clamping, wrapping/);
  }
  h.dispatch(11);
  assert.deepEqual(h.traceCalls, [['END']], 'a local parameter shadows the built-in without acquiring timeline behavior');
});
