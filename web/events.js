/* Shared bounded AS3 EventDispatcher bridge. No DOM or display handles are serialized.
 * Listener priority, phase snapshots and redispatch cloning follow the public EventDispatcher
 * contract. Native Flash exception routing, subclasses and broadcast ordering remain separate.
 */
(function (global) {
  'use strict';
  const states = new WeakMap();
  const dispatchers = new WeakSet();
  const EventPhase = Object.freeze({ CAPTURING_PHASE: 1, AT_TARGET: 2, BUBBLING_PHASE: 3 });
  function state(event) {
    const value = states.get(event);
    if (!value) throw new TypeError('Expected a NeoFlash Event, MouseEvent or KeyboardEvent.');
    return value;
  }
  function numeric(value, name) {
    const number = Number(value);
    if (!Number.isFinite(number)) throw new TypeError(`${name} must be finite.`);
    return number;
  }
  class Event {
    constructor(type, bubbles = false, cancelable = false) {
      if (type === null || type === undefined) throw new TypeError('Event type must be a string.');
      states.set(this, { type: String(type), bubbles: Boolean(bubbles), cancelable: Boolean(cancelable),
        target: null, currentTarget: null, phase: EventPhase.AT_TARGET, stopped: false, immediate: false,
        prevented: false, dispatching: false, kind: 'event' });
    }
    get type() { return state(this).type; }
    get bubbles() { return state(this).bubbles; }
    get cancelable() { return state(this).cancelable; }
    get target() { return state(this).target; }
    get currentTarget() { return state(this).currentTarget; }
    get eventPhase() { return state(this).phase; }
    stopPropagation() { state(this).stopped = true; }
    stopImmediatePropagation() { const s = state(this); s.stopped = true; s.immediate = true; }
    preventDefault() { const s = state(this); if (s.cancelable) s.prevented = true; }
    isDefaultPrevented() { return state(this).prevented; }
    clone() { return new Event(this.type, this.bubbles, this.cancelable); }
    toString() { return `[Event type="${this.type}" bubbles=${this.bubbles} cancelable=${this.cancelable} eventPhase=${this.eventPhase}]`; }
  }
  Object.assign(Event, { ENTER_FRAME: 'enterFrame', EXIT_FRAME: 'exitFrame', FRAME_CONSTRUCTED: 'frameConstructed',
    ADDED: 'added', REMOVED: 'removed', ADDED_TO_STAGE: 'addedToStage', REMOVED_FROM_STAGE: 'removedFromStage',
    RESIZE: 'resize', CHANGE: 'change', OPEN: 'open', COMPLETE: 'complete', ACTIVATE: 'activate', DEACTIVATE: 'deactivate' });

  class MouseEvent extends Event {
    constructor(type, bubbles = true, cancelable = false, localX = 0, localY = 0, relatedObject = null,
      ctrlKey = false, altKey = false, shiftKey = false, buttonDown = false, delta = 0,
      commandKey = false, controlKey = false, clickCount = 0) {
      super(type, bubbles, cancelable);
      this.localX = numeric(localX, 'localX'); this.localY = numeric(localY, 'localY');
      this.relatedObject = relatedObject;
      this.ctrlKey = Boolean(ctrlKey); this.altKey = Boolean(altKey); this.shiftKey = Boolean(shiftKey);
      this.buttonDown = Boolean(buttonDown); this.delta = Number(delta) | 0;
      this.commandKey = Boolean(commandKey); this.controlKey = Boolean(controlKey); this.clickCount = Number(clickCount) | 0;
      Object.assign(state(this), { stageX: this.localX, stageY: this.localY, stagePointSet: false, kind: 'mouse' });
    }
    get stageX() { return state(this).stageX; }
    get stageY() { return state(this).stageY; }
    clone() {
      const copy = new MouseEvent(this.type, this.bubbles, this.cancelable, this.localX, this.localY,
        this.relatedObject, this.ctrlKey, this.altKey, this.shiftKey, this.buttonDown, this.delta,
        this.commandKey, this.controlKey, this.clickCount);
      const s = state(this);
      if (s.stagePointSet) setStagePoint(copy, s.stageX, s.stageY);
      return copy;
    }
    updateAfterEvent() { state(this).updateRequested = true; }
    toString() { return `[MouseEvent type="${this.type}" bubbles=${this.bubbles} localX=${this.localX} localY=${this.localY}]`; }
  }
  Object.assign(MouseEvent, { CLICK: 'click', DOUBLE_CLICK: 'doubleClick', MOUSE_DOWN: 'mouseDown',
    MOUSE_UP: 'mouseUp', MOUSE_MOVE: 'mouseMove', MOUSE_OVER: 'mouseOver', MOUSE_OUT: 'mouseOut',
    MOUSE_WHEEL: 'mouseWheel', ROLL_OVER: 'rollOver', ROLL_OUT: 'rollOut' });

  class KeyboardEvent extends Event {
    constructor(type, bubbles = true, cancelable = false, charCode = 0, keyCode = 0, keyLocation = 0,
      ctrlKey = false, altKey = false, shiftKey = false, controlKey = false, commandKey = false) {
      super(type, bubbles, cancelable);
      this.charCode = Number(charCode) >>> 0; this.keyCode = Number(keyCode) >>> 0;
      this.keyLocation = Number(keyLocation) >>> 0;
      this.ctrlKey = Boolean(ctrlKey); this.altKey = Boolean(altKey); this.shiftKey = Boolean(shiftKey);
      this.controlKey = Boolean(controlKey); this.commandKey = Boolean(commandKey);
      state(this).kind = 'keyboard';
    }
    clone() { return new KeyboardEvent(this.type, this.bubbles, this.cancelable, this.charCode, this.keyCode,
      this.keyLocation, this.ctrlKey, this.altKey, this.shiftKey, this.controlKey, this.commandKey); }
    updateAfterEvent() { state(this).updateRequested = true; }
    toString() { return `[KeyboardEvent type="${this.type}" keyCode=${this.keyCode} charCode=${this.charCode}]`; }
  }
  Object.assign(KeyboardEvent, { KEY_DOWN: 'keyDown', KEY_UP: 'keyUp' });

  // Host-only coordinate initializer. Dispatch does not expose mutable event target fields.
  function setStagePoint(event, x, y) {
    if (state(event).kind !== 'mouse') throw new TypeError('Stage coordinates require MouseEvent.');
    Object.assign(state(event), { stageX: numeric(x, 'stageX'), stageY: numeric(y, 'stageY'), stagePointSet: true });
    return event;
  }

  function create(options = {}) {
    const parentOf = options.parentOf || (() => null);
    const alive = options.alive || (() => true);
    let records = options.weakTargets ? new WeakMap() : new Map();
    const bindings = new WeakMap();
    let sequence = 0, depth = 0, invocations = 0, batches = 0;
    const maxDepth = options.maxDepth || 64, maxInvocations = options.maxInvocations || 10000;
    function assertTarget(target) {
      if (target === null || (typeof target !== 'object' && typeof target !== 'function')) throw new TypeError('Event target must be an object.');
      if (!alive(target)) throw new Error('Event target was removed or replaced.');
    }
    function listeners(target) {
      let record = records.get(target);
      if (!record) { record = new Map(); records.set(target, record); }
      return record;
    }
    function callback(entry) { return entry.weak ? entry.listener.deref() : entry.listener; }
    function add(target, type, listener, useCapture = false, priority = 0, useWeakReference = false) {
      assertTarget(target);
      if (typeof listener !== 'function') throw new TypeError('Event listener must be a function.');
      type = String(type); useCapture = Boolean(useCapture); priority = Number(priority) | 0;
      if (useWeakReference && typeof WeakRef !== 'function') throw new Error('Weak event listeners are unavailable in this browser.');
      const record = listeners(target), entries = record.get(type) || [];
      if (entries.some(item => callback(item) === listener && item.capture === useCapture)) return;
      entries.push({ listener: useWeakReference ? new WeakRef(listener) : listener, weak: Boolean(useWeakReference),
        capture: useCapture, priority, sequence: sequence++ });
      entries.sort((a, b) => b.priority - a.priority || a.sequence - b.sequence);
      record.set(type, entries);
    }
    function remove(target, type, listener, useCapture = false) {
      assertTarget(target);
      const record = records.get(target);
      if (!record) return;
      type = String(type);
      const remaining = (record.get(type) || []).filter(item => callback(item) &&
        !(callback(item) === listener && item.capture === Boolean(useCapture)));
      if (remaining.length) record.set(type, remaining); else record.delete(type);
      if (!record.size) records.delete(target);
    }
    function has(target, type) {
      assertTarget(target);
      return (records.get(target)?.get(String(type)) || []).some(item => callback(item));
    }
    function ancestry(target) {
      const result = [], seen = new Set([target]);
      let parent = parentOf(target);
      while (parent !== null && parent !== undefined) {
        if (seen.has(parent) || result.length >= 256) throw new Error('Invalid or excessive event ancestry.');
        if (!alive(parent)) break;
        result.push(parent); seen.add(parent); parent = parentOf(parent);
      }
      return result;
    }
    function will(target, type) {
      assertTarget(target);
      return [target, ...ancestry(target)].some(node => has(node, type));
    }
    function capturedParents(target, route) {
      // Host-only historical route: [target, oldParent, ..., stage]. The public
      // dispatchEvent wrapper never accepts a route. Copy before listener code
      // runs; retired ancestors may remain here but invoke still checks alive.
      const count = Array.isArray(route) ? route.length : 0;
      if (!Number.isInteger(count) || count < 1 || count > 257) {
        throw new TypeError('Captured event route must contain its target first and at most 256 ancestors.');
      }
      const result = [], seen = new Set();
      for (let index = 0; index < count; index++) {
        const node = route[index];
        if ((index === 0 && node !== target) || node === null || (typeof node !== 'object' && typeof node !== 'function') || seen.has(node)) {
          throw new TypeError('Captured event route contains an invalid or duplicate target.');
        }
        seen.add(node);
        if (index !== 0) result.push(node);
      }
      return result;
    }
    function invoke(target, event, phase, capture) {
      if (!alive(target)) return;
      const s = state(event);
      s.currentTarget = target; s.phase = phase;
      // Flash snapshots this node's current phase. Removals still run in that snapshot;
      // additions may run on a later phase but cannot join the phase in progress.
      const snapshot = (records.get(target)?.get(s.type) || [])
        .filter(entry => entry.capture === capture).map(callback).filter(Boolean);
      for (const listener of snapshot) {
        if (s.immediate || !alive(target)) break;
        if (++invocations > maxInvocations) throw new Error('Event listener work limit exceeded.');
        Reflect.apply(listener, target, [event]);
      }
    }
    function dispatch(target, original, capturedRoute) {
      assertTarget(target);
      const captured = capturedRoute === undefined ? null : capturedParents(target, capturedRoute);
      const previous = state(original);
      const event = previous.target !== null || previous.dispatching ? original.clone() : original;
      const s = state(event);
      if (s.target !== null || s.dispatching) throw new TypeError('Event.clone() must return a fresh Event.');
      if (depth >= maxDepth) throw new Error('Event dispatch recursion limit exceeded.');
      const parents = s.bubbles ? (captured ?? ancestry(target)) : [];
      if (s.kind === 'mouse' && !s.stagePointSet && options.globalPoint) {
        const point = options.globalPoint(target, event.localX, event.localY);
        if (point) setStagePoint(event, point[0], point[1]);
      }
      if (depth === 0 && batches === 0) invocations = 0;
      depth++; s.target = target; s.dispatching = true;
      try {
        for (let i = parents.length - 1; i >= 0 && !s.stopped; i--) invoke(parents[i], event, EventPhase.CAPTURING_PHASE, true);
        if (!s.stopped && alive(target)) invoke(target, event, EventPhase.AT_TARGET, false);
        if (s.bubbles) for (const parent of parents) {
          if (s.stopped) break;
          invoke(parent, event, EventPhase.BUBBLING_PHASE, false);
        }
        return !s.prevented;
      } finally {
        s.currentTarget = null; s.dispatching = false; depth--;
      }
    }
    function methods(target) {
      dispatchers.add(target);
      if (!bindings.has(target)) bindings.set(target, Object.freeze({
        addEventListener: (type, listener, capture = false, priority = 0, weak = false) => add(target, type, listener, capture, priority, weak),
        removeEventListener: (type, listener, capture = false) => remove(target, type, listener, capture),
        hasEventListener: type => has(target, type),
        willTrigger: type => will(target, type),
        dispatchEvent: event => dispatch(target, event),
      }));
      return bindings.get(target);
    }
    function prune() {
      if (options.weakTargets) return;
      for (const [target, record] of records) {
        if (!alive(target)) { records.delete(target); continue; }
        for (const [type, entries] of record) {
          const remaining = entries.filter(entry => callback(entry));
          if (remaining.length) record.set(type, remaining); else record.delete(type);
        }
        if (!record.size) records.delete(target);
      }
    }
    return Object.freeze({ methods, dispatch, prune, release: target => records.delete(target),
      clear: () => { records = options.weakTargets ? new WeakMap() : new Map(); },
      broadcast(type, targets) {
        // Explicit host list snapshots the active lifetimes; new placements wait for the next broadcast.
        if (depth === 0 && batches === 0) invocations = 0;
        batches++;
        try {
          for (const target of [...new Set(targets)]) if (alive(target) && has(target, type)) dispatch(target, new Event(type));
        } finally { batches--; }
      },
    });
  }
  const standalone = create({ weakTargets: true });
  class EventDispatcher {
    constructor(target = null) {
      if (target !== null) throw new TypeError('Aggregated EventDispatcher targets are not supported yet; use an ordinary dispatcher.');
      Object.assign(this, standalone.methods(this));
    }
  }
  EventDispatcher.__nfIs = value => dispatchers.has(value);
  EventDispatcher.__nfInitialize = (receiver, args = []) => {
    if (args.length && args[0] !== null && args[0] !== undefined) throw new TypeError('Aggregated EventDispatcher targets are not supported yet.');
    // Display proxies already carry the host's lifetime-aware hub. Never replace it.
    if (!dispatchers.has(receiver)) Object.assign(receiver, standalone.methods(receiver));
  };
  for (const [Constructor, kind] of [[Event, 'event'], [MouseEvent, 'mouse'], [KeyboardEvent, 'keyboard']]) {
    Constructor.__nfIs = value => states.has(value) && (kind === 'event' || state(value).kind === kind);
    Constructor.__nfInitialize = (receiver, args = []) => {
      const initialized = new Constructor(...args);
      states.set(receiver, state(initialized));
      Object.assign(receiver, initialized);
    };
  }
  const api = Object.freeze({ Event, MouseEvent, KeyboardEvent, EventPhase, EventDispatcher, create, setStagePoint });
  global.NeoFlashEvents = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
