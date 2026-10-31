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
function compileBatch(list, prefix) {
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const result = spawnSync(process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo'),
    ['run', '--quiet', '--locked', '-p', 'neoflash-script', '--example', 'compile_batch'],
    { cwd: root, encoding: 'utf8', input: JSON.stringify(list), maxBuffer: 8 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const outputs = JSON.parse(result.stdout);
  return list.map((script, index) => {
    assert.ok(outputs[index].Ok, JSON.stringify(outputs[index]));
    return { script, code: outputs[index].Ok, location: `${prefix} ${index}` };
  });
}
function compiledScripts() {
  return compiled ||= compileBatch(scripts, 'timeline fixture');
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

// Two authored scenes run through the real WASM WebPlayer, which flattens them
// into one main timeline; the browser adapter only forwards goto arguments.
test('scenes play as one main timeline with scene-aware goto in AS3 and AS2', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const as3 = source => ({ language: 'action_script3', source });
  const as2 = source => ({ language: 'action_script2', source });
  const sceneScripts = [
    as3('trace(currentScene.name, currentFrame, scenes.length, currentScene.numFrames); gotoAndPlay("middle", "Scene 2");'),
    as3('trace(currentScene.name, currentFrame, currentLabel, totalFrames, currentLabels.length); stop();'),
    as2('gotoAndStop("Scene 1", 3); trace(_currentframe);'),
    as2('prevScene(); trace(_currentframe);'),
  ];
  const key = (frame, label = '') => ({ frame, elements: [], label, tween: null });
  const layer = (id, keys) => ({ id, name: id, visible: true, locked: false, keyframes: keys });
  const scene = (id, name, layers, scripts) => ({ id, name, kind: 'movie_clip', timeline: { frame_count: 3, layers, scripts } });
  const project = { format_version: 6, name: 'Scenes', width: 100, height: 100, fps: 24, background: [255, 255, 255, 255], root: 'scene-1', scenes: ['scene-1', 'scene-2'], symbols: [
    scene('scene-1', 'Scene 1', [layer('intro', [key(0, 'begin')])], [{ frame: 0, script: sceneScripts[0] }]),
    scene('scene-2', 'Scene 2', [layer('main', [key(0), key(1, 'middle')])], [{ frame: 0, script: sceneScripts[3] }, { frame: 1, script: sceneScripts[1] }, { frame: 2, script: sceneScripts[2] }]),
  ] };
  const player = new runtimeWasm.WebPlayer(JSON.stringify(project));
  try {
    assert.equal(player.total_frames(), 6, 'the published main timeline concatenates both scenes');
    const { host, traceCalls } = createRuntimeHost(root, player, compileBatch(sceneScripts, 'scene fixture'));
    host.dispatchScripts();
    assert.deepEqual(traceCalls, [['Scene 1', 1, 2, 3], ['Scene 2', 2, 'middle', 6, 1]]);
    assert.equal(player.current_frame(), 4);
    const rootClip = host.proxyFor([]);
    assert.throws(() => rootClip.gotoAndStop(1, 'Missing'), /Scene 'Missing' was not found\. Available scenes: 'Scene 1', 'Scene 2'/);
    assert.throws(() => rootClip.gotoAndStop('begin', 'Scene 2'), /Frame label 'begin' does not exist in scene 'Scene 2'/);
    assert.equal(player.current_frame(), 4, 'a rejected goto never jumps');
    assert.equal(rootClip._currentframe, 5, 'AS2 _currentframe stays absolute');
    rootClip.nextFrame(); host.dispatchScripts();
    assert.deepEqual(traceCalls.at(-1), [3], 'AS2 gotoAndStop(scene, frame) lands in scene 1');
    assert.equal(player.current_frame(), 2);
    rootClip.nextScene(); host.dispatchScripts();
    assert.deepEqual(traceCalls.slice(-3), [[1], ['Scene 1', 1, 2, 3], ['Scene 2', 2, 'middle', 6, 1]], 'AS2 prevScene returns to scene 1 frame 1');
    rootClip.gotoAndStop(3);
    assert.equal(player.current_frame(), 5, 'AS3 numeric frames are relative to the current scene');
    assert.deepEqual([rootClip.currentScene.name, rootClip.currentFrame, rootClip.scenes.map(item => [item.name, item.numFrames, item.labels.map(label => [label.name, label.frame])])],
      ['Scene 2', 3, [['Scene 1', 3, [['begin', 1]]], ['Scene 2', 3, [['middle', 2]]]]]);
  } finally { player.free(); }
});
