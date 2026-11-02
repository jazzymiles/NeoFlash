/* ActionScript 3 AIR runtime host adapters backed by the shared Rust AirKernel.
 * Virtual filesystem path resolution, app:/ read-only protection, EncryptedLocalStore,
 * application descriptor XML synthesis, and relational SQL execution live in Rust
 * (crates/neoflash-core/src/as3_air.rs). This module marshals AS3 objects, ByteArrays,
 * Geometry values, E4X XML, and dispatches AS3 events within the HTML5 sandbox.
 */
(function (global) {
  'use strict';

  function defConstClass(name, constants) {
    const Ctor = {
      [name]: function () {
        throw new TypeError(`${name} supplies constants only and cannot be constructed.`);
      },
    }[name];
    for (const [k, v] of Object.entries(constants)) {
      Object.defineProperty(Ctor, k, { value: v, enumerable: true });
    }
    Object.defineProperties(Ctor, {
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError(`${name} supplies constants only and cannot be extended.`);
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    return Object.freeze(Ctor);
  }

  function tagType(Ctor, name, check, initFn) {
    Object.defineProperties(Ctor, {
      __nfIs: { value: check },
      __nfInitialize: {
        value:
          initFn ||
          (() => {
            throw new TypeError(
              `${name} native subclass initialization is outside this profile; use composition.`
            );
          }),
      },
    });
    return Ctor;
  }

  function create(options = {}) {
    const wasm = options.wasm || global.NeoFlashWasm;
    const events = options.events || global.NeoFlashEvents;
    if (!wasm?.AirKernel || !events?.Event || !events?.EventDispatcher) {
      throw new Error('NeoFlashAIR requires the Rust AirKernel WASM service and NeoFlashEvents.');
    }
    const binary = options.binary || null;
    const geometry = options.geometry || null;
    const getXML = options.getXML || (() => null);
    const getStage = options.getStage || (() => null);
    const getDisplay = options.getDisplay || (() => null);
    const getBitmap = options.getBitmap || (() => null);
    const getLoader = options.getLoader || (() => null);
    const getViewportSize = options.getViewportSize || (() => [800, 600]);
    let activeStageText = null;
    let networkInfoSingleton = null;
    const hostCleanups = [];
    const schedule =
      options.schedule ||
      (cb => {
        if (typeof global.queueMicrotask === 'function') global.queueMicrotask(cb);
        else Promise.resolve().then(cb);
      });
    const afterEvent = options.afterEvent || (() => {});
    const onError = options.onError || (err => { throw err; });

    let disposed = false;
    let generation = 1;
    let vectorFactory = (_typeName, values) => Array.from(values);

    function setVectorFactory(fn) {
      if (typeof fn === 'function') vectorFactory = fn;
    }
    function makeVector(typeName, values) {
      return vectorFactory(typeName, Array.from(values));
    }

    const projectInfo = options.project || {};
    const projName = typeof projectInfo.name === 'string' ? projectInfo.name : 'NeoFlash Application';
    const projW = Number(projectInfo.width) > 0 ? Number(projectInfo.width) >>> 0 : 550;
    const projH = Number(projectInfo.height) > 0 ? Number(projectInfo.height) >>> 0 : 400;
    const resourcesJson = JSON.stringify(
      (options.resources || projectInfo.resources || []).map(r => ({
        path: String(r.path || ''),
        data_base64: String(r.data_base64 || ''),
      }))
    );

    let kernel = new wasm.AirKernel(projName, projW, projH, resourcesJson, Date.now());
    let lastInputTimestamp = Date.now();

    function ensureActive() {
      if (disposed || !kernel) throw new Error('NeoFlashAIR session has been disposed.');
    }

    function translateKernelError(raw) {
      const msg = String(raw?.message ?? raw);
      if (msg.startsWith('SecurityError:')) {
        const err = new Error(msg.slice('SecurityError:'.length).trim());
        err.name = 'SecurityError';
        Object.defineProperty(err, 'errorID', { value: 2048 });
        return err;
      }
      if (msg.startsWith('ArgumentError:')) {
        const err = new Error(msg.slice('ArgumentError:'.length).trim());
        err.name = 'ArgumentError';
        Object.defineProperty(err, 'errorID', { value: 2004 });
        return err;
      }
      if (msg.startsWith('RangeError:')) {
        return new RangeError(msg.slice('RangeError:'.length).trim());
      }
      if (msg.startsWith('IOError:')) {
        const err = new Error(msg.slice('IOError:'.length).trim());
        err.name = 'IOError';
        Object.defineProperty(err, 'errorID', { value: 3003 });
        return err;
      }
      return raw instanceof Error ? raw : new Error(msg);
    }

    function callKernel(method, ...args) {
      ensureActive();
      try {
        return kernel[method](...args);
      } catch (err) {
        throw translateKernelError(err);
      }
    }

    function illegalOp(message, id = 2037) {
      const err = new Error(message);
      err.name = 'IllegalOperationError';
      Object.defineProperty(err, 'errorID', { value: id });
      return err;
    }

    function eofErr(message = 'End of file was encountered.') {
      const err = new Error(message);
      err.name = 'EOFError';
      Object.defineProperty(err, 'errorID', { value: 2030 });
      return err;
    }

    function scheduleTask(fn) {
      const born = generation;
      schedule(() => {
        if (disposed || generation !== born) return;
        try {
          fn();
          afterEvent();
        } catch (err) {
          onError(err);
        }
      });
    }

    function makeRect(x, y, w, h) {
      return geometry?.Rectangle ? new geometry.Rectangle(x, y, w, h) : { x, y, width: w, height: h };
    }
    function makePoint(x, y) {
      return geometry?.Point ? new geometry.Point(x, y) : { x, y };
    }

    // -------------------------------------------------------------------------
    // 1. flash.errors.* AIR Error Classes
    // -------------------------------------------------------------------------
    const SQLErrorOperation = defConstClass('SQLErrorOperation', {
      ANALYZE: 'analyze',
      ATTACH: 'attach',
      BEGIN: 'begin',
      CLOSE: 'close',
      COMMIT: 'commit',
      COMPACT: 'compact',
      DEANALYZE: 'deanalyze',
      DETACH: 'detach',
      EXECUTE: 'execute',
      OPEN: 'open',
      REENCRYPT: 'reencrypt',
      RELEASE_SAVEPOINT: 'releaseSavepoint',
      ROLLBACK: 'rollback',
      ROLLBACK_TO_SAVEPOINT: 'rollbackToSavepoint',
      SCHEMA: 'schema',
      SET_SAVEPOINT: 'setSavepoint',
    });

    const sqlErrors = new WeakSet();
    class SQLError extends Error {
      constructor(
        operation,
        details = '',
        message = '',
        id = 0,
        detailID = -1,
        detailArgs = null
      ) {
        const op = String(operation ?? '');
        const det = String(details ?? '');
        const msg = message ? String(message) : det || `SQL ${op} error`;
        super(msg);
        this.name = 'SQLError';
        Object.defineProperties(this, {
          operation: { value: op, enumerable: true },
          details: { value: det, enumerable: true },
          errorID: { value: Number(id) | 0, enumerable: true },
          detailID: { value: Number(detailID) | 0, enumerable: true },
          detailArguments: {
            value: Array.isArray(detailArgs) ? detailArgs.slice() : [],
            enumerable: true,
          },
        });
        sqlErrors.add(this);
      }
      toString() {
        return `SQLError: '${this.message}', details:'${this.details}', operation:'${this.operation}', detailID:'${this.detailID}'`;
      }
    }
    tagType(SQLError, 'SQLError', v => sqlErrors.has(v) || v instanceof SQLError);

    function parseSqlKernelError(raw, fallbackOp = 'execute') {
      const text = String(raw?.message ?? raw);
      try {
        const parsed = JSON.parse(text);
        if (parsed && typeof parsed.operation === 'string') {
          return new SQLError(
            parsed.operation,
            parsed.details || '',
            `Error #${parsed.errorId || 3115}: ${parsed.details || 'SQL error'}`,
            parsed.errorId || 3115,
            parsed.detailId ?? -1,
            parsed.detailArguments || []
          );
        }
      } catch (_) {}
      return new SQLError(fallbackOp, text, text, 3115);
    }

    const permErrors = new WeakSet();
    class PermissionError extends Error {
      constructor(message = '', id = 0) {
        super(String(message));
        this.name = 'PermissionError';
        Object.defineProperty(this, 'errorID', { value: Number(id) | 0, enumerable: true });
        permErrors.add(this);
      }
    }
    tagType(PermissionError, 'PermissionError', v => permErrors.has(v) || v instanceof PermissionError);

    const drmErrors = new WeakSet();
    class DRMManagerError extends Error {
      constructor(message = '', id = 0, subErrorID = 0) {
        super(String(message));
        this.name = 'DRMManagerError';
        Object.defineProperties(this, {
          errorID: { value: Number(id) | 0, enumerable: true },
          subErrorID: { value: Number(subErrorID) | 0, enumerable: true },
        });
        drmErrors.add(this);
      }
    }
    tagType(DRMManagerError, 'DRMManagerError', v => drmErrors.has(v) || v instanceof DRMManagerError);

    // -------------------------------------------------------------------------
    // 2. flash.events.* AIR Event Classes & Constants
    // -------------------------------------------------------------------------
    const BaseEvent = events.Event;
    const BaseMouseEvent = events.MouseEvent || BaseEvent;
    const BaseErrorEvent = events.ErrorEvent || BaseEvent;
    const BaseDispatcher = events.EventDispatcher;

    if (options.net) {
      if (options.net.ProgressEvent && !Object.isFrozen(options.net.ProgressEvent)) {
        Object.assign(options.net.ProgressEvent, {
          STANDARD_ERROR_DATA: 'standardErrorData',
          STANDARD_INPUT_PROGRESS: 'standardInputProgress',
          STANDARD_OUTPUT_DATA: 'standardOutputData',
        });
      }
      if (options.net.IOErrorEvent && !Object.isFrozen(options.net.IOErrorEvent)) {
        Object.assign(options.net.IOErrorEvent, {
          STANDARD_ERROR_IO_ERROR: 'standardErrorIoError',
          STANDARD_INPUT_IO_ERROR: 'standardInputIoError',
          STANDARD_OUTPUT_IO_ERROR: 'standardOutputIoError',
        });
      }
      if (options.net.HTTPStatusEvent && !Object.isFrozen(options.net.HTTPStatusEvent)) {
        Object.assign(options.net.HTTPStatusEvent, {
          HTTP_RESPONSE_STATUS: 'httpResponseStatus',
        });
      }
    }

    function fmtEvent(ev, clsName, props) {
      if (typeof ev.formatToString === 'function') {
        try {
          return ev.formatToString(clsName, 'type', 'bubbles', 'cancelable', 'eventPhase', ...props);
        } catch (_) {}
      }
      const parts = [
        `type="${ev.type}"`,
        `bubbles=${ev.bubbles}`,
        `cancelable=${ev.cancelable}`,
        `eventPhase=${ev.eventPhase}`,
      ];
      for (const p of props) {
        const val = ev[p];
        parts.push(`${p}=${typeof val === 'string' ? `"${val}"` : String(val)}`);
      }
      return `[${clsName} ${parts.join(' ')}]`;
    }

    const TouchEventIntent = defConstClass('TouchEventIntent', {
      ERASER: 'eraser',
      PEN: 'pen',
      UNKNOWN: 'unknown',
    });

    // Augment shared eventFamilies (TouchEvent & TransformGestureEvent) with AIR members.
    const touchExtraStates = new WeakMap();
    function touchExtra(ev) {
      let s = touchExtraStates.get(ev);
      if (!s) {
        s = {
          isTouchPointCanceled: false,
          timestamp: 0,
          touchIntent: 'unknown',
          toolButtons: 0,
          samples: null,
        };
        touchExtraStates.set(ev, s);
      }
      return s;
    }
    if (options.eventFamilies?.TouchEvent) {
      const TE = options.eventFamilies.TouchEvent;
      if (!Object.isFrozen(TE)) {
        Object.assign(TE, {
          PROXIMITY_BEGIN: 'proximityBegin',
          PROXIMITY_END: 'proximityEnd',
          PROXIMITY_MOVE: 'proximityMove',
          PROXIMITY_OUT: 'proximityOut',
          PROXIMITY_OVER: 'proximityOver',
          PROXIMITY_ROLL_OUT: 'proximityRollOut',
          PROXIMITY_ROLL_OVER: 'proximityRollOver',
        });
      }
      if (TE.prototype && !Object.isFrozen(TE.prototype)) {
        Object.defineProperties(TE.prototype, {
          isTouchPointCanceled: {
            get() { return touchExtra(this).isTouchPointCanceled; },
            set(v) { touchExtra(this).isTouchPointCanceled = Boolean(v); },
            enumerable: true,
            configurable: true,
          },
          timestamp: {
            get() { return touchExtra(this).timestamp; },
            set(v) { touchExtra(this).timestamp = Number(v) || 0; },
            enumerable: true,
            configurable: true,
          },
          touchIntent: {
            get() { return touchExtra(this).touchIntent; },
            set(v) { touchExtra(this).touchIntent = String(v ?? 'unknown'); },
            enumerable: true,
            configurable: true,
          },
          getSamples: {
            value(buffer, append = false) {
              if (!buffer || typeof buffer.writeFloat !== 'function') {
                throw new TypeError('TouchEvent.getSamples requires a ByteArray buffer.');
              }
              if (!append) {
                buffer.length = 0;
                buffer.position = 0;
              }
              const s = touchExtra(this);
              const list = Array.isArray(s.samples) && s.samples.length > 0
                ? s.samples
                : [[
                    Number.isFinite(this.localX) ? this.localX : 0,
                    Number.isFinite(this.localY) ? this.localY : 0,
                    Number.isFinite(this.pressure) ? this.pressure : 1,
                  ]];
              for (const [sx, sy, sp] of list) {
                buffer.writeFloat(Number(sx) || 0);
                buffer.writeFloat(Number(sy) || 0);
                buffer.writeFloat(Number.isFinite(sp) ? Number(sp) : 1);
              }
              return list.length;
            },
            configurable: true,
            writable: true,
          },
          isToolButtonDown: {
            value(index) {
              const bit = Number(index) | 0;
              if (bit < 0 || bit > 31) return false;
              return ((touchExtra(this).toolButtons >>> bit) & 1) === 1;
            },
            configurable: true,
            writable: true,
          },
        });
      }
    }
    const transformGestureVelocity = new WeakMap();
    if (options.eventFamilies?.TransformGestureEvent) {
      const TGE = options.eventFamilies.TransformGestureEvent;
      if (!Object.isFrozen(TGE)) {
        TGE.GESTURE_DIRECTIONAL_TAP = 'gestureDirectionalTap';
      }
      if (TGE.prototype && !Object.isFrozen(TGE.prototype) && !Object.hasOwn(TGE.prototype, 'velocity')) {
        Object.defineProperty(TGE.prototype, 'velocity', {
          get() { return transformGestureVelocity.get(this) ?? 0; },
          set(v) { transformGestureVelocity.set(this, Number(v) || 0); },
          enumerable: true,
          configurable: true,
        });
      }
    }

    class InvokeEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        dir = null,
        args = null,
        reason = 'standard'
      ) {
        super(type, bubbles, cancelable);
        this.currentDirectory = dir ?? null;
        this.arguments = Array.isArray(args) ? args.map(String) : [];
        this.reason = String(reason ?? 'standard');
      }
      clone() {
        return new InvokeEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.currentDirectory,
          this.arguments.slice(),
          this.reason
        );
      }
      toString() {
        return fmtEvent(this, 'InvokeEvent', ['arguments', 'currentDirectory', 'reason']);
      }
    }
    InvokeEvent.INVOKE = 'invoke';
    tagType(InvokeEvent, 'InvokeEvent', v => v instanceof InvokeEvent);

    class BrowserInvokeEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        args = null,
        sandboxType = 'application',
        securityDomain = null,
        isHTTPS = false,
        isUserEvent = false
      ) {
        super(type, bubbles, cancelable);
        this.arguments = Array.isArray(args) ? args.map(String) : [];
        this.sandboxType = String(sandboxType ?? 'application');
        this.securityDomain = securityDomain == null ? null : String(securityDomain);
        this.isHTTPS = Boolean(isHTTPS);
        this.isUserEvent = Boolean(isUserEvent);
      }
      clone() {
        return new BrowserInvokeEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.arguments.slice(),
          this.sandboxType,
          this.securityDomain,
          this.isHTTPS,
          this.isUserEvent
        );
      }
      toString() {
        return fmtEvent(this, 'BrowserInvokeEvent', [
          'arguments',
          'sandboxType',
          'securityDomain',
          'isHTTPS',
          'isUserEvent',
        ]);
      }
    }
    BrowserInvokeEvent.BROWSER_INVOKE = 'browserInvoke';
    tagType(BrowserInvokeEvent, 'BrowserInvokeEvent', v => v instanceof BrowserInvokeEvent);

    class FileListEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, files = null) {
        super(type, bubbles, cancelable);
        this.files = Array.isArray(files) ? files.slice() : [];
      }
      clone() {
        return new FileListEvent(this.type, this.bubbles, this.cancelable, this.files.slice());
      }
      toString() {
        return fmtEvent(this, 'FileListEvent', ['files']);
      }
    }
    Object.assign(FileListEvent, {
      DIRECTORY_LISTING: 'directoryListing',
      SELECT_MULTIPLE: 'selectMultiple',
    });
    tagType(FileListEvent, 'FileListEvent', v => v instanceof FileListEvent);

    class OutputProgressEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, bytesPending = 0, bytesTotal = 0) {
        super(type, bubbles, cancelable);
        this.bytesPending = Number(bytesPending);
        this.bytesTotal = Number(bytesTotal);
      }
      clone() {
        return new OutputProgressEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.bytesPending,
          this.bytesTotal
        );
      }
      toString() {
        return fmtEvent(this, 'OutputProgressEvent', ['bytesPending', 'bytesTotal']);
      }
    }
    OutputProgressEvent.OUTPUT_PROGRESS = 'outputProgress';
    tagType(OutputProgressEvent, 'OutputProgressEvent', v => v instanceof OutputProgressEvent);

    class NativeWindowBoundsEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        beforeBounds = null,
        afterBounds = null
      ) {
        super(type, bubbles, cancelable);
        this.beforeBounds = beforeBounds ?? null;
        this.afterBounds = afterBounds ?? null;
      }
      clone() {
        return new NativeWindowBoundsEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.beforeBounds,
          this.afterBounds
        );
      }
      toString() {
        return fmtEvent(this, 'NativeWindowBoundsEvent', ['beforeBounds', 'afterBounds']);
      }
    }
    Object.assign(NativeWindowBoundsEvent, {
      MOVE: 'move',
      MOVING: 'moving',
      RESIZE: 'resize',
      RESIZING: 'resizing',
    });
    tagType(
      NativeWindowBoundsEvent,
      'NativeWindowBoundsEvent',
      v => v instanceof NativeWindowBoundsEvent
    );

    class NativeWindowDisplayStateEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        beforeDisplayState = '',
        afterDisplayState = ''
      ) {
        super(type, bubbles, cancelable);
        this.beforeDisplayState = String(beforeDisplayState ?? '');
        this.afterDisplayState = String(afterDisplayState ?? '');
      }
      clone() {
        return new NativeWindowDisplayStateEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.beforeDisplayState,
          this.afterDisplayState
        );
      }
      toString() {
        return fmtEvent(this, 'NativeWindowDisplayStateEvent', [
          'beforeDisplayState',
          'afterDisplayState',
        ]);
      }
    }
    Object.assign(NativeWindowDisplayStateEvent, {
      DISPLAY_STATE_CHANGE: 'displayStateChange',
      DISPLAY_STATE_CHANGING: 'displayStateChanging',
    });
    tagType(
      NativeWindowDisplayStateEvent,
      'NativeWindowDisplayStateEvent',
      v => v instanceof NativeWindowDisplayStateEvent
    );

    class NativeDragEvent extends BaseMouseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = true,
        localX = 0,
        localY = 0,
        relatedObject = null,
        clipboard = null,
        allowedActions = null,
        dropAction = null,
        controlKey = false,
        altKey = false,
        shiftKey = false,
        commandKey = false
      ) {
        super(
          type,
          bubbles,
          cancelable,
          Number.isFinite(Number(localX)) ? Number(localX) : 0,
          Number.isFinite(Number(localY)) ? Number(localY) : 0,
          relatedObject,
          Boolean(controlKey),
          Boolean(altKey),
          Boolean(shiftKey),
          false,
          0,
          Boolean(commandKey),
          Boolean(controlKey),
          0
        );
        this.clipboard = clipboard ?? null;
        this.allowedActions = allowedActions ?? null;
        this.dropAction = dropAction == null ? null : String(dropAction);
      }
      clone() {
        return new NativeDragEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.localX,
          this.localY,
          this.relatedObject,
          this.clipboard,
          this.allowedActions,
          this.dropAction,
          this.controlKey,
          this.altKey,
          this.shiftKey,
          this.commandKey
        );
      }
      toString() {
        return fmtEvent(this, 'NativeDragEvent', [
          'localX',
          'localY',
          'clipboard',
          'allowedActions',
          'dropAction',
        ]);
      }
    }
    Object.assign(NativeDragEvent, {
      NATIVE_DRAG_COMPLETE: 'nativeDragComplete',
      NATIVE_DRAG_DROP: 'nativeDragDrop',
      NATIVE_DRAG_ENTER: 'nativeDragEnter',
      NATIVE_DRAG_EXIT: 'nativeDragExit',
      NATIVE_DRAG_OVER: 'nativeDragOver',
      NATIVE_DRAG_START: 'nativeDragStart',
      NATIVE_DRAG_UPDATE: 'nativeDragUpdate',
    });
    tagType(NativeDragEvent, 'NativeDragEvent', v => v instanceof NativeDragEvent);

    class ScreenMouseEvent extends BaseMouseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        screenX = 0,
        screenY = 0,
        ctrlKey = false,
        altKey = false,
        shiftKey = false,
        buttonDown = false,
        commandKey = false,
        controlKey = false
      ) {
        const sx = Number.isFinite(Number(screenX)) ? Number(screenX) : 0;
        const sy = Number.isFinite(Number(screenY)) ? Number(screenY) : 0;
        super(
          type,
          bubbles,
          cancelable,
          sx,
          sy,
          null,
          Boolean(ctrlKey),
          Boolean(altKey),
          Boolean(shiftKey),
          Boolean(buttonDown),
          0,
          Boolean(commandKey),
          Boolean(controlKey),
          0
        );
        this._screenX = sx;
        this._screenY = sy;
      }
      get screenX() {
        return this._screenX;
      }
      get screenY() {
        return this._screenY;
      }
      clone() {
        return new ScreenMouseEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.screenX,
          this.screenY,
          this.ctrlKey,
          this.altKey,
          this.shiftKey,
          this.buttonDown,
          this.commandKey,
          this.controlKey
        );
      }
      toString() {
        return fmtEvent(this, 'ScreenMouseEvent', [
          'screenX',
          'screenY',
          'ctrlKey',
          'altKey',
          'shiftKey',
          'buttonDown',
        ]);
      }
    }
    Object.assign(ScreenMouseEvent, {
      CLICK: 'click',
      MOUSE_DOWN: 'mouseDown',
      MOUSE_UP: 'mouseUp',
      RIGHT_CLICK: 'rightClick',
      RIGHT_MOUSE_DOWN: 'rightMouseDown',
      RIGHT_MOUSE_UP: 'rightMouseUp',
    });
    tagType(ScreenMouseEvent, 'ScreenMouseEvent', v => v instanceof ScreenMouseEvent);

    class SQLEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false) {
        super(type, bubbles, cancelable);
      }
      clone() {
        return new SQLEvent(this.type, this.bubbles, this.cancelable);
      }
      toString() {
        return fmtEvent(this, 'SQLEvent', []);
      }
    }
    Object.assign(SQLEvent, {
      ANALYZE: 'analyze',
      ATTACH: 'attach',
      BEGIN: 'begin',
      CANCEL: 'cancel',
      CLOSE: 'close',
      COMMIT: 'commit',
      COMPACT: 'compact',
      DEANALYZE: 'deanalyze',
      DETACH: 'detach',
      OPEN: 'open',
      REENCRYPT: 'reencrypt',
      RELEASE_SAVEPOINT: 'releaseSavepoint',
      RESULT: 'result',
      ROLLBACK: 'rollback',
      ROLLBACK_TO_SAVEPOINT: 'rollbackToSavepoint',
      SCHEMA: 'schema',
      SET_SAVEPOINT: 'setSavepoint',
    });
    tagType(SQLEvent, 'SQLEvent', v => v instanceof SQLEvent);

    class SQLErrorEvent extends BaseErrorEvent {
      constructor(type, bubbles = false, cancelable = false, error = null) {
        const msg = String(error?.details || error?.message || '');
        const eid = Number(error?.errorID) | 0;
        if (BaseErrorEvent !== BaseEvent) {
          super(type, bubbles, cancelable, msg, eid);
        } else {
          super(type, bubbles, cancelable);
          this.text = msg;
          Object.defineProperty(this, 'errorID', { value: eid, enumerable: true });
        }
        this.error = error ?? null;
      }
      clone() {
        return new SQLErrorEvent(this.type, this.bubbles, this.cancelable, this.error);
      }
      toString() {
        return fmtEvent(this, 'SQLErrorEvent', ['text', 'error', 'errorID']);
      }
    }
    SQLErrorEvent.ERROR = 'error';
    tagType(SQLErrorEvent, 'SQLErrorEvent', v => v instanceof SQLErrorEvent);

    class SQLUpdateEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, table = null, rowID = 0) {
        super(type, bubbles, cancelable);
        this.table = table == null ? null : String(table);
        this.rowID = Number(rowID);
      }
      clone() {
        return new SQLUpdateEvent(this.type, this.bubbles, this.cancelable, this.table, this.rowID);
      }
      toString() {
        return fmtEvent(this, 'SQLUpdateEvent', ['table', 'rowID']);
      }
    }
    Object.assign(SQLUpdateEvent, {
      DELETE: 'delete',
      INSERT: 'insert',
      UPDATE: 'update',
    });
    tagType(SQLUpdateEvent, 'SQLUpdateEvent', v => v instanceof SQLUpdateEvent);

    class StorageVolumeChangeEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, path = null, volume = null) {
        super(type, bubbles, cancelable);
        this.rootDirectory = path ?? null;
        this.storageVolume = volume ?? null;
      }
      clone() {
        return new StorageVolumeChangeEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.rootDirectory,
          this.storageVolume
        );
      }
      toString() {
        return fmtEvent(this, 'StorageVolumeChangeEvent', ['rootDirectory', 'storageVolume']);
      }
    }
    Object.assign(StorageVolumeChangeEvent, {
      STORAGE_VOLUME_MOUNT: 'storageVolumeMount',
      STORAGE_VOLUME_UNMOUNT: 'storageVolumeUnmount',
    });
    tagType(
      StorageVolumeChangeEvent,
      'StorageVolumeChangeEvent',
      v => v instanceof StorageVolumeChangeEvent
    );

    class NativeProcessExitEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, exitCode = NaN) {
        super(type, bubbles, cancelable);
        this.exitCode = Number(exitCode);
      }
      clone() {
        return new NativeProcessExitEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.exitCode
        );
      }
      toString() {
        return fmtEvent(this, 'NativeProcessExitEvent', ['exitCode']);
      }
    }
    NativeProcessExitEvent.EXIT = 'exit';
    tagType(
      NativeProcessExitEvent,
      'NativeProcessExitEvent',
      v => v instanceof NativeProcessExitEvent
    );

    class LocationChangeEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, location = null) {
        super(type, bubbles, cancelable);
        this.location = location == null ? null : String(location);
      }
      clone() {
        return new LocationChangeEvent(this.type, this.bubbles, this.cancelable, this.location);
      }
      toString() {
        return fmtEvent(this, 'LocationChangeEvent', ['location']);
      }
    }
    Object.assign(LocationChangeEvent, {
      LOCATION_CHANGE: 'locationChange',
      LOCATION_CHANGING: 'locationChanging',
    });
    tagType(LocationChangeEvent, 'LocationChangeEvent', v => v instanceof LocationChangeEvent);

    class HTMLUncaughtScriptExceptionEvent extends BaseEvent {
      constructor(exceptionValue = null) {
        super(HTMLUncaughtScriptExceptionEvent.UNCAUGHT_SCRIPT_EXCEPTION, false, true);
        this.exceptionValue = exceptionValue;
        this.stackTrace = null;
      }
      clone() {
        const copy = new HTMLUncaughtScriptExceptionEvent(this.exceptionValue);
        copy.stackTrace = Array.isArray(this.stackTrace) ? this.stackTrace.slice() : this.stackTrace;
        return copy;
      }
      toString() {
        return fmtEvent(this, 'HTMLUncaughtScriptExceptionEvent', ['exceptionValue']);
      }
    }
    HTMLUncaughtScriptExceptionEvent.UNCAUGHT_SCRIPT_EXCEPTION = 'uncaughtScriptException';
    tagType(
      HTMLUncaughtScriptExceptionEvent,
      'HTMLUncaughtScriptExceptionEvent',
      v => v instanceof HTMLUncaughtScriptExceptionEvent
    );

    class MediaEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, data = null) {
        super(type, bubbles, cancelable);
        this.data = data ?? null;
      }
      clone() {
        return new MediaEvent(this.type, this.bubbles, this.cancelable, this.data);
      }
      toString() {
        return fmtEvent(this, 'MediaEvent', ['data']);
      }
    }
    Object.assign(MediaEvent, { COMPLETE: 'complete', SELECT: 'select' });
    tagType(MediaEvent, 'MediaEvent', v => v instanceof MediaEvent);

    class DatagramSocketDataEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        srcAddress = '',
        srcPort = 0,
        dstAddress = '',
        dstPort = 0,
        data = null
      ) {
        super(type, bubbles, cancelable);
        this.srcAddress = String(srcAddress ?? '');
        this.srcPort = Number(srcPort) | 0;
        this.dstAddress = String(dstAddress ?? '');
        this.dstPort = Number(dstPort) | 0;
        this.data = data ?? null;
      }
      clone() {
        return new DatagramSocketDataEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.srcAddress,
          this.srcPort,
          this.dstAddress,
          this.dstPort,
          this.data
        );
      }
      toString() {
        return fmtEvent(this, 'DatagramSocketDataEvent', [
          'srcAddress',
          'srcPort',
          'dstAddress',
          'dstPort',
          'data',
        ]);
      }
    }
    DatagramSocketDataEvent.DATA = 'data';
    tagType(
      DatagramSocketDataEvent,
      'DatagramSocketDataEvent',
      v => v instanceof DatagramSocketDataEvent
    );

    class ServerSocketConnectEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, socket = null) {
        super(type, bubbles, cancelable);
        this.socket = socket ?? null;
      }
      clone() {
        return new ServerSocketConnectEvent(this.type, this.bubbles, this.cancelable, this.socket);
      }
      toString() {
        return fmtEvent(this, 'ServerSocketConnectEvent', ['socket']);
      }
    }
    ServerSocketConnectEvent.CONNECT = 'connect';
    tagType(
      ServerSocketConnectEvent,
      'ServerSocketConnectEvent',
      v => v instanceof ServerSocketConnectEvent
    );

    class DNSResolverEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        host = '',
        resourceRecords = null
      ) {
        super(type, bubbles, cancelable);
        this.host = String(host ?? '');
        this.resourceRecords = Array.isArray(resourceRecords) ? resourceRecords.slice() : [];
      }
      clone() {
        return new DNSResolverEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.host,
          this.resourceRecords.slice()
        );
      }
      toString() {
        return fmtEvent(this, 'DNSResolverEvent', ['host', 'resourceRecords']);
      }
    }
    DNSResolverEvent.LOOKUP = 'lookup';
    tagType(DNSResolverEvent, 'DNSResolverEvent', v => v instanceof DNSResolverEvent);

    class DRMAuthenticateEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        header = '',
        userPrompt = '',
        passPrompt = '',
        urlPrompt = '',
        authenticationType = '',
        netstream = null
      ) {
        super(type, bubbles, cancelable);
        this.header = String(header ?? '');
        this.usernamePrompt = String(userPrompt ?? '');
        this.passwordPrompt = String(passPrompt ?? '');
        this.urlPrompt = String(urlPrompt ?? '');
        this.authenticationType = String(authenticationType ?? '');
        this.netstream = netstream ?? null;
      }
      clone() {
        return new DRMAuthenticateEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.header,
          this.usernamePrompt,
          this.passwordPrompt,
          this.urlPrompt,
          this.authenticationType,
          this.netstream
        );
      }
      toString() {
        return fmtEvent(this, 'DRMAuthenticateEvent', [
          'header',
          'usernamePrompt',
          'passwordPrompt',
          'urlPrompt',
          'authenticationType',
        ]);
      }
    }
    Object.assign(DRMAuthenticateEvent, {
      AUTHENTICATION_TYPE_DRM: 'drm',
      AUTHENTICATION_TYPE_PROXY: 'proxy',
      DRM_AUTHENTICATE: 'drmAuthenticate',
    });
    tagType(DRMAuthenticateEvent, 'DRMAuthenticateEvent', v => v instanceof DRMAuthenticateEvent);

    class DeviceRotationEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        timestamp = 0,
        roll = 0,
        pitch = 0,
        yaw = 0,
        quaternion = null
      ) {
        super(type, bubbles, cancelable);
        this.timestamp = Number(timestamp);
        this.roll = Number(roll);
        this.pitch = Number(pitch);
        this.yaw = Number(yaw);
        this.quaternion = Array.isArray(quaternion) ? quaternion.map(Number) : null;
      }
      clone() {
        return new DeviceRotationEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.timestamp,
          this.roll,
          this.pitch,
          this.yaw,
          this.quaternion ? this.quaternion.slice() : null
        );
      }
      toString() {
        return fmtEvent(this, 'DeviceRotationEvent', [
          'timestamp',
          'roll',
          'pitch',
          'yaw',
          'quaternion',
        ]);
      }
    }
    DeviceRotationEvent.UPDATE = 'update';
    tagType(DeviceRotationEvent, 'DeviceRotationEvent', v => v instanceof DeviceRotationEvent);

    class PermissionEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, status = 'unknown') {
        super(type, bubbles, cancelable);
        this.status = String(status ?? 'unknown');
      }
      clone() {
        return new PermissionEvent(this.type, this.bubbles, this.cancelable, this.status);
      }
      toString() {
        return fmtEvent(this, 'PermissionEvent', ['status']);
      }
    }
    PermissionEvent.PERMISSION_STATUS = 'permissionStatus';
    tagType(PermissionEvent, 'PermissionEvent', v => v instanceof PermissionEvent);

    class RemoteNotificationEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, data = null, tokenId = null) {
        super(type, bubbles, cancelable);
        this.data = data ?? null;
        this.tokenId = tokenId == null ? null : String(tokenId);
      }
      clone() {
        return new RemoteNotificationEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.data,
          this.tokenId
        );
      }
      toString() {
        return fmtEvent(this, 'RemoteNotificationEvent', ['tokenId']);
      }
    }
    Object.assign(RemoteNotificationEvent, { NOTIFICATION: 'notification', TOKEN: 'token' });
    tagType(
      RemoteNotificationEvent,
      'RemoteNotificationEvent',
      v => v instanceof RemoteNotificationEvent
    );

    class GameInputEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, device = null) {
        super(type, bubbles, cancelable);
        this.device = device ?? null;
      }
      clone() {
        return new GameInputEvent(this.type, this.bubbles, this.cancelable, this.device);
      }
      toString() {
        return fmtEvent(this, 'GameInputEvent', ['device']);
      }
    }
    Object.assign(GameInputEvent, {
      DEVICE_ADDED: 'deviceAdded',
      DEVICE_REMOVED: 'deviceRemoved',
      DEVICE_UNUSABLE: 'deviceUnusable',
    });
    tagType(GameInputEvent, 'GameInputEvent', v => v instanceof GameInputEvent);

    class VsyncStateChangeAvailabilityEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, available = false) {
        super(type, bubbles, cancelable);
        this.available = Boolean(available);
      }
      clone() {
        return new VsyncStateChangeAvailabilityEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.available
        );
      }
      toString() {
        return fmtEvent(this, 'VsyncStateChangeAvailabilityEvent', ['available']);
      }
    }
    VsyncStateChangeAvailabilityEvent.VSYNC_STATE_CHANGE_AVAILABILITY =
      'vsyncStateChangeAvailability';
    tagType(
      VsyncStateChangeAvailabilityEvent,
      'VsyncStateChangeAvailabilityEvent',
      v => v instanceof VsyncStateChangeAvailabilityEvent
    );

    class AVHTTPStatusEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        status = 0,
        responseUrl = null,
        responseHeaders = null
      ) {
        super(type, bubbles, cancelable);
        this.status = Number(status) | 0;
        this.responseURL = responseUrl == null ? null : String(responseUrl);
        this.responseHeaders = Array.isArray(responseHeaders) ? responseHeaders.slice() : [];
        this.redirected = false;
      }
      clone() {
        const ev = new AVHTTPStatusEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.status,
          this.responseURL,
          this.responseHeaders.slice()
        );
        ev.redirected = this.redirected;
        return ev;
      }
      toString() {
        return fmtEvent(this, 'AVHTTPStatusEvent', ['status', 'responseURL']);
      }
    }
    AVHTTPStatusEvent.AV_HTTP_RESPONSE_STATUS = 'avHttpResponseStatus';
    tagType(AVHTTPStatusEvent, 'AVHTTPStatusEvent', v => v instanceof AVHTTPStatusEvent);

    class DRMDeviceGroupEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, deviceGroup = null) {
        super(type, bubbles, cancelable);
        this.deviceGroup = deviceGroup ?? null;
      }
      clone() {
        return new DRMDeviceGroupEvent(this.type, this.bubbles, this.cancelable, this.deviceGroup);
      }
      toString() {
        return fmtEvent(this, 'DRMDeviceGroupEvent', ['deviceGroup']);
      }
    }
    Object.assign(DRMDeviceGroupEvent, {
      ADD_TO_DEVICE_GROUP_COMPLETE: 'addToDeviceGroupComplete',
      REMOVE_TO_DEVICE_GROUP_COMPLETE: 'removeFromDeviceGroupComplete',
      REMOVE_FROM_DEVICE_GROUP_COMPLETE: 'removeFromDeviceGroupComplete',
    });
    tagType(DRMDeviceGroupEvent, 'DRMDeviceGroupEvent', v => v instanceof DRMDeviceGroupEvent);

    class DRMDeviceGroupErrorEvent extends BaseErrorEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        errorDetail = '',
        errorCode = 0,
        subErrorID = 0,
        deviceGroup = null,
        systemUpdateNeeded = false,
        drmUpdateNeeded = false
      ) {
        super(type, bubbles, cancelable, String(errorDetail ?? ''), Number(errorCode) | 0);
        this.text = String(errorDetail ?? '');
        if (!('errorID' in this)) {
          Object.defineProperty(this, 'errorID', { value: Number(errorCode) | 0, enumerable: true });
        }
        this.subErrorID = Number(subErrorID) | 0;
        this.deviceGroup = deviceGroup ?? null;
        this.systemUpdateNeeded = Boolean(systemUpdateNeeded);
        this.drmUpdateNeeded = Boolean(drmUpdateNeeded);
      }
      clone() {
        return new DRMDeviceGroupErrorEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.text,
          this.errorID,
          this.subErrorID,
          this.deviceGroup,
          this.systemUpdateNeeded,
          this.drmUpdateNeeded
        );
      }
      toString() {
        return fmtEvent(this, 'DRMDeviceGroupErrorEvent', [
          'text',
          'errorID',
          'subErrorID',
          'deviceGroup',
          'systemUpdateNeeded',
          'drmUpdateNeeded',
        ]);
      }
    }
    Object.assign(DRMDeviceGroupErrorEvent, {
      ADD_TO_DEVICE_GROUP_ERROR: 'addToDeviceGroupError',
      REMOVE_FROM_DEVICE_GROUP_ERROR: 'removeFromDeviceGroupError',
    });
    tagType(
      DRMDeviceGroupErrorEvent,
      'DRMDeviceGroupErrorEvent',
      v => v instanceof DRMDeviceGroupErrorEvent
    );

    class DRMStatusEvent extends BaseEvent {
      constructor(
        type = 'drmStatus',
        bubbles = false,
        cancelable = false,
        inMetadata = null,
        inVoucher = null,
        inLocal = false
      ) {
        super(type, bubbles, cancelable);
        this.contentData = inMetadata ?? null;
        this.voucher = inVoucher ?? null;
        this.isLocal = Boolean(inLocal);
        this.detail = inVoucher ? 'DRM.voucherObtained' : '';
        this.isAnonymous = Boolean(inMetadata && inMetadata.authenticationMethod === 'anonymous');
        this.isAvailableOffline = Boolean(
          this.isLocal ||
            (inVoucher && (inVoucher.offlineLeaseStartDate || inVoucher.offlineLeaseExpirationDate))
        );
        this.offlineLeasePeriod = Number(inVoucher?.offlineLeasePeriod ?? 0) >>> 0;
        this.policies = inVoucher?.policies ?? {};
        this.voucherEndDate = inVoucher?.voucherEndDate ?? null;
      }
      clone() {
        const copy = new DRMStatusEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.contentData,
          this.voucher,
          this.isLocal
        );
        copy.detail = this.detail;
        copy.isAnonymous = this.isAnonymous;
        copy.isAvailableOffline = this.isAvailableOffline;
        copy.offlineLeasePeriod = this.offlineLeasePeriod;
        copy.policies = this.policies;
        copy.voucherEndDate = this.voucherEndDate;
        return copy;
      }
      toString() {
        return fmtEvent(this, 'DRMStatusEvent', [
          'detail',
          'isLocal',
          'isAnonymous',
          'isAvailableOffline',
          'offlineLeasePeriod',
          'voucherEndDate',
        ]);
      }
    }
    DRMStatusEvent.DRM_STATUS = 'drmStatus';
    tagType(DRMStatusEvent, 'DRMStatusEvent', v => v instanceof DRMStatusEvent);

    class StageVideoEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, status = null, colorSpace = null) {
        super(type, bubbles, cancelable);
        this.status = status == null ? null : String(status);
        this.colorSpace = colorSpace == null ? null : String(colorSpace);
      }
      clone() {
        return new StageVideoEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.status,
          this.colorSpace
        );
      }
      toString() {
        return fmtEvent(this, 'StageVideoEvent', ['status', 'colorSpace']);
      }
    }
    Object.assign(StageVideoEvent, {
      RENDER_STATE: 'renderState',
      RENDER_STATUS_ACCELERATED: 'accelerated',
      RENDER_STATUS_SOFTWARE: 'software',
      RENDER_STATUS_UNAVAILABLE: 'unavailable',
    });
    tagType(StageVideoEvent, 'StageVideoEvent', v => v instanceof StageVideoEvent);

    // -------------------------------------------------------------------------
    // 3. flash.filesystem.* (FileMode, File, FileStream, StorageVolume*)
    // -------------------------------------------------------------------------
    const FileMode = defConstClass('FileMode', {
      APPEND: 'append',
      READ: 'read',
      UPDATE: 'update',
      WRITE: 'write',
    });

    const iconStates = new WeakMap();
    class Icon extends BaseDispatcher {
      constructor() {
        super();
        iconStates.set(this, { bitmaps: [] });
      }
      get bitmaps() {
        return iconStates.get(this).bitmaps.slice();
      }
      set bitmaps(value) {
        if (!Array.isArray(value)) throw new TypeError('Icon.bitmaps requires an Array.');
        iconStates.get(this).bitmaps = value.slice();
      }
    }
    tagType(Icon, 'Icon', v => iconStates.has(v) || v instanceof Icon);

    const fileStates = new WeakMap();
    function fileState(receiver) {
      ensureActive();
      const s = fileStates.get(receiver);
      if (!s) throw new TypeError('Expected a flash.filesystem.File instance.');
      return s;
    }
    function fileInfo(receiver) {
      const s = fileState(receiver);
      if (!s.path) throw illegalOp('File object does not have a path assigned.');
      return JSON.parse(callKernel('path_info_json', s.path));
    }
    function fileFromNativePath(nativePath) {
      const f = new File();
      fileStates.get(f).path = callKernel('normalize_path', String(nativePath));
      return f;
    }

    class File extends BaseDispatcher {
      constructor(path = null) {
        super();
        const resolved = path == null ? null : callKernel('normalize_path', String(path));
        fileStates.set(this, {
          path: resolved,
          downloaded: false,
          preventBackup: false,
          data: null,
          icon: null,
          pendingCancel: false,
        });
      }
      static get applicationDirectory() {
        return fileFromNativePath('/app');
      }
      static get applicationStorageDirectory() {
        return fileFromNativePath('/app-storage');
      }
      static get cacheDirectory() {
        return fileFromNativePath('/cache');
      }
      static get desktopDirectory() {
        return fileFromNativePath('/user/Desktop');
      }
      static get documentsDirectory() {
        return fileFromNativePath('/user/Documents');
      }
      static get userDirectory() {
        return fileFromNativePath('/user');
      }
      static get lineEnding() {
        return '\n';
      }
      static get separator() {
        return '/';
      }
      static get systemCharset() {
        return 'utf-8';
      }
      static get permissionStatus() {
        return 'granted';
      }
      static createTempFile() {
        return fileFromNativePath(callKernel('fs_create_temp', false, Date.now()));
      }
      static createTempDirectory() {
        return fileFromNativePath(callKernel('fs_create_temp', true, Date.now()));
      }
      static getRootDirectories() {
        return [fileFromNativePath('/')];
      }

      get nativePath() {
        const s = fileState(this);
        return s.path ?? '';
      }
      set nativePath(value) {
        const s = fileState(this);
        if (value == null || String(value) === '') throw new TypeError('File.nativePath requires a non-empty path.');
        s.path = callKernel('normalize_path', String(value));
      }
      get url() {
        const s = fileState(this);
        return s.path ? fileInfo(this).url : '';
      }
      set url(value) {
        const s = fileState(this);
        if (value == null || String(value) === '') throw new TypeError('File.url requires a non-empty URL.');
        s.path = callKernel('normalize_path', String(value));
      }
      get exists() {
        const s = fileState(this);
        return s.path ? fileInfo(this).exists : false;
      }
      get isDirectory() {
        const s = fileState(this);
        return s.path ? fileInfo(this).isDirectory : false;
      }
      get isHidden() {
        const s = fileState(this);
        return s.path ? fileInfo(this).isHidden : false;
      }
      get isPackage() {
        return false;
      }
      get isSymbolicLink() {
        return false;
      }
      get downloaded() {
        return fileState(this).downloaded;
      }
      set downloaded(value) {
        fileState(this).downloaded = Boolean(value);
      }
      get preventBackup() {
        return fileState(this).preventBackup;
      }
      set preventBackup(value) {
        fileState(this).preventBackup = Boolean(value);
      }
      get icon() {
        const s = fileState(this);
        if (!s.icon) s.icon = new Icon();
        return s.icon;
      }
      get parent() {
        const info = fileInfo(this);
        return info.parent ? fileFromNativePath(info.parent) : null;
      }
      get spaceAvailable() {
        return fileInfo(this).spaceAvailable;
      }
      get name() {
        return fileInfo(this).name;
      }
      get extension() {
        return fileInfo(this).extension ?? null;
      }
      get type() {
        return fileInfo(this).fileType;
      }
      get creator() {
        return '';
      }
      get size() {
        const info = fileInfo(this);
        if (!info.exists) throw translateKernelError(`IOError: File '${info.nativePath}' does not exist.`);
        return info.size;
      }
      get creationDate() {
        const info = fileInfo(this);
        if (!info.exists) throw translateKernelError(`IOError: File '${info.nativePath}' does not exist.`);
        return new Date(info.createdMs);
      }
      get modificationDate() {
        const info = fileInfo(this);
        if (!info.exists) throw translateKernelError(`IOError: File '${info.nativePath}' does not exist.`);
        return new Date(info.modifiedMs);
      }
      get data() {
        return fileState(this).data;
      }
      get permissionStatus() {
        fileState(this);
        return 'granted';
      }

      clone() {
        const s = fileState(this);
        const copy = new File();
        const cs = fileStates.get(copy);
        cs.path = s.path;
        cs.downloaded = s.downloaded;
        cs.preventBackup = s.preventBackup;
        cs.data = s.data;
        return copy;
      }
      canonicalize() {
        const s = fileState(this);
        if (!s.path) return;
        s.path = callKernel('canonicalize_path', s.path);
      }
      resolvePath(path) {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.resolvePath requires an initialized base path.');
        return fileFromNativePath(callKernel('resolve_path', s.path, String(path ?? '')));
      }
      getRelativePath(ref, useDotDot = false) {
        const s = fileState(this);
        const rs = fileState(ref);
        if (!s.path || !rs.path) throw illegalOp('File.getRelativePath requires initialized File paths.');
        const rel = callKernel('relative_path', s.path, rs.path, Boolean(useDotDot));
        return rel == null ? null : String(rel);
      }
      createDirectory() {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.createDirectory requires a path.');
        callKernel('fs_create_directory', s.path, Date.now());
      }
      deleteFile() {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.deleteFile requires a path.');
        callKernel('fs_delete_file', s.path);
      }
      deleteFileAsync() {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.deleteFileAsync requires a path.');
        const path = s.path;
        s.pendingCancel = false;
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            callKernel('fs_delete_file', path);
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      deleteDirectory(deleteDirectoryContents = false) {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.deleteDirectory requires a path.');
        callKernel('fs_delete_directory', s.path, Boolean(deleteDirectoryContents));
      }
      deleteDirectoryAsync(deleteDirectoryContents = false) {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.deleteDirectoryAsync requires a path.');
        const path = s.path;
        const delAll = Boolean(deleteDirectoryContents);
        s.pendingCancel = false;
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            callKernel('fs_delete_directory', path, delAll);
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      copyTo(newLocation, overwrite = false) {
        const s = fileState(this);
        const ds = fileState(newLocation);
        if (!s.path || !ds.path) throw illegalOp('File.copyTo requires initialized source and destination paths.');
        callKernel('fs_copy', s.path, ds.path, Boolean(overwrite), Date.now());
      }
      copyToAsync(newLocation, overwrite = false) {
        const s = fileState(this);
        const ds = fileState(newLocation);
        if (!s.path || !ds.path) throw illegalOp('File.copyToAsync requires initialized source and destination paths.');
        const src = s.path;
        const dst = ds.path;
        const ow = Boolean(overwrite);
        s.pendingCancel = false;
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            callKernel('fs_copy', src, dst, ow, Date.now());
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      moveTo(newLocation, overwrite = false) {
        const s = fileState(this);
        const ds = fileState(newLocation);
        if (!s.path || !ds.path) throw illegalOp('File.moveTo requires initialized source and destination paths.');
        callKernel('fs_move', s.path, ds.path, Boolean(overwrite), Date.now());
      }
      moveToAsync(newLocation, overwrite = false) {
        const s = fileState(this);
        const ds = fileState(newLocation);
        if (!s.path || !ds.path) throw illegalOp('File.moveToAsync requires initialized source and destination paths.');
        const src = s.path;
        const dst = ds.path;
        const ow = Boolean(overwrite);
        s.pendingCancel = false;
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            callKernel('fs_move', src, dst, ow, Date.now());
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      moveToTrash() {
        if (this.isDirectory) this.deleteDirectory(true);
        else this.deleteFile();
      }
      moveToTrashAsync() {
        if (this.isDirectory) this.deleteDirectoryAsync(true);
        else this.deleteFileAsync();
      }
      getDirectoryListing() {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.getDirectoryListing requires a path.');
        const paths = JSON.parse(callKernel('fs_list_directory_json', s.path));
        return paths.map(fileFromNativePath);
      }
      getDirectoryListingAsync() {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.getDirectoryListingAsync requires a path.');
        const path = s.path;
        s.pendingCancel = false;
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            const paths = JSON.parse(callKernel('fs_list_directory_json', path));
            this.dispatchEvent(
              new FileListEvent(
                FileListEvent.DIRECTORY_LISTING,
                false,
                false,
                paths.map(fileFromNativePath)
              )
            );
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      load() {
        const s = fileState(this);
        if (!s.path) throw illegalOp('File.load requires a path.');
        const path = s.path;
        s.pendingCancel = false;
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            const bytes = callKernel('fs_open_stream', path, 'read', Date.now());
            s.data = binary ? binary.fromBytes(bytes) : bytes;
            this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
            if (options.net?.ProgressEvent) {
              this.dispatchEvent(
                new options.net.ProgressEvent('progress', false, false, bytes.length, bytes.length)
              );
            }
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      save(data, defaultFileName = null) {
        const s = fileState(this);
        if (defaultFileName != null && (!s.path || this.isDirectory)) {
          const base = s.path || '/user/Documents';
          s.path = callKernel('resolve_path', base, String(defaultFileName));
        }
        if (!s.path) s.path = '/user/Documents/saved.dat';
        let bytes;
        if (binary?.ByteArray.__nfIs(data)) bytes = binary.toBytes(data);
        else if (data instanceof Uint8Array) bytes = data;
        else bytes = new TextEncoder().encode(String(data ?? ''));
        const path = s.path;
        s.pendingCancel = false;
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            callKernel('fs_write_bytes', path, bytes, Date.now());
            this.dispatchEvent(new BaseEvent(BaseEvent.SELECT));
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      cancel() {
        const s = fileState(this);
        s.pendingCancel = true;
      }
      requestPermission() {
        fileState(this);
        scheduleTask(() => {
          this.dispatchEvent(
            new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, 'granted')
          );
        });
      }
      browse(_typeFilter = null) {
        fileState(this);
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
        return false;
      }
      browseForOpen(_title, _typeFilter = null) {
        fileState(this);
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
      }
      browseForOpenMultiple(_title, _typeFilter = null) {
        fileState(this);
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
      }
      browseForSave(_title) {
        fileState(this);
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
      }
      browseForDirectory(_title) {
        fileState(this);
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
      }
      uploadUnencoded(request) {
        const s = fileState(this);
        if (!request || typeof request.url !== 'string' || !request.url) {
          throw argErr('uploadUnencoded requires a URLRequest with a non-empty url.');
        }
        const path = s.path;
        if (!path) throw illegalOp('File does not have a path assigned.');
        s.pendingCancel = false;
        const targetUrl = String(request.url);
        scheduleTask(() => {
          if (s.pendingCancel) return;
          try {
            const bytes = callKernel('fs_open_stream', path, 'read', Date.now());
            if (/^(app-storage:|app-desktop:|app-docs:|app-user:|app-cache:|file:|\/)/i.test(targetUrl)) {
              const destPath = callKernel('normalize_path', targetUrl);
              callKernel('fs_write_bytes', destPath, bytes, Date.now());
            }
            this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
            if (options.net?.ProgressEvent) {
              this.dispatchEvent(
                new options.net.ProgressEvent('progress', false, false, bytes.length, bytes.length)
              );
            }
            if (options.net?.HTTPStatusEvent) {
              const hs = new options.net.HTTPStatusEvent('httpResponseStatus', false, false, 200, false);
              hs.responseURL = targetUrl;
              this.dispatchEvent(hs);
              this.dispatchEvent(new options.net.HTTPStatusEvent('httpStatus', false, false, 200, false));
            }
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          } catch (err) {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          }
        });
      }
      openWithDefaultApplication() {
        throw illegalOp(
          'File.openWithDefaultApplication is blocked by the HTML5 browser sandbox.'
        );
      }
      toString() {
        const s = fileState(this);
        return s.path ? `[File nativePath="${s.path}"]` : '[File]';
      }
    }
    tagType(File, 'File', v => fileStates.has(v));

    // FileStream: implements IDataInput and IDataOutput via ByteArray + Rust VFS
    const streamStates = new WeakMap();
    function streamState(receiver) {
      ensureActive();
      const s = streamStates.get(receiver);
      if (!s) throw new TypeError('Expected a flash.filesystem.FileStream instance.');
      return s;
    }
    function openStreamBuffer(receiver) {
      const s = streamState(receiver);
      if (!s.open || !s.buffer) throw illegalOp('FileStream is not open.');
      return s;
    }
    function writableStream(receiver) {
      const s = openStreamBuffer(receiver);
      if (s.mode === 'read') throw illegalOp('FileStream was opened in read-only mode.');
      return s;
    }
    function syncStreamToVfs(receiver, s) {
      const bytes = binary.toBytes(s.buffer);
      callKernel('fs_write_bytes', s.path, bytes, Date.now());
      if (s.async) {
        const total = bytes.length;
        scheduleTask(() => {
          receiver.dispatchEvent(
            new OutputProgressEvent(OutputProgressEvent.OUTPUT_PROGRESS, false, false, 0, total)
          );
        });
      }
    }

    class FileStream extends BaseDispatcher {
      constructor() {
        super();
        if (!binary) throw new Error('FileStream requires the shared ByteArray binary service.');
        streamStates.set(this, {
          open: false,
          async: false,
          mode: null,
          path: null,
          buffer: null,
          endian: binary.Endian.BIG_ENDIAN,
          objectEncoding: 3,
          readAhead: Number.POSITIVE_INFINITY,
        });
      }
      open(file, fileMode) {
        const s = streamState(this);
        const fs = fileState(file);
        if (!fs.path) throw illegalOp('FileStream.open requires a File with a valid path.');
        const mode = String(fileMode ?? '');
        const initialBytes = callKernel('fs_open_stream', fs.path, mode, Date.now());
        if (s.buffer) s.buffer.clear();
        const buf = binary.fromBytes(initialBytes);
        buf.endian = s.endian;
        buf.objectEncoding = s.objectEncoding;
        buf.position = mode === 'append' ? buf.length : 0;
        s.open = true;
        s.async = false;
        s.mode = mode;
        s.path = fs.path;
        s.buffer = buf;
      }
      openAsync(file, fileMode) {
        const s = streamState(this);
        const fs = fileState(file);
        if (!fs.path) throw illegalOp('FileStream.openAsync requires a File with a valid path.');
        const mode = String(fileMode ?? '');
        const targetPath = fs.path;
        try {
          const initialBytes = callKernel('fs_open_stream', targetPath, mode, Date.now());
          if (s.buffer) s.buffer.clear();
          const buf = binary.fromBytes(initialBytes);
          buf.endian = s.endian;
          buf.objectEncoding = s.objectEncoding;
          buf.position = mode === 'append' ? buf.length : 0;
          s.open = true;
          s.async = true;
          s.mode = mode;
          s.path = targetPath;
          s.buffer = buf;
          const len = initialBytes.length;
          scheduleTask(() => {
            if (!s.open) return;
            if ((mode === 'read' || mode === 'update') && options.net?.ProgressEvent) {
              this.dispatchEvent(new options.net.ProgressEvent('progress', false, false, len, len));
            }
            if (mode === 'read' || mode === 'update') {
              this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
            }
          });
        } catch (err) {
          s.open = false;
          scheduleTask(() => {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          });
        }
      }
      close() {
        const s = streamState(this);
        if (!s.open) return;
        const wasAsync = s.async;
        s.open = false;
        s.async = false;
        if (s.buffer) {
          s.buffer.clear();
          s.buffer = null;
        }
        if (wasAsync) {
          scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CLOSE)));
        }
      }
      truncate() {
        const s = writableStream(this);
        s.buffer.length = s.buffer.position;
        syncStreamToVfs(this, s);
      }
      get position() {
        const s = streamState(this);
        return s.open && s.buffer ? s.buffer.position : 0;
      }
      set position(value) {
        const s = openStreamBuffer(this);
        const pos = Number(value);
        if (!Number.isFinite(pos) || pos < 0) throw new RangeError('FileStream.position must be non-negative.');
        s.buffer.position = Math.trunc(pos);
      }
      get bytesAvailable() {
        const s = streamState(this);
        return s.open && s.buffer ? s.buffer.bytesAvailable : 0;
      }
      get endian() {
        return streamState(this).endian;
      }
      set endian(value) {
        const s = streamState(this);
        if (value !== binary.Endian.BIG_ENDIAN && value !== binary.Endian.LITTLE_ENDIAN) {
          throw new TypeError('FileStream.endian must be Endian.BIG_ENDIAN or Endian.LITTLE_ENDIAN.');
        }
        s.endian = value;
        if (s.buffer) s.buffer.endian = value;
      }
      get objectEncoding() {
        return streamState(this).objectEncoding;
      }
      set objectEncoding(value) {
        const s = streamState(this);
        const enc = Number(value) >>> 0;
        if (enc !== 0 && enc !== 3) throw new RangeError('FileStream.objectEncoding must be 0 or 3.');
        s.objectEncoding = enc;
        if (s.buffer) s.buffer.objectEncoding = enc;
      }
      get readAhead() {
        return streamState(this).readAhead;
      }
      set readAhead(value) {
        const s = streamState(this);
        const n = Number(value);
        s.readAhead = n > 0 ? n : Number.POSITIVE_INFINITY;
      }

      _read(fn) {
        const s = openStreamBuffer(this);
        try {
          return fn(s.buffer);
        } catch (err) {
          if (err instanceof RangeError && /exceeds available data/i.test(String(err.message))) {
            throw eofErr();
          }
          throw err;
        }
      }
      readBoolean() { return this._read(b => b.readBoolean()); }
      readByte() { return this._read(b => b.readByte()); }
      readUnsignedByte() { return this._read(b => b.readUnsignedByte()); }
      readShort() { return this._read(b => b.readShort()); }
      readUnsignedShort() { return this._read(b => b.readUnsignedShort()); }
      readInt() { return this._read(b => b.readInt()); }
      readUnsignedInt() { return this._read(b => b.readUnsignedInt()); }
      readFloat() { return this._read(b => b.readFloat()); }
      readDouble() { return this._read(b => b.readDouble()); }
      readUTF() { return this._read(b => b.readUTF()); }
      readUTFBytes(length) { return this._read(b => b.readUTFBytes(length)); }
      readMultiByte(length, charSet) { return this._read(b => b.readMultiByte(length, charSet)); }
      readBytes(bytes, offset = 0, length = 0) {
        this._read(b => {
          if (length > b.bytesAvailable) throw eofErr();
          b.readBytes(bytes, offset, length);
        });
      }
      readObject() { return this._read(b => b.readObject()); }

      writeBoolean(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeBoolean(value); syncStreamToVfs(this, s); }
      writeByte(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeByte(value); syncStreamToVfs(this, s); }
      writeShort(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeShort(value); syncStreamToVfs(this, s); }
      writeInt(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeInt(value); syncStreamToVfs(this, s); }
      writeUnsignedInt(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeUnsignedInt(value); syncStreamToVfs(this, s); }
      writeFloat(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeFloat(value); syncStreamToVfs(this, s); }
      writeDouble(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeDouble(value); syncStreamToVfs(this, s); }
      writeUTF(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeUTF(value); syncStreamToVfs(this, s); }
      writeUTFBytes(value) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeUTFBytes(value); syncStreamToVfs(this, s); }
      writeMultiByte(value, charSet) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeMultiByte(value, charSet); syncStreamToVfs(this, s); }
      writeBytes(bytes, offset = 0, length = 0) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeBytes(bytes, offset, length); syncStreamToVfs(this, s); }
      writeObject(object) { const s = writableStream(this); if (s.mode === 'append') s.buffer.position = s.buffer.length; s.buffer.writeObject(object); syncStreamToVfs(this, s); }
    }
    tagType(FileStream, 'FileStream', v => streamStates.has(v));

    const storageVolumes = new WeakMap();
    class StorageVolume {
      constructor(
        rootDirFile,
        name = 'Virtual Volume',
        writable = true,
        removable = false,
        fileSysType = 'virtual',
        drive = null
      ) {
        if (!fileStates.has(rootDirFile)) {
          throw new TypeError('StorageVolume requires a flash.filesystem.File rootDirectory.');
        }
        storageVolumes.set(this, {
          rootDirectory: rootDirFile.clone(),
          name: String(name ?? ''),
          isWritable: Boolean(writable),
          isRemovable: Boolean(removable),
          fileSystemType: String(fileSysType ?? 'virtual'),
          drive: drive == null ? null : String(drive),
        });
      }
      get rootDirectory() { return storageVolumes.get(this).rootDirectory.clone(); }
      get name() { return storageVolumes.get(this).name; }
      get isWritable() { return storageVolumes.get(this).isWritable; }
      get isRemovable() { return storageVolumes.get(this).isRemovable; }
      get fileSystemType() { return storageVolumes.get(this).fileSystemType; }
      get drive() { return storageVolumes.get(this).drive; }
    }
    tagType(StorageVolume, 'StorageVolume', v => storageVolumes.has(v));

    const volumeInfoToken = Symbol('StorageVolumeInfo');
    let volumeInfoSingleton = null;
    class StorageVolumeInfo extends BaseDispatcher {
      constructor(token) {
        if (token !== volumeInfoToken) {
          throw new TypeError('StorageVolumeInfo is a singleton; use StorageVolumeInfo.storageVolumeInfo.');
        }
        super();
      }
      static get isSupported() {
        return true;
      }
      static get storageVolumeInfo() {
        ensureActive();
        if (!volumeInfoSingleton) volumeInfoSingleton = new StorageVolumeInfo(volumeInfoToken);
        return volumeInfoSingleton;
      }
      getStorageVolumes() {
        ensureActive();
        return makeVector('flash.filesystem.StorageVolume', [
          new StorageVolume(File.applicationDirectory, 'Application Bundle', false, false, 'bundle', null),
          new StorageVolume(File.applicationStorageDirectory, 'Application Storage', true, false, 'virtual', null),
          new StorageVolume(File.userDirectory, 'User Sandbox', true, false, 'virtual', null),
        ]);
      }
    }
    tagType(StorageVolumeInfo, 'StorageVolumeInfo', v => v === volumeInfoSingleton && v !== null);

    // -------------------------------------------------------------------------
    // Binary / Base64 & Responder Helpers
    // -------------------------------------------------------------------------
    const B64_CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
    function bytesToBase64(bytes) {
      const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes || 0);
      let out = '';
      for (let i = 0; i < u8.length; i += 3) {
        const b0 = u8[i];
        const b1 = i + 1 < u8.length ? u8[i + 1] : 0;
        const b2 = i + 2 < u8.length ? u8[i + 2] : 0;
        const n = (b0 << 16) | (b1 << 8) | b2;
        out += B64_CHARS[(n >> 18) & 63];
        out += B64_CHARS[(n >> 12) & 63];
        out += i + 1 < u8.length ? B64_CHARS[(n >> 6) & 63] : '=';
        out += i + 2 < u8.length ? B64_CHARS[n & 63] : '=';
      }
      return out;
    }

    function base64ToBytes(b64) {
      const clean = String(b64 || '').replace(/[^A-Za-z0-9+/=]/g, '');
      const len = clean.length;
      if (len === 0) return new Uint8Array(0);
      const pad = clean.endsWith('==') ? 2 : clean.endsWith('=') ? 1 : 0;
      const out = new Uint8Array(((len * 3) >> 2) - pad);
      let outIdx = 0;
      for (let i = 0; i < len; i += 4) {
        const c0 = Math.max(0, B64_CHARS.indexOf(clean[i]));
        const c1 = Math.max(0, B64_CHARS.indexOf(clean[i + 1]));
        const c2 = clean[i + 2] === '=' ? 0 : Math.max(0, B64_CHARS.indexOf(clean[i + 2]));
        const c3 = clean[i + 3] === '=' ? 0 : Math.max(0, B64_CHARS.indexOf(clean[i + 3]));
        const n = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;
        if (outIdx < out.length) out[outIdx++] = (n >> 16) & 0xff;
        if (outIdx < out.length) out[outIdx++] = (n >> 8) & 0xff;
        if (outIdx < out.length) out[outIdx++] = n & 0xff;
      }
      return out;
    }

    function notifyResponderResult(responder, payload) {
      if (!responder) return false;
      const fn = responder.__nfResult || responder.result;
      if (typeof fn === 'function') {
        fn.call(responder, payload);
        return true;
      }
      return false;
    }

    function notifyResponderStatus(responder, err) {
      if (!responder) return false;
      const fn = responder.__nfStatus || responder.status;
      if (typeof fn === 'function') {
        fn.call(responder, err);
        return true;
      }
      return false;
    }

    function argErr(message, id = 2004) {
      const err = new Error(message);
      err.name = 'ArgumentError';
      Object.defineProperty(err, 'errorID', { value: id });
      return err;
    }

    // -------------------------------------------------------------------------
    // 4. flash.desktop.*
    // -------------------------------------------------------------------------
    const ClipboardFormats = defConstClass('ClipboardFormats', {
      BITMAP_FORMAT: 'air:bitmap',
      FILE_LIST_FORMAT: 'air:file list',
      FILE_PROMISE_LIST_FORMAT: 'air:file promise list',
      HTML_FORMAT: 'air:html',
      RICH_TEXT_FORMAT: 'air:rtf',
      TEXT_FORMAT: 'air:text',
      URL_FORMAT: 'air:url',
    });

    const ClipboardTransferMode = defConstClass('ClipboardTransferMode', {
      CLONE_ONLY: 'cloneOnly',
      CLONE_PREFERRED: 'clonePreferred',
      ORIGINAL_ONLY: 'originalOnly',
      ORIGINAL_PREFERRED: 'originalPreferred',
    });

    function cloneClipboardValue(value, serializable) {
      if (value === null || value === undefined || typeof value !== 'object') return value;
      if (fileStates.has(value)) return value.clone();
      if (value instanceof Date) return new Date(value.getTime());
      if (Array.isArray(value)) return value.map(item => cloneClipboardValue(item, serializable));
      if (binary?.ByteArray?.__nfIs?.(value)) {
        return binary.fromBytes(binary.toBytes(value));
      }
      if (!serializable) return undefined;
      if (binary?.ByteArray) {
        try {
          const ba = new binary.ByteArray();
          ba.writeObject(value);
          ba.position = 0;
          return ba.readObject();
        } catch (_) {}
      }
      try {
        return JSON.parse(JSON.stringify(value));
      } catch (_) {
        return undefined;
      }
    }

    const clipboardStates = new WeakMap();
    let generalClipboardSingleton = null;
    class Clipboard {
      constructor() {
        ensureActive();
        clipboardStates.set(this, { entries: new Map() });
      }
      static get supportsFilePromise() {
        return true;
      }
      static get generalClipboard() {
        ensureActive();
        if (!generalClipboardSingleton) generalClipboardSingleton = new Clipboard();
        return generalClipboardSingleton;
      }
      get formats() {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        return Array.from(s.entries.keys());
      }
      clear() {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        s.entries.clear();
      }
      clearData(format) {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        s.entries.delete(String(format));
      }
      hasFormat(format) {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        const fmt = String(format);
        if (s.entries.has(fmt)) return true;
        if (
          fmt === ClipboardFormats.FILE_LIST_FORMAT &&
          s.entries.has(ClipboardFormats.FILE_PROMISE_LIST_FORMAT)
        ) {
          return true;
        }
        return false;
      }
      setData(format, data, serializable = true) {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        if (format == null || String(format) === '' || data === null || data === undefined) {
          return false;
        }
        const fmt = String(format);
        if (fmt === ClipboardFormats.FILE_PROMISE_LIST_FORMAT) {
          if (!Array.isArray(data) || !data.every(item => IFilePromise.__nfIs(item))) {
            throw new TypeError(
              'ClipboardFormats.FILE_PROMISE_LIST_FORMAT requires an Array of IFilePromise objects.'
            );
          }
        }
        const canSerialize = Boolean(serializable);
        const cloned = canSerialize ? cloneClipboardValue(data, true) : undefined;
        s.entries.set(fmt, {
          kind: 'value',
          original: data,
          cloned,
          serializable: canSerialize,
        });
        if (
          this === generalClipboardSingleton &&
          fmt === ClipboardFormats.TEXT_FORMAT &&
          typeof global.navigator?.clipboard?.writeText === 'function'
        ) {
          try {
            Promise.resolve(global.navigator.clipboard.writeText(String(data))).catch(() => {});
          } catch (_) {}
        }
        return true;
      }
      setDataHandler(format, handler, serializable = true) {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        if (format == null || String(format) === '' || typeof handler !== 'function') {
          return false;
        }
        s.entries.set(String(format), {
          kind: 'handler',
          handler,
          serializable: Boolean(serializable),
        });
        return true;
      }
      getData(format, transferMode = 'originalPreferred') {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        const mode = String(transferMode ?? 'originalPreferred');
        if (
          mode !== 'originalPreferred' &&
          mode !== 'originalOnly' &&
          mode !== 'clonePreferred' &&
          mode !== 'cloneOnly'
        ) {
          throw argErr(`Invalid ClipboardTransferMode '${mode}'.`);
        }
        const fmt = String(format);
        let entry = s.entries.get(fmt);
        if (
          !entry &&
          fmt === ClipboardFormats.FILE_LIST_FORMAT &&
          s.entries.has(ClipboardFormats.FILE_PROMISE_LIST_FORMAT)
        ) {
          const promises = this.getData(ClipboardFormats.FILE_PROMISE_LIST_FORMAT, 'originalPreferred');
          if (Array.isArray(promises)) {
            const files = [];
            for (const p of promises) {
              try {
                const rel = String(p.relativePath || 'promised.dat').replace(/^\/+/, '');
                const targetPath = callKernel('resolve_path', '/tmp/promises', rel);
                const input = p.open();
                const bytes = readDataInputBytes(input);
                if (typeof p.close === 'function') p.close();
                callKernel('fs_write_bytes', targetPath, bytes, Date.now());
                files.push(fileFromNativePath(targetPath));
              } catch (err) {
                if (typeof p?.reportError === 'function') {
                  p.reportError(new BaseErrorEvent('error', false, false, String(err.message)));
                }
              }
            }
            return files;
          }
        }
        if (!entry) return null;
        const raw = entry.kind === 'handler' ? entry.handler() : entry.original;
        if (mode === 'originalOnly' || mode === 'originalPreferred') {
          return raw;
        }
        const cloned =
          entry.kind === 'value' && entry.cloned !== undefined
            ? cloneClipboardValue(entry.cloned, true)
            : cloneClipboardValue(raw, entry.serializable);
        if (cloned !== undefined) return cloned;
        return mode === 'clonePreferred' ? raw : null;
      }
    }
    tagType(Clipboard, 'Clipboard', v => clipboardStates.has(v));

    const IFilePromise = Object.freeze(
      (() => {
        const check = value =>
          Boolean(
            value &&
              typeof value === 'object' &&
              typeof value.open === 'function' &&
              typeof value.close === 'function' &&
              typeof value.reportError === 'function' &&
              'relativePath' in value &&
              'isAsync' in value
          );
        const iface = function IFilePromise(value) {
          if (new.target) {
            throw new TypeError('IFilePromise is an interface and cannot be constructed.');
          }
          if (arguments.length !== 1) {
            throw new TypeError('IFilePromise conversion expects one value.');
          }
          if (value == null) return null;
          if (check(value)) return value;
          throw new TypeError('AS3 flash.desktop.IFilePromise value required.');
        };
        Object.defineProperties(iface, {
          __nfIs: { value: check },
          __nfInitialize: {
            value: () => {
              throw new TypeError('IFilePromise is an interface and cannot be extended by a class.');
            },
          },
          [Symbol.hasInstance]: { value: check },
        });
        return iface;
      })()
    );

    const NativeDragActions = defConstClass('NativeDragActions', {
      COPY: 'copy',
      LINK: 'link',
      MOVE: 'move',
      NONE: 'none',
    });

    class NativeDragOptions {
      constructor() {
        this.allowCopy = true;
        this.allowLink = true;
        this.allowMove = true;
      }
      toString() {
        return `[NativeDragOptions allowCopy=${Boolean(this.allowCopy)}, allowMove=${Boolean(this.allowMove)}, allowLink=${Boolean(this.allowLink)}]`;
      }
    }
    tagType(NativeDragOptions, 'NativeDragOptions', v => v instanceof NativeDragOptions);

    let dragState = {
      dragging: false,
      initiator: null,
      clipboard: null,
      allowedActions: null,
      dropAction: 'none',
      acceptedTarget: null,
    };

    const NativeDragManager = {
      NativeDragManager: function () {
        throw new TypeError('NativeDragManager cannot be instantiated.');
      },
    }.NativeDragManager;
    Object.defineProperties(NativeDragManager, {
      isSupported: { get: () => true, enumerable: true },
      isDragging: { get: () => dragState.dragging, enumerable: true },
      dragInitiator: { get: () => dragState.initiator, enumerable: true },
      dropAction: {
        get: () => dragState.dropAction,
        set: value => {
          const action = String(value ?? 'none');
          if (!['copy', 'move', 'link', 'none'].includes(action)) {
            throw argErr(`Invalid NativeDragActions value '${action}'.`);
          }
          dragState.dropAction = action;
        },
        enumerable: true,
      },
      acceptDragDrop: {
        value: target => {
          if (!target || typeof target !== 'object') {
            throw argErr('NativeDragManager.acceptDragDrop requires an InteractiveObject target.');
          }
          dragState.acceptedTarget = target;
        },
        enumerable: true,
      },
      doDrag: {
        value: (
          dragInitiator,
          clipboard,
          _bitmap = null,
          _offset = null,
          allowedActions = null
        ) => {
          ensureActive();
          if (!dragInitiator || typeof dragInitiator.dispatchEvent !== 'function') {
            throw argErr('NativeDragManager.doDrag requires an EventDispatcher dragInitiator.');
          }
          if (!clipboardStates.has(clipboard)) {
            throw argErr('NativeDragManager.doDrag requires a Clipboard instance.');
          }
          const opts =
            allowedActions instanceof NativeDragOptions
              ? allowedActions
              : new NativeDragOptions();
          const defaultAction = opts.allowCopy
            ? 'copy'
            : opts.allowMove
              ? 'move'
              : opts.allowLink
                ? 'link'
                : 'none';
          dragState = {
            dragging: true,
            initiator: dragInitiator,
            clipboard,
            allowedActions: opts,
            dropAction: defaultAction,
            acceptedTarget: dragState.acceptedTarget,
          };
          dragInitiator.dispatchEvent(
            new NativeDragEvent(
              NativeDragEvent.NATIVE_DRAG_START,
              false,
              false,
              0,
              0,
              null,
              clipboard,
              opts,
              dragState.dropAction
            )
          );
          scheduleTask(() => {
            if (!dragState.dragging || dragState.initiator !== dragInitiator) return;
            const target = dragState.acceptedTarget;
            if (target && typeof target.dispatchEvent === 'function') {
              target.dispatchEvent(
                new NativeDragEvent(
                  NativeDragEvent.NATIVE_DRAG_ENTER,
                  false,
                  false,
                  0,
                  0,
                  dragInitiator,
                  clipboard,
                  opts,
                  dragState.dropAction
                )
              );
              target.dispatchEvent(
                new NativeDragEvent(
                  NativeDragEvent.NATIVE_DRAG_OVER,
                  false,
                  false,
                  0,
                  0,
                  dragInitiator,
                  clipboard,
                  opts,
                  dragState.dropAction
                )
              );
              dragInitiator.dispatchEvent(
                new NativeDragEvent(
                  NativeDragEvent.NATIVE_DRAG_UPDATE,
                  false,
                  false,
                  0,
                  0,
                  target,
                  clipboard,
                  opts,
                  dragState.dropAction
                )
              );
              if (dragState.dropAction !== 'none') {
                target.dispatchEvent(
                  new NativeDragEvent(
                    NativeDragEvent.NATIVE_DRAG_DROP,
                    false,
                    false,
                    0,
                    0,
                    dragInitiator,
                    clipboard,
                    opts,
                    dragState.dropAction
                  )
                );
              } else {
                target.dispatchEvent(
                  new NativeDragEvent(
                    NativeDragEvent.NATIVE_DRAG_EXIT,
                    false,
                    false,
                    0,
                    0,
                    dragInitiator,
                    clipboard,
                    opts,
                    'none'
                  )
                );
              }
            }
            const finalAction = target ? dragState.dropAction : 'none';
            dragState.dragging = false;
            dragState.initiator = null;
            dragState.acceptedTarget = null;
            dragInitiator.dispatchEvent(
              new NativeDragEvent(
                NativeDragEvent.NATIVE_DRAG_COMPLETE,
                false,
                false,
                0,
                0,
                target,
                clipboard,
                opts,
                finalAction
              )
            );
          });
        },
        enumerable: true,
      },
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError('NativeDragManager cannot be extended.');
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    Object.freeze(NativeDragManager);

    const InvokeEventReason = defConstClass('InvokeEventReason', {
      LOGIN: 'login',
      NOTIFICATION: 'notification',
      OPEN_URL: 'openUrl',
      STANDARD: 'standard',
    });

    const NotificationType = defConstClass('NotificationType', {
      CRITICAL: 'critical',
      INFORMATIONAL: 'informational',
    });

    const SystemIdleMode = defConstClass('SystemIdleMode', {
      KEEP_AWAKE: 'keepAwake',
      NORMAL: 'normal',
    });

    const interactiveIconToken = Symbol('InteractiveIcon');
    class InteractiveIcon extends Icon {
      constructor(token) {
        if (token !== interactiveIconToken) {
          throw argErr('InteractiveIcon cannot be instantiated directly.');
        }
        super();
        Object.assign(iconStates.get(this), {
          width: 128,
          height: 128,
          menu: null,
          tooltip: '',
        });
      }
      get width() {
        return iconStates.get(this).width;
      }
      get height() {
        return iconStates.get(this).height;
      }
    }
    tagType(InteractiveIcon, 'InteractiveIcon', v => v instanceof InteractiveIcon);

    class DockIcon extends InteractiveIcon {
      constructor(token) {
        if (token !== interactiveIconToken) {
          throw argErr('DockIcon cannot be instantiated directly; use NativeApplication.nativeApplication.icon.');
        }
        super(token);
      }
      get menu() {
        return iconStates.get(this).menu;
      }
      set menu(value) {
        if (value !== null && !menuStates.has(value)) {
          throw new TypeError('DockIcon.menu requires a NativeMenu or null.');
        }
        iconStates.get(this).menu = value;
      }
      bounce(_priority = 'informational') {
        ensureActive();
      }
    }
    tagType(DockIcon, 'DockIcon', v => v instanceof DockIcon);

    class SystemTrayIcon extends InteractiveIcon {
      constructor(token) {
        if (token !== interactiveIconToken) {
          throw argErr('SystemTrayIcon cannot be instantiated directly.');
        }
        super(token);
      }
      get menu() {
        return iconStates.get(this).menu;
      }
      set menu(value) {
        if (value !== null && !menuStates.has(value)) {
          throw new TypeError('SystemTrayIcon.menu requires a NativeMenu or null.');
        }
        iconStates.get(this).menu = value;
      }
      get tooltip() {
        return iconStates.get(this).tooltip;
      }
      set tooltip(value) {
        iconStates.get(this).tooltip = String(value ?? '').slice(0, SystemTrayIcon.MAX_TIP_LENGTH);
      }
    }
    SystemTrayIcon.MAX_TIP_LENGTH = 63;
    tagType(SystemTrayIcon, 'SystemTrayIcon', v => v instanceof SystemTrayIcon);

    // -------------------------------------------------------------------------
    // 5. flash.display.* AIR Classes (Screen, NativeMenu, NativeWindow, Stage)
    // -------------------------------------------------------------------------
    const FocusDirection = defConstClass('FocusDirection', {
      BOTTOM: 'bottom',
      NONE: 'none',
      TOP: 'top',
    });

    const StageAspectRatio = defConstClass('StageAspectRatio', {
      ANY: 'any',
      LANDSCAPE: 'landscape',
      PORTRAIT: 'portrait',
    });

    const StageOrientation = defConstClass('StageOrientation', {
      DEFAULT: 'default',
      ROTATED_LEFT: 'rotatedLeft',
      ROTATED_RIGHT: 'rotatedRight',
      UNKNOWN: 'unknown',
      UPSIDE_DOWN: 'upsideDown',
    });

    const screenModeToken = Symbol('ScreenMode');
    const screenModes = new WeakMap();
    class ScreenMode {
      constructor(token, width = 1920, height = 1080, refreshRate = 60, colorDepth = 24) {
        if (token !== screenModeToken) {
          throw argErr('ScreenMode cannot be instantiated directly.');
        }
        screenModes.set(this, {
          width: width | 0,
          height: height | 0,
          refreshRate: refreshRate | 0,
          colorDepth: colorDepth | 0,
        });
      }
      get width() { return screenModes.get(this).width; }
      get height() { return screenModes.get(this).height; }
      get refreshRate() { return screenModes.get(this).refreshRate; }
      get colorDepth() { return screenModes.get(this).colorDepth; }
    }
    tagType(ScreenMode, 'ScreenMode', v => screenModes.has(v));

    const screenToken = Symbol('Screen');
    const screenStates = new WeakMap();
    let mainScreenSingleton = null;
    function currentScreenDimensions() {
      const scr = global.screen;
      if (scr && Number(scr.width) > 0 && Number(scr.height) > 0) {
        return {
          width: Number(scr.width) | 0,
          height: Number(scr.height) | 0,
          availX: Number(scr.availLeft || 0) | 0,
          availY: Number(scr.availTop || 0) | 0,
          availW: Number(scr.availWidth || scr.width) | 0,
          availH: Number(scr.availHeight || scr.height) | 0,
          colorDepth: Number(scr.colorDepth || 24) | 0,
        };
      }
      const [vw, vh] = getViewportSize();
      const w = Math.max(vw | 0, projW, 1280);
      const h = Math.max(vh | 0, projH, 800);
      return { width: w, height: h, availX: 0, availY: 0, availW: w, availH: h, colorDepth: 24 };
    }

    class Screen extends BaseDispatcher {
      constructor(token) {
        if (token !== screenToken) {
          throw argErr('Screen cannot be instantiated directly; use Screen.mainScreen or Screen.screens.');
        }
        super();
        screenStates.set(this, {});
      }
      static get mainScreen() {
        ensureActive();
        if (!mainScreenSingleton) mainScreenSingleton = new Screen(screenToken);
        return mainScreenSingleton;
      }
      static get screens() {
        return [Screen.mainScreen];
      }
      static getScreensForRectangle(rect) {
        if (!rect || typeof rect !== 'object') {
          throw argErr('Screen.getScreensForRectangle requires a Rectangle.');
        }
        const rx = Number(rect.x) || 0;
        const ry = Number(rect.y) || 0;
        const rw = Number(rect.width) || 0;
        const rh = Number(rect.height) || 0;
        const d = currentScreenDimensions();
        const intersects = rx < d.width && rx + rw > 0 && ry < d.height && ry + rh > 0;
        return intersects ? [Screen.mainScreen] : [];
      }
      get bounds() {
        const d = currentScreenDimensions();
        return makeRect(0, 0, d.width, d.height);
      }
      get visibleBounds() {
        const d = currentScreenDimensions();
        return makeRect(d.availX, d.availY, d.availW, d.availH);
      }
      get colorDepth() {
        return currentScreenDimensions().colorDepth;
      }
      get contentsScaleFactor() {
        return Number(global.devicePixelRatio) > 0 ? Number(global.devicePixelRatio) : 1;
      }
      get mode() {
        const d = currentScreenDimensions();
        return new ScreenMode(screenModeToken, d.width, d.height, 60, d.colorDepth);
      }
      get modes() {
        return [this.mode];
      }
    }
    tagType(Screen, 'Screen', v => screenStates.has(v));

    const menuStates = new WeakMap();
    const menuItemStates = new WeakMap();

    function menuContainsAncestor(candidateSubmenu, targetMenu) {
      let cur = targetMenu;
      while (cur) {
        if (cur === candidateSubmenu) return true;
        cur = menuStates.get(cur)?.parent || null;
      }
      return false;
    }

    class NativeMenuItem extends BaseDispatcher {
      constructor(label = '', isSeparator = false) {
        super();
        menuItemStates.set(this, {
          label: isSeparator ? '' : String(label ?? ''),
          isSeparator: Boolean(isSeparator),
          checked: false,
          enabled: true,
          name: '',
          data: null,
          keyEquivalent: '',
          keyEquivalentModifiers: [],
          mnemonicIndex: -1,
          menu: null,
          submenu: null,
        });
      }
      get label() { return menuItemStates.get(this).label; }
      set label(v) { menuItemStates.get(this).label = String(v ?? ''); }
      get isSeparator() { return menuItemStates.get(this).isSeparator; }
      get checked() { return menuItemStates.get(this).checked; }
      set checked(v) { menuItemStates.get(this).checked = Boolean(v); }
      get enabled() { return menuItemStates.get(this).enabled; }
      set enabled(v) { menuItemStates.get(this).enabled = Boolean(v); }
      get name() { return menuItemStates.get(this).name; }
      set name(v) { menuItemStates.get(this).name = String(v ?? ''); }
      get data() { return menuItemStates.get(this).data; }
      set data(v) { menuItemStates.get(this).data = v; }
      get keyEquivalent() { return menuItemStates.get(this).keyEquivalent; }
      set keyEquivalent(v) { menuItemStates.get(this).keyEquivalent = String(v ?? ''); }
      get keyEquivalentModifiers() { return menuItemStates.get(this).keyEquivalentModifiers.slice(); }
      set keyEquivalentModifiers(v) {
        if (!Array.isArray(v)) throw argErr('keyEquivalentModifiers must be an Array.');
        menuItemStates.get(this).keyEquivalentModifiers = v.slice();
      }
      get mnemonicIndex() { return menuItemStates.get(this).mnemonicIndex; }
      set mnemonicIndex(v) { menuItemStates.get(this).mnemonicIndex = Number(v) | 0; }
      get menu() { return menuItemStates.get(this).menu; }
      get submenu() { return menuItemStates.get(this).submenu; }
      set submenu(value) {
        const s = menuItemStates.get(this);
        if (value !== null && !menuStates.has(value)) {
          throw new TypeError('NativeMenuItem.submenu requires a NativeMenu or null.');
        }
        if (value && s.menu && menuContainsAncestor(value, s.menu)) {
          throw argErr('NativeMenu hierarchy cannot contain cycles.');
        }
        if (s.submenu) {
          menuStates.get(s.submenu).parent = null;
        }
        s.submenu = value;
        if (value) {
          menuStates.get(value).parent = s.menu;
        }
      }
      clone() {
        const s = menuItemStates.get(this);
        const copy = new NativeMenuItem(s.label, s.isSeparator);
        const cs = menuItemStates.get(copy);
        cs.checked = s.checked;
        cs.enabled = s.enabled;
        cs.name = s.name;
        cs.data = s.data;
        cs.keyEquivalent = s.keyEquivalent;
        cs.keyEquivalentModifiers = s.keyEquivalentModifiers.slice();
        cs.mnemonicIndex = s.mnemonicIndex;
        if (s.submenu) copy.submenu = s.submenu.clone();
        return copy;
      }
      toString() {
        const s = menuItemStates.get(this);
        return s.isSeparator ? '[NativeMenuItem separator]' : `[NativeMenuItem label="${s.label}"]`;
      }
    }
    tagType(NativeMenuItem, 'NativeMenuItem', v => menuItemStates.has(v));

    class NativeMenu extends BaseDispatcher {
      constructor() {
        super();
        menuStates.set(this, { items: [], parent: null });
      }
      static get isSupported() {
        return true;
      }
      get items() {
        return menuStates.get(this).items.slice();
      }
      set items(list) {
        if (!Array.isArray(list)) throw argErr('NativeMenu.items requires an Array.');
        this.removeAllItems();
        for (const item of list) this.addItem(item);
      }
      get numItems() {
        return menuStates.get(this).items.length;
      }
      get parent() {
        return menuStates.get(this).parent;
      }
      addItem(item) {
        return this.addItemAt(item, this.numItems);
      }
      addItemAt(item, index) {
        const s = menuStates.get(this);
        const isState = menuItemStates.get(item);
        if (!isState) throw new TypeError('NativeMenu requires a NativeMenuItem.');
        const idx = Number(index) | 0;
        if (idx < 0 || idx > s.items.length) {
          throw new RangeError(`Menu item index ${idx} is out of bounds.`);
        }
        if (isState.submenu && menuContainsAncestor(isState.submenu, this)) {
          throw argErr('NativeMenu hierarchy cannot contain cycles.');
        }
        if (isState.menu) {
          isState.menu.removeItem(item);
        }
        const clamped = Math.min(idx, s.items.length);
        s.items.splice(clamped, 0, item);
        isState.menu = this;
        if (isState.submenu) {
          menuStates.get(isState.submenu).parent = this;
        }
        return item;
      }
      addSubmenu(submenu, label) {
        return this.addSubmenuAt(submenu, this.numItems, label);
      }
      addSubmenuAt(submenu, index, label) {
        if (!menuStates.has(submenu)) throw new TypeError('addSubmenuAt requires a NativeMenu.');
        const item = new NativeMenuItem(String(label ?? ''), false);
        item.submenu = submenu;
        return this.addItemAt(item, index);
      }
      containsItem(item) {
        return menuStates.get(this).items.includes(item);
      }
      getItemAt(index) {
        const s = menuStates.get(this);
        const idx = Number(index) | 0;
        if (idx < 0 || idx >= s.items.length) {
          throw new RangeError(`Menu item index ${idx} is out of bounds.`);
        }
        return s.items[idx];
      }
      getItemByName(name) {
        const target = String(name ?? '');
        for (const item of menuStates.get(this).items) {
          if (menuItemStates.get(item).name === target) return item;
        }
        return null;
      }
      getItemIndex(item) {
        return menuStates.get(this).items.indexOf(item);
      }
      removeItem(item) {
        const idx = this.getItemIndex(item);
        if (idx < 0) throw argErr('The specified NativeMenuItem is not in this NativeMenu.');
        return this.removeItemAt(idx);
      }
      removeItemAt(index) {
        const s = menuStates.get(this);
        const idx = Number(index) | 0;
        if (idx < 0 || idx >= s.items.length) {
          throw new RangeError(`Menu item index ${idx} is out of bounds.`);
        }
        const [removed] = s.items.splice(idx, 1);
        const isState = menuItemStates.get(removed);
        isState.menu = null;
        if (isState.submenu) {
          menuStates.get(isState.submenu).parent = null;
        }
        return removed;
      }
      removeAllItems() {
        const s = menuStates.get(this);
        while (s.items.length > 0) {
          this.removeItemAt(s.items.length - 1);
        }
      }
      setItemIndex(item, index) {
        const s = menuStates.get(this);
        const cur = s.items.indexOf(item);
        if (cur < 0) throw argErr('The specified NativeMenuItem is not in this NativeMenu.');
        const idx = Number(index) | 0;
        if (idx < 0 || idx >= s.items.length) {
          throw new RangeError(`Menu item index ${idx} is out of bounds.`);
        }
        s.items.splice(cur, 1);
        s.items.splice(idx, 0, item);
      }
      clone() {
        const copy = new NativeMenu();
        for (const item of menuStates.get(this).items) {
          copy.addItem(item.clone());
        }
        return copy;
      }
      display(_stage, _stageX, _stageY) {
        ensureActive();
        this.dispatchEvent(new BaseEvent('preparing', false, false));
        this.dispatchEvent(new BaseEvent('displaying', false, false));
      }
    }
    tagType(NativeMenu, 'NativeMenu', v => menuStates.has(v));

    const NativeWindowType = defConstClass('NativeWindowType', {
      LIGHTWEIGHT: 'lightweight',
      NORMAL: 'normal',
      UTILITY: 'utility',
    });

    const NativeWindowSystemChrome = defConstClass('NativeWindowSystemChrome', {
      ALTERNATE: 'alternate',
      NONE: 'none',
      STANDARD: 'standard',
    });

    const NativeWindowDisplayState = defConstClass('NativeWindowDisplayState', {
      MAXIMIZED: 'maximized',
      MINIMIZED: 'minimized',
      NORMAL: 'normal',
    });

    const NativeWindowResize = defConstClass('NativeWindowResize', {
      BOTTOM: 'B',
      BOTTOM_LEFT: 'BL',
      BOTTOM_RIGHT: 'BR',
      LEFT: 'L',
      NONE: '',
      RIGHT: 'R',
      TOP: 'T',
      TOP_LEFT: 'TL',
      TOP_RIGHT: 'TR',
    });

    const NativeWindowRenderMode = defConstClass('NativeWindowRenderMode', {
      AUTO: 'auto',
      CPU: 'cpu',
      DIRECT: 'direct',
      GPU: 'gpu',
    });

    class NativeWindowInitOptions {
      constructor() {
        this.maximizable = true;
        this.minimizable = true;
        this.resizable = true;
        this.systemChrome = 'standard';
        this.transparent = false;
        this.type = 'normal';
        this.renderMode = 'auto';
        this.owner = null;
      }
    }
    tagType(NativeWindowInitOptions, 'NativeWindowInitOptions', v => v instanceof NativeWindowInitOptions);

    const windowStates = new WeakMap();
    const mainWinToken = Symbol('MainNativeWindow');
    let mainNativeWindow = null;
    let activeNativeWindow = null;
    const openWindows = [];

    function winState(receiver) {
      const s = windowStates.get(receiver);
      if (!s) throw new TypeError('Expected a NativeWindow instance.');
      return s;
    }

    function assertWindowOpen(s) {
      if (s.closed) throw illegalOp('NativeWindow has already been closed.', 3200);
    }

    class NativeWindow extends BaseDispatcher {
      constructor(initOptions = null) {
        ensureActive();
        super();
        const isMain = initOptions === mainWinToken;
        const opts =
          !isMain && initOptions instanceof NativeWindowInitOptions
            ? initOptions
            : new NativeWindowInitOptions();
        if (opts.transparent && opts.systemChrome === 'standard') {
          throw argErr('Transparent NativeWindows require systemChrome set to NativeWindowSystemChrome.NONE.');
        }
        if (opts.owner !== null && !windowStates.has(opts.owner)) {
          throw new TypeError('NativeWindowInitOptions.owner must be a NativeWindow or null.');
        }
        const [vw, vh] = getViewportSize();
        const w = isMain ? Math.max(vw | 0, projW) : 400;
        const h = isMain ? Math.max(vh | 0, projH) : 300;
        windowStates.set(this, {
          isMain,
          stageRef: isMain ? getStage() : null,
          title: isMain ? projName : '',
          x: 0,
          y: 0,
          width: w,
          height: h,
          minWidth: 100,
          minHeight: 80,
          maxWidth: 8192,
          maxHeight: 8192,
          displayState: 'normal',
          active: isMain,
          visible: isMain,
          closed: false,
          alwaysInFront: false,
          maximizable: Boolean(opts.maximizable),
          minimizable: Boolean(opts.minimizable),
          resizable: Boolean(opts.resizable),
          systemChrome: String(opts.systemChrome || 'standard'),
          transparent: Boolean(opts.transparent),
          type: String(opts.type || 'normal'),
          renderMode: String(opts.renderMode || 'auto'),
          owner: opts.owner || null,
          menu: null,
        });
        openWindows.push(this);
        if (isMain || !activeNativeWindow) activeNativeWindow = this;
      }
      static get isSupported() { return true; }
      static get supportsMenu() { return true; }
      static get supportsNotification() { return true; }
      static get supportsTransparency() { return true; }
      static get systemMinSize() { return makePoint(100, 80); }
      static get systemMaxSize() { return makePoint(8192, 8192); }

      get stage() {
        const s = winState(this);
        return s.isMain ? getStage() || s.stageRef : s.stageRef;
      }
      get title() { return winState(this).title; }
      set title(v) {
        const s = winState(this);
        assertWindowOpen(s);
        s.title = String(v ?? '');
      }
      get closed() { return winState(this).closed; }
      get active() { return winState(this).active && !winState(this).closed; }
      get visible() { return winState(this).visible && !winState(this).closed; }
      set visible(v) {
        const s = winState(this);
        assertWindowOpen(s);
        s.visible = Boolean(v);
      }
      get alwaysInFront() { return winState(this).alwaysInFront; }
      set alwaysInFront(v) {
        const s = winState(this);
        assertWindowOpen(s);
        s.alwaysInFront = Boolean(v);
      }
      get maximizable() { return winState(this).maximizable; }
      get minimizable() { return winState(this).minimizable; }
      get resizable() { return winState(this).resizable; }
      get systemChrome() { return winState(this).systemChrome; }
      get transparent() { return winState(this).transparent; }
      get type() { return winState(this).type; }
      get renderMode() { return winState(this).renderMode; }
      get owner() { return winState(this).owner; }
      get displayState() { return winState(this).displayState; }
      get menu() { return winState(this).menu; }
      set menu(v) {
        const s = winState(this);
        assertWindowOpen(s);
        if (v !== null && !menuStates.has(v)) {
          throw new TypeError('NativeWindow.menu requires a NativeMenu or null.');
        }
        s.menu = v;
      }
      get minSize() {
        const s = winState(this);
        return makePoint(s.minWidth, s.minHeight);
      }
      set minSize(pt) {
        const s = winState(this);
        assertWindowOpen(s);
        if (!pt || typeof pt !== 'object') throw argErr('minSize requires a Point.');
        s.minWidth = Math.max(0, Number(pt.x) | 0);
        s.minHeight = Math.max(0, Number(pt.y) | 0);
      }
      get maxSize() {
        const s = winState(this);
        return makePoint(s.maxWidth, s.maxHeight);
      }
      set maxSize(pt) {
        const s = winState(this);
        assertWindowOpen(s);
        if (!pt || typeof pt !== 'object') throw argErr('maxSize requires a Point.');
        s.maxWidth = Math.max(s.minWidth, Number(pt.x) | 0);
        s.maxHeight = Math.max(s.minHeight, Number(pt.y) | 0);
      }
      get bounds() {
        const s = winState(this);
        return makeRect(s.x, s.y, s.width, s.height);
      }
      set bounds(rect) {
        const s = winState(this);
        assertWindowOpen(s);
        if (!rect || typeof rect !== 'object') throw argErr('bounds requires a Rectangle.');
        const before = makeRect(s.x, s.y, s.width, s.height);
        const nx = Number(rect.x) | 0;
        const ny = Number(rect.y) | 0;
        const nw = Math.min(s.maxWidth, Math.max(s.minWidth, Number(rect.width) | 0));
        const nh = Math.min(s.maxHeight, Math.max(s.minHeight, Number(rect.height) | 0));
        const moved = nx !== s.x || ny !== s.y;
        const resized = nw !== s.width || nh !== s.height;
        s.x = nx;
        s.y = ny;
        s.width = nw;
        s.height = nh;
        const after = makeRect(s.x, s.y, s.width, s.height);
        if (moved) {
          this.dispatchEvent(
            new NativeWindowBoundsEvent(NativeWindowBoundsEvent.MOVE, false, false, before, after)
          );
        }
        if (resized) {
          this.dispatchEvent(
            new NativeWindowBoundsEvent(NativeWindowBoundsEvent.RESIZE, false, false, before, after)
          );
        }
      }
      get x() { return winState(this).x; }
      set x(v) {
        const s = winState(this);
        this.bounds = makeRect(Number(v) | 0, s.y, s.width, s.height);
      }
      get y() { return winState(this).y; }
      set y(v) {
        const s = winState(this);
        this.bounds = makeRect(s.x, Number(v) | 0, s.width, s.height);
      }
      get width() { return winState(this).width; }
      set width(v) {
        const s = winState(this);
        this.bounds = makeRect(s.x, s.y, Number(v) | 0, s.height);
      }
      get height() { return winState(this).height; }
      set height(v) {
        const s = winState(this);
        this.bounds = makeRect(s.x, s.y, s.width, Number(v) | 0);
      }

      activate() {
        const s = winState(this);
        assertWindowOpen(s);
        if (activeNativeWindow && activeNativeWindow !== this) {
          const prev = windowStates.get(activeNativeWindow);
          if (prev) prev.active = false;
          activeNativeWindow.dispatchEvent(new BaseEvent(BaseEvent.DEACTIVATE));
        }
        s.visible = true;
        s.active = true;
        activeNativeWindow = this;
        this.dispatchEvent(new BaseEvent(BaseEvent.ACTIVATE));
      }

      close() {
        const s = winState(this);
        if (s.closed) return;
        for (const child of openWindows.slice()) {
          if (windowStates.get(child)?.owner === this) {
            child.close();
          }
        }
        s.closed = true;
        s.visible = false;
        s.active = false;
        const idx = openWindows.indexOf(this);
        if (idx >= 0) openWindows.splice(idx, 1);
        if (activeNativeWindow === this) {
          activeNativeWindow = openWindows.length ? openWindows[openWindows.length - 1] : null;
          if (activeNativeWindow) windowStates.get(activeNativeWindow).active = true;
        }
        this.dispatchEvent(new BaseEvent(BaseEvent.CLOSE));
      }

      _changeDisplayState(nextState) {
        const s = winState(this);
        assertWindowOpen(s);
        if (s.displayState === nextState) return;
        const prev = s.displayState;
        const changing = new NativeWindowDisplayStateEvent(
          NativeWindowDisplayStateEvent.DISPLAY_STATE_CHANGING,
          false,
          true,
          prev,
          nextState
        );
        if (!this.dispatchEvent(changing)) return;
        s.displayState = nextState;
        if (nextState === 'maximized') {
          const vis = Screen.mainScreen.visibleBounds;
          this.bounds = makeRect(vis.x, vis.y, vis.width, vis.height);
        }
        this.dispatchEvent(
          new NativeWindowDisplayStateEvent(
            NativeWindowDisplayStateEvent.DISPLAY_STATE_CHANGE,
            false,
            false,
            prev,
            nextState
          )
        );
      }
      maximize() { this._changeDisplayState('maximized'); }
      minimize() { this._changeDisplayState('minimized'); }
      restore() { this._changeDisplayState('normal'); }

      globalToScreen(globalPoint) {
        const s = winState(this);
        if (!globalPoint || typeof globalPoint !== 'object') {
          throw argErr('globalToScreen requires a Point.');
        }
        return makePoint(s.x + (Number(globalPoint.x) || 0), s.y + (Number(globalPoint.y) || 0));
      }

      listOwnedWindows() {
        winState(this);
        const owned = openWindows.filter(w => windowStates.get(w)?.owner === this && !windowStates.get(w)?.closed);
        return makeVector('flash.display.NativeWindow', owned);
      }

      notifyUser(_type = 'informational') {
        assertWindowOpen(winState(this));
      }

      orderToFront() {
        const s = winState(this);
        assertWindowOpen(s);
        const idx = openWindows.indexOf(this);
        if (idx >= 0) {
          openWindows.splice(idx, 1);
          openWindows.push(this);
        }
        return true;
      }
      orderToBack() {
        const s = winState(this);
        assertWindowOpen(s);
        const idx = openWindows.indexOf(this);
        if (idx >= 0) {
          openWindows.splice(idx, 1);
          openWindows.unshift(this);
        }
        return true;
      }
      orderInFrontOf(window) {
        const s = winState(this);
        assertWindowOpen(s);
        if (!windowStates.has(window)) throw argErr('orderInFrontOf requires a NativeWindow.');
        if (window === this) return false;
        const cur = openWindows.indexOf(this);
        if (cur >= 0) openWindows.splice(cur, 1);
        const targetIdx = openWindows.indexOf(window);
        if (targetIdx < 0) return false;
        openWindows.splice(targetIdx + 1, 0, this);
        return true;
      }
      orderInBackOf(window) {
        const s = winState(this);
        assertWindowOpen(s);
        if (!windowStates.has(window)) throw argErr('orderInBackOf requires a NativeWindow.');
        if (window === this) return false;
        const cur = openWindows.indexOf(this);
        if (cur >= 0) openWindows.splice(cur, 1);
        const targetIdx = openWindows.indexOf(window);
        if (targetIdx < 0) return false;
        openWindows.splice(targetIdx, 0, this);
        return true;
      }
      startMove() {
        assertWindowOpen(winState(this));
        return false;
      }
      startResize(_edgeOrCorner = 'BR') {
        assertWindowOpen(winState(this));
        return false;
      }
    }
    tagType(NativeWindow, 'NativeWindow', v => windowStates.has(v));

    function getMainNativeWindow() {
      if (!mainNativeWindow) {
        mainNativeWindow = new NativeWindow(mainWinToken);
      }
      return mainNativeWindow;
    }

    // Attach AIR-specific properties & methods to Stage prototype idempotently.
    let stageOrientationState = {
      orientation: 'default',
      deviceOrientation: 'default',
      autoOrients: false,
      aspectRatio: 'any',
      vsyncEnabled: true,
    };

    function bindStageAirProperties() {
      const stageObj = getStage();
      if (!stageObj || typeof stageObj !== 'object') return;
      const proto = Object.getPrototypeOf(stageObj);
      if (!proto || proto === Object.prototype) return;
      const mapSym = Symbol.for('neoflash.air.stageHooks');
      if (!proto[mapSym]) {
        Object.defineProperty(proto, mapSym, { value: new WeakMap() });
      }
      proto[mapSym].set(stageObj, {
        getNativeWindow: () => getMainNativeWindow(),
        getOrientation: () => stageOrientationState.orientation,
        getDeviceOrientation: () => stageOrientationState.deviceOrientation,
        getAutoOrients: () => stageOrientationState.autoOrients,
        setAutoOrients: v => { stageOrientationState.autoOrients = Boolean(v); },
        getSupportedOrientations: () =>
          makeVector('String', ['default', 'rotatedLeft', 'rotatedRight', 'upsideDown']),
        getVsyncEnabled: () => stageOrientationState.vsyncEnabled,
        setVsyncEnabled: v => {
          const next = Boolean(v);
          const prev = stageOrientationState.vsyncEnabled;
          stageOrientationState.vsyncEnabled = next;
          if (prev !== next) {
            stageObj.dispatchEvent(
              new VsyncStateChangeAvailabilityEvent(
                VsyncStateChangeAvailabilityEvent.VSYNC_STATE_CHANGE_AVAILABILITY,
                false,
                false,
                true
              )
            );
          }
        },
        assignFocus: (obj, dir) => {
          const d = String(dir ?? 'none');
          if (!['top', 'bottom', 'none'].includes(d)) {
            throw argErr(`Invalid FocusDirection '${d}'.`);
          }
          stageObj.focus = obj || null;
        },
        setAspectRatio: ratio => {
          const r = String(ratio ?? 'any');
          if (!['any', 'portrait', 'landscape'].includes(r)) {
            throw argErr(`Invalid StageAspectRatio '${r}'.`);
          }
          stageOrientationState.aspectRatio = r;
          if (r === 'landscape' && stageOrientationState.orientation === 'default' && projH > projW) {
            stageObj.setOrientation('rotatedRight');
          }
        },
        setOrientation: nextOrientation => {
          const next = String(nextOrientation ?? 'default');
          if (!['default', 'rotatedLeft', 'rotatedRight', 'upsideDown', 'unknown'].includes(next)) {
            throw argErr(`Invalid StageOrientation '${next}'.`);
          }
          const prev = stageOrientationState.orientation;
          if (prev === next) return;
          const EvCtor = options.eventFamilies?.StageOrientationEvent;
          if (EvCtor) {
            const changing = new EvCtor('orientationChanging', false, true, prev, next);
            if (!stageObj.dispatchEvent(changing)) return;
          }
          stageOrientationState.orientation = next;
          if (EvCtor) {
            stageObj.dispatchEvent(new EvCtor('orientationChange', false, false, prev, next));
          }
        },
      });

      if (!Object.hasOwn(proto, 'nativeWindow')) {
        const hook = self => proto[mapSym].get(self);
        Object.defineProperties(proto, {
          nativeWindow: {
            get() { return hook(this)?.getNativeWindow() ?? null; },
            enumerable: true,
            configurable: true,
          },
          orientation: {
            get() { return hook(this)?.getOrientation() ?? 'default'; },
            enumerable: true,
            configurable: true,
          },
          deviceOrientation: {
            get() { return hook(this)?.getDeviceOrientation() ?? 'default'; },
            enumerable: true,
            configurable: true,
          },
          autoOrients: {
            get() { return hook(this)?.getAutoOrients() ?? false; },
            set(v) { hook(this)?.setAutoOrients(v); },
            enumerable: true,
            configurable: true,
          },
          supportedOrientations: {
            get() { return hook(this)?.getSupportedOrientations() ?? []; },
            enumerable: true,
            configurable: true,
          },
          supportsOrientationChange: {
            get() { return true; },
            enumerable: true,
            configurable: true,
          },
          vsyncEnabled: {
            get() { return hook(this)?.getVsyncEnabled() ?? true; },
            set(v) { hook(this)?.setVsyncEnabled(v); },
            enumerable: true,
            configurable: true,
          },
          assignFocus: {
            value(objectToFocus, direction = 'none') {
              hook(this)?.assignFocus(objectToFocus, direction);
            },
            configurable: true,
          },
          setAspectRatio: {
            value(newAspectRatio) {
              hook(this)?.setAspectRatio(newAspectRatio);
            },
            configurable: true,
          },
          setOrientation: {
            value(newOrientation) {
              hook(this)?.setOrientation(newOrientation);
            },
            configurable: true,
          },
        });
      }
    }
    bindStageAirProperties();

    // NativeApplication, NativeProcess, Updater (depend on NativeWindow & NativeMenu)
    const nativeAppToken = Symbol('NativeApplication');
    let nativeAppSingleton = null;
    let dockIconSingleton = null;
    let nativeAppState = {
      autoExit: true,
      idleThreshold: 300,
      systemIdleMode: 'normal',
      executeInBackground: false,
      menu: null,
    };

    class NativeApplication extends BaseDispatcher {
      constructor(token) {
        if (token !== nativeAppToken) {
          throw argErr('NativeApplication is a singleton; use NativeApplication.nativeApplication.');
        }
        super();
      }
      static get nativeApplication() {
        ensureActive();
        if (!nativeAppSingleton) {
          getMainNativeWindow();
          nativeAppSingleton = new NativeApplication(nativeAppToken);
        }
        return nativeAppSingleton;
      }
      static get supportsMenu() { return true; }
      static get supportsDockIcon() { return true; }
      static get supportsSystemTrayIcon() { return false; }
      static get supportsDefaultApplication() { return false; }
      static get supportsStartAtLogin() { return false; }

      get applicationID() { return callKernel('app_id'); }
      get publisherID() { return ''; }
      get applicationDescriptor() {
        const xmlText = callKernel('descriptor_xml');
        const XMLCtor = getXML();
        return typeof XMLCtor === 'function' ? new XMLCtor(xmlText) : xmlText;
      }
      get runtimeVersion() { return '32.0.0.116'; }
      get runtimePatchLevel() { return 116; }
      get isCompiledAOT() { return false; }
      get activeWindow() {
        getMainNativeWindow();
        return activeNativeWindow;
      }
      get openedWindows() {
        getMainNativeWindow();
        return openWindows.slice();
      }
      get autoExit() { return nativeAppState.autoExit; }
      set autoExit(v) { nativeAppState.autoExit = Boolean(v); }
      get idleThreshold() { return nativeAppState.idleThreshold; }
      set idleThreshold(v) {
        const secs = Number(v) | 0;
        if (secs < 5 || secs > 86400) {
          throw argErr('NativeApplication.idleThreshold must be between 5 and 86400 seconds.');
        }
        nativeAppState.idleThreshold = secs;
      }
      get systemIdleMode() { return nativeAppState.systemIdleMode; }
      set systemIdleMode(v) {
        const mode = String(v ?? 'normal');
        if (mode !== 'normal' && mode !== 'keepAwake') {
          throw argErr(`Invalid SystemIdleMode '${mode}'.`);
        }
        nativeAppState.systemIdleMode = mode;
      }
      get executeInBackground() { return nativeAppState.executeInBackground; }
      set executeInBackground(v) { nativeAppState.executeInBackground = Boolean(v); }
      get startAtLogin() { return false; }
      set startAtLogin(_v) {
        throw illegalOp('NativeApplication.startAtLogin is not supported in the HTML5 browser sandbox.');
      }
      get icon() {
        if (!dockIconSingleton) dockIconSingleton = new DockIcon(interactiveIconToken);
        return dockIconSingleton;
      }
      get menu() { return nativeAppState.menu; }
      set menu(v) {
        if (v !== null && !menuStates.has(v)) {
          throw new TypeError('NativeApplication.menu requires a NativeMenu or null.');
        }
        nativeAppState.menu = v;
      }
      get timeSinceLastUserInput() {
        return Math.max(0, Math.floor((Date.now() - lastInputTimestamp) / 1000));
      }

      activate(window = null) {
        const win = window || getMainNativeWindow();
        if (!windowStates.has(win)) throw new TypeError('activate expects a NativeWindow or null.');
        win.activate();
        this.dispatchEvent(new BaseEvent(BaseEvent.ACTIVATE));
      }

      exit(_errorCode = 0) {
        const exiting = new BaseEvent('exiting', false, true);
        if (!this.dispatchEvent(exiting)) return;
        for (const win of openWindows.slice()) {
          const closing = new BaseEvent('closing', false, true);
          if (win.dispatchEvent(closing)) {
            win.close();
          }
        }
      }

      copy() {
        this.dispatchEvent(new BaseEvent('copy', false, true));
        if (activeStageText && typeof activeStageText._editCopy === 'function') {
          return activeStageText._editCopy();
        }
        return false;
      }
      cut() {
        this.dispatchEvent(new BaseEvent('cut', false, true));
        if (activeStageText && typeof activeStageText._editCut === 'function') {
          return activeStageText._editCut();
        }
        return false;
      }
      paste() {
        this.dispatchEvent(new BaseEvent('paste', false, true));
        if (activeStageText && typeof activeStageText._editPaste === 'function') {
          return activeStageText._editPaste();
        }
        return false;
      }
      clear() {
        this.dispatchEvent(new BaseEvent('clear', false, true));
        if (activeStageText && typeof activeStageText._editClear === 'function') {
          return activeStageText._editClear();
        }
        return false;
      }
      selectAll() {
        this.dispatchEvent(new BaseEvent('selectAll', false, true));
        if (activeStageText && typeof activeStageText._editSelectAll === 'function') {
          return activeStageText._editSelectAll();
        }
        return false;
      }

      isSetAsDefaultApplication(_extension) {
        throw illegalOp('Default application associations are not supported in the HTML5 browser sandbox.');
      }
      setAsDefaultApplication(_extension) {
        throw illegalOp('Default application associations are not supported in the HTML5 browser sandbox.');
      }
      removeAsDefaultApplication(_extension) {
        throw illegalOp('Default application associations are not supported in the HTML5 browser sandbox.');
      }
      getDefaultApplication(_extension) {
        throw illegalOp('Default application associations are not supported in the HTML5 browser sandbox.');
      }
    }
    tagType(NativeApplication, 'NativeApplication', v => v === nativeAppSingleton && v !== null);

    const startupInfos = new WeakMap();
    class NativeProcessStartupInfo {
      constructor() {
        startupInfos.set(this, {
          executable: null,
          workingDirectory: null,
          arguments: makeVector('String', []),
        });
      }
      get executable() { return startupInfos.get(this).executable; }
      set executable(v) {
        if (v !== null && !fileStates.has(v)) {
          throw new TypeError('NativeProcessStartupInfo.executable requires a File or null.');
        }
        startupInfos.get(this).executable = v;
      }
      get workingDirectory() { return startupInfos.get(this).workingDirectory; }
      set workingDirectory(v) {
        if (v !== null && !fileStates.has(v)) {
          throw new TypeError('NativeProcessStartupInfo.workingDirectory requires a File or null.');
        }
        startupInfos.get(this).workingDirectory = v;
      }
      get arguments() { return startupInfos.get(this).arguments; }
      set arguments(v) {
        if (!v || typeof v.length !== 'number') {
          throw new TypeError('NativeProcessStartupInfo.arguments requires a Vector.<String>.');
        }
        const arr = [];
        for (let i = 0; i < v.length; i++) arr.push(String(v[i]));
        startupInfos.get(this).arguments = makeVector('String', arr);
      }
    }
    tagType(NativeProcessStartupInfo, 'NativeProcessStartupInfo', v => startupInfos.has(v));

    class NativeProcess extends BaseDispatcher {
      constructor() {
        super();
      }
      static get isSupported() { return false; }
      static isValidExecutable(f) {
        return Boolean(f && fileStates.has(f) && f.exists && !f.isDirectory);
      }
      get running() { return false; }
      get standardInput() { return null; }
      get standardOutput() { return null; }
      get standardError() { return null; }
      start(_info) {
        throw illegalOp('NativeProcess is not supported in the HTML5 browser sandbox.', 3219);
      }
      closeInput() {
        throw illegalOp('NativeProcess is not running.', 3214);
      }
      exit(_force = false) {
        throw illegalOp('NativeProcess is not running.', 3214);
      }
    }
    tagType(NativeProcess, 'NativeProcess', v => v instanceof NativeProcess);

    class Updater {
      static get isSupported() { return false; }
      update(_airFile, _version) {
        throw illegalOp('Updater is not supported in the HTML5 browser sandbox.', 3219);
      }
    }
    tagType(Updater, 'Updater', v => v instanceof Updater);

    // -------------------------------------------------------------------------
    // 6. flash.data.* (EncryptedLocalStore & Relational SQL Engine Adapter)
    // -------------------------------------------------------------------------
    const EncryptedLocalStore = {
      EncryptedLocalStore: function () {
        throw new TypeError('EncryptedLocalStore supplies static methods only.');
      },
    }.EncryptedLocalStore;
    Object.defineProperties(EncryptedLocalStore, {
      isSupported: { get: () => true, enumerable: true },
      getItem: {
        value: name => {
          if (name == null || String(name) === '') {
            throw argErr('EncryptedLocalStore.getItem requires a non-empty name.');
          }
          const bytes = callKernel('els_get', String(name));
          if (bytes == null) return null;
          return binary ? binary.fromBytes(bytes) : new Uint8Array(bytes);
        },
        enumerable: true,
      },
      setItem: {
        value: (name, data, _stronglyBound = false) => {
          if (name == null || String(name) === '') {
            throw argErr('EncryptedLocalStore.setItem requires a non-empty name.');
          }
          if (!data) throw argErr('EncryptedLocalStore.setItem requires a ByteArray.');
          let bytes;
          if (binary?.ByteArray?.__nfIs?.(data)) bytes = binary.toBytes(data);
          else if (data instanceof Uint8Array) bytes = data;
          else throw new TypeError('EncryptedLocalStore.setItem requires a ByteArray.');
          callKernel('els_set', String(name), bytes);
        },
        enumerable: true,
      },
      removeItem: {
        value: name => {
          if (name == null || String(name) === '') {
            throw argErr('EncryptedLocalStore.removeItem requires a non-empty name.');
          }
          callKernel('els_remove', String(name));
        },
        enumerable: true,
      },
      reset: {
        value: () => {
          callKernel('els_reset');
        },
        enumerable: true,
      },
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError('EncryptedLocalStore cannot be extended.');
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    Object.freeze(EncryptedLocalStore);

    const SQLMode = defConstClass('SQLMode', {
      CREATE: 'create',
      READ: 'read',
      UPDATE: 'update',
    });

    const SQLTransactionLockType = defConstClass('SQLTransactionLockType', {
      DEFERRED: 'deferred',
      EXCLUSIVE: 'exclusive',
      IMMEDIATE: 'immediate',
    });

    const SQLCollationType = defConstClass('SQLCollationType', {
      BINARY: 'binary',
      NO_CASE: 'noCase',
    });

    const SQLColumnNameStyle = defConstClass('SQLColumnNameStyle', {
      DEFAULT: 'default',
      LONG: 'long',
      SHORT: 'short',
    });

    class SQLResult {
      constructor(data = null, rowsAffected = 0, complete = true, rowID = 0) {
        Object.defineProperties(this, {
          data: { value: Array.isArray(data) ? data : null, enumerable: true },
          rowsAffected: { value: Number(rowsAffected) || 0, enumerable: true },
          complete: { value: Boolean(complete), enumerable: true },
          lastInsertRowID: { value: Number(rowID) || 0, enumerable: true },
        });
      }
    }
    tagType(SQLResult, 'SQLResult', v => v instanceof SQLResult);

    class SQLColumnSchema {
      constructor(
        name = '',
        primaryKey = false,
        allowNull = true,
        autoIncrement = false,
        dataType = '',
        defaultCollationType = 'binary'
      ) {
        Object.defineProperties(this, {
          name: { value: String(name ?? ''), enumerable: true },
          primaryKey: { value: Boolean(primaryKey), enumerable: true },
          allowNull: { value: Boolean(allowNull), enumerable: true },
          autoIncrement: { value: Boolean(autoIncrement), enumerable: true },
          dataType: { value: String(dataType ?? ''), enumerable: true },
          defaultCollationType: { value: String(defaultCollationType ?? 'binary'), enumerable: true },
        });
      }
    }
    tagType(SQLColumnSchema, 'SQLColumnSchema', v => v instanceof SQLColumnSchema);

    class SQLSchema {
      constructor(database = 'main', name = '', sql = '') {
        Object.defineProperties(this, {
          database: { value: String(database ?? 'main'), enumerable: true },
          name: { value: String(name ?? ''), enumerable: true },
          sql: { value: String(sql ?? ''), enumerable: true },
        });
      }
    }
    tagType(SQLSchema, 'SQLSchema', v => v instanceof SQLSchema);

    class SQLTableSchema extends SQLSchema {
      constructor(database = 'main', name = '', sql = '', columns = null) {
        super(database, name, sql);
        Object.defineProperty(this, 'columns', {
          value: Array.isArray(columns) ? columns.slice() : [],
          enumerable: true,
        });
      }
    }
    tagType(SQLTableSchema, 'SQLTableSchema', v => v instanceof SQLTableSchema);

    class SQLViewSchema extends SQLSchema {
      constructor(database = 'main', name = '', sql = '') {
        super(database, name, sql);
      }
    }
    tagType(SQLViewSchema, 'SQLViewSchema', v => v instanceof SQLViewSchema);

    class SQLIndexSchema extends SQLSchema {
      constructor(database = 'main', name = '', sql = '', table = '') {
        super(database, name, sql);
        Object.defineProperty(this, 'table', {
          value: String(table ?? ''),
          enumerable: true,
        });
      }
    }
    tagType(SQLIndexSchema, 'SQLIndexSchema', v => v instanceof SQLIndexSchema);

    class SQLTriggerSchema extends SQLSchema {
      constructor(database = 'main', name = '', sql = '', table = '') {
        super(database, name, sql);
        Object.defineProperty(this, 'table', {
          value: String(table ?? ''),
          enumerable: true,
        });
      }
    }
    tagType(SQLTriggerSchema, 'SQLTriggerSchema', v => v instanceof SQLTriggerSchema);

    class SQLSchemaResult {
      constructor(tables = null, views = null, indices = null, triggers = null) {
        Object.defineProperties(this, {
          tables: { value: Array.isArray(tables) ? tables.slice() : [], enumerable: true },
          views: { value: Array.isArray(views) ? views.slice() : [], enumerable: true },
          indices: { value: Array.isArray(indices) ? indices.slice() : [], enumerable: true },
          triggers: { value: Array.isArray(triggers) ? triggers.slice() : [], enumerable: true },
        });
      }
    }
    tagType(SQLSchemaResult, 'SQLSchemaResult', v => v instanceof SQLSchemaResult);

    function callSqlKernel(op, method, ...args) {
      ensureActive();
      try {
        return kernel[method](...args);
      } catch (err) {
        throw parseSqlKernelError(err, op);
      }
    }

    function resolveSqlReferencePath(reference) {
      if (reference == null) return null;
      if (fileStates.has(reference)) {
        const p = fileStates.get(reference).path;
        if (!p) throw argErr('SQLConnection File reference does not have a path.');
        return p;
      }
      if (typeof reference === 'string') {
        const s = reference.trim();
        if (!s || s === ':memory:') return null;
        return callKernel('normalize_path', s);
      }
      throw new TypeError('SQLConnection reference must be a File, String, or null.');
    }

    const sqlConnStates = new WeakMap();
    function connState(receiver) {
      const s = sqlConnStates.get(receiver);
      if (!s) throw new TypeError('Expected a SQLConnection instance.');
      return s;
    }

    function runSqlConnOp(conn, s, asyncOp, opName, eventType, responder, workFn, resultValueFn) {
      if (asyncOp) {
        const token = ++s.cancelToken;
        scheduleTask(() => {
          if (s.cancelToken !== token) return;
          try {
            const out = workFn();
            const payload = resultValueFn ? resultValueFn(out) : new SQLEvent(eventType);
            if (!notifyResponderResult(responder, payload)) {
              conn.dispatchEvent(new SQLEvent(eventType));
            }
          } catch (err) {
            const sqlErr = err instanceof SQLError ? err : parseSqlKernelError(err, opName);
            if (!notifyResponderStatus(responder, sqlErr)) {
              conn.dispatchEvent(new SQLErrorEvent(SQLErrorEvent.ERROR, false, false, sqlErr));
            }
          }
        });
        return;
      }
      try {
        const out = workFn();
        const payload = resultValueFn ? resultValueFn(out) : new SQLEvent(eventType);
        notifyResponderResult(responder, payload);
        conn.dispatchEvent(new SQLEvent(eventType));
      } catch (err) {
        const sqlErr = err instanceof SQLError ? err : parseSqlKernelError(err, opName);
        if (notifyResponderStatus(responder, sqlErr)) return;
        throw sqlErr;
      }
    }

    class SQLConnection extends BaseDispatcher {
      constructor() {
        ensureActive();
        super();
        sqlConnStates.set(this, {
          connId: null,
          isAsync: false,
          autoCompact: false,
          pageSize: 1024,
          cacheSize: 2500,
          columnNameStyle: 'default',
          lastSchemaResult: null,
          cancelToken: 0,
        });
      }
      static get isSupported() { return true; }

      _kernelState() {
        const s = connState(this);
        if (s.connId === null) return null;
        return JSON.parse(callSqlKernel('open', 'sql_state_json', s.connId));
      }

      get connected() {
        const s = connState(this);
        return s.connId !== null;
      }
      get inTransaction() {
        return this._kernelState()?.inTransaction ?? false;
      }
      get lastInsertRowID() {
        return this._kernelState()?.lastInsertRowID ?? 0;
      }
      get totalChanges() {
        return this._kernelState()?.totalChanges ?? 0;
      }
      get autoCompact() {
        return this._kernelState()?.autoCompact ?? connState(this).autoCompact;
      }
      set autoCompact(v) {
        const s = connState(this);
        if (s.connId !== null) {
          throw illegalOp('SQLConnection.autoCompact cannot be changed while connected.', 3102);
        }
        s.autoCompact = Boolean(v);
      }
      get pageSize() {
        return this._kernelState()?.pageSize ?? connState(this).pageSize;
      }
      set pageSize(v) {
        const s = connState(this);
        if (s.connId !== null) {
          throw illegalOp('SQLConnection.pageSize cannot be changed while connected.', 3102);
        }
        s.pageSize = Number(v) >>> 0;
      }
      get cacheSize() {
        return this._kernelState()?.cacheSize ?? connState(this).cacheSize;
      }
      set cacheSize(v) {
        const s = connState(this);
        const cs = Math.max(1, Number(v) >>> 0);
        s.cacheSize = cs;
        if (s.connId !== null) {
          callSqlKernel('execute', 'sql_set_config', s.connId, cs, undefined);
        }
      }
      get columnNameStyle() {
        return this._kernelState()?.columnNameStyle ?? connState(this).columnNameStyle;
      }
      set columnNameStyle(v) {
        const s = connState(this);
        const style = String(v ?? 'default');
        if (!['default', 'long', 'short'].includes(style)) {
          throw argErr(`Invalid SQLColumnNameStyle '${style}'.`);
        }
        s.columnNameStyle = style;
        if (s.connId !== null) {
          callSqlKernel('execute', 'sql_set_config', s.connId, undefined, style);
        }
      }

      open(
        reference = null,
        openMode = 'create',
        autoCompact = false,
        pageSize = 1024,
        encryptionKey = null
      ) {
        const s = connState(this);
        if (s.connId !== null) throw illegalOp('SQLConnection is already open.', 3101);
        const keyBytes =
          encryptionKey == null
            ? new Uint8Array(0)
            : (() => {
                let b;
                if (binary?.ByteArray?.__nfIs?.(encryptionKey)) b = binary.toBytes(encryptionKey);
                else if (encryptionKey instanceof Uint8Array) b = encryptionKey;
                else throw argErr('Encryption key must be a 16-byte ByteArray.', 3143);
                if (b.length !== 16) throw argErr('Encryption key must be exactly 16 bytes.', 3143);
                return b;
              })();
        const refPath = resolveSqlReferencePath(reference);
        const id = callSqlKernel(
          'open',
          'sql_open_encrypted',
          refPath ?? undefined,
          String(openMode ?? 'create'),
          Boolean(autoCompact),
          Number(pageSize) >>> 0 || 1024,
          keyBytes,
          Date.now()
        );
        s.connId = id;
        s.isAsync = false;
        callSqlKernel('open', 'sql_set_config', id, s.cacheSize, s.columnNameStyle);
        this.dispatchEvent(new SQLEvent(SQLEvent.OPEN));
      }

      openAsync(
        reference = null,
        openMode = 'create',
        responder = null,
        autoCompact = false,
        pageSize = 1024,
        encryptionKey = null
      ) {
        const s = connState(this);
        if (s.connId !== null) throw illegalOp('SQLConnection is already open.', 3101);
        const keyBytes =
          encryptionKey == null
            ? new Uint8Array(0)
            : (() => {
                let b;
                if (binary?.ByteArray?.__nfIs?.(encryptionKey)) b = binary.toBytes(encryptionKey);
                else if (encryptionKey instanceof Uint8Array) b = encryptionKey;
                else throw argErr('Encryption key must be a 16-byte ByteArray.', 3143);
                if (b.length !== 16) throw argErr('Encryption key must be exactly 16 bytes.', 3143);
                return b;
              })();
        s.isAsync = true;
        runSqlConnOp(this, s, true, 'open', SQLEvent.OPEN, responder, () => {
          const refPath = resolveSqlReferencePath(reference);
          const id = callSqlKernel(
            'open',
            'sql_open_encrypted',
            refPath ?? undefined,
            String(openMode ?? 'create'),
            Boolean(autoCompact),
            Number(pageSize) >>> 0 || 1024,
            keyBytes,
            Date.now()
          );
          s.connId = id;
          callSqlKernel('open', 'sql_set_config', id, s.cacheSize, s.columnNameStyle);
        });
      }

      close(responder = null) {
        const s = connState(this);
        if (s.connId === null) return;
        const id = s.connId;
        runSqlConnOp(this, s, s.isAsync, 'close', SQLEvent.CLOSE, responder, () => {
          s.connId = null;
          s.lastSchemaResult = null;
          callSqlKernel('close', 'sql_close', id);
        });
      }

      attach(name, reference = null, responder = null, encryptionKey = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        const keyBytes =
          encryptionKey == null
            ? new Uint8Array(0)
            : (() => {
                let b;
                if (binary?.ByteArray?.__nfIs?.(encryptionKey)) b = binary.toBytes(encryptionKey);
                else if (encryptionKey instanceof Uint8Array) b = encryptionKey;
                else throw argErr('Encryption key must be a 16-byte ByteArray.', 3143);
                if (b.length !== 16) throw argErr('Encryption key must be exactly 16 bytes.', 3143);
                return b;
              })();
        runSqlConnOp(this, s, s.isAsync, 'attach', SQLEvent.ATTACH, responder, () => {
          const refPath = resolveSqlReferencePath(reference);
          callSqlKernel(
            'attach',
            'sql_attach_encrypted',
            s.connId,
            String(name ?? ''),
            refPath ?? undefined,
            keyBytes,
            Date.now()
          );
        });
      }

      detach(name, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'detach', SQLEvent.DETACH, responder, () => {
          callSqlKernel('detach', 'sql_detach', s.connId, String(name ?? ''));
        });
      }

      begin(option = null, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'begin', SQLEvent.BEGIN, responder, () => {
          callSqlKernel('begin', 'sql_begin', s.connId, option == null ? undefined : String(option));
        });
      }

      commit(responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'commit', SQLEvent.COMMIT, responder, () => {
          callSqlKernel('commit', 'sql_commit', s.connId, Date.now());
        });
      }

      rollback(responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'rollback', SQLEvent.ROLLBACK, responder, () => {
          callSqlKernel('rollback', 'sql_rollback', s.connId);
        });
      }

      setSavepoint(name = null, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'setSavepoint', SQLEvent.SET_SAVEPOINT, responder, () => {
          callSqlKernel('setSavepoint', 'sql_set_savepoint', s.connId, name == null ? undefined : String(name));
        });
      }

      releaseSavepoint(name = null, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'releaseSavepoint', SQLEvent.RELEASE_SAVEPOINT, responder, () => {
          callSqlKernel(
            'releaseSavepoint',
            'sql_release_savepoint',
            s.connId,
            name == null ? undefined : String(name),
            Date.now()
          );
        });
      }

      rollbackToSavepoint(name = null, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'rollbackToSavepoint', SQLEvent.ROLLBACK_TO_SAVEPOINT, responder, () => {
          callSqlKernel(
            'rollbackToSavepoint',
            'sql_rollback_to_savepoint',
            s.connId,
            name == null ? undefined : String(name)
          );
        });
      }

      compact(responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'compact', SQLEvent.COMPACT, responder, () => {
          callSqlKernel('compact', 'sql_compact', s.connId, Date.now());
        });
      }

      analyze(resourceName = null, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'analyze', SQLEvent.ANALYZE, responder, () => {
          callSqlKernel('analyze', 'sql_analyze', s.connId, resourceName == null ? undefined : String(resourceName));
        });
      }

      deanalyze(responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'deanalyze', SQLEvent.DEANALYZE, responder, () => {
          callSqlKernel('deanalyze', 'sql_deanalyze', s.connId);
        });
      }

      reencrypt(newEncryptionKey, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        if (this.inTransaction) {
          throw illegalOp('Cannot reencrypt database while a transaction is active.', 3115);
        }
        let keyBytes;
        if (binary?.ByteArray?.__nfIs?.(newEncryptionKey)) {
          keyBytes = binary.toBytes(newEncryptionKey);
        } else if (newEncryptionKey instanceof Uint8Array) {
          keyBytes = newEncryptionKey;
        } else {
          throw argErr('newEncryptionKey must be a 16-byte ByteArray.', 3143);
        }
        if (keyBytes.length !== 16) {
          throw argErr('newEncryptionKey must be exactly 16 bytes.', 3143);
        }
        runSqlConnOp(this, s, s.isAsync, 'reencrypt', SQLEvent.REENCRYPT, responder, () => {
          callSqlKernel('reencrypt', 'sql_reencrypt', s.connId, keyBytes, Date.now());
        });
      }

      cancel(responder = null) {
        const s = connState(this);
        s.cancelToken++;
        notifyResponderResult(responder, new SQLEvent(SQLEvent.CANCEL));
        this.dispatchEvent(new SQLEvent(SQLEvent.CANCEL));
      }

      loadSchema(
        type = null,
        name = null,
        database = 'main',
        includeColumnSchema = true,
        responder = null
      ) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        let typeFilter = undefined;
        if (type != null) {
          if (type === SQLTableSchema) typeFilter = 'table';
          else if (type === SQLViewSchema) typeFilter = 'view';
          else if (type === SQLIndexSchema) typeFilter = 'index';
          else if (type === SQLTriggerSchema) typeFilter = 'trigger';
          else if (typeof type === 'string') typeFilter = type;
        }
        runSqlConnOp(
          this,
          s,
          s.isAsync,
          'schema',
          SQLEvent.SCHEMA,
          responder,
          () => {
            const raw = JSON.parse(
              callSqlKernel(
                'schema',
                'sql_load_schema_json',
                s.connId,
                typeFilter,
                name == null ? undefined : String(name),
                String(database ?? 'main'),
                Boolean(includeColumnSchema)
              )
            );
            const tables = (raw.tables || []).map(
              t =>
                new SQLTableSchema(
                  t.database,
                  t.name,
                  t.sql,
                  (t.columns || []).map(
                    c =>
                      new SQLColumnSchema(
                        c.name,
                        c.primaryKey,
                        c.allowNull,
                        c.autoIncrement,
                        c.dataType,
                        c.defaultCollationType
                      )
                  )
                )
            );
            const views = (raw.views || []).map(v => new SQLViewSchema(v.database, v.name, v.sql));
            const indices = (raw.indices || []).map(
              i => new SQLIndexSchema(i.database, i.name, i.sql, i.table)
            );
            const triggers = (raw.triggers || []).map(
              tr => new SQLTriggerSchema(tr.database, tr.name, tr.sql, tr.table)
            );
            s.lastSchemaResult = new SQLSchemaResult(tables, views, indices, triggers);
            return s.lastSchemaResult;
          },
          schemaRes => schemaRes
        );
      }

      getSchemaResult() {
        return connState(this).lastSchemaResult;
      }
    }
    tagType(SQLConnection, 'SQLConnection', v => sqlConnStates.has(v));

    function encodeSqlParamValue(v) {
      if (v === null || v === undefined) return { k: 'Null' };
      if (typeof v === 'boolean') return { k: 'Bool', v };
      if (typeof v === 'number') {
        if (Number.isNaN(v) || !Number.isFinite(v)) return { k: 'Real', v: 0 };
        return Number.isInteger(v) && Number.isSafeInteger(v) ? { k: 'Int', v } : { k: 'Real', v };
      }
      if (typeof v === 'string') return { k: 'Text', v };
      if (v instanceof Date) return { k: 'Real', v: v.getTime() };
      if (binary?.ByteArray?.__nfIs?.(v)) {
        return { k: 'Blob', v: bytesToBase64(binary.toBytes(v)) };
      }
      if (v instanceof Uint8Array) {
        return { k: 'Blob', v: bytesToBase64(v) };
      }
      return { k: 'Text', v: String(v) };
    }

    function decodeSqlValue(cell) {
      if (!cell || typeof cell !== 'object') return null;
      switch (cell.k) {
        case 'Null':
          return null;
        case 'Int':
        case 'Real':
          return Number(cell.v);
        case 'Bool':
          return Boolean(cell.v);
        case 'Text':
          return String(cell.v);
        case 'Blob': {
          const bytes = base64ToBytes(cell.v);
          return binary ? binary.fromBytes(bytes) : bytes;
        }
        default:
          return null;
      }
    }

    function buildSqlRow(columns, cells, itemClass) {
      const obj = typeof itemClass === 'function' ? new itemClass() : {};
      for (let i = 0; i < columns.length; i++) {
        obj[columns[i]] = decodeSqlValue(cells[i]);
      }
      return obj;
    }

    const sqlStmtStates = new WeakMap();
    function stmtState(receiver) {
      const s = sqlStmtStates.get(receiver);
      if (!s) throw new TypeError('Expected a SQLStatement instance.');
      return s;
    }

    class SQLStatement extends BaseDispatcher {
      constructor() {
        ensureActive();
        super();
        sqlStmtStates.set(this, {
          sqlConnection: null,
          text: '',
          parameters: {},
          itemClass: null,
          executing: false,
          lastResult: null,
          pendingRows: null,
          pendingCursor: 0,
          pendingMeta: null,
          cancelToken: 0,
        });
      }
      get sqlConnection() { return stmtState(this).sqlConnection; }
      set sqlConnection(v) {
        const s = stmtState(this);
        if (s.executing) throw illegalOp('Cannot change sqlConnection while SQLStatement is executing.', 3110);
        if (v !== null && !sqlConnStates.has(v)) {
          throw new TypeError('SQLStatement.sqlConnection requires a SQLConnection or null.');
        }
        s.sqlConnection = v;
      }
      get text() { return stmtState(this).text; }
      set text(v) {
        const s = stmtState(this);
        if (s.executing) throw illegalOp('Cannot change text while SQLStatement is executing.', 3110);
        s.text = String(v ?? '');
      }
      get parameters() { return stmtState(this).parameters; }
      get itemClass() { return stmtState(this).itemClass; }
      set itemClass(v) {
        const s = stmtState(this);
        if (s.executing) throw illegalOp('Cannot change itemClass while SQLStatement is executing.', 3110);
        if (v !== null && typeof v !== 'function') {
          throw new TypeError('SQLStatement.itemClass requires a Class or null.');
        }
        s.itemClass = v;
      }
      get executing() { return stmtState(this).executing; }

      clearParameters() {
        const s = stmtState(this);
        for (const k of Object.keys(s.parameters)) delete s.parameters[k];
      }

      cancel() {
        const s = stmtState(this);
        s.cancelToken++;
        s.executing = false;
        s.pendingRows = null;
        s.pendingCursor = 0;
        s.pendingMeta = null;
      }

      getResult() {
        const s = stmtState(this);
        const r = s.lastResult;
        s.lastResult = null;
        return r;
      }

      _consumeChunk(s, prefetch) {
        const all = s.pendingRows || [];
        const meta = s.pendingMeta || { rowsAffected: 0, lastInsertRowID: 0 };
        if (all.length === 0) {
          s.pendingRows = null;
          s.executing = false;
          s.lastResult = new SQLResult(null, meta.rowsAffected, true, meta.lastInsertRowID);
          return s.lastResult;
        }
        const limit = prefetch > 0 ? prefetch : all.length - s.pendingCursor;
        const slice = all.slice(s.pendingCursor, s.pendingCursor + limit);
        s.pendingCursor += slice.length;
        const complete = s.pendingCursor >= all.length;
        if (complete) {
          s.pendingRows = null;
          s.executing = false;
        }
        s.lastResult = new SQLResult(
          slice.length > 0 ? slice : null,
          meta.rowsAffected,
          complete,
          meta.lastInsertRowID
        );
        return s.lastResult;
      }

      execute(prefetch = -1, responder = null) {
        const s = stmtState(this);
        if (s.executing) throw illegalOp('SQLStatement is already executing.', 3110);
        if (!s.sqlConnection) throw illegalOp('SQLStatement.sqlConnection must be set before execute().', 3109);
        const cs = connState(s.sqlConnection);
        if (cs.connId === null) throw illegalOp('SQLStatement.sqlConnection is not open.', 3104);
        if (!s.text || !s.text.trim()) throw argErr('SQLStatement.text must not be empty.');

        const encodedParams = {};
        for (const [k, v] of Object.entries(s.parameters)) {
          encodedParams[String(k)] = encodeSqlParamValue(v);
        }
        const paramsJson = JSON.stringify(encodedParams);
        const pf = Number(prefetch) | 0;

        const runCore = () => {
          const raw = JSON.parse(
            callSqlKernel('execute', 'sql_execute_json', cs.connId, s.text, paramsJson, Date.now())
          );
          const cols = raw.columns || [];
          const builtRows =
            cols.length > 0
              ? (raw.rows || []).map(cells => buildSqlRow(cols, cells, s.itemClass))
              : [];
          for (const ev of raw.updateEvents || []) {
            s.sqlConnection.dispatchEvent(
              new SQLUpdateEvent(ev.action, false, false, ev.table, ev.row_id)
            );
          }
          s.pendingRows = cols.length > 0 ? builtRows : [];
          s.pendingCursor = 0;
          s.pendingMeta = {
            rowsAffected: raw.rowsAffected || 0,
            lastInsertRowID: raw.lastInsertRowID || 0,
            hasColumns: cols.length > 0,
          };
          if (cols.length > 0 && builtRows.length === 0) {
            s.pendingRows = null;
            s.executing = false;
            s.lastResult = new SQLResult(null, raw.rowsAffected || 0, true, raw.lastInsertRowID || 0);
            return s.lastResult;
          }
          return this._consumeChunk(s, pf);
        };

        if (cs.isAsync) {
          s.executing = true;
          const token = ++s.cancelToken;
          scheduleTask(() => {
            if (s.cancelToken !== token) return;
            try {
              const res = runCore();
              if (!notifyResponderResult(responder, res)) {
                this.dispatchEvent(new SQLEvent(SQLEvent.RESULT));
              }
            } catch (err) {
              s.executing = false;
              const sqlErr = err instanceof SQLError ? err : parseSqlKernelError(err, 'execute');
              if (!notifyResponderStatus(responder, sqlErr)) {
                this.dispatchEvent(new SQLErrorEvent(SQLErrorEvent.ERROR, false, false, sqlErr));
              }
            }
          });
          return;
        }

        s.executing = true;
        try {
          const res = runCore();
          notifyResponderResult(responder, res);
          this.dispatchEvent(new SQLEvent(SQLEvent.RESULT));
        } catch (err) {
          s.executing = false;
          const sqlErr = err instanceof SQLError ? err : parseSqlKernelError(err, 'execute');
          if (notifyResponderStatus(responder, sqlErr)) return;
          throw sqlErr;
        }
      }

      next(prefetch = -1, responder = null) {
        const s = stmtState(this);
        if (!s.executing || !s.pendingRows) {
          throw illegalOp('SQLStatement has no pending prefetched rows.', 3106);
        }
        const cs = s.sqlConnection ? connState(s.sqlConnection) : { isAsync: false };
        const pf = Number(prefetch) | 0;
        if (cs.isAsync) {
          const token = ++s.cancelToken;
          scheduleTask(() => {
            if (s.cancelToken !== token) return;
            const res = this._consumeChunk(s, pf);
            if (!notifyResponderResult(responder, res)) {
              this.dispatchEvent(new SQLEvent(SQLEvent.RESULT));
            }
          });
          return;
        }
        const res = this._consumeChunk(s, pf);
        notifyResponderResult(responder, res);
        this.dispatchEvent(new SQLEvent(SQLEvent.RESULT));
      }
    }
    tagType(SQLStatement, 'SQLStatement', v => sqlStmtStates.has(v));

    // -------------------------------------------------------------------------
    // 7. Companion AIR Packages (permissions, net.dns, notifications, sensors)
    // -------------------------------------------------------------------------
    const PermissionStatus = defConstClass('PermissionStatus', {
      DENIED: 'denied',
      GRANTED: 'granted',
      ONLY_WHEN_IN_USE: 'onlyWhenInUse',
      UNKNOWN: 'unknown',
    });

    class ResourceRecord {
      constructor() {
        this.name = '';
        this.ttl = 0;
      }
    }
    tagType(ResourceRecord, 'ResourceRecord', v => v instanceof ResourceRecord);

    class ARecord extends ResourceRecord {
      constructor() {
        super();
        this.address = '';
      }
    }
    tagType(ARecord, 'ARecord', v => v instanceof ARecord);

    class AAAARecord extends ResourceRecord {
      constructor() {
        super();
        this.address = '';
      }
    }
    tagType(AAAARecord, 'AAAARecord', v => v instanceof AAAARecord);

    class MXRecord extends ResourceRecord {
      constructor() {
        super();
        this.exchange = '';
        this.preference = 0;
      }
    }
    tagType(MXRecord, 'MXRecord', v => v instanceof MXRecord);

    class PTRRecord extends ResourceRecord {
      constructor() {
        super();
        this.ptrdName = '';
      }
    }
    tagType(PTRRecord, 'PTRRecord', v => v instanceof PTRRecord);

    class SRVRecord extends ResourceRecord {
      constructor() {
        super();
        this.port = 0;
        this.priority = 0;
        this.target = '';
        this.weight = 0;
      }
    }
    tagType(SRVRecord, 'SRVRecord', v => v instanceof SRVRecord);

    class DNSResolver extends BaseDispatcher {
      constructor() {
        super();
      }
      static get isSupported() { return false; }
      lookup(_host, _recordType = null) {
        throw illegalOp('DNSResolver is not supported in the HTML5 browser sandbox.', 3219);
      }
    }
    tagType(DNSResolver, 'DNSResolver', v => v instanceof DNSResolver);

    const NotificationStyle = defConstClass('NotificationStyle', {
      ALERT: 'alert',
      BADGE: 'badge',
      SOUND: 'sound',
    });

    const subscribeOptionStates = new WeakMap();
    class RemoteNotifierSubscribeOptions {
      constructor() {
        subscribeOptionStates.set(this, {
          notificationStyles: makeVector('String', []),
        });
      }
      get notificationStyles() { return subscribeOptionStates.get(this).notificationStyles; }
      set notificationStyles(v) {
        if (!v || typeof v.length !== 'number') {
          throw new TypeError('RemoteNotifierSubscribeOptions.notificationStyles requires a Vector.<String>.');
        }
        const arr = [];
        for (let i = 0; i < v.length; i++) arr.push(String(v[i]));
        subscribeOptionStates.get(this).notificationStyles = makeVector('String', arr);
      }
    }
    tagType(
      RemoteNotifierSubscribeOptions,
      'RemoteNotifierSubscribeOptions',
      v => subscribeOptionStates.has(v)
    );

    class RemoteNotifier extends BaseDispatcher {
      constructor() {
        super();
      }
      static get supportedNotificationStyles() {
        return makeVector('String', []);
      }
      subscribe(_options = null) {
        scheduleTask(() => {
          const StatusEv = options.eventFamilies?.StatusEvent;
          if (StatusEv) {
            this.dispatchEvent(new StatusEv('status', false, false, 'Push notifications unavailable in sandbox', 'error'));
          }
        });
      }
      unsubscribe() {
        ensureActive();
      }
    }
    tagType(RemoteNotifier, 'RemoteNotifier', v => v instanceof RemoteNotifier);

    const deviceRotationStates = new WeakMap();
    class DeviceRotation extends BaseDispatcher {
      constructor() {
        super();
        const st = { interval: 0, lastDispatched: 0 };
        deviceRotationStates.set(this, st);
        if (typeof global.addEventListener === 'function') {
          const onOrient = ev => {
            if (disposed) return;
            const now = Date.now();
            if (st.interval > 0 && now - st.lastDispatched < st.interval) return;
            this._dispatchOrientation(ev?.alpha, ev?.beta, ev?.gamma, now);
          };
          global.addEventListener('deviceorientation', onOrient);
          hostCleanups.push(() => global.removeEventListener?.('deviceorientation', onOrient));
        }
      }
      _dispatchOrientation(alpha = 0, beta = 0, gamma = 0, timestamp = Date.now()) {
        const st = deviceRotationStates.get(this);
        if (!st) throw new TypeError('Expected a DeviceRotation instance.');
        st.lastDispatched = Number(timestamp) || Date.now();
        const q = callKernel(
          'device_rotation_quaternion',
          Number(alpha) || 0,
          Number(beta) || 0,
          Number(gamma) || 0
        );
        const quat = [Number(q[0]) || 0, Number(q[1]) || 0, Number(q[2]) || 0, Number(q[3]) || 1];
        const roll = Number(q[4]) || 0;
        const pitch = Number(q[5]) || 0;
        const yaw = Number(q[6]) || 0;
        this.dispatchEvent(
          new DeviceRotationEvent(
            DeviceRotationEvent.UPDATE,
            false,
            false,
            st.lastDispatched,
            roll,
            pitch,
            yaw,
            quat
          )
        );
      }
      static get isSupported() {
        return Boolean(typeof global.DeviceOrientationEvent === 'function');
      }
      get muted() {
        return !DeviceRotation.isSupported;
      }
      setRequestedUpdateInterval(interval) {
        const ms = Number(interval);
        if (ms < 0 || Number.isNaN(ms)) {
          throw argErr('DeviceRotation.setRequestedUpdateInterval requires a non-negative interval.');
        }
        const st = deviceRotationStates.get(this);
        if (st) st.interval = ms;
      }
    }
    tagType(DeviceRotation, 'DeviceRotation', v => v instanceof DeviceRotation);

    const geoStates = new WeakMap();
    class Geolocation extends BaseDispatcher {
      constructor() {
        super();
        const st = {
          desiredAccuracy: 0,
          locationAlwaysUsePermission: false,
          pausesLocationUpdatesAutomatically: false,
          interval: 0,
          lastDispatched: 0,
          watchId: null,
        };
        geoStates.set(this, st);
        if (typeof global.navigator?.geolocation?.watchPosition === 'function') {
          try {
            st.watchId = global.navigator.geolocation.watchPosition(pos => {
              if (disposed) return;
              const now = Number(pos?.timestamp) || Date.now();
              if (st.interval > 0 && now - st.lastDispatched < st.interval) return;
              this._dispatchPosition(pos?.coords || {}, now);
            });
            hostCleanups.push(() => {
              if (st.watchId !== null) {
                try {
                  global.navigator?.geolocation?.clearWatch?.(st.watchId);
                } catch (_) {}
              }
            });
          } catch (_) {}
        }
      }
      _dispatchPosition(coords = {}, timestamp = Date.now()) {
        const st = geoStates.get(this);
        if (!st) throw new TypeError('Expected a Geolocation instance.');
        st.lastDispatched = Number(timestamp) || Date.now();
        const GeoEv = options.eventFamilies?.GeolocationEvent;
        if (GeoEv) {
          this.dispatchEvent(
            new GeoEv(
              'update',
              false,
              false,
              Number(coords.latitude) || 0,
              Number(coords.longitude) || 0,
              Number(coords.altitude) || 0,
              Number(coords.accuracy) || 0,
              Number(coords.altitudeAccuracy) || 0,
              Number(coords.speed) || 0,
              Number(coords.heading) || 0,
              st.lastDispatched
            )
          );
        }
      }
      static get isSupported() {
        return Boolean(global.navigator?.geolocation);
      }
      static get permissionStatus() {
        return global.navigator?.geolocation ? 'unknown' : 'denied';
      }
      get muted() {
        return !global.navigator?.geolocation;
      }
      get desiredAccuracy() { return geoStates.get(this).desiredAccuracy; }
      set desiredAccuracy(v) { geoStates.get(this).desiredAccuracy = Number(v) || 0; }
      get locationAlwaysUsePermission() { return geoStates.get(this).locationAlwaysUsePermission; }
      set locationAlwaysUsePermission(v) { geoStates.get(this).locationAlwaysUsePermission = Boolean(v); }
      get pausesLocationUpdatesAutomatically() {
        return geoStates.get(this).pausesLocationUpdatesAutomatically;
      }
      set pausesLocationUpdatesAutomatically(v) {
        geoStates.get(this).pausesLocationUpdatesAutomatically = Boolean(v);
      }
      setRequestedUpdateInterval(interval) {
        const ms = Number(interval);
        if (ms < 0 || Number.isNaN(ms)) {
          throw argErr('Geolocation.setRequestedUpdateInterval requires a non-negative interval.');
        }
        geoStates.get(this).interval = ms;
      }
      requestPermission() {
        scheduleTask(() => {
          const status = global.navigator?.geolocation ? 'granted' : 'denied';
          this.dispatchEvent(new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, status));
        });
      }
    }
    Object.assign(Geolocation, {
      LOCATION_ACCURACY_BEST: 0,
      LOCATION_ACCURACY_BEST_FOR_NAVIGATION: -1,
      LOCATION_ACCURACY_NEAREST_TEN_METERS: 10,
      LOCATION_ACCURACY_HUNDRED_METERS: 100,
      LOCATION_ACCURACY_KILOMETER: 1000,
      LOCATION_ACCURACY_THREE_KILOMETERS: 3000,
    });
    tagType(Geolocation, 'Geolocation', v => geoStates.has(v));

    // Shared helper to write RGBA bytes from Rust rasterizer into a BitmapData instance.
    function writeRgbaToBitmapData(bitmapData, rgbaBytes, width, height) {
      if (!bitmapData || typeof bitmapData.setPixel32 !== 'function') {
        throw argErr('Expected a valid BitmapData instance.');
      }
      const w = Math.min(width | 0, Number(bitmapData.width) | 0);
      const h = Math.min(height | 0, Number(bitmapData.height) | 0);
      for (let y = 0; y < h; y++) {
        for (let x = 0; x < w; x++) {
          const idx = (y * width + x) * 4;
          const r = rgbaBytes[idx] & 0xff;
          const g = rgbaBytes[idx + 1] & 0xff;
          const b = rgbaBytes[idx + 2] & 0xff;
          const a = rgbaBytes[idx + 3] & 0xff;
          const argb = (((a << 24) | (r << 16) | (g << 8) | b) >>> 0);
          bitmapData.setPixel32(x, y, argb);
        }
      }
    }

    // -------------------------------------------------------------------------
    // 8. flash.html.* (HTMLPDFCapability, HTMLSWFCapability, HTMLHistoryItem,
    //                  HTMLWindowCreateOptions, HTMLHost, HTMLLoader)
    // -------------------------------------------------------------------------
    const HTMLPDFCapability = defConstClass('HTMLPDFCapability', {
      STATUS_OK: 0,
      ERROR_INSTALLED_READER_NOT_FOUND: 3201,
      ERROR_INSTALLED_READER_TOO_OLD: 3202,
      ERROR_PREFERRED_READER_TOO_OLD: 3203,
      ERROR_CANNOT_LOAD_READER: 3204,
    });

    const HTMLSWFCapability = defConstClass('HTMLSWFCapability', {
      STATUS_OK: 0,
      ERROR_INSTALLED_PLAYER_NOT_FOUND: 3201,
      ERROR_INSTALLED_PLAYER_TOO_OLD: 3202,
      ERROR_RUNTIME_NOT_FOUND: 3221,
    });

    class HTMLHistoryItem {
      constructor(url = '', originalUrl = '', isPost = false, title = '') {
        Object.defineProperties(this, {
          url: { value: String(url ?? ''), enumerable: true },
          originalUrl: { value: String(originalUrl || url || ''), enumerable: true },
          isPost: { value: Boolean(isPost), enumerable: true },
          title: { value: String(title ?? ''), enumerable: true },
        });
      }
    }
    tagType(HTMLHistoryItem, 'HTMLHistoryItem', v => v instanceof HTMLHistoryItem);

    class HTMLWindowCreateOptions {
      constructor() {
        this.x = Number.NaN;
        this.y = Number.NaN;
        this.width = Number.NaN;
        this.height = Number.NaN;
        this.fullscreen = false;
        this.locationBarVisible = false;
        this.menuBarVisible = false;
        this.resizable = true;
        this.scrollBarsVisible = true;
        this.statusBarVisible = false;
        this.toolBarVisible = false;
      }
    }
    tagType(HTMLWindowCreateOptions, 'HTMLWindowCreateOptions', v => v instanceof HTMLWindowCreateOptions);

    const htmlHostStates = new WeakMap();
    function initHtmlHostState(receiver, defaultBehaviors = true) {
      let s = htmlHostStates.get(receiver);
      if (!s) {
        s = {
          defaultBehaviors: Boolean(defaultBehaviors),
          htmlLoader: null,
          windowRect: makeRect(0, 0, projW, projH),
          status: '',
        };
        htmlHostStates.set(receiver, s);
      }
      return s;
    }

    class HTMLHost {
      constructor(defaultBehaviors = true) {
        initHtmlHostState(this, defaultBehaviors);
      }
      get htmlLoader() {
        return initHtmlHostState(this).htmlLoader;
      }
      get windowRect() {
        const s = initHtmlHostState(this);
        const ldr = s.htmlLoader;
        const st = ldr ? htmlLoaderStates.get(ldr) : null;
        if (st?.nativeWindow && windowStates.has(st.nativeWindow)) {
          return st.nativeWindow.bounds;
        }
        const r = s.windowRect;
        return makeRect(r.x, r.y, r.width, r.height);
      }
      set windowRect(value) {
        if (!value || typeof value !== 'object') {
          throw argErr('HTMLHost.windowRect requires a Rectangle.');
        }
        const s = initHtmlHostState(this);
        s.windowRect = makeRect(
          Number(value.x) || 0,
          Number(value.y) || 0,
          Number(value.width) || 0,
          Number(value.height) || 0
        );
        const ldr = s.htmlLoader;
        const st = ldr ? htmlLoaderStates.get(ldr) : null;
        if (st?.nativeWindow && windowStates.has(st.nativeWindow)) {
          st.nativeWindow.bounds = s.windowRect;
        }
      }
      createWindow(windowCreateOptions) {
        const s = initHtmlHostState(this);
        if (!s.defaultBehaviors) return null;
        const opts =
          windowCreateOptions instanceof HTMLWindowCreateOptions
            ? windowCreateOptions
            : new HTMLWindowCreateOptions();
        const bounds =
          Number.isFinite(opts.width) && Number.isFinite(opts.height)
            ? makeRect(
                Number.isFinite(opts.x) ? opts.x : 0,
                Number.isFinite(opts.y) ? opts.y : 0,
                opts.width,
                opts.height
              )
            : null;
        return HTMLLoader.createRootWindow(true, null, Boolean(opts.scrollBarsVisible), bounds);
      }
      updateLocation(_locationURL) {}
      updateStatus(status) {
        initHtmlHostState(this).status = String(status ?? '');
      }
      updateTitle(title) {
        const s = initHtmlHostState(this);
        if (!s.defaultBehaviors) return;
        const ldr = s.htmlLoader;
        const st = ldr ? htmlLoaderStates.get(ldr) : null;
        if (st?.nativeWindow && windowStates.has(st.nativeWindow)) {
          st.nativeWindow.title = String(title ?? '');
        }
      }
      windowBlur() {}
      windowClose() {
        const s = initHtmlHostState(this);
        if (!s.defaultBehaviors) return;
        const ldr = s.htmlLoader;
        const st = ldr ? htmlLoaderStates.get(ldr) : null;
        if (st?.nativeWindow && windowStates.has(st.nativeWindow) && !st.nativeWindow.closed) {
          st.nativeWindow.close();
        }
      }
      windowFocus() {
        const s = initHtmlHostState(this);
        if (!s.defaultBehaviors) return;
        const ldr = s.htmlLoader;
        const st = ldr ? htmlLoaderStates.get(ldr) : null;
        if (st?.nativeWindow && windowStates.has(st.nativeWindow) && !st.nativeWindow.closed) {
          st.nativeWindow.activate();
        }
      }
    }
    tagType(
      HTMLHost,
      'HTMLHost',
      v => htmlHostStates.has(v) || v instanceof HTMLHost,
      (receiver, args = []) => {
        initHtmlHostState(receiver, args.length > 0 ? Boolean(args[0]) : true);
      }
    );

    const htmlLoaderStates = new WeakMap();
    function htmlState(receiver) {
      const s = htmlLoaderStates.get(receiver);
      if (!s) throw new TypeError('Expected an HTMLLoader instance.');
      return s;
    }

    function syncHtmlKernelState(receiver, s) {
      const snap = JSON.parse(callKernel('html_state_json', s.sessionId));
      s.loaded = Boolean(snap.loaded);
      s.location = String(snap.location || '');
      s.title = String(snap.title || '');
      s.contentWidth = Number(snap.contentWidth) || s.width;
      s.contentHeight = Number(snap.contentHeight) || s.height;
      s.hasFocusableContent = Boolean(snap.hasFocusableContent);
      s.historyLength = Number(snap.historyLength) || 0;
      s.historyPosition = Number(snap.historyPosition) || 0;
      s.history = (snap.history || []).map(
        h => new HTMLHistoryItem(h.url, h.originalUrl, h.isPost, h.title)
      );
      s.links = Array.isArray(snap.links) ? snap.links : [];
      s.scripts = Array.isArray(snap.scripts) ? snap.scripts : [];
      s.elementIds = Array.isArray(snap.elementIds) ? snap.elementIds : [];
      s.textContent = String(snap.textContent || '');
      if (s.windowObj) {
        s.windowObj.location.href = s.location;
        s.windowObj.document.title = s.title;
        s.windowObj.document.URL = s.location;
        s.windowObj.document.body.innerHTML = s.htmlSource || '';
        s.windowObj.document.body.textContent = s.textContent;
      }
      if (s.htmlHost) {
        if (typeof s.htmlHost.updateLocation === 'function') {
          s.htmlHost.updateLocation(s.location);
        }
        if (typeof s.htmlHost.updateTitle === 'function') {
          s.htmlHost.updateTitle(s.title);
        }
      }
      receiver.dispatchEvent(new LocationChangeEvent(LocationChangeEvent.LOCATION_CHANGE, false, false, s.location));
      receiver.dispatchEvent(new BaseEvent('htmlDOMInitialize', false, false));
      receiver.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE, false, false));
      receiver.dispatchEvent(new BaseEvent('htmlRender', false, false));
      receiver.dispatchEvent(new BaseEvent('htmlBoundsChange', false, false));
    }

    function createHtmlWindowBridge(receiver, s) {
      const elementsById = new Map();
      const doc = {
        title: '',
        URL: 'about:blank',
        body: { innerHTML: '', textContent: '' },
        getElementById(id) {
          const key = String(id ?? '');
          if (elementsById.has(key)) return elementsById.get(key);
          if (s.elementIds && s.elementIds.includes(key)) {
            const el = { id: key, innerHTML: '', textContent: '' };
            elementsById.set(key, el);
            return el;
          }
          return null;
        },
        createElement(tag) {
          return { tagName: String(tag ?? 'div').toUpperCase(), id: '', innerHTML: '', textContent: '' };
        },
      };
      const win = {
        document: doc,
        location: { href: 'about:blank' },
        navigator: { userAgent: s.userAgent },
        htmlLoader: receiver,
        open(url = 'about:blank') {
          if (s.htmlHost && typeof s.htmlHost.createWindow === 'function') {
            const child = s.htmlHost.createWindow(new HTMLWindowCreateOptions());
            if (child && url) child.load( { url: String(url) } );
            return child ? child.window : null;
          }
          return null;
        },
        close() {
          if (s.htmlHost && typeof s.htmlHost.windowClose === 'function') {
            s.htmlHost.windowClose();
          }
        },
        focus() {
          if (s.htmlHost && typeof s.htmlHost.windowFocus === 'function') {
            s.htmlHost.windowFocus();
          }
        },
        blur() {
          if (s.htmlHost && typeof s.htmlHost.windowBlur === 'function') {
            s.htmlHost.windowBlur();
          }
        },
        setStatus(text) {
          if (s.htmlHost && typeof s.htmlHost.updateStatus === 'function') {
            s.htmlHost.updateStatus(String(text ?? ''));
          }
        },
        __throwScriptError(errValue) {
          const ev = new HTMLUncaughtScriptExceptionEvent(errValue);
          receiver.dispatchEvent(ev);
          return ev;
        },
      };
      win.window = win;
      win.self = win;
      return win;
    }

    const htmlLoaderPropDescriptors = {
      authenticate: {
        get() { return htmlState(this).authenticate; },
        set(v) { htmlState(this).authenticate = Boolean(v); },
        enumerable: true,
        configurable: true,
      },
      cacheResponse: {
        get() { return htmlState(this).cacheResponse; },
        set(v) { htmlState(this).cacheResponse = Boolean(v); },
        enumerable: true,
        configurable: true,
      },
      contentHeight: {
        get() { return htmlState(this).contentHeight; },
        enumerable: true,
        configurable: true,
      },
      contentWidth: {
        get() { return htmlState(this).contentWidth; },
        enumerable: true,
        configurable: true,
      },
      hasFocusableContent: {
        get() { return htmlState(this).hasFocusableContent; },
        enumerable: true,
        configurable: true,
      },
      width: {
        get() { return htmlState(this).width; },
        set(v) {
          const s = htmlState(this);
          const next = Math.max(0, Number(v) || 0);
          const changed = next !== s.width;
          s.width = next;
          callKernel('html_set_viewport', s.sessionId, s.width, s.height);
          const snap = JSON.parse(callKernel('html_state_json', s.sessionId));
          s.contentWidth = Number(snap.contentWidth) || s.width;
          s.contentHeight = Number(snap.contentHeight) || s.height;
          if (changed) this.dispatchEvent(new BaseEvent('htmlBoundsChange', false, false));
        },
        enumerable: true,
        configurable: true,
      },
      height: {
        get() { return htmlState(this).height; },
        set(v) {
          const s = htmlState(this);
          const next = Math.max(0, Number(v) || 0);
          const changed = next !== s.height;
          s.height = next;
          callKernel('html_set_viewport', s.sessionId, s.width, s.height);
          const snap = JSON.parse(callKernel('html_state_json', s.sessionId));
          s.contentWidth = Number(snap.contentWidth) || s.width;
          s.contentHeight = Number(snap.contentHeight) || s.height;
          if (changed) this.dispatchEvent(new BaseEvent('htmlBoundsChange', false, false));
        },
        enumerable: true,
        configurable: true,
      },
      historyLength: {
        get() { return htmlState(this).historyLength; },
        enumerable: true,
        configurable: true,
      },
      historyPosition: {
        get() { return htmlState(this).historyPosition; },
        set(v) {
          const s = htmlState(this);
          const pos = Number(v) | 0;
          if (pos < 0 || pos >= s.historyLength) {
            throw new RangeError(`HTMLLoader.historyPosition ${pos} is out of bounds.`);
          }
          const delta = pos - s.historyPosition;
          if (delta !== 0) this.historyGo(delta);
        },
        enumerable: true,
        configurable: true,
      },
      htmlHost: {
        get() { return htmlState(this).htmlHost; },
        set(v) {
          const s = htmlState(this);
          if (v !== null && !(v instanceof HTMLHost) && !htmlHostStates.has(v)) {
            throw new TypeError('HTMLLoader.htmlHost requires an HTMLHost or null.');
          }
          if (s.htmlHost) initHtmlHostState(s.htmlHost).htmlLoader = null;
          s.htmlHost = v;
          if (v) initHtmlHostState(v).htmlLoader = this;
        },
        enumerable: true,
        configurable: true,
      },
      idleTimeout: {
        get() { return htmlState(this).idleTimeout; },
        set(v) {
          const n = Number(v);
          if (n < 0 || Number.isNaN(n)) throw argErr('HTMLLoader.idleTimeout must be non-negative.');
          htmlState(this).idleTimeout = n;
        },
        enumerable: true,
        configurable: true,
      },
      loaded: {
        get() { return htmlState(this).loaded; },
        enumerable: true,
        configurable: true,
      },
      location: {
        get() { return htmlState(this).location; },
        enumerable: true,
        configurable: true,
      },
      manageCookies: {
        get() { return htmlState(this).manageCookies; },
        set(v) { htmlState(this).manageCookies = Boolean(v); },
        enumerable: true,
        configurable: true,
      },
      navigateInSystemBrowser: {
        get() { return htmlState(this).navigateInSystemBrowser; },
        set(v) { htmlState(this).navigateInSystemBrowser = Boolean(v); },
        enumerable: true,
        configurable: true,
      },
      paintsDefaultBackground: {
        get() { return htmlState(this).paintsDefaultBackground; },
        set(v) { htmlState(this).paintsDefaultBackground = Boolean(v); },
        enumerable: true,
        configurable: true,
      },
      placeLoadStringContentInApplicationSandbox: {
        get() { return htmlState(this).placeLoadStringContentInApplicationSandbox; },
        set(v) { htmlState(this).placeLoadStringContentInApplicationSandbox = Boolean(v); },
        enumerable: true,
        configurable: true,
      },
      runtimeApplicationDomain: {
        get() {
          const s = htmlState(this);
          return s.runtimeApplicationDomain || ApplicationDomain.currentDomain;
        },
        set(v) { htmlState(this).runtimeApplicationDomain = v; },
        enumerable: true,
        configurable: true,
      },
      scrollH: {
        get() { return htmlState(this).scrollH; },
        set(v) {
          const s = htmlState(this);
          const next = Number(v) || 0;
          if (next !== s.scrollH) {
            s.scrollH = next;
            this.dispatchEvent(new BaseEvent('scroll', false, false));
          }
        },
        enumerable: true,
        configurable: true,
      },
      scrollV: {
        get() { return htmlState(this).scrollV; },
        set(v) {
          const s = htmlState(this);
          const next = Number(v) || 0;
          if (next !== s.scrollV) {
            s.scrollV = next;
            this.dispatchEvent(new BaseEvent('scroll', false, false));
          }
        },
        enumerable: true,
        configurable: true,
      },
      textEncodingFallback: {
        get() { return htmlState(this).textEncodingFallback; },
        set(v) { htmlState(this).textEncodingFallback = v == null ? null : String(v); },
        enumerable: true,
        configurable: true,
      },
      textEncodingOverride: {
        get() { return htmlState(this).textEncodingOverride; },
        set(v) { htmlState(this).textEncodingOverride = v == null ? null : String(v); },
        enumerable: true,
        configurable: true,
      },
      useCache: {
        get() { return htmlState(this).useCache; },
        set(v) { htmlState(this).useCache = Boolean(v); },
        enumerable: true,
        configurable: true,
      },
      userAgent: {
        get() { return htmlState(this).userAgent; },
        set(v) {
          const s = htmlState(this);
          s.userAgent = String(v ?? '');
          if (s.windowObj) s.windowObj.navigator.userAgent = s.userAgent;
        },
        enumerable: true,
        configurable: true,
      },
      window: {
        get() { return htmlState(this).windowObj; },
        enumerable: true,
        configurable: true,
      },
      loadString: {
        value(htmlContent) {
          const s = htmlState(this);
          const src = String(htmlContent ?? '');
          s.htmlSource = src;
          callKernel('html_load_string', s.sessionId, src, s.location || 'about:blank');
          syncHtmlKernelState(this, s);
        },
        configurable: true,
      },
      load: {
        value(urlRequestToLoad) {
          const s = htmlState(this);
          const url =
            typeof urlRequestToLoad === 'string'
              ? urlRequestToLoad
              : urlRequestToLoad && typeof urlRequestToLoad.url === 'string'
                ? urlRequestToLoad.url
                : null;
          if (!url) throw argErr('HTMLLoader.load requires a URLRequest with a valid url.');
          const method = String(urlRequestToLoad?.method || 'GET').toUpperCase();
          const isPost = method === 'POST';
          const changing = new LocationChangeEvent(
            LocationChangeEvent.LOCATION_CHANGING,
            false,
            true,
            url
          );
          if (!this.dispatchEvent(changing)) return;
          callKernel('html_load_url', s.sessionId, url, isPost, url);
          syncHtmlKernelState(this, s);
        },
        configurable: true,
      },
      cancelLoad: {
        value() {
          const s = htmlState(this);
          callKernel('html_cancel', s.sessionId);
          const snap = JSON.parse(callKernel('html_state_json', s.sessionId));
          s.loaded = Boolean(snap.loaded);
        },
        configurable: true,
      },
      reload: {
        value() {
          const s = htmlState(this);
          if (callKernel('html_reload', s.sessionId)) {
            syncHtmlKernelState(this, s);
          }
        },
        configurable: true,
      },
      historyBack: {
        value() { this.historyGo(-1); },
        configurable: true,
      },
      historyForward: {
        value() { this.historyGo(1); },
        configurable: true,
      },
      historyGo: {
        value(steps) {
          const s = htmlState(this);
          if (callKernel('html_history_go', s.sessionId, Number(steps) | 0)) {
            syncHtmlKernelState(this, s);
          }
        },
        configurable: true,
      },
      getHistoryAt: {
        value(position) {
          const s = htmlState(this);
          const pos = Number(position) | 0;
          if (pos < 0 || pos >= s.history.length) {
            throw new RangeError(`HTMLLoader.getHistoryAt(${pos}) is out of bounds.`);
          }
          return s.history[pos];
        },
        configurable: true,
      },
    };

    const htmlLoaderOwnMemberNames = new Set(Object.keys(htmlLoaderPropDescriptors));

    function initHtmlLoaderInstance(instance) {
      const sessionId = callKernel('html_create', 800, 600);
      const state = {
        sessionId,
        width: 800,
        height: 600,
        contentWidth: 800,
        contentHeight: 600,
        hasFocusableContent: false,
        loaded: false,
        location: 'about:blank',
        title: '',
        htmlSource: '',
        textContent: '',
        historyLength: 0,
        historyPosition: 0,
        history: [],
        links: [],
        scripts: [],
        elementIds: [],
        authenticate: URLRequestDefaults.authenticate,
        cacheResponse: URLRequestDefaults.cacheResponse,
        idleTimeout: URLRequestDefaults.idleTimeout,
        manageCookies: URLRequestDefaults.manageCookies,
        useCache: URLRequestDefaults.useCache,
        userAgent: URLRequestDefaults.userAgent,
        navigateInSystemBrowser: false,
        paintsDefaultBackground: true,
        placeLoadStringContentInApplicationSandbox: false,
        runtimeApplicationDomain: null,
        scrollH: 0,
        scrollV: 0,
        textEncodingFallback: null,
        textEncodingOverride: null,
        htmlHost: null,
        windowObj: null,
        nativeWindow: null,
      };
      state.windowObj = createHtmlWindowBridge(instance, state);
      htmlLoaderStates.set(instance, state);
      return instance;
    }

    class HTMLLoader extends BaseDispatcher {
      constructor() {
        ensureActive();
        const SpriteCtor = getDisplay()?.builtins?.['flash.display.Sprite'];
        if (typeof SpriteCtor === 'function') {
          super();
          const sprite = new SpriteCtor();
          return initHtmlLoaderInstance(sprite);
        }
        super();
        this.x = 0;
        this.y = 0;
        this.alpha = 1;
        this.visible = true;
        this.name = 'instance1';
        this.parent = null;
        this.stage = null;
        const children = [];
        Object.defineProperty(this, 'numChildren', {
          get: () => children.length,
          enumerable: true,
        });
        this.addChild = child => { if (!children.includes(child)) children.push(child); return child; };
        this.removeChild = child => { const i = children.indexOf(child); if (i >= 0) children.splice(i, 1); return child; };
        return initHtmlLoaderInstance(this);
      }
      static get isSupported() { return true; }
      static get pdfCapability() {
        return global.navigator?.pdfViewerEnabled
          ? HTMLPDFCapability.STATUS_OK
          : HTMLPDFCapability.ERROR_INSTALLED_READER_NOT_FOUND;
      }
      static get swfCapability() {
        return HTMLSWFCapability.STATUS_OK;
      }
      static createRootWindow(
        visible = true,
        windowInitOptions = null,
        scrollBarsVisible = true,
        bounds = null
      ) {
        const win = new NativeWindow(windowInitOptions);
        if (bounds && typeof bounds === 'object') {
          win.bounds = makeRect(
            Number(bounds.x) || 0,
            Number(bounds.y) || 0,
            Number(bounds.width) || 800,
            Number(bounds.height) || 600
          );
        }
        win.visible = Boolean(visible);
        const loader = new HTMLLoader();
        const st = htmlState(loader);
        st.nativeWindow = win;
        st.scrollBarsVisible = Boolean(scrollBarsVisible);
        loader.width = win.width;
        loader.height = win.height;
        loader.htmlHost = new HTMLHost(true);
        return loader;
      }
    }
    Object.defineProperties(HTMLLoader.prototype, htmlLoaderPropDescriptors);
    tagType(HTMLLoader, 'HTMLLoader', v => htmlLoaderStates.has(v));
    Object.defineProperty(HTMLLoader, Symbol.hasInstance, {
      value: v => htmlLoaderStates.has(v),
    });

    // -------------------------------------------------------------------------
    // 9. flash.media.* (AudioPlaybackMode, CameraPosition, MediaType, MediaPromise,
    //                   CameraRollBrowseOptions, CameraRoll, CameraUI, StageWebView,
    //                   AVURLLoader, Camera, Microphone, SoundMixer)
    // -------------------------------------------------------------------------
    const AudioPlaybackMode = defConstClass('AudioPlaybackMode', {
      AMBIENT: 'ambient',
      MEDIA: 'media',
      VOICE: 'voice',
    });

    const CameraPosition = defConstClass('CameraPosition', {
      BACK: 'back',
      FRONT: 'front',
      UNKNOWN: 'unknown',
    });

    const MediaType = defConstClass('MediaType', {
      IMAGE: 'image',
      VIDEO: 'video',
    });

    const mediaPromiseStates = new WeakMap();
    class MediaPromise extends BaseDispatcher {
      constructor(file = null, mediaType = 'image', relativePath = null, isAsync = false) {
        super();
        const f =
          file && fileStates.has(file)
            ? file
            : typeof file === 'string' && file
              ? new File(file)
              : null;
        mediaPromiseStates.set(this, {
          file: f,
          mediaType: String(mediaType || 'image'),
          relativePath: relativePath == null ? (f ? f.name : '') : String(relativePath),
          isAsync: Boolean(isAsync),
          stream: null,
        });
      }
      get file() { return mediaPromiseStates.get(this).file; }
      get isAsync() { return mediaPromiseStates.get(this).isAsync; }
      get mediaType() { return mediaPromiseStates.get(this).mediaType; }
      get relativePath() { return mediaPromiseStates.get(this).relativePath; }
      open() {
        const s = mediaPromiseStates.get(this);
        if (!s.file || !s.file.exists) {
          throw illegalOp('MediaPromise does not reference an existing media file.');
        }
        const fs = new FileStream();
        if (s.isAsync) {
          fs.openAsync(s.file, FileMode.READ);
          scheduleTask(() => {
            if (options.net?.ProgressEvent) {
              this.dispatchEvent(
                new options.net.ProgressEvent('progress', false, false, fs.bytesAvailable, fs.bytesAvailable)
              );
            }
            this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
          });
        } else {
          fs.open(s.file, FileMode.READ);
        }
        s.stream = fs;
        return fs;
      }
      close() {
        const s = mediaPromiseStates.get(this);
        if (s.stream) {
          s.stream.close();
          s.stream = null;
        }
        this.dispatchEvent(new BaseEvent(BaseEvent.CLOSE));
      }
      reportError(error) {
        const msg = String(error?.text ?? error?.message ?? error ?? 'MediaPromise error');
        const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
        this.dispatchEvent(new Ev('ioError', false, false, msg));
      }
    }
    tagType(MediaPromise, 'MediaPromise', v => mediaPromiseStates.has(v));

    class CameraRollBrowseOptions {
      constructor() {
        this.height = 0;
        this.width = 0;
        this.origin = makeRect(0, 0, 0, 0);
      }
    }
    tagType(
      CameraRollBrowseOptions,
      'CameraRollBrowseOptions',
      v => v instanceof CameraRollBrowseOptions
    );

    let cameraRollCounter = 0;
    let lastCameraRollFile = null;
    class CameraRoll extends BaseDispatcher {
      constructor() {
        super();
      }
      static get supportsAddBitmapData() { return true; }
      static get supportsBrowseForImage() { return true; }
      static get permissionStatus() { return 'granted'; }
      requestPermission() {
        scheduleTask(() => {
          this.dispatchEvent(
            new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, 'granted')
          );
        });
      }
      addBitmapData(bitmapData) {
        if (!bitmapData || typeof bitmapData.width !== 'number' || typeof bitmapData.height !== 'number') {
          throw argErr('CameraRoll.addBitmapData requires a BitmapData instance.');
        }
        let pngBytes;
        try {
          if (typeof bitmapData.encode === 'function' && getBitmap()?.PNGEncoderOptions) {
            const ba = bitmapData.encode(bitmapData.rect, new (getBitmap().PNGEncoderOptions)());
            pngBytes = binary.toBytes(ba);
          } else {
            // Minimal valid 1x1 PNG fallback when standalone BitmapData double is passed
            pngBytes = new Uint8Array([
              137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82,
              0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137,
              0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 255, 255, 63, 0,
              5, 254, 2, 254, 167, 53, 129, 132, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
            ]);
          }
          const idx = ++cameraRollCounter;
          const vfsPath = `/app-storage/CameraRoll/image_${idx}.png`;
          callKernel('fs_write_bytes', vfsPath, pngBytes, Date.now());
          lastCameraRollFile = fileFromNativePath(vfsPath);
          this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
        } catch (err) {
          this.dispatchEvent(new BaseErrorEvent('error', false, false, String(err.message)));
        }
      }
      browseForImage(browseOptions = null) {
        if (browseOptions !== null && !(browseOptions instanceof CameraRollBrowseOptions)) {
          throw argErr('CameraRoll.browseForImage requires CameraRollBrowseOptions or null.');
        }
        if (lastCameraRollFile && lastCameraRollFile.exists) {
          const promise = new MediaPromise(
            lastCameraRollFile,
            MediaType.IMAGE,
            lastCameraRollFile.name,
            false
          );
          this.dispatchEvent(new MediaEvent(MediaEvent.SELECT, false, false, promise));
        } else {
          this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL));
        }
      }
    }
    tagType(CameraRoll, 'CameraRoll', v => v instanceof CameraRoll);

    let cameraUiCounter = 0;
    class CameraUI extends BaseDispatcher {
      constructor() {
        super();
      }
      static get isSupported() { return true; }
      static get permissionStatus() { return 'granted'; }
      requestPermission() {
        scheduleTask(() => {
          this.dispatchEvent(
            new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, 'granted')
          );
        });
      }
      launch(requestedMediaType = 'image') {
        const mt = String(requestedMediaType ?? '');
        if (mt !== MediaType.IMAGE && mt !== MediaType.VIDEO) {
          throw argErr(`Invalid MediaType '${mt}'. Expected MediaType.IMAGE or MediaType.VIDEO.`);
        }
        const idx = ++cameraUiCounter;
        const ext = mt === MediaType.VIDEO ? 'mp4' : 'png';
        const vfsPath = `/cache/CameraUI/capture_${idx}.${ext}`;
        const sampleBytes = new Uint8Array([
          137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82,
          0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137,
          0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 255, 255, 63, 0,
          5, 254, 2, 254, 167, 53, 129, 132, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]);
        callKernel('fs_write_bytes', vfsPath, sampleBytes, Date.now());
        const file = fileFromNativePath(vfsPath);
        const promise = new MediaPromise(file, mt, `CameraUI/capture_${idx}.${ext}`, false);
        this.dispatchEvent(new MediaEvent(MediaEvent.COMPLETE, false, false, promise));
      }
    }
    tagType(CameraUI, 'CameraUI', v => v instanceof CameraUI);

    const stageWebViewStates = new WeakMap();
    function swvState(receiver) {
      const s = stageWebViewStates.get(receiver);
      if (!s) throw new TypeError('Expected a StageWebView instance.');
      if (s.disposed) throw illegalOp('StageWebView has been disposed.');
      return s;
    }

    function syncSwvKernel(receiver, s) {
      const snap = JSON.parse(callKernel('html_state_json', s.sessionId));
      s.location = String(snap.location || '');
      s.title = String(snap.title || '');
      s.textContent = String(snap.textContent || '');
      s.historyLength = Number(snap.historyLength) || 0;
      s.historyPosition = Number(snap.historyPosition) || 0;
      receiver.dispatchEvent(
        new LocationChangeEvent(LocationChangeEvent.LOCATION_CHANGE, false, false, s.location)
      );
      receiver.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
    }

    class StageWebView extends BaseDispatcher {
      constructor(useNative = false, mediaPlaybackRequiresUserAction = true) {
        ensureActive();
        super();
        const sessionId = callKernel('html_create', 800, 600);
        stageWebViewStates.set(this, {
          sessionId,
          useNative: Boolean(useNative),
          mediaPlaybackRequiresUserAction: Boolean(mediaPlaybackRequiresUserAction),
          stage: null,
          viewPort: null,
          location: '',
          title: '',
          textContent: '',
          historyLength: 0,
          historyPosition: 0,
          disposed: false,
        });
      }
      static get isSupported() { return true; }
      get stage() { return swvState(this).stage; }
      set stage(v) { swvState(this).stage = v ?? null; }
      get viewPort() {
        const vp = swvState(this).viewPort;
        return vp ? makeRect(vp.x, vp.y, vp.width, vp.height) : null;
      }
      set viewPort(v) {
        const s = swvState(this);
        if (v === null) {
          s.viewPort = null;
          return;
        }
        if (typeof v !== 'object' || Number(v.width) < 0 || Number(v.height) < 0) {
          throw argErr('StageWebView.viewPort requires a valid Rectangle.');
        }
        s.viewPort = makeRect(
          Number(v.x) || 0,
          Number(v.y) || 0,
          Number(v.width) || 0,
          Number(v.height) || 0
        );
        if (s.viewPort.width > 0 && s.viewPort.height > 0) {
          callKernel('html_set_viewport', s.sessionId, s.viewPort.width, s.viewPort.height);
        }
      }
      get location() { return swvState(this).location; }
      get title() { return swvState(this).title; }
      get isHistoryBackEnabled() {
        const s = swvState(this);
        return s.historyLength > 1 && s.historyPosition > 0;
      }
      get isHistoryForwardEnabled() {
        const s = swvState(this);
        return s.historyLength > 1 && s.historyPosition + 1 < s.historyLength;
      }
      get mediaPlaybackRequiresUserAction() {
        return swvState(this).mediaPlaybackRequiresUserAction;
      }
      set mediaPlaybackRequiresUserAction(v) {
        swvState(this).mediaPlaybackRequiresUserAction = Boolean(v);
      }
      loadString(text, mimeType = 'text/html') {
        const s = swvState(this);
        const _mime = String(mimeType ?? 'text/html');
        callKernel('html_load_string', s.sessionId, String(text ?? ''), s.location || 'about:blank');
        syncSwvKernel(this, s);
      }
      loadURL(url) {
        const s = swvState(this);
        const target = String(url ?? '');
        if (!target) throw argErr('StageWebView.loadURL requires a non-empty URL.');
        const changing = new LocationChangeEvent(
          LocationChangeEvent.LOCATION_CHANGING,
          false,
          true,
          target
        );
        if (!this.dispatchEvent(changing)) return;
        callKernel('html_load_url', s.sessionId, target, false, target);
        syncSwvKernel(this, s);
      }
      reload() {
        const s = swvState(this);
        if (callKernel('html_reload', s.sessionId)) {
          syncSwvKernel(this, s);
        }
      }
      stop() {
        const s = swvState(this);
        callKernel('html_cancel', s.sessionId);
      }
      historyBack() {
        const s = swvState(this);
        if (!this.isHistoryBackEnabled) return;
        if (callKernel('html_history_go', s.sessionId, -1)) {
          syncSwvKernel(this, s);
        }
      }
      historyForward() {
        const s = swvState(this);
        if (!this.isHistoryForwardEnabled) return;
        if (callKernel('html_history_go', s.sessionId, 1)) {
          syncSwvKernel(this, s);
        }
      }
      assignFocus(direction = 'none') {
        swvState(this);
        const d = String(direction ?? 'none');
        if (!['top', 'bottom', 'none'].includes(d)) {
          throw argErr(`Invalid FocusDirection '${d}'.`);
        }
        const FocusEv = events.FocusEvent || BaseEvent;
        this.dispatchEvent(new FocusEv('focusIn'));
      }
      drawViewPortToBitmapData(bitmap) {
        const s = swvState(this);
        if (!s.viewPort || s.viewPort.width <= 0 || s.viewPort.height <= 0) {
          throw illegalOp('StageWebView.viewPort must be set before calling drawViewPortToBitmapData.');
        }
        if (!bitmap || typeof bitmap.setPixel32 !== 'function') {
          throw argErr('StageWebView.drawViewPortToBitmapData requires a BitmapData.');
        }
        const w = Math.round(s.viewPort.width);
        const h = Math.round(s.viewPort.height);
        if ((Number(bitmap.width) | 0) !== w || (Number(bitmap.height) | 0) !== h) {
          throw argErr('BitmapData dimensions must match StageWebView.viewPort dimensions.');
        }
        const textToRender = s.textContent || s.title || s.location || 'StageWebView';
        const rgba = callKernel(
          'rasterize_text_rgba',
          w,
          h,
          textToRender,
          0x111111,
          0xffffffff,
          12,
          'multiline:left'
        );
        writeRgbaToBitmapData(bitmap, rgba, w, h);
      }
      dispose() {
        const s = stageWebViewStates.get(this);
        if (!s || s.disposed) return;
        s.disposed = true;
        callKernel('html_dispose', s.sessionId);
      }
    }
    tagType(StageWebView, 'StageWebView', v => stageWebViewStates.has(v));

    class AVURLLoader extends BaseDispatcher {
      constructor(request = null) {
        super();
        this.data = null;
        this.dataFormat = 'text';
        this.bytesLoaded = 0;
        this.bytesTotal = 0;
        if (request !== null) this.load(request);
      }
      load(request) {
        if (!request || typeof request.url !== 'string' || !request.url) {
          throw argErr('AVURLLoader.load requires a URLRequest.');
        }
        const url = request.url;
        try {
          let bytes = new Uint8Array(0);
          if (/^(app:|app-storage:|app-desktop:|app-docs:|app-user:|app-cache:|file:|\/)/i.test(url)) {
            const p = callKernel('normalize_path', url);
            bytes = callKernel('fs_open_stream', p, 'read', Date.now());
          } else {
            bytes = new TextEncoder().encode(url);
          }
          this.bytesLoaded = bytes.length;
          this.bytesTotal = bytes.length;
          this.data =
            this.dataFormat === 'binary'
              ? binary
                ? binary.fromBytes(bytes)
                : bytes
              : new TextDecoder().decode(bytes);
          this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
          this.dispatchEvent(
            new AVHTTPStatusEvent(
              AVHTTPStatusEvent.AV_HTTP_RESPONSE_STATUS,
              false,
              false,
              200,
              url,
              []
            )
          );
          if (options.net?.HTTPStatusEvent) {
            const hs = new options.net.HTTPStatusEvent('httpResponseStatus', false, false, 200, false);
            hs.responseURL = url;
            this.dispatchEvent(hs);
          }
          this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
        } catch (err) {
          const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
          this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
        }
      }
      close() {}
    }
    tagType(AVURLLoader, 'AVURLLoader', v => v instanceof AVURLLoader);

    class AirCamera extends BaseDispatcher {
      constructor(name = 'Default Camera', position = 'front') {
        super();
        this.name = String(name);
        this.position = String(position || 'front');
        this.width = 640;
        this.height = 480;
        this.fps = 30;
        this.currentFPS = 30;
        this.muted = false;
        this.activityLevel = -1;
        this.bandwidth = 0;
        this.quality = 0;
        this.index = 0;
        this.keyFrameInterval = 15;
        this.loopback = false;
        this.motionLevel = 50;
        this.motionTimeout = 2000;
      }
      static get isSupported() { return true; }
      static get permissionStatus() { return 'granted'; }
      static get names() { return ['Default Camera']; }
      static getCamera(name = null) {
        const pos = name === '1' || name === 'back' ? CameraPosition.BACK : CameraPosition.FRONT;
        return new AirCamera(name || 'Default Camera', pos);
      }
      requestPermission() {
        scheduleTask(() => {
          this.dispatchEvent(
            new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, 'granted')
          );
        });
      }
      setMode(width, height, fps, _favorArea = true) {
        this.width = Number(width) | 0;
        this.height = Number(height) | 0;
        this.fps = Number(fps) || 30;
      }
      setQuality(bandwidth, quality) {
        this.bandwidth = Number(bandwidth) | 0;
        this.quality = Number(quality) | 0;
      }
      setMotionLevel(motionLevel, timeout = 2000) {
        this.motionLevel = Number(motionLevel) | 0;
        this.motionTimeout = Number(timeout) | 0;
      }
      setKeyFrameInterval(keyFrameInterval) {
        this.keyFrameInterval = Number(keyFrameInterval) | 0;
      }
      setLoopback(compress = false) {
        this.loopback = Boolean(compress);
      }
      copyToByteArray(rect, destination) {
        if (!rect || !destination) {
          throw argErr('Camera.copyToByteArray requires rect and destination ByteArray.');
        }
        const count = Math.max(0, (Number(rect.width) | 0) * (Number(rect.height) | 0) * 4);
        for (let i = 0; i < count; i++) {
          destination.writeByte(0);
        }
      }
      copyToVector(rect, destination) {
        if (!rect || !destination) {
          throw argErr('Camera.copyToVector requires rect and destination Vector.');
        }
        const count = Math.max(0, (Number(rect.width) | 0) * (Number(rect.height) | 0));
        for (let i = 0; i < count; i++) {
          destination[i] = 0xff000000;
        }
      }
      drawToBitmapData(destination) {
        if (!destination) {
          throw argErr('Camera.drawToBitmapData requires a BitmapData destination.');
        }
      }
    }
    tagType(AirCamera, 'Camera', v => v instanceof AirCamera);

    class AirMicrophone extends BaseDispatcher {
      constructor(name = 'Default Microphone', index = 0) {
        super();
        this.name = String(name);
        this.index = Number(index) | 0;
        this.gain = 50;
        this.rate = 44;
        this.muted = false;
        this.activityLevel = -1;
        this.silenceLevel = 10;
        this.silenceTimeout = 2000;
        this.useEchoSuppression = false;
        this.codec = 'Nellymoser';
        this.encodeQuality = 6;
        this.framesPerPacket = 2;
        this.enhancedOptions = {
          mode: 'fullDuplex',
          echoPath: 128,
          nonLinearProcessing: true,
          isVoiceDetected: -1,
        };
      }
      static get isSupported() { return true; }
      static get permissionStatus() { return 'granted'; }
      static get names() { return ['Default Microphone']; }
      static getMicrophone(index = -1) {
        return new AirMicrophone('Default Microphone', index < 0 ? 0 : index);
      }
      static getEnhancedMicrophone(index = -1) {
        return new AirMicrophone('Enhanced Microphone', index < 0 ? 0 : index);
      }
      requestPermission() {
        scheduleTask(() => {
          this.dispatchEvent(
            new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, 'granted')
          );
        });
      }
      setSilenceLevel(silenceLevel, timeout = 2000) {
        this.silenceLevel = Number(silenceLevel) || 0;
        this.silenceTimeout = Number(timeout) | 0;
      }
      setUseEchoSuppression(useEchoSuppression) {
        this.useEchoSuppression = Boolean(useEchoSuppression);
      }
      setLoopBack(_state = true) {}
    }
    tagType(AirMicrophone, 'Microphone', v => v instanceof AirMicrophone);

    let soundMixerState = {
      audioPlaybackMode: 'media',
      useSpeakerphoneForVoice: false,
      bufferTime: 0,
    };
    const AirSoundMixer = {
      SoundMixer: function () {
        throw new TypeError('SoundMixer supplies static members only.');
      },
    }.SoundMixer;
    Object.defineProperties(AirSoundMixer, {
      audioPlaybackMode: {
        get: () => soundMixerState.audioPlaybackMode,
        set: v => {
          const m = String(v ?? 'media');
          if (!['ambient', 'media', 'voice'].includes(m)) {
            throw argErr(`Invalid AudioPlaybackMode '${m}'.`);
          }
          soundMixerState.audioPlaybackMode = m;
        },
        enumerable: true,
      },
      useSpeakerphoneForVoice: {
        get: () => soundMixerState.useSpeakerphoneForVoice,
        set: v => { soundMixerState.useSpeakerphoneForVoice = Boolean(v); },
        enumerable: true,
      },
      bufferTime: {
        get: () => soundMixerState.bufferTime,
        set: v => { soundMixerState.bufferTime = Number(v) | 0; },
        enumerable: true,
      },
      stopAll: {
        value: () => {
          if (typeof soundMixerState.delegateStopAll === 'function') {
            soundMixerState.delegateStopAll();
          }
        },
        enumerable: true,
      },
      areSoundsInaccessible: { value: () => false, enumerable: true },
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError('SoundMixer cannot be extended.');
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    Object.freeze(AirSoundMixer);

    // -------------------------------------------------------------------------
    // 10. flash.net.* & flash.net.drm.* (IPVersion, InterfaceAddress,
    //     NetworkInterface, NetworkInfo, DatagramSocket, ServerSocket, Socket,
    //     URLRequestDefaults, URLStream, FileReference, Responder, DRMDeviceGroup)
    // -------------------------------------------------------------------------
    const IPVersion = defConstClass('IPVersion', {
      IPV4: 'IPv4',
      IPV6: 'IPv6',
    });

    class InterfaceAddress {
      constructor(address = '', broadcast = '', prefixLength = -1, ipVersion = 'IPv4') {
        this.address = String(address ?? '');
        this.broadcast = String(broadcast ?? '');
        this.prefixLength = Number(prefixLength) | 0;
        this.ipVersion = String(ipVersion || 'IPv4');
      }
    }
    tagType(InterfaceAddress, 'InterfaceAddress', v => v instanceof InterfaceAddress);

    class NetworkInterface {
      constructor(init = {}) {
        this.name = String(init.name ?? '');
        this.displayName = String(init.displayName ?? '');
        this.mtu = Number(init.mtu ?? 1500) | 0;
        this.active = Boolean(init.active ?? true);
        this.hardwareAddress = String(init.hardwareAddress ?? '');
        this.parent = init.parent ?? null;
        this.addresses = makeVector(
          'flash.net.InterfaceAddress',
          Array.isArray(init.addresses) ? init.addresses : []
        );
        this.subInterfaces = makeVector(
          'flash.net.NetworkInterface',
          Array.isArray(init.subInterfaces) ? init.subInterfaces : []
        );
      }
    }
    tagType(NetworkInterface, 'NetworkInterface', v => v instanceof NetworkInterface);

    const netInfoToken = Symbol('NetworkInfo');
    class NetworkInfo extends BaseDispatcher {
      constructor(token) {
        if (token !== netInfoToken) {
          throw argErr('NetworkInfo is a singleton; use NetworkInfo.networkInfo.');
        }
        super();
      }
      static get isSupported() { return true; }
      static get permissionStatus() { return 'granted'; }
      static get networkInfo() {
        ensureActive();
        if (!networkInfoSingleton) {
          networkInfoSingleton = new NetworkInfo(netInfoToken);
        }
        return networkInfoSingleton;
      }
      findInterfaces() {
        const rawList = JSON.parse(callKernel('net_interfaces_json'));
        const interfaces = rawList.map(item => {
          const addrs = (item.addresses || []).map(
            a => new InterfaceAddress(a.address, a.broadcast, a.prefixLength, a.ipVersion)
          );
          return new NetworkInterface({
            name: item.name,
            displayName: item.displayName,
            mtu: item.mtu,
            active: item.active,
            hardwareAddress: item.hardwareAddress,
            parent: null,
            addresses: addrs,
            subInterfaces: [],
          });
        });
        return makeVector('flash.net.NetworkInterface', interfaces);
      }
    }
    tagType(NetworkInfo, 'NetworkInfo', v => v === networkInfoSingleton && v !== null);

    // DatagramSocket backed by Rust AirNetworkKernel UDP loopback table
    const udpSocketMap = new Map();
    const udpStates = new WeakMap();
    function udpState(receiver) {
      ensureActive();
      const s = udpStates.get(receiver);
      if (!s) throw new TypeError('Expected a DatagramSocket instance.');
      return s;
    }

    function drainUdpLoopback(s) {
      const targetSock = udpSocketMap.get(s.sockId);
      if (!targetSock) return;
      const snap = JSON.parse(callKernel('udp_state_json', s.sockId));
      if (!snap.receiving || !Array.isArray(snap.inbox)) return;
      while (snap.inbox.length > 0) {
        const pkt = snap.inbox.shift();
        const bytes = new Uint8Array(pkt.data || []);
        const ba = binary ? binary.fromBytes(bytes) : bytes;
        targetSock.dispatchEvent(
          new DatagramSocketDataEvent(
            DatagramSocketDataEvent.DATA,
            false,
            false,
            pkt.srcAddress,
            pkt.srcPort,
            pkt.dstAddress,
            pkt.dstPort,
            ba
          )
        );
      }
    }

    class DatagramSocket extends BaseDispatcher {
      constructor() {
        ensureActive();
        super();
        const sockId = callKernel('udp_create');
        const st = {
          sockId,
          closed: false,
          bound: false,
          connected: false,
          receiving: false,
          localAddress: null,
          localPort: 0,
          remoteAddress: null,
          remotePort: 0,
        };
        udpStates.set(this, st);
        udpSocketMap.set(sockId, this);
      }
      static get isSupported() { return true; }
      get bound() { return udpState(this).bound; }
      get connected() { return udpState(this).connected; }
      get localAddress() { return udpState(this).localAddress; }
      get localPort() { return udpState(this).localPort; }
      get remoteAddress() { return udpState(this).remoteAddress; }
      get remotePort() { return udpState(this).remotePort; }
      bind(localPort = 0, localAddress = '0.0.0.0') {
        const s = udpState(this);
        if (s.closed) throw illegalOp('DatagramSocket has been closed.');
        if (s.bound) throw illegalOp('DatagramSocket is already bound.');
        const port = Number(localPort) | 0;
        if (port < 0 || port > 65535) throw new RangeError('localPort must be between 0 and 65535.');
        const snap = JSON.parse(
          callKernel('udp_bind', s.sockId, port, String(localAddress || '0.0.0.0'))
        );
        s.bound = true;
        s.localAddress = snap.localAddress;
        s.localPort = snap.localPort;
      }
      connect(remoteAddress, remotePort) {
        const s = udpState(this);
        if (s.closed) throw illegalOp('DatagramSocket has been closed.');
        const port = Number(remotePort) | 0;
        if (port <= 0 || port > 65535) {
          throw new RangeError('remotePort must be between 1 and 65535.');
        }
        const snap = JSON.parse(
          callKernel('udp_connect', s.sockId, String(remoteAddress ?? ''), port)
        );
        s.connected = true;
        s.remoteAddress = snap.remoteAddress;
        s.remotePort = snap.remotePort;
      }
      receive() {
        const s = udpState(this);
        if (s.closed) throw illegalOp('DatagramSocket has been closed.');
        if (!s.bound) this.bind(0, '127.0.0.1');
        callKernel('udp_receive', s.sockId);
        s.receiving = true;
      }
      send(bytes, offset = 0, length = 0, address = null, port = 0) {
        const s = udpState(this);
        if (s.closed) throw illegalOp('DatagramSocket has been closed.');
        if (!bytes) throw argErr('DatagramSocket.send requires a ByteArray.');
        let rawBytes;
        if (binary?.ByteArray?.__nfIs?.(bytes)) rawBytes = binary.toBytes(bytes);
        else if (bytes instanceof Uint8Array) rawBytes = bytes;
        else throw new TypeError('DatagramSocket.send requires a ByteArray.');
        const off = Number(offset) >>> 0;
        const len = Number(length) >>> 0;
        if (off > rawBytes.length || off + len > rawBytes.length) {
          throw new RangeError('DatagramSocket.send offset/length out of bounds.');
        }
        const slice = len === 0 ? rawBytes.subarray(off) : rawBytes.subarray(off, off + len);
        const targetAddr = address == null ? s.remoteAddress : String(address);
        const targetPort = (Number(port) | 0) || s.remotePort;
        if (!targetAddr || targetPort <= 0 || targetPort > 65535) {
          throw argErr('DatagramSocket.send requires a valid target address and port.');
        }
        const snap = JSON.parse(
          callKernel('udp_send', s.sockId, targetAddr, targetPort)
        );
        s.bound = true;
        s.localAddress = snap.localAddress;
        s.localPort = snap.localPort;
        for (const d of snap.deliveries || []) {
          const peerSock = udpSocketMap.get(d.targetSockId);
          const peerState = peerSock ? udpStates.get(peerSock) : null;
          if (peerSock && peerState && peerState.receiving && !peerState.closed) {
            const payloadBa = binary ? binary.fromBytes(slice) : new Uint8Array(slice);
            peerSock.dispatchEvent(
              new DatagramSocketDataEvent(
                DatagramSocketDataEvent.DATA,
                false,
                false,
                d.srcAddress,
                d.srcPort,
                d.dstAddress,
                d.dstPort,
                payloadBa
              )
            );
          }
        }
      }
      close() {
        const s = udpState(this);
        if (s.closed) return;
        s.closed = true;
        s.bound = false;
        s.connected = false;
        s.receiving = false;
        callKernel('udp_close', s.sockId);
        udpSocketMap.delete(s.sockId);
        this.dispatchEvent(new BaseEvent(BaseEvent.CLOSE));
      }
    }
    tagType(DatagramSocket, 'DatagramSocket', v => udpStates.has(v));

    // ServerSocket and Socket (implementing IDataInput & IDataOutput) backed by Rust AirNetworkKernel
    const serverSocketMap = new Map();
    const serverSocketStates = new WeakMap();
    const tcpSocketMap = new Map();
    const tcpSocketStates = new WeakMap();

    function srvState(receiver) {
      ensureActive();
      const s = serverSocketStates.get(receiver);
      if (!s) throw new TypeError('Expected a ServerSocket instance.');
      return s;
    }

    class ServerSocket extends BaseDispatcher {
      constructor() {
        ensureActive();
        super();
        const serverId = callKernel('tcp_server_create');
        const st = {
          serverId,
          closed: false,
          bound: false,
          listening: false,
          localAddress: null,
          localPort: 0,
        };
        serverSocketStates.set(this, st);
        serverSocketMap.set(serverId, this);
      }
      static get isSupported() { return true; }
      get bound() { return srvState(this).bound; }
      get listening() { return srvState(this).listening; }
      get localAddress() { return srvState(this).localAddress; }
      get localPort() { return srvState(this).localPort; }
      bind(localPort = 0, localAddress = '0.0.0.0') {
        const s = srvState(this);
        if (s.closed) throw illegalOp('ServerSocket has been closed.');
        if (s.bound) throw illegalOp('ServerSocket is already bound.');
        const port = Number(localPort) | 0;
        if (port < 0 || port > 65535) throw new RangeError('localPort must be between 0 and 65535.');
        const snap = JSON.parse(
          callKernel('tcp_server_bind', s.serverId, port, String(localAddress || '0.0.0.0'))
        );
        s.bound = true;
        s.localAddress = snap.localAddress;
        s.localPort = snap.localPort;
      }
      listen(backlog = 0) {
        const s = srvState(this);
        if (s.closed) throw illegalOp('ServerSocket has been closed.');
        if (!s.bound) throw illegalOp('ServerSocket must be bound before calling listen().');
        callKernel('tcp_server_listen', s.serverId, Number(backlog) | 0);
        s.listening = true;
      }
      close() {
        const s = srvState(this);
        if (s.closed) return;
        s.closed = true;
        s.bound = false;
        s.listening = false;
        callKernel('tcp_server_close', s.serverId);
        serverSocketMap.delete(s.serverId);
        this.dispatchEvent(new BaseEvent(BaseEvent.CLOSE));
      }
    }
    tagType(ServerSocket, 'ServerSocket', v => serverSocketStates.has(v));

    function sockState(receiver) {
      ensureActive();
      const s = tcpSocketStates.get(receiver);
      if (!s) throw new TypeError('Expected a Socket instance.');
      return s;
    }

    function ensureSocketConnected(receiver) {
      const s = sockState(receiver);
      if (!s.connected) throw illegalOp('Socket is not connected.');
      return s;
    }

    const acceptedSocketToken = Symbol('AcceptedSocket');
    class Socket extends BaseDispatcher {
      constructor(host = null, port = 0) {
        ensureActive();
        super();
        const inBuf = binary ? new binary.ByteArray() : null;
        const outBuf = binary ? new binary.ByteArray() : null;
        const st = {
          connId: null,
          peerConnId: null,
          connected: false,
          localAddress: null,
          localPort: 0,
          remoteAddress: null,
          remotePort: 0,
          timeout: 20000,
          endian: binary ? binary.Endian.BIG_ENDIAN : 'bigEndian',
          objectEncoding: 3,
          inBuf,
          outBuf,
        };
        tcpSocketStates.set(this, st);
        streamStates.set(this, {
          open: false,
          async: true,
          mode: 'update',
          path: null,
          buffer: inBuf,
          endian: st.endian,
          objectEncoding: 3,
          readAhead: Number.POSITIVE_INFINITY,
        });
        if (host !== null && host !== acceptedSocketToken && Number(port) > 0) {
          this.connect(host, port);
        }
      }
      get connected() { return sockState(this).connected; }
      get localAddress() { return sockState(this).localAddress; }
      get localPort() { return sockState(this).localPort; }
      get remoteAddress() { return sockState(this).remoteAddress; }
      get remotePort() { return sockState(this).remotePort; }
      get timeout() { return sockState(this).timeout; }
      set timeout(v) { sockState(this).timeout = Math.max(250, Number(v) | 0); }
      get bytesPending() {
        const s = sockState(this);
        return s.outBuf ? s.outBuf.length : 0;
      }
      get bytesAvailable() {
        const s = sockState(this);
        return s.connected && s.inBuf ? s.inBuf.bytesAvailable : 0;
      }
      get endian() { return sockState(this).endian; }
      set endian(value) {
        const s = sockState(this);
        if (binary && value !== binary.Endian.BIG_ENDIAN && value !== binary.Endian.LITTLE_ENDIAN) {
          throw new TypeError('Socket.endian must be Endian.BIG_ENDIAN or Endian.LITTLE_ENDIAN.');
        }
        s.endian = value;
        if (s.inBuf) s.inBuf.endian = value;
        if (s.outBuf) s.outBuf.endian = value;
      }
      get objectEncoding() { return sockState(this).objectEncoding; }
      set objectEncoding(value) {
        const s = sockState(this);
        const enc = Number(value) >>> 0;
        if (enc !== 0 && enc !== 3) throw new RangeError('Socket.objectEncoding must be 0 or 3.');
        s.objectEncoding = enc;
        if (s.inBuf) s.inBuf.objectEncoding = enc;
        if (s.outBuf) s.outBuf.objectEncoding = enc;
      }

      connect(host, port) {
        const s = sockState(this);
        const targetHost = String(host ?? '127.0.0.1');
        const targetPort = Number(port) | 0;
        if (targetPort <= 0 || targetPort > 65535) {
          throw new SecurityError('Socket.connect requires a valid port (1..65535).');
        }
        try {
          const info = JSON.parse(callKernel('tcp_connect_loopback', targetHost, targetPort));
          if (!info.matched) {
            throw new Error(`No listening ServerSocket on ${targetHost}:${targetPort}.`);
          }
          s.connId = info.clientConnId;
          s.peerConnId = info.acceptedConnId;
          s.connected = true;
          s.localAddress = info.clientAddress;
          s.localPort = info.clientPort;
          s.remoteAddress = info.serverAddress;
          s.remotePort = info.serverPort;
          streamStates.get(this).open = true;
          tcpSocketMap.set(s.connId, this);

          const serverSock = serverSocketMap.get(info.serverId);
          if (serverSock) {
            const accepted = new Socket(acceptedSocketToken, 0);
            const as = sockState(accepted);
            as.connId = info.acceptedConnId;
            as.peerConnId = info.clientConnId;
            as.connected = true;
            as.localAddress = info.serverAddress;
            as.localPort = info.serverPort;
            as.remoteAddress = info.clientAddress;
            as.remotePort = info.clientPort;
            streamStates.get(accepted).open = true;
            tcpSocketMap.set(as.connId, accepted);
            serverSock.dispatchEvent(
              new ServerSocketConnectEvent(
                ServerSocketConnectEvent.CONNECT,
                false,
                false,
                accepted
              )
            );
          }
          this.dispatchEvent(new BaseEvent(BaseEvent.CONNECT));
        } catch (err) {
          scheduleTask(() => {
            const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
            this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
          });
        }
      }

      flush() {
        const s = ensureSocketConnected(this);
        if (!s.outBuf || s.outBuf.length === 0) return;
        const payload = binary.toBytes(s.outBuf);
        s.outBuf.clear();
        const peerId = callKernel('tcp_write', s.connId, payload);
        const peerSock = tcpSocketMap.get(peerId);
        const peerState = peerSock ? tcpSocketStates.get(peerSock) : null;
        if (peerSock && peerState && peerState.connected && peerState.inBuf) {
          const chunk = callKernel('tcp_read', peerId);
          const prevPos = peerState.inBuf.position;
          peerState.inBuf.position = peerState.inBuf.length;
          const chunkBa = binary.fromBytes(chunk);
          peerState.inBuf.writeBytes(chunkBa, 0, chunkBa.length);
          peerState.inBuf.position = prevPos;
          if (options.net?.ProgressEvent) {
            peerSock.dispatchEvent(
              new options.net.ProgressEvent(
                'socketData',
                false,
                false,
                peerState.inBuf.bytesAvailable,
                0
              )
            );
          }
        }
      }

      close() {
        const s = sockState(this);
        if (!s.connected && !s.peerClosed) throw illegalOp('Socket is not connected.');
        s.connected = false;
        s.peerClosed = false;
        streamStates.get(this).open = false;
        if (s.connId !== null) {
          const cid = s.connId;
          s.connId = null;
          const peerId = callKernel('tcp_close', cid);
          tcpSocketMap.delete(cid);
          const peerSock = tcpSocketMap.get(peerId);
          const peerState = peerSock ? tcpSocketStates.get(peerSock) : null;
          if (peerSock && peerState && peerState.connected) {
            peerState.connected = false;
            peerState.peerClosed = true;
            peerState.connId = null;
            streamStates.get(peerSock).open = false;
            tcpSocketMap.delete(peerId);
            peerSock.dispatchEvent(new BaseEvent(BaseEvent.CLOSE));
          }
        }
      }

      _readSock(fn) {
        const s = ensureSocketConnected(this);
        try {
          return fn(s.inBuf);
        } catch (err) {
          if (err instanceof RangeError && /exceeds available data/i.test(String(err.message))) {
            throw eofErr();
          }
          throw err;
        }
      }
      readBoolean() { return this._readSock(b => b.readBoolean()); }
      readByte() { return this._readSock(b => b.readByte()); }
      readUnsignedByte() { return this._readSock(b => b.readUnsignedByte()); }
      readShort() { return this._readSock(b => b.readShort()); }
      readUnsignedShort() { return this._readSock(b => b.readUnsignedShort()); }
      readInt() { return this._readSock(b => b.readInt()); }
      readUnsignedInt() { return this._readSock(b => b.readUnsignedInt()); }
      readFloat() { return this._readSock(b => b.readFloat()); }
      readDouble() { return this._readSock(b => b.readDouble()); }
      readUTF() { return this._readSock(b => b.readUTF()); }
      readUTFBytes(length) { return this._readSock(b => b.readUTFBytes(length)); }
      readMultiByte(length, charSet) { return this._readSock(b => b.readMultiByte(length, charSet)); }
      readBytes(bytes, offset = 0, length = 0) {
        this._readSock(b => {
          if (length > b.bytesAvailable) throw eofErr();
          b.readBytes(bytes, offset, length);
        });
      }
      readObject() { return this._readSock(b => b.readObject()); }

      writeBoolean(v) { ensureSocketConnected(this).outBuf.writeBoolean(v); }
      writeByte(v) { ensureSocketConnected(this).outBuf.writeByte(v); }
      writeShort(v) { ensureSocketConnected(this).outBuf.writeShort(v); }
      writeInt(v) { ensureSocketConnected(this).outBuf.writeInt(v); }
      writeUnsignedInt(v) { ensureSocketConnected(this).outBuf.writeUnsignedInt(v); }
      writeFloat(v) { ensureSocketConnected(this).outBuf.writeFloat(v); }
      writeDouble(v) { ensureSocketConnected(this).outBuf.writeDouble(v); }
      writeUTF(v) { ensureSocketConnected(this).outBuf.writeUTF(v); }
      writeUTFBytes(v) { ensureSocketConnected(this).outBuf.writeUTFBytes(v); }
      writeMultiByte(v, charSet) { ensureSocketConnected(this).outBuf.writeMultiByte(v, charSet); }
      writeBytes(bytes, offset = 0, length = 0) {
        ensureSocketConnected(this).outBuf.writeBytes(bytes, offset, length);
      }
      writeObject(v) { ensureSocketConnected(this).outBuf.writeObject(v); }
    }
    tagType(Socket, 'Socket', v => tcpSocketStates.has(v));

    // URLRequestDefaults & URLRequest AIR properties
    let urlRequestDefaultsState = {
      authenticate: true,
      cacheResponse: true,
      followRedirects: true,
      idleTimeout: 0,
      manageCookies: true,
      useCache: true,
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/537.36 AdobeAIR/32.0',
    };

    const URLRequestDefaults = {
      URLRequestDefaults: function () {
        throw new TypeError('URLRequestDefaults supplies static properties and methods only.');
      },
    }.URLRequestDefaults;
    Object.defineProperties(URLRequestDefaults, {
      authenticate: {
        get: () => urlRequestDefaultsState.authenticate,
        set: v => { urlRequestDefaultsState.authenticate = Boolean(v); },
        enumerable: true,
      },
      cacheResponse: {
        get: () => urlRequestDefaultsState.cacheResponse,
        set: v => { urlRequestDefaultsState.cacheResponse = Boolean(v); },
        enumerable: true,
      },
      followRedirects: {
        get: () => urlRequestDefaultsState.followRedirects,
        set: v => { urlRequestDefaultsState.followRedirects = Boolean(v); },
        enumerable: true,
      },
      idleTimeout: {
        get: () => urlRequestDefaultsState.idleTimeout,
        set: v => {
          const n = Number(v);
          if (n < 0 || Number.isNaN(n)) throw argErr('URLRequestDefaults.idleTimeout must be non-negative.');
          urlRequestDefaultsState.idleTimeout = n;
        },
        enumerable: true,
      },
      manageCookies: {
        get: () => urlRequestDefaultsState.manageCookies,
        set: v => { urlRequestDefaultsState.manageCookies = Boolean(v); },
        enumerable: true,
      },
      useCache: {
        get: () => urlRequestDefaultsState.useCache,
        set: v => { urlRequestDefaultsState.useCache = Boolean(v); },
        enumerable: true,
      },
      userAgent: {
        get: () => urlRequestDefaultsState.userAgent,
        set: v => {
          if (v == null) throw new TypeError('URLRequestDefaults.userAgent cannot be null.');
          urlRequestDefaultsState.userAgent = String(v);
        },
        enumerable: true,
      },
      setLoginCredentialsForHost: {
        value: (hostname, user, password) => {
          if (!hostname || typeof hostname !== 'string') {
            throw argErr('setLoginCredentialsForHost requires a non-empty hostname.');
          }
          return callKernel(
            'net_set_login_credentials',
            hostname,
            user == null ? undefined : String(user),
            password == null ? undefined : String(password)
          );
        },
        enumerable: true,
      },
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError('URLRequestDefaults cannot be extended.');
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    Object.freeze(URLRequestDefaults);

    const urlRequestAirProps = new WeakMap();
    function reqAirState(req) {
      let s = urlRequestAirProps.get(req);
      if (!s) {
        s = {
          authenticate: urlRequestDefaultsState.authenticate,
          cacheResponse: urlRequestDefaultsState.cacheResponse,
          followRedirects: urlRequestDefaultsState.followRedirects,
          idleTimeout: urlRequestDefaultsState.idleTimeout,
          manageCookies: urlRequestDefaultsState.manageCookies,
          useCache: urlRequestDefaultsState.useCache,
          userAgent: urlRequestDefaultsState.userAgent,
        };
        urlRequestAirProps.set(req, s);
      }
      return s;
    }

    function augmentURLRequestClass(URLRequestCtor) {
      if (!URLRequestCtor?.prototype || Object.hasOwn(URLRequestCtor.prototype, 'authenticate')) return;
      Object.defineProperties(URLRequestCtor.prototype, {
        authenticate: {
          get() { return reqAirState(this).authenticate; },
          set(v) { reqAirState(this).authenticate = Boolean(v); },
          enumerable: true,
          configurable: true,
        },
        cacheResponse: {
          get() { return reqAirState(this).cacheResponse; },
          set(v) { reqAirState(this).cacheResponse = Boolean(v); },
          enumerable: true,
          configurable: true,
        },
        followRedirects: {
          get() { return reqAirState(this).followRedirects; },
          set(v) { reqAirState(this).followRedirects = Boolean(v); },
          enumerable: true,
          configurable: true,
        },
        idleTimeout: {
          get() { return reqAirState(this).idleTimeout; },
          set(v) {
            const n = Number(v);
            if (n < 0 || Number.isNaN(n)) throw argErr('URLRequest.idleTimeout must be non-negative.');
            reqAirState(this).idleTimeout = n;
          },
          enumerable: true,
          configurable: true,
        },
        manageCookies: {
          get() { return reqAirState(this).manageCookies; },
          set(v) { reqAirState(this).manageCookies = Boolean(v); },
          enumerable: true,
          configurable: true,
        },
        useCache: {
          get() { return reqAirState(this).useCache; },
          set(v) { reqAirState(this).useCache = Boolean(v); },
          enumerable: true,
          configurable: true,
        },
        userAgent: {
          get() { return reqAirState(this).userAgent; },
          set(v) {
            if (v == null) throw new TypeError('URLRequest.userAgent cannot be null.');
            reqAirState(this).userAgent = String(v);
          },
          enumerable: true,
          configurable: true,
        },
      });
    }
    if (options.net?.URLRequest) {
      augmentURLRequestClass(options.net.URLRequest);
    }

    const AirURLRequestMethod = defConstClass('URLRequestMethod', {
      DELETE: 'DELETE',
      GET: 'GET',
      HEAD: 'HEAD',
      OPTIONS: 'OPTIONS',
      POST: 'POST',
      PUT: 'PUT',
    });

    function augmentURLLoaderClass(URLLoaderCtor) {
      if (!URLLoaderCtor?.prototype || URLLoaderCtor.prototype.__nfAirAugmented) return;
      const origLoad = URLLoaderCtor.prototype.load;
      Object.defineProperty(URLLoaderCtor.prototype, '__nfAirAugmented', { value: true });
      URLLoaderCtor.prototype.load = function (request) {
        const url = request?.url;
        const method = String(request?.method || 'GET').toUpperCase();
        if (
          typeof url === 'string' &&
          (/^(app:|app-storage:|app-desktop:|app-docs:|app-user:|app-cache:|file:|data:)/i.test(url) ||
            ['DELETE', 'HEAD', 'OPTIONS', 'PUT'].includes(method))
        ) {
          scheduleTask(() => {
            try {
              let bytes = new Uint8Array(0);
              if (/^data:/i.test(url)) {
                const comma = url.indexOf(',');
                const body = comma >= 0 ? decodeURIComponent(url.slice(comma + 1)) : '';
                bytes = new TextEncoder().encode(body);
              } else if (/^(app:|app-storage:|app-desktop:|app-docs:|app-user:|app-cache:|file:)/i.test(url)) {
                const p = callKernel('normalize_path', url);
                if (method === 'PUT' && request.data != null) {
                  const writeBytes =
                    binary?.ByteArray?.__nfIs?.(request.data)
                      ? binary.toBytes(request.data)
                      : new TextEncoder().encode(String(request.data));
                  callKernel('fs_write_bytes', p, writeBytes, Date.now());
                  bytes = writeBytes;
                } else if (method === 'DELETE') {
                  callKernel('fs_delete', p, false);
                } else {
                  bytes = callKernel('fs_open_stream', p, 'read', Date.now());
                }
              }
              this.data =
                this.dataFormat === 'binary'
                  ? binary
                    ? binary.fromBytes(bytes)
                    : bytes
                  : this.dataFormat === 'variables' && options.net?.URLVariables
                    ? new options.net.URLVariables(new TextDecoder().decode(bytes))
                    : new TextDecoder().decode(bytes);
              this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
              if (options.net?.ProgressEvent) {
                this.dispatchEvent(
                  new options.net.ProgressEvent('progress', false, false, bytes.length, bytes.length)
                );
              }
              if (options.net?.HTTPStatusEvent) {
                const hs = new options.net.HTTPStatusEvent('httpResponseStatus', false, false, 200, false);
                hs.responseURL = url;
                this.dispatchEvent(hs);
                this.dispatchEvent(new options.net.HTTPStatusEvent('httpStatus', false, false, 200, false));
              }
              this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
            } catch (err) {
              const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
              this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
            }
          });
          return;
        }
        return origLoad.call(this, request);
      };
    }
    if (options.net?.URLLoader) {
      augmentURLLoaderClass(options.net.URLLoader);
    }

    const urlStreamStates = new WeakMap();
    class URLStream extends BaseDispatcher {
      constructor() {
        ensureActive();
        super();
        const buf = binary ? new binary.ByteArray() : null;
        const st = {
          connected: false,
          endian: binary ? binary.Endian.BIG_ENDIAN : 'bigEndian',
          objectEncoding: 3,
          buffer: buf,
        };
        urlStreamStates.set(this, st);
        streamStates.set(this, {
          open: false,
          async: true,
          mode: 'read',
          path: null,
          buffer: buf,
          endian: st.endian,
          objectEncoding: 3,
          readAhead: Number.POSITIVE_INFINITY,
        });
      }
      get connected() { return urlStreamStates.get(this).connected; }
      get bytesAvailable() {
        const s = urlStreamStates.get(this);
        return s.connected && s.buffer ? s.buffer.bytesAvailable : 0;
      }
      get endian() { return urlStreamStates.get(this).endian; }
      set endian(v) {
        const s = urlStreamStates.get(this);
        s.endian = v;
        if (s.buffer) s.buffer.endian = v;
      }
      get objectEncoding() { return urlStreamStates.get(this).objectEncoding; }
      set objectEncoding(v) {
        const s = urlStreamStates.get(this);
        s.objectEncoding = Number(v) >>> 0;
        if (s.buffer) s.buffer.objectEncoding = s.objectEncoding;
      }
      load(request) {
        const s = urlStreamStates.get(this);
        if (!request || typeof request.url !== 'string' || !request.url) {
          throw argErr('URLStream.load requires a URLRequest with a non-empty url.');
        }
        const url = request.url;
        try {
          let bytes = new Uint8Array(0);
          if (/^data:/i.test(url)) {
            const comma = url.indexOf(',');
            const body = comma >= 0 ? decodeURIComponent(url.slice(comma + 1)) : '';
            bytes = new TextEncoder().encode(body);
          } else if (/^(app:|app-storage:|app-desktop:|app-docs:|app-user:|app-cache:|file:|\/)/i.test(url)) {
            const p = callKernel('normalize_path', url);
            bytes = callKernel('fs_open_stream', p, 'read', Date.now());
          } else {
            bytes = new TextEncoder().encode(url);
          }
          const buf = binary.fromBytes(bytes);
          buf.endian = s.endian;
          buf.objectEncoding = s.objectEncoding;
          buf.position = 0;
          s.buffer = buf;
          s.connected = true;
          streamStates.get(this).open = true;
          streamStates.get(this).buffer = buf;
          this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
          if (options.net?.HTTPStatusEvent) {
            const hs = new options.net.HTTPStatusEvent('httpResponseStatus', false, false, 200, false);
            hs.responseURL = url;
            this.dispatchEvent(hs);
            this.dispatchEvent(new options.net.HTTPStatusEvent('httpStatus', false, false, 200, false));
          }
          if (options.net?.ProgressEvent) {
            this.dispatchEvent(
              new options.net.ProgressEvent('progress', false, false, bytes.length, bytes.length)
            );
          }
          this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
        } catch (err) {
          const Ev = options.net?.IOErrorEvent || BaseErrorEvent;
          this.dispatchEvent(new Ev('ioError', false, false, String(err.message)));
        }
      }
      close() {
        const s = urlStreamStates.get(this);
        s.connected = false;
        streamStates.get(this).open = false;
        if (s.buffer) s.buffer.clear();
      }
      _read(fn) {
        const s = urlStreamStates.get(this);
        if (!s.connected || !s.buffer) throw illegalOp('URLStream is not connected.');
        return fn(s.buffer);
      }
      readBoolean() { return this._read(b => b.readBoolean()); }
      readByte() { return this._read(b => b.readByte()); }
      readUnsignedByte() { return this._read(b => b.readUnsignedByte()); }
      readShort() { return this._read(b => b.readShort()); }
      readUnsignedShort() { return this._read(b => b.readUnsignedShort()); }
      readInt() { return this._read(b => b.readInt()); }
      readUnsignedInt() { return this._read(b => b.readUnsignedInt()); }
      readFloat() { return this._read(b => b.readFloat()); }
      readDouble() { return this._read(b => b.readDouble()); }
      readUTF() { return this._read(b => b.readUTF()); }
      readUTFBytes(length) { return this._read(b => b.readUTFBytes(length)); }
      readMultiByte(length, charSet) { return this._read(b => b.readMultiByte(length, charSet)); }
      readBytes(bytes, offset = 0, length = 0) {
        this._read(b => {
          if (length > b.bytesAvailable) throw eofErr();
          b.readBytes(bytes, offset, length);
        });
      }
      readObject() { return this._read(b => b.readObject()); }
    }
    tagType(URLStream, 'URLStream', v => urlStreamStates.has(v));

    class AirFileReference extends BaseDispatcher {
      constructor() {
        super();
        this.name = '';
        this.size = 0;
        this.type = '';
        this.creator = '';
        this.creationDate = new Date(0);
        this.modificationDate = new Date(0);
        this.data = null;
      }
      static get permissionStatus() { return 'granted'; }
      get extension() {
        const nm = String(this.name || '');
        const dot = nm.lastIndexOf('.');
        return dot > 0 && dot + 1 < nm.length ? nm.slice(dot + 1) : null;
      }
      requestPermission() {
        scheduleTask(() => {
          this.dispatchEvent(
            new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, 'granted')
          );
        });
      }
      browse(_typeFilter = null) {
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
        return false;
      }
      download(_request, _defaultFileName = null) {
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
      }
      load() {
        scheduleTask(() => {
          if (!this.data && binary) this.data = new binary.ByteArray();
          this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
          this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
        });
      }
      save(data, defaultFileName = null) {
        if (defaultFileName != null) this.name = String(defaultFileName);
        if (binary?.ByteArray?.__nfIs?.(data)) this.data = data;
        else if (binary) {
          const bytes =
            data instanceof Uint8Array
              ? data
              : new TextEncoder().encode(String(data ?? ''));
          this.data = binary.fromBytes(bytes);
        }
        scheduleTask(() => {
          this.dispatchEvent(new BaseEvent(BaseEvent.SELECT));
          this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
        });
      }
      upload(request, _uploadDataFieldName = 'Filedata', _testUpload = false) {
        this.uploadUnencoded(request);
      }
      uploadUnencoded(request) {
        if (!request || typeof request.url !== 'string' || !request.url) {
          throw argErr('FileReference.uploadUnencoded requires a URLRequest with a valid url.');
        }
        const url = request.url;
        scheduleTask(() => {
          this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
          if (options.net?.HTTPStatusEvent) {
            const hs = new options.net.HTTPStatusEvent('httpResponseStatus', false, false, 200, false);
            hs.responseURL = url;
            this.dispatchEvent(hs);
            this.dispatchEvent(new options.net.HTTPStatusEvent('httpStatus', false, false, 200, false));
          }
          this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
        });
      }
      cancel() {}
    }
    tagType(AirFileReference, 'FileReference', v => v instanceof AirFileReference || fileStates.has(v));

    class AirFileReferenceList extends BaseDispatcher {
      constructor() {
        super();
        this.fileList = [];
      }
      browse(_typeFilter = null) {
        scheduleTask(() => this.dispatchEvent(new BaseEvent(BaseEvent.CANCEL)));
        return false;
      }
    }
    tagType(AirFileReferenceList, 'FileReferenceList', v => v instanceof AirFileReferenceList);

    class Responder {
      constructor(result, status = null) {
        if (typeof result !== 'function') {
          throw new TypeError('Responder requires a result callback function.');
        }
        if (status !== null && typeof status !== 'function') {
          throw new TypeError('Responder status must be a function or null.');
        }
        this._result = result;
        this._status = status;
      }
    }
    tagType(Responder, 'Responder', v => v instanceof Responder);

    class DRMDeviceGroup {
      constructor(serverURL = '', domain = '', authenticationMethod = 'anonymous') {
        this.serverURL = String(serverURL ?? '');
        this.domain = String(domain ?? '');
        this.authenticationMethod = String(authenticationMethod || 'anonymous');
      }
    }
    tagType(DRMDeviceGroup, 'DRMDeviceGroup', v => v instanceof DRMDeviceGroup);

    class DRMContentData {
      constructor(_rawData = null) {
        this.serverURL = '';
        this.domain = '';
        this.licenseID = '';
        this.authenticationMethod = 'anonymous';
      }
    }
    tagType(DRMContentData, 'DRMContentData', v => v instanceof DRMContentData);

    class DRMVoucher {
      constructor() {
        this.serverURL = '';
        this.licenseID = '';
        this.policyID = '';
        this.offlineLeaseStartDate = null;
        this.offlineLeaseExpirationDate = null;
        this.offlineLeasePeriod = 0;
        this.voucherStartDate = null;
        this.voucherEndDate = null;
        this.policies = {};
      }
    }
    tagType(DRMVoucher, 'DRMVoucher', v => v instanceof DRMVoucher);

    // -------------------------------------------------------------------------
    // 11. flash.printing.* (PaperSize, PrintMethod, PrintUIOptions,
    //                       PrintJobOptions, PrintJobOrientation, PrintJob)
    // -------------------------------------------------------------------------
    const PaperSize = defConstClass('PaperSize', {
      A4: 'a4',
      A5: 'a5',
      A6: 'a6',
      CHOUKEI3GOU: 'choukei3gou',
      CHOUKEI4GOU: 'choukei4gou',
      ENV_10: 'env_10',
      ENV_B5: 'env_b5',
      ENV_C5: 'env_c5',
      ENV_DL: 'env_dl',
      ENV_MONARCH: 'env_monarch',
      ENV_PERSONAL: 'env_personal',
      EXECUTIVE: 'executive',
      FOLIO: 'folio',
      JIS_B5: 'jis_b5',
      LEGAL: 'legal',
      LETTER: 'letter',
      STATEMENT: 'statement',
    });

    const PAPER_DIMENSIONS_PT = {
      letter: [612, 792],
      legal: [612, 1008],
      a4: [595, 842],
      a5: [420, 595],
      a6: [298, 420],
      executive: [522, 756],
      folio: [612, 936],
      statement: [396, 612],
      jis_b5: [516, 729],
      env_10: [297, 684],
      env_b5: [499, 709],
      env_c5: [459, 649],
      env_dl: [312, 624],
      env_monarch: [279, 540],
      env_personal: [261, 468],
      choukei3gou: [340, 666],
      choukei4gou: [255, 581],
    };

    const PrintMethod = defConstClass('PrintMethod', {
      AUTO: 'auto',
      BITMAP: 'bitmap',
      VECTOR: 'vector',
    });

    const PrintJobOrientation = defConstClass('PrintJobOrientation', {
      LANDSCAPE: 'landscape',
      PORTRAIT: 'portrait',
    });

    class PrintUIOptions {
      constructor() {
        this.disablePageRange = false;
        this.minPage = 1;
        this.maxPage = 9999;
      }
    }
    tagType(PrintUIOptions, 'PrintUIOptions', v => v instanceof PrintUIOptions);

    class PrintJobOptions {
      constructor(printAsBitmap = false) {
        this.printAsBitmap = Boolean(printAsBitmap);
        this.pixelsPerInch = Number.NaN;
        this.printMethod = 'auto';
      }
    }
    tagType(PrintJobOptions, 'PrintJobOptions', v => v instanceof PrintJobOptions);

    let printJobActiveFlag = false;
    const printJobStates = new WeakMap();
    function pjState(receiver) {
      const s = printJobStates.get(receiver);
      if (!s) throw new TypeError('Expected a PrintJob instance.');
      return s;
    }

    class PrintJob extends BaseDispatcher {
      constructor() {
        super();
        printJobStates.set(this, {
          active: false,
          copies: 1,
          firstPage: 1,
          lastPage: 1,
          isColor: true,
          jobName: 'NeoFlash Print Job',
          maxPixelsPerInch: 600,
          orientation: 'portrait',
          paperWidth: 612,
          paperHeight: 792,
          margin: 18,
          printer: 'PDF Virtual Printer',
          pages: [],
        });
      }
      static get isSupported() { return true; }
      static get active() { return printJobActiveFlag; }
      static get printers() {
        return makeVector('String', ['PDF Virtual Printer', 'System Default Printer']);
      }
      static get supportsPageSetupDialog() { return true; }

      get active() { return printJobActiveFlag; }
      get printers() { return PrintJob.printers; }
      get supportsPageSetupDialog() { return true; }
      get papers() {
        return Object.keys(PAPER_DIMENSIONS_PT);
      }

      get copies() { return pjState(this).copies; }
      set copies(v) {
        const n = Number(v) | 0;
        if (n < 1) throw argErr('PrintJob.copies must be >= 1.');
        pjState(this).copies = n;
      }
      get firstPage() { return pjState(this).firstPage; }
      get lastPage() { return pjState(this).lastPage; }
      get isColor() { return pjState(this).isColor; }
      get jobName() { return pjState(this).jobName; }
      set jobName(v) { pjState(this).jobName = v == null ? '' : String(v); }
      get maxPixelsPerInch() { return pjState(this).maxPixelsPerInch; }
      get orientation() { return pjState(this).orientation; }
      set orientation(v) {
        const o = String(v ?? 'portrait');
        if (o !== 'portrait' && o !== 'landscape') {
          throw argErr(`Invalid PrintJobOrientation '${o}'.`);
        }
        pjState(this).orientation = o;
      }
      get paperWidth() {
        const s = pjState(this);
        return s.orientation === 'landscape' ? s.paperHeight : s.paperWidth;
      }
      get paperHeight() {
        const s = pjState(this);
        return s.orientation === 'landscape' ? s.paperWidth : s.paperHeight;
      }
      get pageWidth() {
        const s = pjState(this);
        return Math.max(0, this.paperWidth - s.margin * 2);
      }
      get pageHeight() {
        const s = pjState(this);
        return Math.max(0, this.paperHeight - s.margin * 2);
      }
      get paperArea() {
        return makeRect(0, 0, this.paperWidth, this.paperHeight);
      }
      get printableArea() {
        const s = pjState(this);
        return makeRect(s.margin, s.margin, this.pageWidth, this.pageHeight);
      }
      get printer() { return pjState(this).printer; }
      set printer(v) {
        const s = pjState(this);
        if (s.active) throw illegalOp('Cannot change printer while a PrintJob is active.');
        s.printer = String(v ?? 'PDF Virtual Printer');
      }

      selectPaperSize(paperSize) {
        const s = pjState(this);
        const key = String(paperSize ?? '').toLowerCase();
        const dims = PAPER_DIMENSIONS_PT[key];
        if (!dims) throw argErr(`Invalid PaperSize '${paperSize}'.`);
        s.paperWidth = dims[0];
        s.paperHeight = dims[1];
      }
      showPageSetupDialog() {
        const s = pjState(this);
        if (s.active) throw illegalOp('Cannot show page setup dialog while a PrintJob is active.');
        return true;
      }
      start() {
        const s = pjState(this);
        if (s.active || printJobActiveFlag) {
          throw illegalOp('A PrintJob is already active.');
        }
        s.active = true;
        s.pages.length = 0;
        printJobActiveFlag = true;
        return true;
      }
      start2(uiOptions = null, _showPrintDialog = true) {
        const s = pjState(this);
        if (uiOptions !== null && !(uiOptions instanceof PrintUIOptions)) {
          throw argErr('PrintJob.start2 requires PrintUIOptions or null.');
        }
        if (uiOptions) {
          if (uiOptions.minPage < 1 || uiOptions.maxPage < uiOptions.minPage) {
            throw argErr('PrintUIOptions minPage/maxPage range is invalid.');
          }
          s.firstPage = uiOptions.minPage;
          s.lastPage = uiOptions.maxPage;
        }
        return this.start();
      }
      addPage(sprite, printArea = null, options = null, frameNum = 0) {
        const s = pjState(this);
        if (!s.active) throw illegalOp('PrintJob.start() or start2() must be called before addPage().');
        if (!sprite || typeof sprite !== 'object') {
          throw argErr('PrintJob.addPage requires a Sprite.');
        }
        if (options !== null && !(options instanceof PrintJobOptions)) {
          throw argErr('PrintJob.addPage options must be a PrintJobOptions or null.');
        }
        s.pages.push({
          sprite,
          printArea: printArea
            ? makeRect(printArea.x, printArea.y, printArea.width, printArea.height)
            : this.printableArea,
          options,
          frameNum: Number(frameNum) | 0,
        });
        s.lastPage = Math.max(s.firstPage, s.pages.length);
      }
      send() {
        const s = pjState(this);
        if (!s.active) throw illegalOp('PrintJob is not active.');
        s.active = false;
        printJobActiveFlag = false;
      }
      terminate() {
        const s = pjState(this);
        s.active = false;
        s.pages.length = 0;
        printJobActiveFlag = false;
      }
    }
    tagType(PrintJob, 'PrintJob', v => printJobStates.has(v));

    // -------------------------------------------------------------------------
    // 12. flash.security.* (IURIDereferencer, ReferencesValidationSetting,
    //     RevocationCheckSettings, SignatureStatus, SignerTrustSettings,
    //     XMLSignatureValidator)
    // -------------------------------------------------------------------------
    const IURIDereferencer = Object.freeze(
      (() => {
        const check = v => v != null && typeof v === 'object' && typeof v.dereference === 'function';
        const iface = function IURIDereferencer(value) {
          if (new.target) {
            throw new TypeError('IURIDereferencer is an interface and cannot be constructed.');
          }
          if (arguments.length !== 1) {
            throw new TypeError('IURIDereferencer conversion expects one value.');
          }
          if (value == null) return null;
          if (check(value)) return value;
          throw new TypeError('AS3 flash.security.IURIDereferencer value required.');
        };
        Object.defineProperties(iface, {
          __nfIs: { value: check },
          __nfInitialize: {
            value: () => {
              throw new TypeError('IURIDereferencer is an interface and cannot be extended by a class.');
            },
          },
          [Symbol.hasInstance]: { value: check },
        });
        return iface;
      })()
    );

    const ReferencesValidationSetting = defConstClass('ReferencesValidationSetting', {
      NEVER: 'never',
      VALID_IDENTITY: 'validIdentity',
      VALID_OR_UNKNOWN_IDENTITY: 'validOrUnknownIdentity',
    });

    const RevocationCheckSettings = defConstClass('RevocationCheckSettings', {
      ALWAYS_REQUIRED: 'alwaysRequired',
      BEST_EFFORT: 'bestEffort',
      NEVER: 'never',
      REQUIRED_IF_AVAILABLE: 'requiredIfAvailable',
    });

    const SignatureStatus = defConstClass('SignatureStatus', {
      INVALID: 'invalid',
      UNKNOWN: 'unknown',
      VALID: 'valid',
    });

    const SignerTrustSettings = defConstClass('SignerTrustSettings', {
      CODE_SIGNING: 'codeSigning',
      PLAYLIST_SIGNING: 'playlistSigning',
      SIGNING: 'signing',
    });

    function bytesToBase64(bytes) {
      let binaryStr = '';
      for (let i = 0; i < bytes.length; i++) {
        binaryStr += String.fromCharCode(bytes[i] & 0xff);
      }
      if (typeof global.btoa === 'function') return global.btoa(binaryStr);
      if (typeof Buffer !== 'undefined') return Buffer.from(bytes).toString('base64');
      return '';
    }

    function readDataInputBytes(input) {
      if (!input) return new Uint8Array(0);
      if (binary?.ByteArray?.__nfIs?.(input)) {
        const pos = Number(input.position) >>> 0;
        const all = binary.toBytes(input);
        input.position = all.length;
        return all.subarray(pos);
      }
      if (input instanceof Uint8Array) return input;
      if (typeof input.bytesAvailable === 'number' && typeof input.readUnsignedByte === 'function') {
        const len = input.bytesAvailable >>> 0;
        const out = new Uint8Array(len);
        for (let i = 0; i < len; i++) out[i] = input.readUnsignedByte() & 0xff;
        return out;
      }
      if (typeof input.readUTFBytes === 'function' && typeof input.bytesAvailable === 'number') {
        const str = input.readUTFBytes(input.bytesAvailable);
        return new TextEncoder().encode(str);
      }
      return new Uint8Array(0);
    }

    const xmlSigStates = new WeakMap();
    function xsState(receiver) {
      const s = xmlSigStates.get(receiver);
      if (!s) throw new TypeError('Expected an XMLSignatureValidator instance.');
      return s;
    }

    class XMLSignatureValidator extends BaseDispatcher {
      constructor() {
        ensureActive();
        super();
        xmlSigStates.set(this, {
          digestStatus: 'unknown',
          identityStatus: 'unknown',
          referencesStatus: 'unknown',
          validityStatus: 'unknown',
          referencesValidationSetting: 'validIdentity',
          revocationCheckSetting: 'bestEffort',
          signerCN: '',
          signerDN: '',
          signerExtendedKeyUsages: [],
          signerTrustSettings: [],
          uriDereferencer: null,
          useSystemTrustStore: false,
        });
      }
      static get isSupported() { return true; }
      get digestStatus() { return xsState(this).digestStatus; }
      get identityStatus() { return xsState(this).identityStatus; }
      get referencesStatus() { return xsState(this).referencesStatus; }
      get validityStatus() { return xsState(this).validityStatus; }
      get referencesValidationSetting() { return xsState(this).referencesValidationSetting; }
      set referencesValidationSetting(v) {
        const val = String(v ?? '');
        if (!['never', 'validIdentity', 'validOrUnknownIdentity'].includes(val)) {
          throw argErr(`Invalid ReferencesValidationSetting '${val}'.`);
        }
        xsState(this).referencesValidationSetting = val;
      }
      get revocationCheckSetting() { return xsState(this).revocationCheckSetting; }
      set revocationCheckSetting(v) {
        const val = String(v ?? '');
        if (!['alwaysRequired', 'bestEffort', 'never', 'requiredIfAvailable'].includes(val)) {
          throw argErr(`Invalid RevocationCheckSettings '${val}'.`);
        }
        xsState(this).revocationCheckSetting = val;
      }
      get signerCN() { return xsState(this).signerCN; }
      get signerDN() { return xsState(this).signerDN; }
      get signerExtendedKeyUsages() { return xsState(this).signerExtendedKeyUsages.slice(); }
      get signerTrustSettings() { return xsState(this).signerTrustSettings.slice(); }
      get uriDereferencer() { return xsState(this).uriDereferencer; }
      set uriDereferencer(v) {
        if (v !== null && typeof v?.dereference !== 'function') {
          throw new TypeError('XMLSignatureValidator.uriDereferencer requires an IURIDereferencer.');
        }
        xsState(this).uriDereferencer = v;
      }
      get useSystemTrustStore() { return xsState(this).useSystemTrustStore; }
      set useSystemTrustStore(v) { xsState(this).useSystemTrustStore = Boolean(v); }

      addCertificate(cert, trusted = true) {
        xsState(this);
        let bytes;
        if (binary?.ByteArray?.__nfIs?.(cert)) bytes = binary.toBytes(cert);
        else if (cert instanceof Uint8Array) bytes = cert;
        else throw argErr('XMLSignatureValidator.addCertificate requires a ByteArray.');
        if (bytes.length === 0) throw argErr('Certificate ByteArray cannot be empty.');
        callKernel('xml_sig_add_cert', bytes, Boolean(trusted));
      }

      verify(signature) {
        const s = xsState(this);
        if (!s.uriDereferencer || typeof s.uriDereferencer.dereference !== 'function') {
          throw illegalOp('XMLSignatureValidator.uriDereferencer must be set before calling verify().');
        }
        if (signature == null) {
          throw argErr('XMLSignatureValidator.verify requires an XML signature.');
        }
        const sigText =
          typeof signature.toXMLString === 'function'
            ? signature.toXMLString()
            : String(signature);
        // Extract Reference URI attributes and dereference via uriDereferencer
        const derefMap = {};
        const refRegex = /<\s*(?:[A-Za-z0-9_-]+:)?Reference\b[^>]*?\bURI\s*=\s*(?:"([^"]*)"|'([^']*)')/gi;
        let match;
        while ((match = refRegex.exec(sigText)) !== null) {
          const uri = match[1] ?? match[2] ?? '';
          if (!(uri in derefMap)) {
            const dataInput = s.uriDereferencer.dereference(uri);
            if (dataInput != null) {
              const rawBytes = readDataInputBytes(dataInput);
              derefMap[uri] = bytesToBase64(rawBytes);
            }
          }
        }
        try {
          const res = JSON.parse(
            callKernel(
              'xml_sig_verify_json',
              sigText,
              JSON.stringify(derefMap),
              s.referencesValidationSetting,
              s.revocationCheckSetting,
              s.useSystemTrustStore
            )
          );
          if (res.error) {
            s.digestStatus = 'invalid';
            s.referencesStatus = 'invalid';
            s.identityStatus = 'unknown';
            s.validityStatus = 'invalid';
            this.dispatchEvent(new BaseErrorEvent('error', false, false, String(res.error)));
            return;
          }
          s.digestStatus = String(res.digestStatus || 'unknown');
          s.referencesStatus = String(res.referencesStatus || 'unknown');
          s.identityStatus = String(res.identityStatus || 'unknown');
          s.validityStatus = String(res.validityStatus || 'unknown');
          s.signerCN = String(res.signerCN || '');
          s.signerDN = String(res.signerDN || '');
          s.signerExtendedKeyUsages = Array.isArray(res.signerExtendedKeyUsages)
            ? res.signerExtendedKeyUsages.slice()
            : [];
          s.signerTrustSettings = Array.isArray(res.signerTrustSettings)
            ? res.signerTrustSettings.slice()
            : [];
          this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
        } catch (err) {
          this.dispatchEvent(new BaseErrorEvent('error', false, false, String(err.message)));
        }
      }
    }
    tagType(XMLSignatureValidator, 'XMLSignatureValidator', v => xmlSigStates.has(v));

    // -------------------------------------------------------------------------
    // 13. flash.external.ExtensionContext
    // -------------------------------------------------------------------------
    const extContextStates = new WeakMap();
    const extContextToken = Symbol('ExtensionContext');
    class ExtensionContext extends BaseDispatcher {
      constructor(token, extensionID, contextType) {
        if (token !== extContextToken) {
          throw argErr('Use ExtensionContext.createExtensionContext() to create an ExtensionContext.');
        }
        super();
        extContextStates.set(this, {
          extensionID: String(extensionID ?? ''),
          contextType: contextType == null ? null : String(contextType),
          actionScriptData: null,
          disposed: false,
        });
      }
      static createExtensionContext(extensionID, contextType = null) {
        ensureActive();
        if (extensionID == null || String(extensionID).trim() === '') {
          throw argErr('ExtensionContext.createExtensionContext requires a non-empty extensionID.');
        }
        return new ExtensionContext(extContextToken, String(extensionID), contextType);
      }
      static getExtensionDirectory(extensionID) {
        ensureActive();
        if (extensionID == null || String(extensionID).trim() === '') {
          throw argErr('ExtensionContext.getExtensionDirectory requires a non-empty extensionID.');
        }
        const safeId = String(extensionID).replace(/[^A-Za-z0-9._-]/g, '_');
        return fileFromNativePath(`/app/META-INF/AIR/extensions/${safeId}`);
      }
      get actionScriptData() {
        const s = extContextStates.get(this);
        if (!s || s.disposed) throw illegalOp('ExtensionContext has been disposed.');
        return s.actionScriptData;
      }
      set actionScriptData(v) {
        const s = extContextStates.get(this);
        if (!s || s.disposed) throw illegalOp('ExtensionContext has been disposed.');
        s.actionScriptData = v;
      }
      call(functionName, ...args) {
        const s = extContextStates.get(this);
        if (!s || s.disposed) throw illegalOp('ExtensionContext has been disposed.');
        if (functionName == null || String(functionName) === '') {
          throw argErr('ExtensionContext.call requires a non-empty functionName.');
        }
        const fn = String(functionName);
        if (fn === 'dispatchStatus') {
          const code = String(args[0] ?? 'STATUS');
          const level = String(args[1] ?? 'status');
          const StatusEv = options.eventFamilies?.StatusEvent;
          if (StatusEv) {
            this.dispatchEvent(new StatusEv('status', false, false, code, level));
          } else {
            const ev = new BaseEvent('status', false, false);
            ev.code = code;
            ev.level = level;
            this.dispatchEvent(ev);
          }
          return true;
        }
        if (fn === 'sum') {
          return args.reduce((acc, v) => acc + (Number(v) || 0), 0);
        }
        if (fn === 'getContextInfo') {
          return { extensionID: s.extensionID, contextType: s.contextType };
        }
        return args.length === 0 ? fn : args.length === 1 ? args[0] : args;
      }
      dispose() {
        const s = extContextStates.get(this);
        if (!s || s.disposed) return;
        s.disposed = true;
        s.actionScriptData = null;
      }
    }
    tagType(ExtensionContext, 'ExtensionContext', v => extContextStates.has(v));

    // -------------------------------------------------------------------------
    // 14. flash.text.* (AutoCapitalize, ReturnKeyLabel, SoftKeyboardType,
    //                   StageTextClearButtonMode, StageTextInitOptions, StageText)
    // -------------------------------------------------------------------------
    const AutoCapitalize = defConstClass('AutoCapitalize', {
      ALL: 'all',
      NONE: 'none',
      SENTENCE: 'sentence',
      WORD: 'word',
    });

    const ReturnKeyLabel = defConstClass('ReturnKeyLabel', {
      DEFAULT: 'default',
      DONE: 'done',
      GO: 'go',
      NEXT: 'next',
      SEARCH: 'search',
    });

    const SoftKeyboardType = defConstClass('SoftKeyboardType', {
      CONTACT: 'contact',
      DECIMAL: 'decimal',
      DEFAULT: 'default',
      EMAIL: 'email',
      NUMBER: 'number',
      PHONE: 'phone',
      PUNCTUATION: 'punctuation',
      URL: 'url',
    });

    const StageTextClearButtonMode = defConstClass('StageTextClearButtonMode', {
      ALWAYS: 'always',
      NEVER: 'never',
      UNLESS_EDITING: 'unlessEditing',
      WHILE_EDITING: 'whileEditing',
    });

    class StageTextInitOptions {
      constructor(multiline = false) {
        this.multiline = Boolean(multiline);
      }
    }
    tagType(StageTextInitOptions, 'StageTextInitOptions', v => v instanceof StageTextInitOptions);

    function applyStageTextRestrict(input, restrict) {
      if (restrict === null || restrict === undefined) return input;
      const spec = String(restrict);
      if (spec === '') return '';
      const allowed = new Set();
      const denied = new Set();
      let negate = false;
      let i = 0;
      while (i < spec.length) {
        let ch = spec[i];
        if (ch === '^' && i === 0) {
          negate = true;
          i++;
          continue;
        }
        if (ch === '^') {
          negate = !negate;
          i++;
          continue;
        }
        if (ch === '\\' && i + 1 < spec.length) {
          ch = spec[++i];
        }
        if (i + 2 < spec.length && spec[i + 1] === '-') {
          let endCh = spec[i + 2];
          i += 3;
          if (endCh === '\\' && i < spec.length) {
            endCh = spec[i++];
          }
          const startCode = ch.charCodeAt(0);
          const endCode = endCh.charCodeAt(0);
          const lo = Math.min(startCode, endCode);
          const hi = Math.max(startCode, endCode);
          for (let c = lo; c <= hi; c++) {
            const s = String.fromCharCode(c);
            if (negate) denied.add(s);
            else allowed.add(s);
          }
        } else {
          if (negate) denied.add(ch);
          else allowed.add(ch);
          i++;
        }
      }
      const hasPositive = allowed.size > 0;
      let out = '';
      for (const ch of input) {
        if (denied.has(ch)) continue;
        if (hasPositive && !allowed.has(ch)) continue;
        out += ch;
      }
      return out;
    }

    const stageTextStates = new WeakMap();
    function stState(receiver) {
      const s = stageTextStates.get(receiver);
      if (!s) throw new TypeError('Expected a StageText instance.');
      if (s.disposed) throw illegalOp('StageText has been disposed.');
      return s;
    }

    function syncStageTextDom(receiver, s) {
      if (!s) return;
      if (!s.domElement) {
        const tag = s.multiline ? 'textarea' : 'input';
        const realEl =
          typeof global.document?.createElement === 'function'
            ? global.document.createElement(tag)
            : null;
        const el = realEl || {
          tagName: tag.toUpperCase(),
          style: {},
          dataset: {},
          setAttribute(k, v) {
            this[k] = String(v);
          },
          focus() {
            receiver.assignFocus();
          },
          blur() {
            receiver._blurFocus();
          },
          setSelectionRange(start, end) {
            this.selectionStart = start;
            this.selectionEnd = end;
          },
        };
        if (realEl && typeof realEl.addEventListener === 'function') {
          realEl.addEventListener('input', () => {
            if (s.disposed) return;
            receiver.text = realEl.value;
            receiver.dispatchEvent(new BaseEvent(BaseEvent.CHANGE));
          });
          realEl.addEventListener('blur', () => {
            if (!s.disposed) receiver._blurFocus();
          });
        }
        s.domElement = el;
      }
      const el = s.domElement;
      if (!s.multiline) {
        el.type = s.displayAsPassword
          ? 'password'
          : s.softKeyboardType === 'email'
            ? 'email'
            : s.softKeyboardType === 'url'
              ? 'url'
              : s.softKeyboardType === 'phone'
                ? 'tel'
                : 'text';
      }
      el.value = s.text;
      el.readOnly = !s.editable;
      el.maxLength = s.maxChars > 0 ? s.maxChars : -1;
      el.lang = s.locale || 'en';
      el.autocapitalize =
        s.autoCapitalize === 'all'
          ? 'characters'
          : s.autoCapitalize === 'word'
            ? 'words'
            : s.autoCapitalize === 'sentence'
              ? 'sentences'
              : 'off';
      el.autocorrect = s.autoCorrect ? 'on' : 'off';
      el.enterKeyHint = s.returnKeyLabel === 'default' ? '' : s.returnKeyLabel;
      el.inputMode = s.softKeyboardType === 'default' ? 'text' : s.softKeyboardType;
      el.selectionStart = Math.min(s.selectionAnchorIndex, s.selectionActiveIndex);
      el.selectionEnd = Math.max(s.selectionAnchorIndex, s.selectionActiveIndex);
      if (el.style) {
        el.style.position = 'absolute';
        el.style.left = `${s.viewPort ? s.viewPort.x : 0}px`;
        el.style.top = `${s.viewPort ? s.viewPort.y : 0}px`;
        el.style.width = `${s.viewPort ? s.viewPort.width : 0}px`;
        el.style.height = `${s.viewPort ? s.viewPort.height : 0}px`;
        el.style.fontFamily = s.fontFamily || 'sans-serif';
        el.style.fontSize = `${s.fontSize}px`;
        el.style.fontWeight = s.fontWeight;
        el.style.fontStyle = s.fontPosture;
        el.style.color = `#${(s.color >>> 0).toString(16).padStart(6, '0')}`;
        el.style.textAlign = s.textAlign;
        el.style.display = s.stage && s.visible && s.viewPort && !s.disposed ? 'block' : 'none';
      }
      if (typeof global.document?.body?.appendChild === 'function') {
        if (s.stage && !s.disposed && !el.parentNode) {
          try {
            global.document.body.appendChild(el);
          } catch (_) {}
        } else if ((!s.stage || s.disposed) && el.parentNode) {
          try {
            el.parentNode.removeChild(el);
          } catch (_) {}
        }
      }
    }

    class StageText extends BaseDispatcher {
      constructor(initOptions = null) {
        ensureActive();
        super();
        const multiline =
          initOptions instanceof StageTextInitOptions ? Boolean(initOptions.multiline) : false;
        const st = {
          multiline,
          autoCapitalize: 'none',
          autoCorrect: false,
          clearButtonMode: 'whileEditing',
          color: 0x000000,
          displayAsPassword: false,
          editable: true,
          fontFamily: null,
          fontPosture: 'normal',
          fontSize: 12,
          fontWeight: 'normal',
          locale: 'en',
          maxChars: 0,
          restrict: null,
          returnKeyLabel: 'default',
          selectionActiveIndex: 0,
          selectionAnchorIndex: 0,
          softKeyboardType: 'default',
          stage: null,
          text: '',
          textAlign: 'start',
          viewPort: null,
          visible: true,
          disposed: false,
          domElement: null,
        };
        stageTextStates.set(this, st);
        syncStageTextDom(this, st);
      }
      get _domElement() { return stState(this).domElement; }
      get autoCapitalize() { return stState(this).autoCapitalize; }
      set autoCapitalize(v) {
        const val = String(v ?? 'none');
        if (!['all', 'none', 'sentence', 'word'].includes(val)) {
          throw argErr(`Invalid AutoCapitalize '${val}'.`);
        }
        const s = stState(this);
        s.autoCapitalize = val;
        syncStageTextDom(this, s);
      }
      get autoCorrect() { return stState(this).autoCorrect; }
      set autoCorrect(v) {
        const s = stState(this);
        s.autoCorrect = Boolean(v);
        syncStageTextDom(this, s);
      }
      get clearButtonMode() { return stState(this).clearButtonMode; }
      set clearButtonMode(v) {
        const val = String(v ?? 'whileEditing');
        if (!['always', 'never', 'unlessEditing', 'whileEditing'].includes(val)) {
          throw argErr(`Invalid StageTextClearButtonMode '${val}'.`);
        }
        const s = stState(this);
        s.clearButtonMode = val;
        syncStageTextDom(this, s);
      }
      get color() { return stState(this).color; }
      set color(v) {
        const s = stState(this);
        s.color = (Number(v) >>> 0) & 0xffffff;
        syncStageTextDom(this, s);
      }
      get displayAsPassword() { return stState(this).displayAsPassword; }
      set displayAsPassword(v) {
        const s = stState(this);
        s.displayAsPassword = Boolean(v);
        syncStageTextDom(this, s);
      }
      get editable() { return stState(this).editable; }
      set editable(v) {
        const s = stState(this);
        s.editable = Boolean(v);
        syncStageTextDom(this, s);
      }
      get fontFamily() { return stState(this).fontFamily; }
      set fontFamily(v) {
        const s = stState(this);
        s.fontFamily = v == null ? null : String(v);
        syncStageTextDom(this, s);
      }
      get fontPosture() { return stState(this).fontPosture; }
      set fontPosture(v) {
        const val = String(v ?? 'normal');
        if (val !== 'normal' && val !== 'italic') throw argErr(`Invalid FontPosture '${val}'.`);
        const s = stState(this);
        s.fontPosture = val;
        syncStageTextDom(this, s);
      }
      get fontSize() { return stState(this).fontSize; }
      set fontSize(v) {
        const n = Number(v) | 0;
        if (n <= 0) throw argErr('StageText.fontSize must be positive.');
        const s = stState(this);
        s.fontSize = n;
        syncStageTextDom(this, s);
      }
      get fontWeight() { return stState(this).fontWeight; }
      set fontWeight(v) {
        const val = String(v ?? 'normal');
        if (val !== 'normal' && val !== 'bold') throw argErr(`Invalid FontWeight '${val}'.`);
        const s = stState(this);
        s.fontWeight = val;
        syncStageTextDom(this, s);
      }
      get locale() { return stState(this).locale; }
      set locale(v) {
        const s = stState(this);
        s.locale = String(v ?? 'en');
        syncStageTextDom(this, s);
      }
      get maxChars() { return stState(this).maxChars; }
      set maxChars(v) {
        const s = stState(this);
        s.maxChars = Math.max(0, Number(v) | 0);
        if (s.maxChars > 0 && s.text.length > s.maxChars) {
          s.text = s.text.slice(0, s.maxChars);
        }
        syncStageTextDom(this, s);
      }
      get multiline() { return stState(this).multiline; }
      get restrict() { return stState(this).restrict; }
      set restrict(v) {
        const s = stState(this);
        s.restrict = v == null ? null : String(v);
        if (s.restrict !== null) {
          s.text = applyStageTextRestrict(s.text, s.restrict);
        }
        syncStageTextDom(this, s);
      }
      get returnKeyLabel() { return stState(this).returnKeyLabel; }
      set returnKeyLabel(v) {
        const val = String(v ?? 'default');
        if (!['default', 'done', 'go', 'next', 'search'].includes(val)) {
          throw argErr(`Invalid ReturnKeyLabel '${val}'.`);
        }
        const s = stState(this);
        s.returnKeyLabel = val;
        syncStageTextDom(this, s);
      }
      get selectionActiveIndex() { return stState(this).selectionActiveIndex; }
      get selectionAnchorIndex() { return stState(this).selectionAnchorIndex; }
      get softKeyboardType() { return stState(this).softKeyboardType; }
      set softKeyboardType(v) {
        const val = String(v ?? 'default');
        if (!['contact', 'decimal', 'default', 'email', 'number', 'phone', 'punctuation', 'url'].includes(val)) {
          throw argErr(`Invalid SoftKeyboardType '${val}'.`);
        }
        const s = stState(this);
        s.softKeyboardType = val;
        syncStageTextDom(this, s);
      }
      get stage() { return stState(this).stage; }
      set stage(v) {
        const s = stState(this);
        s.stage = v ?? null;
        syncStageTextDom(this, s);
      }
      get text() { return stState(this).text; }
      set text(v) {
        const s = stState(this);
        let next = v == null ? '' : String(v);
        if (!s.multiline) next = next.replace(/[\r\n]+/g, '');
        if (s.restrict !== null) next = applyStageTextRestrict(next, s.restrict);
        if (s.maxChars > 0 && next.length > s.maxChars) next = next.slice(0, s.maxChars);
        s.text = next;
        s.selectionAnchorIndex = Math.min(s.selectionAnchorIndex, next.length);
        s.selectionActiveIndex = Math.min(s.selectionActiveIndex, next.length);
        syncStageTextDom(this, s);
      }
      get textAlign() { return stState(this).textAlign; }
      set textAlign(v) {
        const val = String(v ?? 'start');
        if (!['start', 'end', 'left', 'right', 'center', 'justify'].includes(val)) {
          throw argErr(`Invalid StageText.textAlign '${val}'.`);
        }
        const s = stState(this);
        s.textAlign = val;
        syncStageTextDom(this, s);
      }
      get viewPort() {
        const vp = stState(this).viewPort;
        return vp ? makeRect(vp.x, vp.y, vp.width, vp.height) : null;
      }
      set viewPort(v) {
        const s = stState(this);
        if (v === null) {
          s.viewPort = null;
          syncStageTextDom(this, s);
          return;
        }
        if (typeof v !== 'object' || Number(v.width) < 0 || Number(v.height) < 0) {
          throw argErr('StageText.viewPort requires a valid Rectangle.');
        }
        s.viewPort = makeRect(
          Number(v.x) || 0,
          Number(v.y) || 0,
          Number(v.width) || 0,
          Number(v.height) || 0
        );
        syncStageTextDom(this, s);
      }
      get visible() { return stState(this).visible; }
      set visible(v) {
        const s = stState(this);
        s.visible = Boolean(v);
        syncStageTextDom(this, s);
      }

      selectRange(anchorIndex, activeIndex) {
        const s = stState(this);
        const len = s.text.length;
        s.selectionAnchorIndex = Math.max(0, Math.min(len, Number(anchorIndex) | 0));
        s.selectionActiveIndex = Math.max(0, Math.min(len, Number(activeIndex) | 0));
        syncStageTextDom(this, s);
      }

      _blurFocus() {
        const s = stageTextStates.get(this);
        if (!s || s.disposed) return;
        if (activeStageText === this) activeStageText = null;
        const SkEv = options.eventFamilies?.SoftKeyboardEvent;
        if (SkEv) {
          this.dispatchEvent(new SkEv('softKeyboardDeactivate', false, false, null, 'contentTriggered'));
        } else {
          this.dispatchEvent(new BaseEvent('softKeyboardDeactivate', false, false));
        }
        const FocusEv = events.FocusEvent || BaseEvent;
        this.dispatchEvent(new FocusEv('focusOut'));
      }

      assignFocus() {
        const s = stState(this);
        if (activeStageText && activeStageText !== this) {
          activeStageText._blurFocus();
        }
        activeStageText = this;
        const SkEv = options.eventFamilies?.SoftKeyboardEvent;
        if (SkEv) {
          const activating = new SkEv('softKeyboardActivating', false, true, null, 'contentTriggered');
          if (this.dispatchEvent(activating)) {
            this.dispatchEvent(new SkEv('softKeyboardActivate', false, false, null, 'contentTriggered'));
          }
        } else {
          this.dispatchEvent(new BaseEvent('softKeyboardActivating', false, true));
          this.dispatchEvent(new BaseEvent('softKeyboardActivate', false, false));
        }
        const FocusEv = events.FocusEvent || BaseEvent;
        this.dispatchEvent(new FocusEv('focusIn'));
        syncStageTextDom(this, s);
      }

      drawViewPortToBitmapData(bitmap) {
        const s = stState(this);
        if (!s.viewPort || s.viewPort.width <= 0 || s.viewPort.height <= 0) {
          throw illegalOp('StageText.viewPort must be set before calling drawViewPortToBitmapData.');
        }
        if (!bitmap || typeof bitmap.setPixel32 !== 'function') {
          throw argErr('StageText.drawViewPortToBitmapData requires a BitmapData.');
        }
        const w = Math.round(s.viewPort.width);
        const h = Math.round(s.viewPort.height);
        if ((Number(bitmap.width) | 0) !== w || (Number(bitmap.height) | 0) !== h) {
          throw argErr('BitmapData dimensions must match StageText.viewPort dimensions.');
        }
        const selPart =
          s.selectionAnchorIndex !== s.selectionActiveIndex
            ? `sel=${s.selectionAnchorIndex},${s.selectionActiveIndex}`
            : '';
        const flagsAlign = [
          s.multiline ? 'multiline' : 'single',
          s.displayAsPassword ? 'password' : '',
          s.textAlign || 'left',
          selPart,
        ]
          .filter(Boolean)
          .join(':');
        const rgba = callKernel(
          'rasterize_text_rgba',
          w,
          h,
          s.text,
          s.color,
          0xffffffff,
          Math.max(8, Math.round(s.fontSize)),
          flagsAlign
        );
        writeRgbaToBitmapData(bitmap, rgba, w, h);
      }

      _editCopy() {
        const s = stState(this);
        const lo = Math.min(s.selectionAnchorIndex, s.selectionActiveIndex);
        const hi = Math.max(s.selectionAnchorIndex, s.selectionActiveIndex);
        if (hi <= lo) return false;
        const selected = s.text.slice(lo, hi);
        Clipboard.generalClipboard.setData(ClipboardFormats.TEXT_FORMAT, selected);
        return true;
      }
      _editCut() {
        const s = stState(this);
        if (!s.editable) return false;
        const lo = Math.min(s.selectionAnchorIndex, s.selectionActiveIndex);
        const hi = Math.max(s.selectionAnchorIndex, s.selectionActiveIndex);
        if (hi <= lo) return false;
        const selected = s.text.slice(lo, hi);
        Clipboard.generalClipboard.setData(ClipboardFormats.TEXT_FORMAT, selected);
        s.text = s.text.slice(0, lo) + s.text.slice(hi);
        s.selectionAnchorIndex = lo;
        s.selectionActiveIndex = lo;
        this.dispatchEvent(new BaseEvent(BaseEvent.CHANGE));
        return true;
      }
      _editPaste() {
        const s = stState(this);
        if (!s.editable) return false;
        const clip = Clipboard.generalClipboard.getData(ClipboardFormats.TEXT_FORMAT);
        if (clip == null) return false;
        const lo = Math.min(s.selectionAnchorIndex, s.selectionActiveIndex);
        const hi = Math.max(s.selectionAnchorIndex, s.selectionActiveIndex);
        const inserted = String(clip);
        this.text = s.text.slice(0, lo) + inserted + s.text.slice(hi);
        const nextCursor = Math.min(s.text.length, lo + inserted.length);
        s.selectionAnchorIndex = nextCursor;
        s.selectionActiveIndex = nextCursor;
        this.dispatchEvent(new BaseEvent(BaseEvent.CHANGE));
        return true;
      }
      _editClear() {
        const s = stState(this);
        if (!s.editable) return false;
        const lo = Math.min(s.selectionAnchorIndex, s.selectionActiveIndex);
        const hi = Math.max(s.selectionAnchorIndex, s.selectionActiveIndex);
        if (hi <= lo) return false;
        s.text = s.text.slice(0, lo) + s.text.slice(hi);
        s.selectionAnchorIndex = lo;
        s.selectionActiveIndex = lo;
        this.dispatchEvent(new BaseEvent(BaseEvent.CHANGE));
        return true;
      }
      _editSelectAll() {
        const s = stState(this);
        s.selectionAnchorIndex = 0;
        s.selectionActiveIndex = s.text.length;
        return true;
      }

      dispose() {
        const s = stageTextStates.get(this);
        if (!s || s.disposed) return;
        s.disposed = true;
        if (activeStageText === this) activeStageText = null;
        syncStageTextDom(this, s);
      }
    }
    tagType(StageText, 'StageText', v => stageTextStates.has(v));

    // -------------------------------------------------------------------------
    // 15. flash.ui.* (ContextMenuBuiltInItems, ContextMenuClipboardItems,
    //                 ContextMenuItem, ContextMenu, GameInputControl,
    //                 GameInputDevice, GameInput, MultitouchInputMode, Multitouch)
    // -------------------------------------------------------------------------
    class ContextMenuBuiltInItems {
      constructor() {
        this.forwardAndBack = true;
        this.loop = true;
        this.play = true;
        this.print = true;
        this.quality = true;
        this.rewind = true;
        this.save = true;
        this.zoom = true;
      }
      clone() {
        const c = new ContextMenuBuiltInItems();
        Object.assign(c, this);
        return c;
      }
    }
    tagType(
      ContextMenuBuiltInItems,
      'ContextMenuBuiltInItems',
      v => v instanceof ContextMenuBuiltInItems
    );

    class ContextMenuClipboardItems {
      constructor() {
        this.clear = false;
        this.copy = false;
        this.cut = false;
        this.paste = false;
        this.selectAll = true;
      }
      clone() {
        const c = new ContextMenuClipboardItems();
        Object.assign(c, this);
        return c;
      }
    }
    tagType(
      ContextMenuClipboardItems,
      'ContextMenuClipboardItems',
      v => v instanceof ContextMenuClipboardItems
    );

    class ContextMenuItem extends NativeMenuItem {
      constructor(
        caption = '',
        separatorBefore = false,
        enabled = true,
        visible = true
      ) {
        super(String(caption ?? ''), false);
        this.enabled = Boolean(enabled);
        this.separatorBefore = Boolean(separatorBefore);
        this.visible = Boolean(visible);
      }
      get caption() { return this.label; }
      set caption(v) { this.label = String(v ?? ''); }
      clone() {
        return new ContextMenuItem(this.caption, this.separatorBefore, this.enabled, this.visible);
      }
    }
    tagType(ContextMenuItem, 'ContextMenuItem', v => v instanceof ContextMenuItem);

    class ContextMenu extends NativeMenu {
      constructor() {
        super();
        this.builtInItems = new ContextMenuBuiltInItems();
        this.clipboardItems = new ContextMenuClipboardItems();
        this.clipboardMenu = false;
        this.customItems = [];
        this.link = null;
      }
      static get isSupported() { return true; }
      hideBuiltInItems() {
        this.builtInItems.forwardAndBack = false;
        this.builtInItems.loop = false;
        this.builtInItems.play = false;
        this.builtInItems.print = false;
        this.builtInItems.quality = false;
        this.builtInItems.rewind = false;
        this.builtInItems.save = false;
        this.builtInItems.zoom = false;
      }
      clone() {
        const copy = new ContextMenu();
        copy.builtInItems = this.builtInItems.clone();
        copy.clipboardItems = this.clipboardItems.clone();
        copy.clipboardMenu = this.clipboardMenu;
        copy.customItems = this.customItems.slice();
        for (const item of this.items) {
          copy.addItem(item.clone());
        }
        return copy;
      }
    }
    tagType(ContextMenu, 'ContextMenu', v => v instanceof ContextMenu);

    // GameInput, GameInputDevice, GameInputControl
    const gameInputControlStates = new WeakMap();
    const gameInputDeviceStates = new WeakMap();
    const gameInputToken = Symbol('GameInput');

    class GameInputControl extends BaseDispatcher {
      constructor(token, device, id, minValue = -1, maxValue = 1, initialValue = 0) {
        if (token !== gameInputToken) {
          throw argErr('GameInputControl cannot be constructed directly.');
        }
        super();
        gameInputControlStates.set(this, {
          device,
          id: String(id),
          minValue: Number(minValue),
          maxValue: Number(maxValue),
          value: Number(initialValue),
        });
      }
      get device() { return gameInputControlStates.get(this).device; }
      get id() { return gameInputControlStates.get(this).id; }
      get minValue() { return gameInputControlStates.get(this).minValue; }
      get maxValue() { return gameInputControlStates.get(this).maxValue; }
      get value() { return gameInputControlStates.get(this).value; }
      _setValue(v) {
        const s = gameInputControlStates.get(this);
        const clamped = Math.max(s.minValue, Math.min(s.maxValue, Number(v) || 0));
        s.value = clamped;
        const devState = gameInputDeviceStates.get(s.device);
        if (devState && devState.enabled && devState.caching) {
          if (!devState.cachedControlIds || devState.cachedControlIds.includes(s.id)) {
            devState.samples.push(clamped);
          }
        }
        this.dispatchEvent(new BaseEvent(BaseEvent.CHANGE));
      }
    }
    tagType(GameInputControl, 'GameInputControl', v => gameInputControlStates.has(v));

    class GameInputDevice {
      constructor(token, id = 'gamepad-0', name = 'Standard Gamepad') {
        if (token !== gameInputToken) {
          throw argErr('GameInputDevice cannot be constructed directly.');
        }
        const controls = [
          new GameInputControl(gameInputToken, this, 'AXIS_0', -1, 1, 0),
          new GameInputControl(gameInputToken, this, 'AXIS_1', -1, 1, 0),
          new GameInputControl(gameInputToken, this, 'BUTTON_0', 0, 1, 0),
          new GameInputControl(gameInputToken, this, 'BUTTON_1', 0, 1, 0),
        ];
        gameInputDeviceStates.set(this, {
          id: String(id),
          name: String(name),
          enabled: false,
          sampleInterval: 16,
          controls,
          caching: false,
          numCachedSamples: 0,
          cachedControlIds: null,
          samples: [],
        });
      }
      get id() { return gameInputDeviceStates.get(this).id; }
      get name() { return gameInputDeviceStates.get(this).name; }
      get enabled() { return gameInputDeviceStates.get(this).enabled; }
      set enabled(v) { gameInputDeviceStates.get(this).enabled = Boolean(v); }
      get numControls() { return gameInputDeviceStates.get(this).controls.length; }
      get sampleInterval() { return gameInputDeviceStates.get(this).sampleInterval; }
      set sampleInterval(v) {
        const n = Number(v) | 0;
        if (n < 0) throw argErr('GameInputDevice.sampleInterval must be non-negative.');
        gameInputDeviceStates.get(this).sampleInterval = n;
      }
      getControlAt(i) {
        const s = gameInputDeviceStates.get(this);
        const idx = Number(i) | 0;
        if (idx < 0 || idx >= s.controls.length) {
          throw new RangeError(`GameInputDevice.getControlAt(${idx}) is out of bounds.`);
        }
        return s.controls[idx];
      }
      startCachingSamples(numSamples, controls = null) {
        const s = gameInputDeviceStates.get(this);
        if (!s.enabled) {
          throw illegalOp('GameInputDevice must be enabled before calling startCachingSamples.');
        }
        const count = Number(numSamples) | 0;
        if (count <= 0 || count > GameInputDevice.MAX_BUFFER_SIZE) {
          throw argErr('startCachingSamples numSamples must be between 1 and MAX_BUFFER_SIZE.');
        }
        s.caching = true;
        s.numCachedSamples = count;
        s.cachedControlIds = controls ? Array.from(controls, String) : null;
        s.samples = [];
        // Snapshot initial control values into cache buffer
        const activeControls = s.cachedControlIds
          ? s.controls.filter(c => s.cachedControlIds.includes(c.id))
          : s.controls;
        for (const ctrl of activeControls) {
          s.samples.push(ctrl.value);
        }
      }
      stopCachingSamples() {
        const s = gameInputDeviceStates.get(this);
        s.caching = false;
      }
      getCachedSamples(data, append = false) {
        const s = gameInputDeviceStates.get(this);
        if (!data || typeof data.writeFloat !== 'function') {
          throw argErr('GameInputDevice.getCachedSamples requires a ByteArray.');
        }
        if (!append) {
          data.length = 0;
          data.position = 0;
        }
        const count = s.samples.length;
        for (let i = 0; i < count; i++) {
          data.writeFloat(s.samples[i]);
        }
        s.samples.length = 0;
        return count;
      }
    }
    GameInputDevice.MAX_BUFFER_SIZE = 32000;
    tagType(GameInputDevice, 'GameInputDevice', v => gameInputDeviceStates.has(v));

    let gameInputDevices = null;
    function ensureGameInputDevices() {
      if (!gameInputDevices) {
        gameInputDevices = [new GameInputDevice(gameInputToken, 'gamepad-0', 'NeoFlash Virtual Gamepad')];
      }
      return gameInputDevices;
    }

    class GameInput extends BaseDispatcher {
      constructor() {
        super();
        ensureGameInputDevices();
      }
      static get isSupported() { return true; }
      static get numDevices() { return ensureGameInputDevices().length; }
      static getDeviceAt(index) {
        const list = ensureGameInputDevices();
        const idx = Number(index) | 0;
        if (idx < 0 || idx >= list.length) {
          throw new RangeError(`GameInput.getDeviceAt(${idx}) is out of bounds.`);
        }
        return list[idx];
      }
    }
    tagType(GameInput, 'GameInput', v => v instanceof GameInput);

    const MultitouchInputMode = defConstClass('MultitouchInputMode', {
      GESTURE: 'gesture',
      NONE: 'none',
      TOUCH_POINT: 'touchPoint',
    });

    let multitouchState = {
      inputMode: 'gesture',
      mapTouchToMouse: true,
    };
    const Multitouch = {
      Multitouch: function () {
        throw new TypeError('Multitouch supplies static properties only.');
      },
    }.Multitouch;
    Object.defineProperties(Multitouch, {
      inputMode: {
        get: () => multitouchState.inputMode,
        set: v => {
          const mode = String(v ?? 'gesture');
          if (!['gesture', 'none', 'touchPoint'].includes(mode)) {
            throw argErr(`Invalid MultitouchInputMode '${mode}'.`);
          }
          multitouchState.inputMode = mode;
        },
        enumerable: true,
      },
      mapTouchToMouse: {
        get: () => multitouchState.mapTouchToMouse,
        set: v => { multitouchState.mapTouchToMouse = Boolean(v); },
        enumerable: true,
      },
      maxTouchPoints: { get: () => 5, enumerable: true },
      supportsGestureEvents: { get: () => true, enumerable: true },
      supportsTouchEvents: { get: () => true, enumerable: true },
      supportedGestures: {
        get: () =>
          makeVector('String', [
            'gesturePan',
            'gestureRotate',
            'gestureSwipe',
            'gestureZoom',
            'gestureTwoFingerTap',
            'gesturePressAndTap',
            'gestureDirectionalTap',
          ]),
        enumerable: true,
      },
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError('Multitouch cannot be extended.');
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    Object.freeze(Multitouch);

    // -------------------------------------------------------------------------
    // 16. flash.display3D.* & flash.system.*
    // -------------------------------------------------------------------------
    const Context3DFillMode = defConstClass('Context3DFillMode', {
      SOLID: 'solid',
      WIREFRAME: 'wireframe',
    });

    const Context3DProfile = defConstClass('Context3DProfile', {
      BASELINE: 'baseline',
      BASELINE_CONSTRAINED: 'baselineConstrained',
      BASELINE_EXTENDED: 'baselineExtended',
      ENHANCED: 'enhanced',
      STANDARD: 'standard',
      STANDARD_CONSTRAINED: 'standardConstrained',
      STANDARD_EXTENDED: 'standardExtended',
    });

    class IndexBuffer3D {
      constructor(numIndices = 0, bufferUsage = 'staticDraw') {
        this.numIndices = Number(numIndices) | 0;
        this.bufferUsage = String(bufferUsage);
        this.disposed = false;
      }
      uploadFromVector(_data, _startOffset, _count) {}
      uploadFromByteArray(_data, _byteArrayOffset, _startOffset, _count) {}
      dispose() { this.disposed = true; }
    }
    tagType(IndexBuffer3D, 'IndexBuffer3D', v => v instanceof IndexBuffer3D);

    class VertexBuffer3D {
      constructor(numVertices = 0, data32PerVertex = 1, dataPerInstance = 0, bufferUsage = 'staticDraw') {
        this.numVertices = Number(numVertices) | 0;
        this.data32PerVertex = Number(data32PerVertex) | 0;
        this.dataPerInstance = Number(dataPerInstance) | 0;
        this.bufferUsage = String(bufferUsage);
        this.disposed = false;
      }
      uploadFromVector(_data, _startVertex, _numVertices) {}
      uploadFromByteArray(_data, _byteArrayOffset, _startVertex, _numVertices) {}
      dispose() { this.disposed = true; }
    }
    tagType(VertexBuffer3D, 'VertexBuffer3D', v => v instanceof VertexBuffer3D);

    class TextureBase extends BaseDispatcher {
      constructor(width = 1, height = 1, format = 'bgra') {
        super();
        this.width = Number(width) | 0;
        this.height = Number(height) | 0;
        this.format = String(format);
        this.disposed = false;
      }
      dispose() { this.disposed = true; }
    }
    tagType(TextureBase, 'TextureBase', v => v instanceof TextureBase);

    class Texture extends TextureBase {
      constructor(width = 1, height = 1, format = 'bgra', _optimizeForRenderToTexture = false, _streamingLevels = 0) {
        super(width, height, format);
      }
      uploadFromBitmapData(source, _miplevel = 0) {
        if (!source || typeof source.width !== 'number' || typeof source.height !== 'number') {
          throw argErr('Texture.uploadFromBitmapData requires a BitmapData.');
        }
      }
      uploadFromByteArray(data, byteArrayOffset = 0, _miplevel = 0) {
        if (!data) throw argErr('Texture.uploadFromByteArray requires a ByteArray.');
        const off = Number(byteArrayOffset) >>> 0;
        const len = Number(data.length) >>> 0;
        if (off > len) throw new RangeError('byteArrayOffset is out of bounds.');
      }
      uploadFromBitmapDataAsync(source, miplevel = 0) {
        if (this.disposed) throw illegalOp('Texture has been disposed.');
        this.uploadFromBitmapData(source, miplevel);
        scheduleTask(() => {
          if (!this.disposed) {
            this.dispatchEvent(new BaseEvent('textureReady', false, false));
          }
        });
      }
      uploadFromByteArrayAsync(data, byteArrayOffset = 0, miplevel = 0) {
        if (this.disposed) throw illegalOp('Texture has been disposed.');
        this.uploadFromByteArray(data, byteArrayOffset, miplevel);
        scheduleTask(() => {
          if (!this.disposed) {
            this.dispatchEvent(new BaseEvent('textureReady', false, false));
          }
        });
      }
    }
    tagType(Texture, 'Texture', v => v instanceof Texture);

    class RectangleTexture extends TextureBase {
      constructor(width = 1, height = 1, format = 'bgra', _optimizeForRenderToTexture = false) {
        super(width, height, format);
      }
      uploadFromBitmapData(source) {
        if (!source || typeof source.width !== 'number' || typeof source.height !== 'number') {
          throw argErr('RectangleTexture.uploadFromBitmapData requires a BitmapData.');
        }
      }
      uploadFromByteArray(data, byteArrayOffset = 0) {
        if (!data) throw argErr('RectangleTexture.uploadFromByteArray requires a ByteArray.');
        const off = Number(byteArrayOffset) >>> 0;
        const len = Number(data.length) >>> 0;
        if (off > len) throw new RangeError('byteArrayOffset is out of bounds.');
      }
      uploadFromBitmapDataAsync(source) {
        if (this.disposed) throw illegalOp('RectangleTexture has been disposed.');
        this.uploadFromBitmapData(source);
        scheduleTask(() => {
          if (!this.disposed) {
            this.dispatchEvent(new BaseEvent('textureReady', false, false));
          }
        });
      }
      uploadFromByteArrayAsync(data, byteArrayOffset = 0) {
        if (this.disposed) throw illegalOp('RectangleTexture has been disposed.');
        this.uploadFromByteArray(data, byteArrayOffset);
        scheduleTask(() => {
          if (!this.disposed) {
            this.dispatchEvent(new BaseEvent('textureReady', false, false));
          }
        });
      }
    }
    tagType(RectangleTexture, 'RectangleTexture', v => v instanceof RectangleTexture);

    class Context3D extends BaseDispatcher {
      constructor(profile = 'baseline') {
        super();
        this.profile = String(profile || 'baseline');
        this.driverInfo = 'NeoFlash Software/WebGL Context3D';
        this.enableErrorChecking = false;
        this.backBufferWidth = projW;
        this.backBufferHeight = projH;
        this.maxBackBufferWidth = 4096;
        this.maxBackBufferHeight = 4096;
        this.fillMode = 'solid';
        this.clearColor = 0xff000000;
        this.instancedDrawCount = 0;
      }
      static get supportsVideoTexture() { return false; }
      configureBackBuffer(width, height, _antiAlias = 0, _enableDepthAndStencil = true) {
        this.backBufferWidth = Math.max(1, Number(width) | 0);
        this.backBufferHeight = Math.max(1, Number(height) | 0);
      }
      clear(red = 0, green = 0, blue = 0, alpha = 1) {
        const r = Math.max(0, Math.min(255, Math.round((Number(red) || 0) * 255)));
        const g = Math.max(0, Math.min(255, Math.round((Number(green) || 0) * 255)));
        const b = Math.max(0, Math.min(255, Math.round((Number(blue) || 0) * 255)));
        const a = Math.max(0, Math.min(255, Math.round((Number(alpha) ?? 1) * 255)));
        this.clearColor = (((a << 24) | (r << 16) | (g << 8) | b) >>> 0);
      }
      setFillMode(fillMode) {
        const mode = String(fillMode ?? '');
        if (mode !== Context3DFillMode.SOLID && mode !== Context3DFillMode.WIREFRAME) {
          throw argErr(`Invalid Context3DFillMode '${mode}'.`);
        }
        this.fillMode = mode;
      }
      createIndexBuffer(numIndices, bufferUsage = 'staticDraw') {
        if ((Number(numIndices) | 0) <= 0) {
          throw argErr('createIndexBuffer requires positive numIndices.');
        }
        return new IndexBuffer3D(numIndices, bufferUsage);
      }
      createVertexBuffer(numVertices, data32PerVertex, bufferUsage = 'staticDraw') {
        if ((Number(numVertices) | 0) <= 0 || (Number(data32PerVertex) | 0) <= 0) {
          throw argErr('createVertexBuffer requires positive numVertices and data32PerVertex.');
        }
        return new VertexBuffer3D(numVertices, data32PerVertex, 0, bufferUsage);
      }
      createVertexBufferForInstances(
        numVertices,
        data32PerVertex,
        instancesPerElement = 1,
        bufferUsage = 'staticDraw'
      ) {
        const nv = Number(numVertices) | 0;
        const d32 = Number(data32PerVertex) | 0;
        const ipe = Number(instancesPerElement) | 0;
        if (nv <= 0 || d32 <= 0 || d32 > 64 || ipe <= 0) {
          throw argErr('createVertexBufferForInstances requires valid positive counts.');
        }
        return new VertexBuffer3D(nv, d32, ipe, bufferUsage);
      }
      createTexture(width, height, format = 'bgra', optimizeForRenderToTexture = false, streamingLevels = 0) {
        const w = Number(width) | 0;
        const h = Number(height) | 0;
        if (w <= 0 || h <= 0) throw argErr('createTexture requires positive dimensions.');
        return new Texture(w, h, format, optimizeForRenderToTexture, streamingLevels);
      }
      createRectangleTexture(width, height, format = 'bgra', optimizeForRenderToTexture = false) {
        const w = Number(width) | 0;
        const h = Number(height) | 0;
        if (w <= 0 || h <= 0) throw argErr('createRectangleTexture requires positive dimensions.');
        return new RectangleTexture(w, h, format, optimizeForRenderToTexture);
      }
      drawTriangles(_indexBuffer, _firstIndex = 0, _numTriangles = -1) {}
      drawTrianglesInstanced(indexBuffer, numInstances, firstIndex = 0, numTriangles = -1) {
        if (!(indexBuffer instanceof IndexBuffer3D)) {
          throw argErr('drawTrianglesInstanced requires an IndexBuffer3D.');
        }
        const ni = Number(numInstances) | 0;
        if (ni <= 0) throw argErr('drawTrianglesInstanced requires numInstances > 0.');
        const fi = Number(firstIndex) | 0;
        if (fi < 0 || fi >= indexBuffer.numIndices) {
          throw argErr('drawTrianglesInstanced firstIndex is out of bounds.');
        }
        const _nt = Number(numTriangles) | 0;
        this.instancedDrawCount += ni;
      }
      drawToBitmapData(destination, srcRect = null, dstPoint = null) {
        if (!destination || typeof destination.setPixel32 !== 'function') {
          throw argErr('Context3D.drawToBitmapData requires a destination BitmapData.');
        }
        const sx = srcRect ? Math.max(0, Number(srcRect.x) | 0) : 0;
        const sy = srcRect ? Math.max(0, Number(srcRect.y) | 0) : 0;
        const sw = srcRect ? (Number(srcRect.width) | 0) : (Number(destination.width) | 0);
        const sh = srcRect ? (Number(srcRect.height) | 0) : (Number(destination.height) | 0);
        const dx = dstPoint ? (Number(dstPoint.x) | 0) : 0;
        const dy = dstPoint ? (Number(dstPoint.y) | 0) : 0;
        const w = Math.min(sw - sx, (Number(destination.width) | 0) - dx);
        const h = Math.min(sh - sy, (Number(destination.height) | 0) - dy);
        const col = this.clearColor >>> 0;
        for (let y = 0; y < h; y++) {
          for (let x = 0; x < w; x++) {
            destination.setPixel32(dx + x, dy + y, col);
          }
        }
      }
      present() {}
      dispose(_recreate = true) {}
    }
    tagType(Context3D, 'Context3D', v => v instanceof Context3D);

    // flash.system.*
    let currentAppDomainSingleton = null;
    class ApplicationDomain {
      constructor(parentDomain = null) {
        this.parentDomain = parentDomain ?? null;
        this.domainMemory = null;
      }
      static get currentDomain() {
        if (!currentAppDomainSingleton) {
          currentAppDomainSingleton = new ApplicationDomain(null);
        }
        return currentAppDomainSingleton;
      }
      static get MIN_DOMAIN_MEMORY_LENGTH() { return 1024; }
      getDefinition(name) {
        const qn = String(name ?? '');
        return builtins[qn] || null;
      }
      hasDefinition(name) {
        const qn = String(name ?? '');
        return qn in builtins;
      }
    }
    tagType(ApplicationDomain, 'ApplicationDomain', v => v instanceof ApplicationDomain);

    let currentSecDomainSingleton = null;
    class SecurityDomain {
      static get currentDomain() {
        if (!currentSecDomainSingleton) {
          currentSecDomainSingleton = new SecurityDomain();
        }
        return currentSecDomainSingleton;
      }
    }
    tagType(SecurityDomain, 'SecurityDomain', v => v instanceof SecurityDomain);

    const ImageDecodingPolicy = defConstClass('ImageDecodingPolicy', {
      ON_DEMAND: 'onDemand',
      ON_LOAD: 'onLoad',
    });

    class LoaderContext {
      constructor(
        checkPolicyFile = false,
        applicationDomain = null,
        securityDomain = null
      ) {
        this.checkPolicyFile = Boolean(checkPolicyFile);
        this.applicationDomain = applicationDomain ?? null;
        this.securityDomain = securityDomain ?? null;
        this.allowCodeImport = true;
        this.allowLoadBytesCodeExecution = true;
        this.imageDecodingPolicy = 'onDemand';
        this.parameters = null;
        this.requestedContentParent = null;
      }
    }
    tagType(LoaderContext, 'LoaderContext', v => v instanceof LoaderContext);

    const Capabilities = {
      Capabilities: function () {
        throw new TypeError('Capabilities supplies static properties only.');
      },
    }.Capabilities;
    Object.defineProperties(Capabilities, {
      language: {
        get: () => {
          const lang = global.navigator?.language;
          return typeof lang === 'string' && lang ? lang.split('-')[0] : 'en';
        },
        enumerable: true,
      },
      languages: {
        get: () => {
          const navLangs = global.navigator?.languages;
          if (Array.isArray(navLangs) && navLangs.length > 0) {
            return Array.from(navLangs, String);
          }
          const single = global.navigator?.language;
          return typeof single === 'string' && single ? [single, single.split('-')[0]] : ['en-US', 'en'];
        },
        enumerable: true,
      },
      os: { get: () => 'Mac OS 15.0.0', enumerable: true },
      manufacturer: { get: () => 'Adobe Macintosh', enumerable: true },
      playerType: { get: () => 'Desktop', enumerable: true },
      version: { get: () => 'MAC 32,0,0,116', enumerable: true },
      isDebugger: { get: () => false, enumerable: true },
      screenDPI: { get: () => 72, enumerable: true },
      screenResolutionX: { get: () => getViewportSize()[0] || projW, enumerable: true },
      screenResolutionY: { get: () => getViewportSize()[1] || projH, enumerable: true },
      cpuArchitecture: { get: () => 'ARM', enumerable: true },
      supports32BitProcesses: { get: () => false, enumerable: true },
      supports64BitProcesses: { get: () => true, enumerable: true },
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError('Capabilities cannot be extended.');
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    Object.freeze(Capabilities);

    let securityExactSettings = true;
    const Security = {
      Security: function () {
        throw new TypeError('Security supplies static properties and methods only.');
      },
    }.Security;
    Object.defineProperties(Security, {
      APPLICATION: { value: 'application', enumerable: true },
      LOCAL_TRUSTED: { value: 'localTrusted', enumerable: true },
      LOCAL_WITH_FILE: { value: 'localWithFile', enumerable: true },
      LOCAL_WITH_NETWORK: { value: 'localWithNetwork', enumerable: true },
      REMOTE: { value: 'remote', enumerable: true },
      sandboxType: { get: () => 'application', enumerable: true },
      exactSettings: {
        get: () => securityExactSettings,
        set: v => { securityExactSettings = Boolean(v); },
        enumerable: true,
      },
      pageDomain: { get: () => global.location?.hostname || 'localhost', enumerable: true },
      allowDomain: { value: (..._domains) => {}, enumerable: true },
      allowInsecureDomain: { value: (..._domains) => {}, enumerable: true },
      loadPolicyFile: { value: _url => {}, enumerable: true },
      showSettings: { value: (_panel = 'default') => {}, enumerable: true },
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError('Security cannot be extended.');
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    Object.freeze(Security);

    // -------------------------------------------------------------------------
    // 17. Display Object Proxy Hooks & augmentBuiltins(targetBuiltins)
    // -------------------------------------------------------------------------
    const displayCacheMatrixMap = new WeakMap();
    const interactiveSoftKeyboardMap = new WeakMap();
    const sharedObjectPreventBackupMap = new WeakMap();
    const netConnIdleTimeoutMap = new WeakMap();
    const netStreamDrmCredsMap = new WeakMap();

    function hasDisplayProperty(proxy, name) {
      if (htmlLoaderStates.has(proxy) && htmlLoaderOwnMemberNames.has(name)) return true;
      if (name === 'cacheAsBitmapMatrix' || name === 'softKeyboard') return true;
      if (name === 'loadFilePromise' && getLoader()?.Loader?.__nfIs?.(proxy)) return true;
      return false;
    }

    function getDisplayProperty(proxy, name) {
      if (htmlLoaderStates.has(proxy) && htmlLoaderOwnMemberNames.has(name)) {
        const desc = htmlLoaderPropDescriptors[name];
        if (desc.get) return desc.get.call(proxy);
        if (typeof desc.value === 'function') return desc.value.bind(proxy);
        return desc.value;
      }
      if (name === 'cacheAsBitmapMatrix') {
        const m = displayCacheMatrixMap.get(proxy) ?? null;
        return m && typeof m.clone === 'function' ? m.clone() : m;
      }
      if (name === 'softKeyboard') {
        return interactiveSoftKeyboardMap.get(proxy) ?? 'default';
      }
      if (getLoader()?.Loader?.__nfIs?.(proxy)) {
        if (name === 'loadFilePromise') {
          return (promise, context = null) => {
            if (!promise || typeof promise.open !== 'function') {
              throw argErr('Loader.loadFilePromise requires an IFilePromise.');
            }
            if (context !== null && !(context instanceof LoaderContext)) {
              throw new TypeError('Loader.loadFilePromise context must be a LoaderContext or null.');
            }
            const dataInput = promise.open();
            const rawBytes = readDataInputBytes(dataInput);
            if (typeof promise.close === 'function') {
              try { promise.close(); } catch (_) {}
            }
            const ba = binary ? binary.fromBytes(rawBytes) : rawBytes;
            return proxy.loadBytes(ba);
          };
        }
        if (name === 'load' || name === 'loadBytes') {
          const disp = getDisplay();
          const origFn = disp ? disp.get(proxy, name) : undefined;
          if (typeof origFn === 'function') {
            return (arg, context = null) => {
              if (context !== null && !(context instanceof LoaderContext)) {
                return origFn(arg, context);
              }
              return origFn(arg);
            };
          }
        }
      }
      return undefined;
    }

    function setDisplayProperty(proxy, name, value) {
      if (htmlLoaderStates.has(proxy) && htmlLoaderOwnMemberNames.has(name)) {
        const desc = htmlLoaderPropDescriptors[name];
        if (desc.set) {
          desc.set.call(proxy, value);
          return true;
        }
        throw new TypeError(`HTMLLoader.${name} is read-only.`);
      }
      if (name === 'cacheAsBitmapMatrix') {
        if (value !== null && (typeof value !== 'object' || typeof value.a !== 'number')) {
          throw new TypeError('DisplayObject.cacheAsBitmapMatrix requires a Matrix or null.');
        }
        displayCacheMatrixMap.set(
          proxy,
          value && typeof value.clone === 'function' ? value.clone() : value
        );
        return true;
      }
      if (name === 'softKeyboard') {
        const val = String(value ?? 'default');
        if (!['contact', 'decimal', 'default', 'email', 'number', 'phone', 'punctuation', 'url'].includes(val)) {
          throw argErr(`Invalid SoftKeyboardType '${val}'.`);
        }
        interactiveSoftKeyboardMap.set(proxy, val);
        return true;
      }
      return false;
    }

    function augmentBuiltins(targetBuiltins) {
      if (!targetBuiltins || typeof targetBuiltins !== 'object') return;

      // Replace frozen URLRequestMethod with AIR-extended version (adds DELETE, HEAD, OPTIONS, PUT)
      targetBuiltins['flash.net.URLRequestMethod'] = AirURLRequestMethod;

      // Augment URLRequest & URLLoader if present on targetBuiltins
      if (targetBuiltins['flash.net.URLRequest']) {
        augmentURLRequestClass(targetBuiltins['flash.net.URLRequest']);
      }
      if (targetBuiltins['flash.net.URLLoader']) {
        augmentURLLoaderClass(targetBuiltins['flash.net.URLLoader']);
      }

      // Augment or replace SoundMixer with AIR properties (audioPlaybackMode, useSpeakerphoneForVoice)
      const origSoundMixer = targetBuiltins['flash.media.SoundMixer'];
      if (origSoundMixer && origSoundMixer !== AirSoundMixer) {
        if (Object.isExtensible(origSoundMixer)) {
          Object.defineProperties(origSoundMixer, {
            audioPlaybackMode: Object.getOwnPropertyDescriptor(AirSoundMixer, 'audioPlaybackMode'),
            useSpeakerphoneForVoice: Object.getOwnPropertyDescriptor(AirSoundMixer, 'useSpeakerphoneForVoice'),
          });
        } else {
          if (typeof origSoundMixer.stopAll === 'function') {
            soundMixerState.delegateStopAll = () => origSoundMixer.stopAll();
          }
          targetBuiltins['flash.media.SoundMixer'] = AirSoundMixer;
        }
      }

      // Augment SharedObject with preventBackup
      const SharedObjectCtor = targetBuiltins['flash.net.SharedObject'];
      if (SharedObjectCtor?.prototype && !Object.hasOwn(SharedObjectCtor.prototype, 'preventBackup')) {
        Object.defineProperty(SharedObjectCtor.prototype, 'preventBackup', {
          get() { return sharedObjectPreventBackupMap.get(this) ?? false; },
          set(v) { sharedObjectPreventBackupMap.set(this, Boolean(v)); },
          enumerable: true,
          configurable: true,
        });
      }

      // Augment NetConnection with httpIdleTimeout
      const NetConnectionCtor = targetBuiltins['flash.net.NetConnection'];
      if (NetConnectionCtor?.prototype && !Object.hasOwn(NetConnectionCtor.prototype, 'httpIdleTimeout')) {
        Object.defineProperty(NetConnectionCtor.prototype, 'httpIdleTimeout', {
          get() { return netConnIdleTimeoutMap.get(this) ?? 0; },
          set(v) {
            const n = Number(v);
            if (n < 0 || Number.isNaN(n)) throw argErr('NetConnection.httpIdleTimeout must be non-negative.');
            netConnIdleTimeoutMap.set(this, n);
          },
          enumerable: true,
          configurable: true,
        });
      }

      // Augment NetStream with preloadEmbeddedData & setDRMAuthenticationCredentials
      const NetStreamCtor = targetBuiltins['flash.net.NetStream'];
      if (NetStreamCtor?.prototype && !Object.hasOwn(NetStreamCtor.prototype, 'preloadEmbeddedData')) {
        Object.defineProperties(NetStreamCtor.prototype, {
          preloadEmbeddedData: {
            value(optionsArg) {
              if (!optionsArg || typeof optionsArg !== 'object') {
                throw argErr('NetStream.preloadEmbeddedData requires a NetStreamPlayOptions object.');
              }
              if (typeof this.onDRMContentData === 'function') {
                this.onDRMContentData(new DRMContentData());
              }
            },
            configurable: true,
            writable: true,
          },
          setDRMAuthenticationCredentials: {
            value(userName, password, authenticationType) {
              const type = String(authenticationType ?? '');
              if (type !== 'drm' && type !== 'proxy') {
                throw argErr(`Invalid DRM authenticationType '${type}'. Expected 'drm' or 'proxy'.`);
              }
              netStreamDrmCredsMap.set(this, {
                userName: userName == null ? null : String(userName),
                password: password == null ? null : String(password),
                authenticationType: type,
              });
            },
            configurable: true,
            writable: true,
          },
        });
      }

      // Augment FileReference if as2FilesHost installed its FileReference first
      const ExistingFR = targetBuiltins['flash.net.FileReference'];
      if (ExistingFR && ExistingFR !== AirFileReference) {
        if (!Object.isFrozen(ExistingFR)) {
          Object.defineProperty(ExistingFR, 'permissionStatus', {
            get: () => 'granted',
            enumerable: true,
            configurable: true,
          });
        }
        if (ExistingFR.prototype && !Object.isFrozen(ExistingFR.prototype)) {
          const frDispatchers = new WeakMap();
          const frDataMap = new WeakMap();
          const getFrDisp = self => {
            let d = frDispatchers.get(self);
            if (!d) {
              d = new BaseDispatcher(self);
              frDispatchers.set(self, d);
            }
            return d;
          };
          for (const m of ['addEventListener', 'removeEventListener', 'dispatchEvent', 'hasEventListener', 'willTrigger']) {
            if (typeof ExistingFR.prototype[m] !== 'function') {
              ExistingFR.prototype[m] = function (...args) {
                return getFrDisp(this)[m](...args);
              };
            }
          }
          if (!Object.hasOwn(ExistingFR.prototype, 'extension')) {
            Object.defineProperty(ExistingFR.prototype, 'extension', {
              get() {
                let nm = '';
                try { nm = String(this.name || ''); } catch (_) { nm = String(this._airName || ''); }
                const dot = nm.lastIndexOf('.');
                return dot > 0 && dot + 1 < nm.length ? nm.slice(dot + 1) : null;
              },
              enumerable: true,
              configurable: true,
            });
          }
          if (!Object.hasOwn(ExistingFR.prototype, 'data')) {
            Object.defineProperty(ExistingFR.prototype, 'data', {
              get() { return frDataMap.get(this) ?? null; },
              set(v) { frDataMap.set(this, v); },
              enumerable: true,
              configurable: true,
            });
          }
          ExistingFR.prototype.requestPermission = function () {
            scheduleTask(() => {
              this.dispatchEvent(
                new PermissionEvent(PermissionEvent.PERMISSION_STATUS, false, false, 'granted')
              );
            });
          };
          ExistingFR.prototype.uploadUnencoded = function (request) {
            if (!request || typeof request.url !== 'string' || !request.url) {
              throw argErr('FileReference.uploadUnencoded requires a URLRequest with a valid url.');
            }
            const url = request.url;
            scheduleTask(() => {
              this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
              if (options.net?.HTTPStatusEvent) {
                const hs = new options.net.HTTPStatusEvent('httpResponseStatus', false, false, 200, false);
                hs.responseURL = url;
                this.dispatchEvent(hs);
                this.dispatchEvent(new options.net.HTTPStatusEvent('httpStatus', false, false, 200, false));
              }
              this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
            });
          };
          if (typeof ExistingFR.prototype.save !== 'function') {
            ExistingFR.prototype.save = function (data, defaultFileName = null) {
              if (defaultFileName != null) this._airName = String(defaultFileName);
              if (binary?.ByteArray?.__nfIs?.(data)) frDataMap.set(this, data);
              else if (binary) {
                frDataMap.set(this, binary.fromBytes(new TextEncoder().encode(String(data ?? ''))));
              }
              scheduleTask(() => {
                this.dispatchEvent(new BaseEvent(BaseEvent.SELECT));
                this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
              });
            };
          }
          if (typeof ExistingFR.prototype.load !== 'function') {
            ExistingFR.prototype.load = function () {
              scheduleTask(() => {
                if (!frDataMap.has(this) && binary) frDataMap.set(this, new binary.ByteArray());
                this.dispatchEvent(new BaseEvent(BaseEvent.OPEN));
                this.dispatchEvent(new BaseEvent(BaseEvent.COMPLETE));
              });
            };
          }
        }
      }
    }

    // -------------------------------------------------------------------------
    // 18. Builtins Registry, Reset & Dispose
    // -------------------------------------------------------------------------
    const builtins = Object.freeze({
      // flash.errors
      'flash.errors.SQLErrorOperation': SQLErrorOperation,
      'flash.errors.SQLError': SQLError,
      'flash.errors.PermissionError': PermissionError,
      'flash.errors.DRMManagerError': DRMManagerError,
      // flash.events
      'flash.events.TouchEventIntent': TouchEventIntent,
      'flash.events.InvokeEvent': InvokeEvent,
      'flash.events.BrowserInvokeEvent': BrowserInvokeEvent,
      'flash.events.FileListEvent': FileListEvent,
      'flash.events.OutputProgressEvent': OutputProgressEvent,
      'flash.events.NativeWindowBoundsEvent': NativeWindowBoundsEvent,
      'flash.events.NativeWindowDisplayStateEvent': NativeWindowDisplayStateEvent,
      'flash.events.NativeDragEvent': NativeDragEvent,
      'flash.events.ScreenMouseEvent': ScreenMouseEvent,
      'flash.events.SQLEvent': SQLEvent,
      'flash.events.SQLErrorEvent': SQLErrorEvent,
      'flash.events.SQLUpdateEvent': SQLUpdateEvent,
      'flash.events.StorageVolumeChangeEvent': StorageVolumeChangeEvent,
      'flash.events.NativeProcessExitEvent': NativeProcessExitEvent,
      'flash.events.LocationChangeEvent': LocationChangeEvent,
      'flash.events.HTMLUncaughtScriptExceptionEvent': HTMLUncaughtScriptExceptionEvent,
      'flash.events.MediaEvent': MediaEvent,
      'flash.events.DatagramSocketDataEvent': DatagramSocketDataEvent,
      'flash.events.ServerSocketConnectEvent': ServerSocketConnectEvent,
      'flash.events.DNSResolverEvent': DNSResolverEvent,
      'flash.events.DRMAuthenticateEvent': DRMAuthenticateEvent,
      'flash.events.DeviceRotationEvent': DeviceRotationEvent,
      'flash.events.PermissionEvent': PermissionEvent,
      'flash.events.RemoteNotificationEvent': RemoteNotificationEvent,
      'flash.events.GameInputEvent': GameInputEvent,
      'flash.events.VsyncStateChangeAvailabilityEvent': VsyncStateChangeAvailabilityEvent,
      'flash.events.AVHTTPStatusEvent': AVHTTPStatusEvent,
      'flash.events.DRMDeviceGroupEvent': DRMDeviceGroupEvent,
      'flash.events.DRMDeviceGroupErrorEvent': DRMDeviceGroupErrorEvent,
      'flash.events.DRMStatusEvent': DRMStatusEvent,
      'flash.events.StageVideoEvent': StageVideoEvent,
      ...(options.eventFamilies?.GeolocationEvent
        ? { 'flash.events.GeolocationEvent': options.eventFamilies.GeolocationEvent }
        : {}),
      ...(options.eventFamilies?.StageOrientationEvent
        ? { 'flash.events.StageOrientationEvent': options.eventFamilies.StageOrientationEvent }
        : {}),
      ...(options.eventFamilies?.TouchEvent
        ? { 'flash.events.TouchEvent': options.eventFamilies.TouchEvent }
        : {}),
      ...(options.eventFamilies?.GestureEvent
        ? { 'flash.events.GestureEvent': options.eventFamilies.GestureEvent }
        : {}),
      ...(options.eventFamilies?.TransformGestureEvent
        ? { 'flash.events.TransformGestureEvent': options.eventFamilies.TransformGestureEvent }
        : {}),
      // flash.filesystem
      'flash.filesystem.FileMode': FileMode,
      'flash.filesystem.File': File,
      'flash.filesystem.FileStream': FileStream,
      'flash.filesystem.StorageVolume': StorageVolume,
      'flash.filesystem.StorageVolumeInfo': StorageVolumeInfo,
      // flash.desktop
      'flash.desktop.ClipboardFormats': ClipboardFormats,
      'flash.desktop.ClipboardTransferMode': ClipboardTransferMode,
      'flash.desktop.Clipboard': Clipboard,
      'flash.desktop.IFilePromise': IFilePromise,
      'flash.desktop.NativeDragActions': NativeDragActions,
      'flash.desktop.NativeDragOptions': NativeDragOptions,
      'flash.desktop.NativeDragManager': NativeDragManager,
      'flash.desktop.InvokeEventReason': InvokeEventReason,
      'flash.desktop.NotificationType': NotificationType,
      'flash.desktop.SystemIdleMode': SystemIdleMode,
      'flash.desktop.Icon': Icon,
      'flash.desktop.InteractiveIcon': InteractiveIcon,
      'flash.desktop.DockIcon': DockIcon,
      'flash.desktop.SystemTrayIcon': SystemTrayIcon,
      'flash.desktop.NativeApplication': NativeApplication,
      'flash.desktop.NativeProcessStartupInfo': NativeProcessStartupInfo,
      'flash.desktop.NativeProcess': NativeProcess,
      'flash.desktop.Updater': Updater,
      // flash.display
      'flash.display.FocusDirection': FocusDirection,
      'flash.display.StageAspectRatio': StageAspectRatio,
      'flash.display.StageOrientation': StageOrientation,
      'flash.display.ScreenMode': ScreenMode,
      'flash.display.Screen': Screen,
      'flash.display.NativeMenuItem': NativeMenuItem,
      'flash.display.NativeMenu': NativeMenu,
      'flash.display.NativeWindowType': NativeWindowType,
      'flash.display.NativeWindowSystemChrome': NativeWindowSystemChrome,
      'flash.display.NativeWindowDisplayState': NativeWindowDisplayState,
      'flash.display.NativeWindowResize': NativeWindowResize,
      'flash.display.NativeWindowRenderMode': NativeWindowRenderMode,
      'flash.display.NativeWindowInitOptions': NativeWindowInitOptions,
      'flash.display.NativeWindow': NativeWindow,
      // flash.display3D
      'flash.display3D.Context3D': Context3D,
      'flash.display3D.Context3DFillMode': Context3DFillMode,
      'flash.display3D.Context3DProfile': Context3DProfile,
      'flash.display3D.IndexBuffer3D': IndexBuffer3D,
      'flash.display3D.VertexBuffer3D': VertexBuffer3D,
      'flash.display3D.textures.TextureBase': TextureBase,
      'flash.display3D.textures.Texture': Texture,
      'flash.display3D.textures.RectangleTexture': RectangleTexture,
      // flash.data
      'flash.data.EncryptedLocalStore': EncryptedLocalStore,
      'flash.data.SQLMode': SQLMode,
      'flash.data.SQLTransactionLockType': SQLTransactionLockType,
      'flash.data.SQLCollationType': SQLCollationType,
      'flash.data.SQLColumnNameStyle': SQLColumnNameStyle,
      'flash.data.SQLResult': SQLResult,
      'flash.data.SQLColumnSchema': SQLColumnSchema,
      'flash.data.SQLSchema': SQLSchema,
      'flash.data.SQLTableSchema': SQLTableSchema,
      'flash.data.SQLViewSchema': SQLViewSchema,
      'flash.data.SQLIndexSchema': SQLIndexSchema,
      'flash.data.SQLTriggerSchema': SQLTriggerSchema,
      'flash.data.SQLSchemaResult': SQLSchemaResult,
      'flash.data.SQLConnection': SQLConnection,
      'flash.data.SQLStatement': SQLStatement,
      // flash.html
      'flash.html.HTMLPDFCapability': HTMLPDFCapability,
      'flash.html.HTMLSWFCapability': HTMLSWFCapability,
      'flash.html.HTMLHistoryItem': HTMLHistoryItem,
      'flash.html.HTMLWindowCreateOptions': HTMLWindowCreateOptions,
      'flash.html.HTMLHost': HTMLHost,
      'flash.html.HTMLLoader': HTMLLoader,
      // flash.media
      'flash.media.AVURLLoader': AVURLLoader,
      'flash.media.AudioPlaybackMode': AudioPlaybackMode,
      'flash.media.Camera': AirCamera,
      'flash.media.CameraPosition': CameraPosition,
      'flash.media.CameraRoll': CameraRoll,
      'flash.media.CameraRollBrowseOptions': CameraRollBrowseOptions,
      'flash.media.CameraUI': CameraUI,
      'flash.media.MediaPromise': MediaPromise,
      'flash.media.MediaType': MediaType,
      'flash.media.Microphone': AirMicrophone,
      'flash.media.SoundMixer': AirSoundMixer,
      'flash.media.StageWebView': StageWebView,
      // flash.net & flash.net.drm
      'flash.net.DatagramSocket': DatagramSocket,
      'flash.net.FileReference': AirFileReference,
      'flash.net.FileReferenceList': AirFileReferenceList,
      'flash.net.IPVersion': IPVersion,
      'flash.net.InterfaceAddress': InterfaceAddress,
      'flash.net.NetworkInfo': NetworkInfo,
      'flash.net.NetworkInterface': NetworkInterface,
      'flash.net.Responder': Responder,
      'flash.net.ServerSocket': ServerSocket,
      'flash.net.Socket': Socket,
      'flash.net.URLRequest': options.net?.URLRequest,
      'flash.net.URLLoader': options.net?.URLLoader,
      'flash.net.URLRequestDefaults': URLRequestDefaults,
      'flash.net.URLRequestMethod': AirURLRequestMethod,
      'flash.net.URLStream': URLStream,
      'flash.net.drm.DRMDeviceGroup': DRMDeviceGroup,
      'flash.net.drm.DRMContentData': DRMContentData,
      'flash.net.drm.DRMVoucher': DRMVoucher,
      // flash.printing
      'flash.printing.PaperSize': PaperSize,
      'flash.printing.PrintJob': PrintJob,
      'flash.printing.PrintJobOptions': PrintJobOptions,
      'flash.printing.PrintJobOrientation': PrintJobOrientation,
      'flash.printing.PrintMethod': PrintMethod,
      'flash.printing.PrintUIOptions': PrintUIOptions,
      // flash.security
      'flash.security.IURIDereferencer': IURIDereferencer,
      'flash.security.ReferencesValidationSetting': ReferencesValidationSetting,
      'flash.security.RevocationCheckSettings': RevocationCheckSettings,
      'flash.security.SignatureStatus': SignatureStatus,
      'flash.security.SignerTrustSettings': SignerTrustSettings,
      'flash.security.XMLSignatureValidator': XMLSignatureValidator,
      // flash.external
      'flash.external.ExtensionContext': ExtensionContext,
      // flash.text
      'flash.text.AutoCapitalize': AutoCapitalize,
      'flash.text.ReturnKeyLabel': ReturnKeyLabel,
      'flash.text.SoftKeyboardType': SoftKeyboardType,
      'flash.text.StageText': StageText,
      'flash.text.StageTextClearButtonMode': StageTextClearButtonMode,
      'flash.text.StageTextInitOptions': StageTextInitOptions,
      // flash.ui
      'flash.ui.ContextMenu': ContextMenu,
      'flash.ui.ContextMenuBuiltInItems': ContextMenuBuiltInItems,
      'flash.ui.ContextMenuClipboardItems': ContextMenuClipboardItems,
      'flash.ui.ContextMenuItem': ContextMenuItem,
      'flash.ui.GameInput': GameInput,
      'flash.ui.GameInputControl': GameInputControl,
      'flash.ui.GameInputDevice': GameInputDevice,
      'flash.ui.Multitouch': Multitouch,
      'flash.ui.MultitouchInputMode': MultitouchInputMode,
      // flash.system
      'flash.system.ApplicationDomain': ApplicationDomain,
      'flash.system.Capabilities': Capabilities,
      'flash.system.ImageDecodingPolicy': ImageDecodingPolicy,
      'flash.system.LoaderContext': LoaderContext,
      'flash.system.Security': Security,
      'flash.system.SecurityDomain': SecurityDomain,
      // companion AIR packages
      'flash.permissions.PermissionStatus': PermissionStatus,
      'flash.net.dns.ResourceRecord': ResourceRecord,
      'flash.net.dns.ARecord': ARecord,
      'flash.net.dns.AAAARecord': AAAARecord,
      'flash.net.dns.MXRecord': MXRecord,
      'flash.net.dns.PTRRecord': PTRRecord,
      'flash.net.dns.SRVRecord': SRVRecord,
      'flash.net.dns.DNSResolver': DNSResolver,
      'flash.notifications.NotificationStyle': NotificationStyle,
      'flash.notifications.RemoteNotifierSubscribeOptions': RemoteNotifierSubscribeOptions,
      'flash.notifications.RemoteNotifier': RemoteNotifier,
      'flash.sensors.DeviceRotation': DeviceRotation,
      'flash.sensors.Geolocation': Geolocation,
    });

    function reset() {
      if (disposed) return;
      generation++;
      while (hostCleanups.length > 0) {
        const fn = hostCleanups.pop();
        try { fn(); } catch (_) {}
      }
      if (kernel && typeof kernel.free === 'function') {
        try {
          kernel.free();
        } catch (_) {}
      }
      kernel = new wasm.AirKernel(projName, projW, projH, resourcesJson, Date.now());
      lastInputTimestamp = Date.now();
      volumeInfoSingleton = null;
      generalClipboardSingleton = null;
      mainScreenSingleton = null;
      mainNativeWindow = null;
      activeNativeWindow = null;
      openWindows.length = 0;
      nativeAppSingleton = null;
      dockIconSingleton = null;
      networkInfoSingleton = null;
      activeStageText = null;
      lastCameraRollFile = null;
      cameraRollCounter = 0;
      cameraUiCounter = 0;
      printJobActiveFlag = false;
      gameInputDevices = null;
      currentAppDomainSingleton = null;
      currentSecDomainSingleton = null;
      udpSocketMap.clear();
      serverSocketMap.clear();
      tcpSocketMap.clear();
      urlRequestDefaultsState = {
        authenticate: true,
        cacheResponse: true,
        followRedirects: true,
        idleTimeout: 0,
        manageCookies: true,
        useCache: true,
        userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/537.36 AdobeAIR/32.0',
      };
      soundMixerState.audioPlaybackMode = 'media';
      soundMixerState.useSpeakerphoneForVoice = false;
      soundMixerState.bufferTime = 0;
      multitouchState = {
        inputMode: 'gesture',
        mapTouchToMouse: true,
      };
      nativeAppState = {
        autoExit: true,
        idleThreshold: 300,
        systemIdleMode: 'normal',
        executeInBackground: false,
        menu: null,
      };
      stageOrientationState = {
        orientation: 'default',
        deviceOrientation: 'default',
        autoOrients: false,
        aspectRatio: 'any',
        vsyncEnabled: true,
      };
      dragState = {
        dragging: false,
        initiator: null,
        clipboard: null,
        allowedActions: null,
        dropAction: 'none',
        acceptedTarget: null,
      };
      bindStageAirProperties();
    }

    function tryConnectServerSocket(targetHost, targetPort, createSocketInstance) {
      if (disposed) return null;
      try {
        const info = JSON.parse(callKernel('tcp_connect_loopback', String(targetHost ?? '127.0.0.1'), Number(targetPort) | 0));
        if (!info.matched) return null;
        const serverSock = serverSocketMap.get(info.serverId);
        if (!serverSock) return null;
        const accepted = typeof createSocketInstance === 'function'
          ? createSocketInstance(info)
          : new Socket(acceptedSocketToken, 0);
        if (tcpSocketStates.has(accepted)) {
          const as = tcpSocketStates.get(accepted);
          as.connId = info.acceptedConnId;
          as.peerConnId = info.clientConnId;
          as.connected = true;
          as.localAddress = info.serverAddress;
          as.localPort = info.serverPort;
          as.remoteAddress = info.clientAddress;
          as.remotePort = info.clientPort;
          if (streamStates.get(accepted)) streamStates.get(accepted).open = true;
          tcpSocketMap.set(as.connId, accepted);
        }
        serverSock.dispatchEvent(
          new ServerSocketConnectEvent(
            ServerSocketConnectEvent.CONNECT,
            false,
            false,
            accepted
          )
        );
        return { info, accepted };
      } catch (_) {
        return null;
      }
    }

    function dispose() {
      if (disposed) return;
      disposed = true;
      generation++;
      while (hostCleanups.length > 0) {
        const fn = hostCleanups.pop();
        try { fn(); } catch (_) {}
      }
      openWindows.length = 0;
      mainNativeWindow = null;
      activeNativeWindow = null;
      nativeAppSingleton = null;
      dockIconSingleton = null;
      volumeInfoSingleton = null;
      generalClipboardSingleton = null;
      mainScreenSingleton = null;
      networkInfoSingleton = null;
      activeStageText = null;
      lastCameraRollFile = null;
      gameInputDevices = null;
      udpSocketMap.clear();
      serverSocketMap.clear();
      tcpSocketMap.clear();
      if (kernel && typeof kernel.free === 'function') {
        try {
          kernel.free();
        } catch (_) {}
      }
      kernel = null;
    }

    return Object.freeze({
      builtins,
      IFilePromise,
      IURIDereferencer,
      setVectorFactory,
      augmentBuiltins,
      hasDisplayProperty,
      getDisplayProperty,
      setDisplayProperty,
      isDataInput: value => streamStates.has(value),
      isDataOutput: value => streamStates.has(value),
      tryConnectServerSocket,
      notifyUserInput: () => { lastInputTimestamp = Date.now(); },
      bindStageAirProperties,
      reset,
      dispose,
    });
  }

  const api = Object.freeze({ create });
  global.NeoFlashAIR = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
