/* Shared bounded AS3 EventDispatcher bridge. No DOM or display handles are serialized.
 * Listener priority, phase snapshots and redispatch cloning follow the public EventDispatcher
 * contract. Native Flash exception routing, subclasses and broadcast ordering remain separate.
 */
(function (global) {
  'use strict';
  const states = new WeakMap();
  const dispatchers = new WeakSet();
  let formatRust = null;
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
    formatToString(className, ...fields) {
      if (!formatRust) throw new Error('Event formatting requires the Rust runtime. Re-export this publication.');
      if (fields.length > 64) throw new RangeError('Event formatting exceeds 64 fields.');
      const entries = fields.map(field => { const key = String(field), value = this[key]; return [key, String(value), typeof value === 'string']; });
      return formatRust(String(className), JSON.stringify(entries));
    }
    toString() { return `[Event type="${this.type}" bubbles=${this.bubbles} cancelable=${this.cancelable} eventPhase=${this.eventPhase}]`; }
  }
  Object.assign(Event, { ENTER_FRAME: 'enterFrame', EXIT_FRAME: 'exitFrame', FRAME_CONSTRUCTED: 'frameConstructed',
    ADDED: 'added', REMOVED: 'removed', ADDED_TO_STAGE: 'addedToStage', REMOVED_FROM_STAGE: 'removedFromStage',
    RESIZE: 'resize', CHANGE: 'change', OPEN: 'open', INIT:'init',UNLOAD:'unload',COMPLETE: 'complete', SOUND_COMPLETE: 'soundComplete', ACTIVATE: 'activate', DEACTIVATE: 'deactivate' });

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

  class FocusEvent extends Event {
    constructor(type,bubbles=true,cancelable=false,relatedObject=null,shiftKey=false,keyCode=0,direction='none') {
      super(type,bubbles,cancelable);this.relatedObject=relatedObject;this.shiftKey=Boolean(shiftKey);this.keyCode=Number(keyCode)>>>0;this.direction=String(direction);this.isRelatedObjectInaccessible=false;state(this).kind='focus';
    }
    clone(){return new FocusEvent(this.type,this.bubbles,this.cancelable,this.relatedObject,this.shiftKey,this.keyCode,this.direction);}
  }
  Object.assign(FocusEvent,{FOCUS_IN:'focusIn',FOCUS_OUT:'focusOut',KEY_FOCUS_CHANGE:'keyFocusChange',MOUSE_FOCUS_CHANGE:'mouseFocusChange'});
  class TextEvent extends Event {
    constructor(type,bubbles=false,cancelable=false,text=''){super(type,bubbles,cancelable);this.text=String(text);state(this).kind='text';}
    clone(){return new TextEvent(this.type,this.bubbles,this.cancelable,this.text);}
  }
  Object.assign(TextEvent,{TEXT_INPUT:'textInput',LINK:'link'});

  // The Rust schema supplies family inheritance, defaults, field types and constants.
  // JS owns only native object references and the existing event hub connection.
  function createFamilies(wasm, options = {}) {
    if (!wasm?.ScriptEventPayload || !wasm?.as_event_schema_json || !wasm?.as_event_format_json)
      throw new Error('Event families require the Rust runtime. Re-export this publication.');
    formatRust = wasm.as_event_format_json;
    const definitions = JSON.parse(wasm.as_event_schema_json()), payloads = new WeakMap();
    const families = { Event, TextEvent };
    const references = new Set(['ref', 'array', 'error', 'display', 'byteArray', 'any']);
    function record(receiver) {
      const result = payloads.get(receiver);
      if (!result) throw new TypeError('Expected a Rust-backed event payload.');
      return result;
    }
    function encode(field, value, refs) {
      const key = field.alias || field.name;
      if (references.has(field.kind)) {
        if (value === null) { refs.delete(key); return null; }
        if (field.kind === 'array' && !Array.isArray(value)) throw new TypeError(`${field.name} requires an Array or null.`);
        if (field.kind !== 'any' && field.kind !== 'array' && (value === undefined || (typeof value !== 'object' && typeof value !== 'function')))
          throw new TypeError(`${field.name} requires an object or null.`);
        if (field.kind === 'byteArray' && options.isByteArray && !options.isByteArray(value)) throw new TypeError('SampleDataEvent.data requires ByteArray.');
        refs.set(key, value); return 0;
      }
      switch (field.kind) {
        case 'bool': return Boolean(value);
        case 'int': return Number(value) | 0;
        case 'number': { const n = Number(value); return Number.isFinite(n) ? n : String(n); }
        case 'string': return value == null ? null : String(value);
        default: throw new TypeError(`Unsupported event field ${field.name}.`);
      }
    }
    for (const definition of definitions) {
      const Parent = families[definition.parent], fields = new Map(definition.fields.map(field => [field.name, field]));
      if (!Parent) throw new Error(`Missing event parent ${definition.parent}.`);
      const Constructor = { [definition.name]: class extends Parent {
        constructor(type, bubbles = definition.bubbles, cancelable = definition.cancelable, ...values) {
          if (type === undefined && definition.name === 'UncaughtErrorEvent') type = 'uncaughtError';
          if (definition.requiredArguments && arguments.length < definition.requiredArguments) throw new TypeError(`${definition.name} requires ${definition.requiredArguments} constructor arguments.`);
          super(type, bubbles, cancelable);
          if (values.length > definition.params.length) throw new TypeError(`${definition.name} constructor arguments exceed this supported profile.`);
          const old = payloads.get(this);
          if (old) old.rust.free();
          for (const field of definition.fields) if (Object.hasOwn(this, field.name)) delete this[field.name];
          const refs = new Map(), input = {};
          definition.params.forEach((name, i) => {
            const field = fields.get(name);
            const value = i < values.length ? values[i] : field.default;
            input[name] = encode(field, value, refs);
          });
          const rust = new wasm.ScriptEventPayload(definition.name, JSON.stringify(input));
          payloads.set(this, { rust, refs });
          const gesture = definition.name.includes('GestureEvent'), touch = definition.name === 'TouchEvent';
          Object.assign(state(this), { kind: gesture ? 'gesture' : touch ? 'touch' : 'event-family',
            stageX: gesture || touch ? this.localX : 0, stageY: gesture || touch ? this.localY : 0,
            tapStageX: fields.has('tapLocalX') ? this.tapLocalX : undefined,
            tapStageY: fields.has('tapLocalY') ? this.tapLocalY : undefined, stagePointSet: false });
        }
        clone() {
          return new Constructor(this.type, this.bubbles, this.cancelable, ...definition.params.map(name => this[name]));
        }
        toString() {
          return this.formatToString(definition.name, 'type', 'bubbles', 'cancelable', 'eventPhase', ...definition.format);
        }
      } }[definition.name];
      for (const field of definition.fields) {
        Object.defineProperty(Constructor.prototype, field.name, {
          enumerable: true, configurable: true,
          get() {
            if (field.kind === 'coordinate') return state(this)[field.name];
            const current = record(this), value = JSON.parse(current.rust.get(field.name));
            if (references.has(field.kind)) return value === null ? null : current.refs.get(field.alias || field.name);
            return field.kind === 'number' && typeof value === 'string' ? Number(value) : value;
          },
          set(value) {
            // TextEvent's superclass initialization runs before our payload exists.
            if (!payloads.has(this)) { Object.defineProperty(this, field.name, { value, writable: true, enumerable: true, configurable: true }); return; }
            if (field.readonly) throw new TypeError(`${definition.name}.${field.name} is read-only.`);
            const current = record(this), refs = new Map(current.refs), encoded = encode(field, value, refs);
            current.rust.set(field.name, JSON.stringify(encoded)); current.refs = refs;
            if (!state(this).stagePointSet && ['localX','localY','tapLocalX','tapLocalY'].includes(field.name)) {
              const destination = { localX: 'stageX', localY: 'stageY', tapLocalX: 'tapStageX', tapLocalY: 'tapStageY' }[field.name];
              state(this)[destination] = this[field.name];
            }
          },
        });
      }
      Object.assign(Constructor, definition.constants);
      Constructor.__nfIs = value => states.has(value) && value instanceof Constructor;
      Constructor.__nfInitialize = () => { throw new TypeError(`${definition.name} native subclass initialization is not supported; construct the event and use listeners through composition.`); };
      if (['GestureEvent','TouchEvent'].includes(definition.name)) Constructor.prototype.updateAfterEvent = function () {
        state(this);
        if (typeof options.requestRender !== 'function') throw new Error(`${definition.name}.updateAfterEvent requires the host render-request hook.`);
        options.requestRender();
      };
      if (definition.name === 'TouchEvent') {
        for (const method of ['getSamples','isToolButtonDown']) Constructor.prototype[method] = () => { throw new Error(`TouchEvent.${method} requires a browser touch-sample adapter, outside this profile.`); };
      }
      families[definition.name] = Constructor;
    }
    for (const [name, constants] of Object.entries(JSON.parse(wasm.as_event_constants_json()))) {
      if (name === 'Event') { Object.assign(Event, constants); continue; }
      const Holder = { [name]: class { constructor() { throw new TypeError(`${name} supplies constants only.`); } } }[name];
      Object.assign(Holder, constants); families[name] = Object.freeze(Holder);
    }
    delete families.Event; delete families.TextEvent;
    return Object.freeze(families);
  }

  // Host-only coordinate initializer. Dispatch does not expose mutable event target fields.
  function setStagePoint(event, x, y) {
    if (!['mouse','gesture','touch'].includes(state(event).kind)) throw new TypeError('Stage coordinates require a mouse, gesture or touch event.');
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
      if (['mouse','gesture','touch'].includes(s.kind) && !s.stagePointSet && options.globalPoint && Number.isFinite(event.localX) && Number.isFinite(event.localY)) {
        const point = options.globalPoint(target, event.localX, event.localY);
        if (point) setStagePoint(event, point[0], point[1]);
        if (Number.isFinite(event.tapLocalX) && Number.isFinite(event.tapLocalY)) {
          const tap = options.globalPoint(target, event.tapLocalX, event.tapLocalY);
          if (tap) { s.tapStageX = tap[0]; s.tapStageY = tap[1]; }
        }
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
  for (const [Constructor, kind] of [[Event, 'event'], [MouseEvent, 'mouse'], [KeyboardEvent, 'keyboard'], [FocusEvent,'focus'], [TextEvent,'text']]) {
    Constructor.__nfIs = value => states.has(value) && (kind === 'event' || state(value).kind === kind || value instanceof Constructor);
    Constructor.__nfInitialize = (receiver, args = []) => {
      const initialized = new Constructor(...args);
      states.set(receiver, state(initialized));
      Object.assign(receiver, initialized);
    };
  }
  const api = Object.freeze({ Event, MouseEvent, KeyboardEvent, FocusEvent, TextEvent, EventPhase, EventDispatcher, create, createFamilies, setStagePoint });
  global.NeoFlashEvents = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
