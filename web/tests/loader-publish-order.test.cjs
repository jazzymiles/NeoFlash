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
  const order = [];
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
        return { close() {} };
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
    },
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
  host.dispose();
});

test('a cancelled asynchronous child mount cannot complete stale init', async () => {
  const target = {};
  const references = new WeakMap([[target, { objectRef: ['holder'] }]]);
  const mount = deferred();
  let isCurrent = () => false;
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
        queueMicrotask(() => callbacks.onComplete(bundleBytes));
        return { close() {} };
      },
    },
    binary: { toBytes: value => value },
    display: {
      alive: value => value === target,
      descriptor: () => ({ owner: 'movie_clip', authored_path: ['holder'] }),
    },
    proxyForRef: () => null,
    referenceOf: value => references.get(value),
    mountPublishedBundle(_record, _bundle, _url, guard) {
      isCurrent = guard;
      return mount.promise;
    },
    onAfterMountPublished() {
      afterMount++;
    },
  });

  const published = host.loadPublish('child/bundle.js', target);
  await nextTurn();
  host.dispose();
  await assert.rejects(published, /cancelled/i);
  assert.equal(isCurrent(), false, 'the mount generation guard is invalidated by cancellation');
  mount.resolve({ contentRef: null, events: [], width: 320, height: 180, frameRate: 24 });
  await nextTurn();
  assert.equal(afterMount, 0, 'a stale async mount cannot dispatch child init or Frame 0 work');
});
