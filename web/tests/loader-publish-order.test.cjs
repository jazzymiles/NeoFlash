const assert = require('node:assert/strict');
const test = require('node:test');

const events = require('../events.js');
const loader = require('../loader.js');

class URLRequest {
  constructor(url) {
    this.url = String(url);
    this.method = 'GET';
    this.data = null;
    this.contentType = null;
    this.requestHeaders = [];
  }
}
Object.defineProperty(URLRequest, '__nfIs', { value: value => value instanceof URLRequest });

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function nextTurn() {
  return new Promise(resolve => setImmediate(resolve));
}

test('loadPublish waits for an asynchronous child mount before init and completion', async () => {
  const target = {};
  const references = new WeakMap([[target, { objectRef: ['holder'] }]]);
  const mount = deferred();
  const order = [], failures = [];
  let afterMountFailure = null, closeFailure = null, resetDuringMount = false;
  const bundleBytes = new TextEncoder().encode(
    `globalThis.NeoFlashBundle = ${JSON.stringify({ project: { symbols: [] }, scripts: [] })};`,
  );
  const host = loader.create({
    player: { pixel_decode() {} },
    events,
    net: {
      URLRequest,
      loadBytes(_request, callbacks) {
        queueMicrotask(() => callbacks.onComplete(bundleBytes));
        return { close() { if (closeFailure) throw closeFailure; } };
      },
    },
    binary: { toBytes: value => value },
    display: {
      alive: value => value === target,
      descriptor: () => ({ owner: 'movie_clip', authored_path: ['holder'] }),
    },
    proxyForRef: () => null,
    referenceOf: value => references.get(value),
    mountPublishedBundle(_record, _bundle, _url, isCurrent) {
      order.push('mount-requested');
      assert.equal(isCurrent(), true);
      return mount.promise;
    },
    onAfterMountPublished() {
      order.push('after-mount');
      if (resetDuringMount) host.reset();
      if (afterMountFailure) throw afterMountFailure;
    },
    onError: error => failures.push(error),
  });

  let settled = false;
  const published = host.loadPublish('child/bundle.js', target).then(value => {
    settled = true;
    order.push('resolved');
    return value;
  });
  await nextTurn();
  assert.deepEqual(order, ['mount-requested']);
  assert.equal(settled, false, 'loadPublish stays pending while embedded fonts delay child mount');

  mount.resolve({ contentRef: null, events: [], width: 320, height: 180, frameRate: 24 });
  assert.equal(await published, target);
  assert.deepEqual(order, ['mount-requested', 'after-mount', 'resolved']);

  for (const cleanupThrows of [false, true]) {
    afterMountFailure = new Error('child authored init failed');
    closeFailure = cleanupThrows ? new Error('transport close failed') : null;
    let outcome = 'pending';
    void host.loadPublish('child/bundle.js', target).then(
      value => { outcome = { value }; }, error => { outcome = { error }; },
    );
    await nextTurn();
    assert.deepEqual(outcome, { error: afterMountFailure }, 'failed init must settle the helper promise with the original error');
    assert.equal(failures.at(-1), afterMountFailure);
    if (cleanupThrows) assert.equal(failures.at(-2), closeFailure);
    afterMountFailure = closeFailure = null;
    assert.equal(await host.loadPublish('child/bundle.js', target), target, 'a subsequent load still completes');
  }
  const reported = failures.length;
  resetDuringMount = true;
  afterMountFailure = new Error('old generation failed after Restart');
  await assert.rejects(host.loadPublish('child/bundle.js', target), /cancelled/);
  await nextTurn();
  assert.equal(failures.length, reported, 'an old callback must not report into the replacement generation');
  resetDuringMount = false;
  afterMountFailure = null;
  assert.equal(await host.loadPublish('child/bundle.js', target), target);
  host.dispose();
});

test('cancelled asynchronous child mounts retire every load despite cleanup failure', async () => {
  const target = {}, other = {};
  const references = new WeakMap([[target, { objectRef: ['holder'] }], [other, { objectRef: ['other'] }]]);
  const mounts = [], transports = [];
  let afterMount = 0;
  const bundleBytes = new TextEncoder().encode(
    `globalThis.NeoFlashBundle = ${JSON.stringify({ project: { symbols: [] }, scripts: [] })};`,
  );
  const host = loader.create({
    player: { pixel_decode() {} },
    events,
    net: {
      URLRequest,
      loadBytes(_request, callbacks) {
        const transport = { callbacks, closed: false, failure: null, close() { this.closed = true; if (this.failure) throw this.failure; } };
        transports.push(transport);
        queueMicrotask(() => callbacks.onComplete(bundleBytes));
        return transport;
      },
    },
    binary: { toBytes: value => value },
    display: {
      alive: value => references.has(value),
      descriptor: () => ({ owner: 'movie_clip', authored_path: ['holder'] }),
    },
    proxyForRef: () => null,
    referenceOf: value => references.get(value),
    mountPublishedBundle(_record, _bundle, _url, guard) {
      const mount = deferred();
      mounts.push({ ...mount, guard });
      return mount.promise;
    },
    onAfterMountPublished() {
      afterMount++;
    },
  });

  const mounted = { contentRef: null, events: [], width: 320, height: 180, frameRate: 24 };
  for (const cleanup of ['reset', 'dispose']) {
    const pending = [host.loadPublish('child/bundle.js', target), host.loadPublish('child/bundle.js', other)];
    const cancelled = Promise.all(pending.map(promise => assert.rejects(promise, /cancelled/i)));
    await nextTurn();
    const oldMounts = mounts.slice(-2), oldTransports = transports.slice(-2), before = afterMount;
    const failure = new Error(`first transport ${cleanup} failed`);
    oldTransports[0].failure = failure;
    assert.throws(() => host[cleanup](), error => error === failure);
    await cancelled;
    assert.deepEqual(oldTransports.map(transport => transport.closed), [true, true]);
    assert.deepEqual(oldMounts.map(mount => mount.guard()), [false, false]);
    for (const mount of oldMounts) mount.resolve(mounted);
    for (const transport of oldTransports) transport.callbacks.onComplete(bundleBytes);
    await nextTurn();
    assert.equal(afterMount, before, 'stale completions cannot dispatch child init after either cleanup path fails');
    if (cleanup === 'reset') {
      const retry = host.loadPublish('child/bundle.js', target);
      await nextTurn();
      mounts.at(-1).resolve(mounted);
      assert.equal(await retry, target, 'a failed reset still leaves a usable new session');
    }
  }
  assert.doesNotThrow(() => host.dispose(), 'failed disposal still retires the service exactly once');
  assert.throws(() => host.loadPublish('child/bundle.js', target), /disposed/);
});

test('a failing byte-load callback cannot cancel the replacement it just started', () => {
  const target = {}, jobs = [], failures = [];
  const original = new Error('old open listener failed');
  const bytes = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
  const host = loader.create({
    player: {
      create_display: () => '["holder"]',
      pixel_decode: () => '["pixels"]',
      bitmap_info: () => '{"width":1,"height":1}',
      loader_replace: () => '{"content_ref":null,"events":[]}',
    },
    events,
    net: { URLRequest, loadBytes() {}, ProgressEvent: class extends events.Event {} },
    binary: { toBytes: value => value },
    display: { alive: value => value === target },
    proxyForRef: () => target,
    referenceOf: value => value === target ? { objectRef: ['holder'] } : null,
    schedule: callback => jobs.push(callback),
    onError: error => failures.push(error),
  });
  const instance = new host.Loader(), methods = host.methods(instance);
  const info = host.get(instance, 'contentLoaderInfo');
  let first = true, completed = 0;
  info.addEventListener('open', () => {
    if (!first) return;
    first = false;
    methods.loadBytes(bytes);
    throw original;
  });
  info.addEventListener('complete', () => completed++);
  methods.loadBytes(bytes);
  assert.equal(jobs.length, 1);
  jobs.shift()();
  assert.deepEqual(failures, [original]);
  assert.equal(jobs.length, 1, 'the replacement has its own scheduled delivery');
  jobs.shift()();
  assert.equal(completed, 1, 'only the replacement reaches COMPLETE');
  host.dispose();
});
