const test = require('node:test');
const assert = require('node:assert/strict');
const { Event, MouseEvent, KeyboardEvent, EventPhase, EventDispatcher, create, setStagePoint } = require('../events.js');

function tree(options = {}) {
  const stage = { name: 'stage' }, root = { name: 'root', parent: stage }, child = { name: 'child', parent: root };
  const live = new Set([stage, root, child]);
  const hub = create({ parentOf: node => node.parent, alive: node => live.has(node), ...options });
  for (const node of live) Object.assign(node, hub.methods(node));
  return { stage, root, child, live, hub };
}

test('one event traverses capture, target and bubble with priority, stable target and correct receiver', () => {
  const { stage, root, child } = tree(), seen = [];
  for (const node of [stage, root, child]) {
    for (const capture of [true, false]) node.addEventListener('signal', function (event) {
      seen.push([this.name, event.target.name, event.currentTarget.name, event.eventPhase, capture]);
    }, capture);
  }
  child.addEventListener('signal', event => seen.push(['priority', event.eventPhase]), false, 5);
  const event = new Event('signal', true, true);
  assert.equal(child.dispatchEvent(event), true);
  assert.deepEqual(seen, [
    ['stage', 'child', 'stage', 1, true], ['root', 'child', 'root', 1, true], ['priority', 2],
    ['child', 'child', 'child', 2, false], ['root', 'child', 'root', 3, false], ['stage', 'child', 'stage', 3, false],
  ]);
  assert.equal(event.target, child);
  assert.equal(event.currentTarget, null);
  assert.equal(EventPhase.AT_TARGET, 2);
  assert.equal(child.hasEventListener('signal'), true);
  assert.equal(child.willTrigger('signal'), true);
  root.addEventListener('ancestor-only', () => {});
  assert.equal(child.hasEventListener('ancestor-only'), false);
  assert.equal(child.willTrigger('ancestor-only'), true);
});

test('phase snapshots retain removed listeners and postpone new registrations; duplicate priority stays unchanged', () => {
  const { root, child } = tree(), seen = [];
  const late = () => seen.push('late'), removed = () => seen.push('removed');
  const stable = () => seen.push('stable');
  child.addEventListener('signal', () => {
    seen.push('first'); child.removeEventListener('signal', removed); child.addEventListener('signal', late);
  }, false, 10);
  child.addEventListener('signal', removed);
  child.addEventListener('signal', stable, false, -1);
  child.addEventListener('signal', stable, false, 100);
  root.addEventListener('signal', () => root.addEventListener('signal', () => seen.push('new-bubble')), true);
  child.dispatchEvent(new Event('signal', true));
  assert.deepEqual(seen, ['first', 'removed', 'stable', 'new-bubble']);
  seen.length = 0;
  child.dispatchEvent(new Event('signal'));
  assert.deepEqual(seen, ['first', 'late', 'stable']);
});

test('stopPropagation finishes current node; immediate stop prevents remaining listeners; default cancellation is independent', () => {
  for (const immediate of [false, true]) {
    const { root, child } = tree(), seen = [];
    child.addEventListener('signal', event => {
      seen.push('first'); event.preventDefault();
      if (immediate) event.stopImmediatePropagation(); else event.stopPropagation();
    });
    child.addEventListener('signal', () => seen.push('second'));
    root.addEventListener('signal', () => seen.push('parent'));
    const event = new Event('signal', true, true);
    assert.equal(child.dispatchEvent(event), false);
    assert.equal(event.isDefaultPrevented(), true);
    assert.deepEqual(seen, immediate ? ['first'] : ['first', 'second']);
    assert.equal(child.dispatchEvent(new Event('signal', true, false)), true);
  }
});

test('redispatch clones typed events without changing first target or leaking propagation/default flags', () => {
  const { root, child } = tree(), received = [];
  child.addEventListener('mouse', event => { event.preventDefault(); event.stopPropagation(); received.push(event); });
  root.addEventListener('mouse', event => received.push(event));
  const event = setStagePoint(new MouseEvent('mouse', true, true, 3, 4, null, true, false, true, true, 2), 103, 204);
  assert.equal(child.dispatchEvent(event), false);
  assert.equal(root.dispatchEvent(event), true);
  assert.notEqual(received[0], received[1]);
  assert.equal(event.target, child);
  assert.equal(received[1].target, root);
  assert.equal(received[1] instanceof MouseEvent, true);
  assert.deepEqual([received[1].localX, received[1].stageX, received[1].stageY, received[1].delta], [3, 103, 204, 2]);
});

test('removal/replacement invalidates old targets and listeners without leaking to new lifetime', () => {
  const { hub, root, child, live } = tree();
  let calls = 0;
  child.addEventListener('signal', () => calls++);
  live.delete(child); hub.prune();
  assert.throws(() => child.dispatchEvent(new Event('signal')), /removed or replaced/);
  const replacement = { name: 'child', parent: root };
  live.add(replacement); Object.assign(replacement, hub.methods(replacement));
  replacement.dispatchEvent(new Event('signal'));
  assert.equal(calls, 0);
  assert.equal(replacement.hasEventListener('signal'), false);
  root.addEventListener('signal', () => live.delete(replacement), true);
  replacement.addEventListener('signal', () => calls++);
  replacement.dispatchEvent(new Event('signal', true));
  assert.equal(calls, 0, 'removed target is not invoked after a capture handler changes the timeline');
});

test('ancestor route is snapshotted, broadcasts snapshot active lifetimes, clear permits clean restart', () => {
  const { hub, stage, root, child, live } = tree(), seen = [];
  root.addEventListener('signal', () => { child.parent = null; }, true);
  root.addEventListener('signal', () => seen.push('old-parent'));
  child.dispatchEvent(new Event('signal', true));
  assert.deepEqual(seen, ['old-parent']);
  // Removal is queued after detachment: the host provides its captured old route.
  const route = [child, root, stage];
  child.addEventListener(Event.REMOVED, () => { route.length = 1; });
  root.addEventListener(Event.REMOVED, () => seen.push('captured-old-parent'));
  hub.dispatch(child, new Event(Event.REMOVED, true), route);
  assert.deepEqual(seen, ['old-parent', 'captured-old-parent']);
  assert.throws(() => hub.dispatch(child, new Event(Event.REMOVED, true), [child, root, child]), /duplicate/);
  live.delete(root);
  hub.dispatch(child, new Event(Event.REMOVED, true), [child, root, stage]);
  assert.deepEqual(seen, ['old-parent', 'captured-old-parent'], 'captured ancestry cannot revive a retired listener');
  live.add(root);
  root.addEventListener(Event.ENTER_FRAME, () => { seen.push('root'); live.delete(child); });
  child.addEventListener(Event.ENTER_FRAME, () => seen.push('child'));
  hub.broadcast(Event.ENTER_FRAME, [root, child, root]);
  assert.deepEqual(seen, ['old-parent', 'captured-old-parent', 'root']);
  assert.throws(() => hub.dispatch(child, new Event(Event.REMOVED, true), [child, root, stage]), /removed or replaced/);
  hub.clear();
  assert.equal(root.hasEventListener(Event.ENTER_FRAME), false);
});

test('standalone and project-initialized dispatcher/event facades share type checks', () => {
  const dispatcher = new EventDispatcher();
  let value;
  dispatcher.addEventListener('ready', function (event) { value = [this, event.target]; });
  dispatcher.dispatchEvent(new Event('ready'));
  assert.deepEqual(value, [dispatcher, dispatcher]);
  assert.equal(EventDispatcher.__nfIs(dispatcher), true);
  assert.throws(() => new EventDispatcher({}), /Aggregated/);
  const { child } = tree();
  const original = child.addEventListener;
  EventDispatcher.__nfInitialize(child, []);
  assert.equal(child.addEventListener, original, 'super constructor must not replace the display lifetime hub');
  const event = Object.create(KeyboardEvent.prototype);
  KeyboardEvent.__nfInitialize(event, ['keyDown', true, true, 65, 65]);
  assert.equal(Event.__nfIs(event), true);
  assert.equal(KeyboardEvent.__nfIs(event), true);
  assert.equal(MouseEvent.__nfIs(event), false);
  assert.equal(event.clone().keyCode, 65);
});

test('bounded dispatch cleans up after exceptions and rejects malformed events or cyclic ancestry', () => {
  const { child, root } = tree({ maxDepth: 3 });
  child.addEventListener('recursive', event => child.dispatchEvent(event));
  assert.throws(() => child.dispatchEvent(new Event('recursive')), /recursion limit/);
  const thrown = new Event('throws');
  const token = {};
  child.addEventListener('throws', () => { throw token; });
  assert.throws(() => child.dispatchEvent(thrown), value => value === token);
  assert.equal(thrown.currentTarget, null);
  assert.equal(child.dispatchEvent(new Event('clean')), true);
  assert.throws(() => child.dispatchEvent({ type: 'fake' }), /Expected a NeoFlash Event/);
  assert.throws(() => child.addEventListener('fake', 3), /must be a function/);
  root.parent = child;
  assert.throws(() => child.willTrigger('fake'), /ancestry/);
});

test('mouse and keyboard clone preserve input values; target fields remain read-only', () => {
  const key = new KeyboardEvent('keyDown', true, true, 97, 65, 1, true, true, true, true, true);
  const copy = key.clone();
  assert.deepEqual([copy.type, copy.charCode, copy.keyCode, copy.keyLocation, copy.commandKey], ['keyDown', 97, 65, 1, true]);
  assert.equal(Reflect.set(copy, 'target', {}), false);
  assert.equal(Reflect.set(copy, 'type', 'changed'), false);
  const { child } = tree({ globalPoint: (_target, x, y) => [x + 10, y + 20] });
  const mouse = new MouseEvent('click', false, false, 3, 4);
  child.dispatchEvent(mouse);
  assert.deepEqual([mouse.stageX, mouse.stageY], [13, 24]);
});

test('one broadcast shares a work budget and failure does not poison a later event', () => {
  const { hub, root, child } = tree({ maxInvocations: 2 });
  root.addEventListener('frame', () => {});
  root.addEventListener('frame', () => {});
  child.addEventListener('frame', () => {});
  assert.throws(() => hub.broadcast('frame', [root, child]), /work limit/);
  assert.equal(child.dispatchEvent(new Event('frame')), true);
});
