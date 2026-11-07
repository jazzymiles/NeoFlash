// Harness protocol checks using tiny fake tools; no Rust builds or WASM required.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const { runCargo } = require('../test-cargo.cjs');

function fixture(t) {
  const root = fs.mkdtempSync(join(tmpdir(), 'neoflash-test-cargo-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const cargo = join(root, 'fake-cargo');
  const executable = join(root, 'separate target', 'actual tool');
  const log = join(root, 'calls.jsonl');
  fs.mkdirSync(join(root, 'separate target'));
  fs.mkdirSync(join(root, 'cargo-home'));
  fs.writeFileSync(cargo, `#!/usr/bin/env node
const fs = require('node:fs');
const args = process.argv.slice(2), input = fs.readFileSync(0, 'utf8');
fs.appendFileSync(process.env.TEST_CARGO_LOG, JSON.stringify({args,input})+'\\n');
if (args[0] !== 'build') { process.stdout.write(JSON.stringify({fallback:true,input,args})); process.exit(0); }
if (process.env.TEST_BUILD_FAIL) {
  console.log(JSON.stringify({reason:'compiler-message',message:{rendered:'compiler diagnostic\\n'}}));
  process.stderr.write('build failed\\n'); process.exit(101);
}
if (!process.env.TEST_MISSING_ARTIFACT) {
  const example = args.includes('--example'), name = args[args.indexOf(example ? '--example' : '-p')+1];
  console.log(JSON.stringify({reason:'compiler-artifact',target:{name:'unrelated',kind:['bin']},executable:'/not-the-tool'}));
  console.log(JSON.stringify({reason:'compiler-artifact',target:{name,kind:[example?'example':'bin']},executable:process.env.TEST_EXECUTABLE}));
}
`, { mode: 0o755 });
  fs.writeFileSync(executable, `#!/usr/bin/env node
const fs = require('node:fs');
process.stdout.write(JSON.stringify({args:process.argv.slice(2),input:fs.readFileSync(0,'utf8'),cwd:process.cwd(),marker:process.env.TEST_MARKER,
  deployment:process.env.TEST_REPORT_DEPLOYMENT?process.env.MACOSX_DEPLOYMENT_TARGET:undefined}));
if (process.argv.includes('--fail')) { process.stderr.write('application failure\\n'); process.exit(23); }
`, { mode: 0o755 });
  const env = { ...process.env, CARGO_HOME: join(root, 'cargo-home'),
    TEST_CARGO_LOG: log, TEST_EXECUTABLE: executable, TEST_MARKER: 'retained' };
  // Keep these tests native regardless of the developer's normal Rust flags.
  for (const key of Object.keys(env)) {
    if (/RUSTFLAGS$/.test(key) || ['CARGO_BUILD_TARGET', 'CARGO_INCREMENTAL'].includes(key)
        || /^CARGO_TARGET_.*_RUNNER$/.test(key)) delete env[key];
  }
  const options = { cwd: root, env, encoding: 'utf8', maxBuffer: 1024 * 1024 };
  const args = ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--'];
  const calls = () => fs.readFileSync(log, 'utf8').trim().split('\n').map(JSON.parse);
  return { cargo, executable, options, args, calls };
}

test('one Cargo build serves repeated real tool invocations without consuming stdin or changing arguments', t => {
  const f = fixture(t);
  for (const input of ['first fixture', 'second fixture']) {
    const result = runCargo(f.cargo, [...f.args, 'input with spaces;$(literal).nflash', '--output', 'out'], { ...f.options, input });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), { args: ['input with spaces;$(literal).nflash', '--output', 'out'],
      input, cwd: fs.realpathSync(f.options.cwd), marker: 'retained' });
  }
  assert.equal(f.calls().length, 1);
  assert.equal(f.calls()[0].input, '', 'only the executable receives fixture JSON');
  assert.ok(f.calls()[0].args.includes('--locked'));
  assert.ok(f.calls()[0].args.includes('--message-format=json'));
});

test('build environment and compiler example identity never share cached artifacts', t => {
  const f = fixture(t);
  const variants = [{}, { RUSTFLAGS: '--cfg harness_check' }, {}, { CARGO_TARGET_DIR: join(f.options.cwd, 'other target') },
    { CARGO_INCREMENTAL: '0' }, { CARGO_INCREMENTAL: '1' }];
  for (const changed of variants) {
    const options = { ...f.options, env: { ...f.options.env, ...changed } };
    for (let i = 0; i < 2; i++) assert.equal(runCargo(f.cargo, f.args, options).status, 0);
  }
  for (const name of ['compile_batch', 'compile_project']) {
    const args = ['run', '--quiet', '-p', 'neoflash-script', '--example', name];
    for (let i = 0; i < 2; i++) assert.equal(runCargo(f.cargo, args, f.options).status, 0);
  }
  assert.equal(f.calls().length, 8, 'A → B → A rebuilds when environments overwrite one executable path');
  assert.ok(f.calls().every(call => call.args.includes('--locked')));
});

test('build failures remain failures, are retryable, and cannot leak artifact JSON as program output', t => {
  const f = fixture(t);
  const options = { ...f.options, env: { ...f.options.env, TEST_BUILD_FAIL: '1' } };
  for (let i = 0; i < 2; i++) {
    const result = runCargo(f.cargo, f.args, options);
    assert.equal(result.status, 101);
    assert.equal(result.stdout, '');
    assert.match(result.stderr, /build failed/);
    assert.match(result.stderr, /compiler diagnostic/);
  }
  assert.equal(f.calls().length, 2);
  const missing = runCargo(f.cargo, f.args, { ...f.options, env: { ...f.options.env, TEST_MISSING_ARTIFACT: '1' } });
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /did not produce/);
  const absentCargo = runCargo(join(f.options.cwd, 'absent-cargo'), f.args, f.options);
  assert.equal(absentCargo.error.code, 'ENOENT');
  assert.equal(absentCargo.status, null);
});

test('application errors and non-native Cargo fallback retain the original process result', t => {
  const f = fixture(t);
  const result = runCargo(f.cargo, [...f.args, '--fail'], { ...f.options, input: 'unchanged' });
  assert.equal(result.status, 23);
  assert.equal(result.stderr, 'application failure\n');
  assert.equal(JSON.parse(result.stdout).input, 'unchanged');
  const options = { ...f.options, input: 'runner input', env: { ...f.options.env, CARGO_BUILD_TARGET: 'custom-target' } };
  const fallback = runCargo(f.cargo, f.args, options);
  assert.deepEqual(JSON.parse(fallback.stdout), { fallback: true, input: 'runner input', args: f.args });
});

test('Cargo config runners and multiline flags fall back, including ancestor and Cargo home files', t => {
  const f = fixture(t);
  const directory = join(f.options.cwd, '.cargo');
  fs.mkdirSync(directory);
  const path = join(directory, 'config.toml');
  const safe = '[env]\nMACOSX_DEPLOYMENT_TARGET = "15.0"\n';
  fs.writeFileSync(path, safe);
  const nested = join(f.options.cwd, 'nested', 'workspace');
  fs.mkdirSync(nested, { recursive: true });
  const env = { ...f.options.env, TEST_REPORT_DEPLOYMENT: '1' };
  delete env.MACOSX_DEPLOYMENT_TARGET;
  const options = { ...f.options, cwd: nested, env, input: 'config fixture' };
  const initial = runCargo(f.cargo, f.args, options);
  assert.equal(initial.status, 0, initial.stderr);
  assert.equal(JSON.parse(initial.stdout).deployment, '15.0', 'known-safe [env] reaches the executable');
  fs.writeFileSync(path, `[target.'cfg(unix)']\nrunner = [\n  "custom-runner",\n  "--flag",\n]\n`);
  const fallback = runCargo(f.cargo, f.args, options);
  assert.deepEqual(JSON.parse(fallback.stdout), { fallback: true, input: 'config fixture', args: f.args });
  fs.writeFileSync(path, safe);
  assert.equal(runCargo(f.cargo, f.args, options).status, 0);
  assert.deepEqual(f.calls().map(call => call.args[0]), ['build', 'run', 'build'],
    'a Cargo fallback invalidates cached paths before returning to a safe configuration');
  fs.writeFileSync(join(env.CARGO_HOME, 'config'), '[build]\nrustflags = [\n  "-C", "prefer-dynamic",\n]\n');
  assert.equal(JSON.parse(runCargo(f.cargo, f.args, options).stdout).fallback, true,
    'legacy-named Cargo home config is included in the conservative check');
});

test('workflow selection preserves default coverage/order and rejects invalid or duplicate IDs before running anything', async () => {
  const { selectWorkflows } = await import('../../crates/neoflash-script/tests/workflow-selection.mjs');
  const workflows = [['first', () => {}], ['second', () => {}], ['third', () => {}]];
  assert.strictEqual(selectWorkflows(workflows, undefined), workflows);
  assert.deepEqual(selectWorkflows(workflows, ' third, first '), [workflows[0], workflows[2]]);
  for (const value of ['', 'unknown', 'first,first', 'first,']) {
    assert.throws(() => selectWorkflows(workflows, value), /requires unique comma-separated workflow IDs/);
  }
});
