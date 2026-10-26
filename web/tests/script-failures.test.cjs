const test = require('node:test');
const assert = require('node:assert/strict');
const { resolve } = require('node:path');
const { readFileSync } = require('node:fs');
const { describeThrown, unwrapScriptThrow } = require('../runtime.js');

test('actual script dispatch preserves arbitrary thrown values without secondary failures', async () => {
  const { verifyHostFailureFormatting } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  verifyHostFailureFormatting(resolve(__dirname, '../..'));
});

test('AS2 boundary only unwraps own data properties and never invokes transport accessors', () => {
  const brand = Symbol.for('neoflash.as2.thrown.v1');
  let reads = 0;
  const accessor = Object.defineProperties({}, {
    [brand]: { get() { reads++; throw new Error('brand access'); } },
    value: { get() { reads++; throw new Error('payload access'); } },
  });
  const inherited = Object.create({ [brand]: true, value: 17 });
  const absent = { [brand]: true };
  const wrong = { [brand]: false, value: 17 };
  for (const value of [accessor, inherited, absent, wrong]) assert.equal(unwrapScriptThrow(value), value);
  assert.equal(reads, 0);
  const nested = { [brand]: true, value: 17 };
  assert.equal(unwrapScriptThrow({ [brand]: true, value: nested }), nested, 'unwrap exactly one transport layer');
});

test('actual failure panel keeps the original error when stack/message/conversion are hostile', () => {
  const runtime = readFileSync(resolve(__dirname, '../runtime.js'), 'utf8');
  const start = runtime.indexOf("  const errorElement = document.getElementById('error');");
  const end = runtime.indexOf('  async function boot()', start);
  assert.ok(start >= 0 && end > start);
  const elements = { error: { style: {}, textContent: '' }, loading: { style: {} } };
  const errors = [];
  const { fail, failed } = new Function('document', 'console', 'describeThrown',
    runtime.slice(start, end) + '\nreturn { fail, failed: () => failed };'
  )({ getElementById: id => elements[id] }, { error: value => errors.push(value) }, describeThrown);
  const hostile = new Proxy({}, { get() { throw new Error('secondary failure'); } });
  fail(hostile);
  assert.equal(failed(), true);
  assert.equal(elements.loading.style.display, 'none');
  assert.equal(elements.error.style.display, 'block');
  assert.equal(elements.error.textContent, 'NeoFlash could not run this project.\n\n<unprintable thrown value>');
  assert.equal(errors[0], hostile);
  const ordinary = new Error('original error');
  fail(ordinary);
  assert.equal(elements.error.textContent, 'NeoFlash could not run this project.\n\n' + ordinary.stack);
  assert.equal(describeThrown({ get stack() { throw 1; }, message: 'surviving reason' }, true), 'surviving reason');
});
