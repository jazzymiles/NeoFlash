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
    const getViewportSize = options.getViewportSize || (() => [800, 600]);
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
        return s.entries.has(String(format));
      }
      setData(format, data, serializable = true) {
        const s = clipboardStates.get(this);
        if (!s) throw new TypeError('Expected a Clipboard instance.');
        if (format == null || String(format) === '' || data === null || data === undefined) {
          return false;
        }
        const fmt = String(format);
        const canSerialize = Boolean(serializable);
        const cloned = canSerialize ? cloneClipboardValue(data, true) : undefined;
        s.entries.set(fmt, {
          kind: 'value',
          original: data,
          cloned,
          serializable: canSerialize,
        });
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
        const entry = s.entries.get(String(format));
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
            acceptedTarget: null,
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
            if (target && typeof target.dispatchEvent === 'function' && dragState.dropAction !== 'none') {
              target.dispatchEvent(
                new NativeDragEvent(
                  NativeDragEvent.NATIVE_DRAG_DROP,
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
        setVsyncEnabled: v => { stageOrientationState.vsyncEnabled = Boolean(v); },
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

      copy() { return false; }
      cut() { return false; }
      paste() { return false; }
      clear() { return false; }
      selectAll() { return false; }

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
        if (encryptionKey != null) {
          throw new SQLError('open', 'Encrypted SQL databases are not supported in the HTML5 sandbox.', '', 3138);
        }
        const refPath = resolveSqlReferencePath(reference);
        const id = callSqlKernel(
          'open',
          'sql_open',
          refPath ?? undefined,
          String(openMode ?? 'create'),
          Boolean(autoCompact),
          Number(pageSize) >>> 0 || 1024,
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
        s.isAsync = true;
        runSqlConnOp(this, s, true, 'open', SQLEvent.OPEN, responder, () => {
          if (encryptionKey != null) {
            throw new SQLError('open', 'Encrypted SQL databases are not supported in the HTML5 sandbox.', '', 3138);
          }
          const refPath = resolveSqlReferencePath(reference);
          const id = callSqlKernel(
            'open',
            'sql_open',
            refPath ?? undefined,
            String(openMode ?? 'create'),
            Boolean(autoCompact),
            Number(pageSize) >>> 0 || 1024,
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
        runSqlConnOp(this, s, s.isAsync, 'attach', SQLEvent.ATTACH, responder, () => {
          if (encryptionKey != null) {
            throw new SQLError('attach', 'Encrypted SQL databases are not supported in the HTML5 sandbox.', '', 3138);
          }
          const refPath = resolveSqlReferencePath(reference);
          callSqlKernel('attach', 'sql_attach', s.connId, String(name ?? ''), refPath ?? undefined, Date.now());
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

      reencrypt(_newEncryptionKey, responder = null) {
        const s = connState(this);
        if (s.connId === null) throw illegalOp('SQLConnection is not open.', 3104);
        runSqlConnOp(this, s, s.isAsync, 'reencrypt', SQLEvent.REENCRYPT, responder, () => {
          throw new SQLError('reencrypt', 'Encrypted SQL databases are not supported in the HTML5 sandbox.', '', 3138);
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

    class DeviceRotation extends BaseDispatcher {
      constructor() {
        super();
      }
      static get isSupported() { return false; }
      get muted() { return true; }
      setRequestedUpdateInterval(interval) {
        if (Number(interval) < 0 || Number.isNaN(Number(interval))) {
          throw argErr('DeviceRotation.setRequestedUpdateInterval requires a non-negative interval.');
        }
      }
    }
    tagType(DeviceRotation, 'DeviceRotation', v => v instanceof DeviceRotation);

    const geoStates = new WeakMap();
    class Geolocation extends BaseDispatcher {
      constructor() {
        super();
        geoStates.set(this, {
          desiredAccuracy: 0,
          locationAlwaysUsePermission: false,
          pausesLocationUpdatesAutomatically: false,
          interval: 0,
        });
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

    // -------------------------------------------------------------------------
    // 8. Builtins Registry, Reset & Dispose
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

    function dispose() {
      if (disposed) return;
      disposed = true;
      generation++;
      openWindows.length = 0;
      mainNativeWindow = null;
      activeNativeWindow = null;
      nativeAppSingleton = null;
      dockIconSingleton = null;
      volumeInfoSingleton = null;
      generalClipboardSingleton = null;
      mainScreenSingleton = null;
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
      setVectorFactory,
      isDataInput: value => streamStates.has(value),
      isDataOutput: value => streamStates.has(value),
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
