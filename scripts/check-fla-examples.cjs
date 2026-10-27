// Real-WASM regression checks for the original XFL/FLA examples. Build web runtime first.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.resolve(__dirname, '..');
const runtime = path.join(root, 'target/web-runtime');
const context = vm.createContext({TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response,
  trace() { throw new Error('WASM must queue source, not execute trace()'); }});
vm.runInContext(fs.readFileSync(path.join(runtime, 'neoflash_web.js'), 'utf8'), context);
const wasm = vm.runInContext('NeoFlashWasm', context);
context.wasmBytes = fs.readFileSync(path.join(runtime, 'neoflash_web_bg.wasm'));
const ready = vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', context);
async function withExample(id, check) {
  await ready;
  const source = path.join(root, 'examples/fla-corpus/authored', id, `${id}.nflash`);
  const player = new wasm.WebPlayer(fs.readFileSync(source, 'utf8'));
  try { check(player); } finally { player.free(); }
}
const scene = player => JSON.parse(player.scene_json());
const drain = player => JSON.parse(player.scripts_json());
const ticks = (player, count) => { for (let i = 0; i < count; i++) player.tick(); };
const child = (player, parent, name) => {
  const result = JSON.parse(player.child_path(JSON.stringify(parent), name));
  assert.ok(Array.isArray(result), `Missing instance ${name}`);
  return result;
};
function pose(player, instance, position, fill, alpha = 1) {
  const items = scene(player).filter(item => instance.every((id, i) => item.instance_path[i] === id));
  assert.equal(items.length, 1);
  assert.equal(items[0].kind.type, 'ellipse');
  assert.deepEqual(items[0].transform.slice(4), position);
  assert.deepEqual(items[0].kind.fill, [...fill, 255]);
  assert.equal(items[0].alpha, alpha);
}

test('imported primitives hold, disappear and return with labels and fill alpha intact', () =>
  withExample('primitives-and-holds', player => {
    assert.equal(player.total_frames(), 72);
    for (const [label, frame] of Object.entries({left:0, right:24, blank:48, return:60}))
      assert.equal(player.resolve_frame('[]', label), frame);
    for (const [frame, x] of [[0,70],[23,70],[24,370],[47,370],[48,null],[59,null],[60,70],[71,70]]) {
      assert.equal(player.control('[]', 'goto_and_stop', frame), true);
      const items = scene(player);
      assert.equal(items.filter(item => item.kind.type === 'text_field').length, 3);
      const shapes = items.filter(item => item.kind.type !== 'text_field');
      assert.equal(shapes.length, x === null ? 0 : 2, `Frame ${frame}`);
      if (x !== null) {
        const square = shapes.find(item => item.name === 'coralSquare');
        const circle = shapes.find(item => item.name === 'mintCircle');
        assert.deepEqual(square.transform.slice(4), [x,155]);
        assert.deepEqual(circle.transform.slice(4), [x+140,165]);
        assert.deepEqual(circle.kind.fill, [100,223,190,204]);
      }
      assert.deepEqual(drain(player), []);
    }
  }));

test('imported nested playback keeps four modes distinct after 24 ticks and a parent stop', () =>
  withExample('nested-playback-as2', player => {
    const pair = child(player, [], 'pair');
    const live = child(player, pair, 'liveOrb'), frozen = child(player, pair, 'frozenOrb');
    const loop = child(player, [], 'loop'), once = child(player, [], 'once'), still = child(player, [], 'still');
    const startup = drain(player);
    assert.deepEqual(startup.map(item => item.script), [
      {language:'action_script2', source:'trace("AS2: root frame 1");'},
      {language:'action_script2', source:'trace("AS2: nested Pair frame 1");'},
    ]);
    assert.deepEqual(startup.map(item => item.target_path), [[],pair]);
    assert.equal(player.resolve_frame(JSON.stringify(live), 'southeast'), 12);
    ticks(player, 24);
    assert.equal(player.current_frame(), 24);
    pose(player, live, [32,190], [100,223,190]);
    pose(player, loop, [290,190], [100,223,190]);
    pose(player, once, [425,244], [128,174,255]);
    pose(player, still, [609,244], [255,132,103]);
    pose(player, frozen, [191,244], [255,132,103], 0.55);
    ticks(player, 6);
    pose(player, live, [86,190], [255,204,102]);
    pose(player, loop, [344,190], [255,204,102]);
    assert.equal(player.control('[]', 'stop', 0), true);
    ticks(player, 6);
    assert.equal(player.current_frame(), 30);
    pose(player, live, [86,244], [255,132,103]); // Independent child advances under a stopped root.
    pose(player, loop, [344,190], [255,204,102]); // Graphic follows the stopped parent.
    pose(player, once, [425,244], [128,174,255]);
    pose(player, still, [609,244], [255,132,103]);
    pose(player, frozen, [191,244], [255,132,103], 0.55);
    assert.deepEqual(drain(player), []);
  }));

test('imported AS3 labels and exact sources dispatch at frames 0/12/24 without executing stop()', () =>
  withExample('frame-scripts-as3', player => {
    const sources = [
      'var stageName:String = "start";\ntrace("AS3: " + stageName);',
      'var amount:int = 2 + 3;\ntrace("AS3: middle " + amount);',
      'trace("AS3: done");\nstop();',
    ];
    for (const [label, frame] of Object.entries({start:0, middle:12, done:24}))
      assert.equal(player.resolve_frame('[]', label), frame);
    for (let frame = 0; frame <= 25; frame++) {
      if (frame) player.tick();
      const pending = drain(player);
      if ([0,12,24].includes(frame)) {
        assert.equal(pending.length, 1);
        assert.deepEqual(pending[0].event, {frame});
        assert.deepEqual(pending[0].target_path, []);
        assert.deepEqual(pending[0].script, {language:'action_script3', source:sources[frame/12]});
        assert.equal(player.is_dispatch_current(JSON.stringify(pending[0])), true);
        const card = scene(player).find(item => item.name === 'stageCard');
        assert.equal(card.transform[4], [45,290,535][frame/12]);
      } else assert.deepEqual(pending, []);
    }
    assert.equal(player.current_frame(), 25); // Browser host must execute the queued AS3 stop().
    assert.equal(player.is_playing(), true);
    assert.equal(player.script_overflowed(), false);
  }));
