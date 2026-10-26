// Execute emitted code in a real JavaScript engine; no golden-string assertions.
// Run from the repository with: node crates/neoflash-script/tests/execute.mjs
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const localCargo = resolve(homedir(), '.cargo/bin/cargo');
const cargo = process.env.CARGO || (existsSync(localCargo) ? localCargo : 'cargo');
const fixtures = [
  {
    language: 'action_script2',
    source: 'var step:Number = 4; _x += step; if (_x >= 8) stop(); trace("stop() _x");',
    context: { _x: 4, stopped: false, stop() { this.stopped = true; } },
    verify(context, logs) {
      assert.equal(context._x, 8); assert.equal(context.step, 4);
      assert.equal(context.stopped, true); assert.deepEqual(logs, [['stop() _x']]);
    },
  },
  {
    language: 'action_script3',
    source: 'var n:int = 2147483647; var before:int = n++; var after:int = ++n;',
    context: {},
    verify(context) {
      assert.equal(context.before, 2147483647);
      assert.equal(context.after, -2147483647); assert.equal(context.n, -2147483647);
    },
  },
  {
    language: 'action_script3',
    source: 'x = 2 + 3 * 4; y = (2 + 3) * 4; sum = 0; for (var i:int = 0; i < 5; i++) { if (i == 2) continue; sum += i; }',
    context: {},
    verify(context) { assert.equal(context.x, 14); assert.equal(context.y, 20); assert.equal(context.sum, 8); },
  },
  {
    language: 'action_script3',
    source: 'var active:Boolean = false; active &&= true; var label:String = null; label = 12; child.gotoAndStop(3);',
    context: (() => {
      let active; const context = { writes: 0, child: { frame: 0, gotoAndStop(frame) { this.frame = frame; } } };
      Object.defineProperty(context, 'active', { get() { return active; }, set(value) { active = value; this.writes++; } });
      return context;
    })(),
    verify(context) {
      assert.equal(context.active, false); assert.equal(context.writes, 1);
      assert.equal(context.label, '12'); assert.equal(context.child.frame, 3);
    },
  },
  {
    language: 'action_script3',
    source: 'var counter:int; counter++;',
    context: {}, repeat: 2,
    verify(context) { assert.equal(context.counter, 2); },
  },
  {
    language: 'type_script',
    source: 'interface Point { x: number; } enum Direction { Left = 2, Right } const p: Point = {x: 4}; this.x = p.x; this.direction = Direction.Right;',
    context: {},
    verify(context) { assert.equal(context.x, 4); assert.equal(context.direction, 3); },
  },
  {
    language: 'java_script',
    source: 'const stop = () => 5; const [a, b] = [stop(), 2]; this.x = a + b; return this.x;',
    context: {},
    verify(context) { assert.equal(context.x, 7); },
  },
];
const result = spawnSync(cargo, ['run', '--quiet', '-p', 'neoflash-script', '--example', 'compile_batch'], {
  cwd: root, encoding: 'utf8', input: JSON.stringify(fixtures.map(({ language, source }) => ({ language, source }))),
});
assert.equal(result.status, 0, result.stderr || result.error?.message);
const outputs = JSON.parse(result.stdout);
for (const [index, fixture] of fixtures.entries()) {
  assert.ok(outputs[index].Ok, JSON.stringify(outputs[index]));
  const logs = [];
  const run = new Function('console', 'stop', '"use strict"; return (function(){\n' + outputs[index].Ok + '\n}).call(this)');
  for (let i = 0; i < (fixture.repeat || 1); i++) {
    run.call(fixture.context, { log: (...args) => logs.push(args) }, () => { throw new Error('wrong binding'); });
  }
  fixture.verify(fixture.context, logs);
}
console.log(`Script execution: ${fixtures.length} behavior fixtures passed.`);
