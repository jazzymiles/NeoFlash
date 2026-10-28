/* Bounded AS2 Key/Mouse/Stage singletons over the existing Stage event hub.
 * Primary API references: Adobe AS2 reference mirrors at
 * https://open-flash.github.io/mirrors/as2-language-reference/{Key,Mouse,Stage}.html
 * Listener objects retain their receiver; registration is idempotent. A broadcast
 * snapshots the object list (mutations affect the next broadcast), but reads each
 * callback when invoked. Reset/dispose cancels the old snapshot immediately.
 * This is an ordinary browser-input profile, not exact AVM1 input conformance:
 * no cross-domain key access, Selection, full-screen/scale modes,
 * OS cursor control or historical keyboard layout rules. getAscii exposes the
 * Stage event's ASCII character/control code, not a remapped English keyboard.
 * Stage width/height are logical dimensions; onResize requires a host logical
 * resize, not preview zoom/window resizing. Mouse visibility requires a host hook.
 * Integration: create after Stage setup; call reset AFTER Stage.reset() on restart.
 * A capture observer reads browser lock-key flags without dispatching extra input.
 */
(function (global) {
  'use strict';
  const constants = Object.freeze({ BACKSPACE: 8, TAB: 9, ENTER: 13, SHIFT: 16,
    CONTROL: 17, CAPSLOCK: 20, ESCAPE: 27, SPACE: 32, PGUP: 33, PGDN: 34,
    END: 35, HOME: 36, LEFT: 37, UP: 38, RIGHT: 39, DOWN: 40, INSERT: 45, DELETEKEY: 46 });

  function create(options) {
    const stage = options?.stage;
    if (!stage?.addEventListener || !stage?.removeEventListener) {
      throw new TypeError('Legacy input requires the live NeoFlash Stage singleton.');
    }
    const keyListeners = new Set(), mouseListeners = new Set(), stageListeners = new Set();
    const pressed = new Set(), subscriptions = [];
    let disposed = false, generation = 0, code = 0, ascii = 0, visible = true;
    const listenerLimit = 1024;
    function active() { if (disposed) throw new Error('This legacy input adapter has been disposed.'); }
    function add(listeners, listener) {
      active();
      if (listener === null || (typeof listener !== 'object' && typeof listener !== 'function')) {
        throw new TypeError('AS2 input listeners must be objects with callback methods.');
      }
      if (!listeners.has(listener) && listeners.size >= listenerLimit) throw new RangeError('NeoFlash input supports at most 1024 listener objects per singleton.');
      listeners.add(listener); // Public addListener has a Void result.
    }
    function broadcast(listeners, name, args = []) {
      const current = generation;
      for (const listener of [...listeners]) {
        if (disposed || generation !== current) break;
        // Authored display objects can disappear between frames. Ordinary
        // listener objects remain live; the host recognizes retired proxies.
        if (options.alive && !options.alive(listener)) { listeners.delete(listener); continue; }
        // Getter failures and callback throws keep their original identity.
        const callback = listener[name];
        if (disposed || generation !== current) break;
        if (typeof callback === 'function') Reflect.apply(callback, listener, args);
      }
    }
    function guarded(callback) {
      return event => {
        if (disposed) return;
        try { callback(event); }
        catch (error) { if (options.onError) options.onError(error); else throw error; }
      };
    }
    function subscribe(type, callback) {
      const capture = guarded(callback);
      const target = guarded(event => { if (event.target === stage) callback(event); });
      // Stage capture sees descendant events even if they stop bubbling. The
      // separate target listener handles unfocused input without double delivery.
      stage.addEventListener(type, capture, true, 2147483647);
      stage.addEventListener(type, target, false, 2147483647);
      subscriptions.push(() => {
        stage.removeEventListener(type, capture, true);
        stage.removeEventListener(type, target, false);
      });
    }
    function key(event, down) {
      code = Number(event.keyCode) >>> 0;
      const character = Number(event.charCode) >>> 0;
      ascii = character > 0 && character < 128 ? character
        : [8, 9, 13, 27].includes(code) ? code : 0;
      if (code) { if (down) pressed.add(code); else pressed.delete(code); }
      broadcast(keyListeners, down ? 'onKeyDown' : 'onKeyUp');
    }
    function bind() {
      subscribe('keyDown', event => key(event, true));
      subscribe('keyUp', event => key(event, false));
      subscribe('mouseDown', () => broadcast(mouseListeners, 'onMouseDown'));
      subscribe('mouseUp', () => broadcast(mouseListeners, 'onMouseUp'));
      subscribe('mouseMove', () => broadcast(mouseListeners, 'onMouseMove'));
      subscribe('mouseWheel', event => broadcast(mouseListeners, 'onMouseWheel', [event.delta, event.target]));
      subscribe('resize', () => broadcast(stageListeners, 'onResize'));
    }
    function unbind() { for (const cleanup of subscriptions.splice(0)) cleanup(); }
    function setVisible(next) {
      active();
      if (typeof options.onVisibility !== 'function') throw new Error('Mouse.hide/show require the host cursor-visibility hook.');
      const previous = visible ? 1 : 0;
      if (next !== visible) { options.onVisibility(next); visible = next; }
      return previous;
    }
    function clear() {
      generation++;
      keyListeners.clear(); mouseListeners.clear(); stageListeners.clear();
      pressed.clear(); code = 0; ascii = 0;
    }
    function blur() { pressed.clear(); }
    const Key = Object.freeze({ ...constants,
      get _listeners() { return Object.freeze([...keyListeners]); },
      isAccessible() { active(); return true; },
      isToggled(value) { active(); if (!options.platform) throw new Error('Key.isToggled requires the Rust AS2 platform adapter.'); return options.platform.isToggled(value); },
      isDown(value) { return pressed.has(Number(value) | 0); },
      getCode() { return code; }, getAscii() { return ascii; },
      addListener(listener) { add(keyListeners, listener); },
      removeListener(listener) { return keyListeners.delete(listener); },
    });
    const Mouse = Object.freeze({
      addListener(listener) { add(mouseListeners, listener); },
      removeListener(listener) { return mouseListeners.delete(listener); },
      hide() { return setVisible(false); }, show() { return setVisible(true); },
    });
    const Stage = Object.freeze({
      get width() { return stage.stageWidth; }, get height() { return stage.stageHeight; },
      addListener(listener) { add(stageListeners, listener); },
      removeListener(listener) { return stageListeners.delete(listener); },
    });
    function reset() {
      active(); unbind(); clear();
      // Stage.reset releases its hub subscriptions; rebind once afterward.
      bind();
      if (!visible) { options.onVisibility(true); visible = true; }
    }
    function dispose() {
      if (disposed) return;
      unbind(); clear(); disposed = true;
      options.keyTarget?.removeEventListener?.('blur', blur);
      options.keyTarget?.removeEventListener?.('keydown', observeKey, true);
      options.keyTarget?.removeEventListener?.('keyup', observeKey, true);
      if (!visible) { options.onVisibility(true); visible = true; }
    }
    function observeKey(event) { options.platform?.observeKeyEvent(event); }
    bind(); options.keyTarget?.addEventListener?.('blur', blur);
    options.keyTarget?.addEventListener?.('keydown', observeKey, true);
    options.keyTarget?.addEventListener?.('keyup', observeKey, true);
    return Object.freeze({ Key, Mouse, Stage, reset, dispose });
  }
  const api = Object.freeze({ create });
  global.NeoFlashLegacyInput = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
