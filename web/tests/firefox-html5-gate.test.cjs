'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const GATE_MODULE_URL = pathToFileURL(
  path.resolve(__dirname, '../../scripts/verify-firefox-html5-gate.mjs'),
).href;

test('missing Firefox binary fails by default and skips only with NEOFLASH_ALLOW_MISSING_FIREFOX=1', async () => {
  const gate = await import(GATE_MODULE_URL);
  const savedBin = process.env.NEOFLASH_GATE_FIREFOX;
  const savedFallback = process.env.FIREFOX_BIN;
  const savedAllow = process.env.NEOFLASH_ALLOW_MISSING_FIREFOX;

  try {
    process.env.NEOFLASH_GATE_FIREFOX = '/nonexistent/path/to/firefox-binary-that-does-not-exist';
    delete process.env.FIREFOX_BIN;
    delete process.env.NEOFLASH_ALLOW_MISSING_FIREFOX;

    assert.equal(gate.resolveFirefoxBinary(), null);
    await assert.rejects(
      () => gate.runFirefoxHtml5Gate({ quiet: true }),
      /Firefox binary not found/,
    );

    process.env.NEOFLASH_ALLOW_MISSING_FIREFOX = '1';
    const skipped = await gate.runFirefoxHtml5Gate({ quiet: true });
    assert.equal(skipped.skipped, true);
    assert.deepEqual(skipped.scenarios, []);
  } finally {
    if (savedBin === undefined) delete process.env.NEOFLASH_GATE_FIREFOX;
    else process.env.NEOFLASH_GATE_FIREFOX = savedBin;
    if (savedFallback === undefined) delete process.env.FIREFOX_BIN;
    else process.env.FIREFOX_BIN = savedFallback;
    if (savedAllow === undefined) delete process.env.NEOFLASH_ALLOW_MISSING_FIREFOX;
    else process.env.NEOFLASH_ALLOW_MISSING_FIREFOX = savedAllow;
  }
});

test('real Firefox executes Full/Balanced/Compact file:// and Shared HTTP across Canvas and DOM', async t => {
  const gate = await import(GATE_MODULE_URL);
  const firefoxBin = gate.resolveFirefoxBinary();

  if (!firefoxBin && process.env.NEOFLASH_ALLOW_MISSING_FIREFOX === '1') {
    t.skip('Firefox binary absent and NEOFLASH_ALLOW_MISSING_FIREFOX=1 is set');
    return;
  }

  const report = await gate.runFirefoxHtml5Gate({ quiet: true });
  assert.equal(report.skipped, false);
  assert.match(report.firefoxVersion, /Firefox/i);
  assert.equal(report.scenarioCount, 10);

  const ids = report.scenarios.map(s => s.id);
  for (const expectedId of [
    'file-full-canvas',
    'file-full-dom',
    'file-balanced-canvas',
    'file-balanced-dom',
    'file-compact-canvas',
    'file-compact-dom',
    'http-shared-canvas',
    'http-shared-dom',
    'file-compact-scriptless-canvas',
    'file-compact-scriptless-dom',
  ]) {
    assert.ok(ids.includes(expectedId), `expected Firefox gate scenario ${expectedId}`);
  }
  for (const scenario of report.scenarios) {
    assert.equal(scenario.passed, true, `${scenario.id} must pass`);
  }
});
