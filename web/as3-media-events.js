(function (global) {
  'use strict';

  function makeArgumentError(message, code = 2008) {
    const err = new Error(message);
    err.name = 'ArgumentError';
    err.errorID = code;
    return err;
  }

  function makeRangeError(message, code = 2006) {
    const err = new RangeError(message);
    err.errorID = code;
    return err;
  }

  function makeIllegalOperationError(message, code = 2004) {
    const err = new Error(message);
    err.name = 'IllegalOperationError';
    err.errorID = code;
    return err;
  }

  function translateKernelError(raw) {
    const msg = String(raw?.message ?? raw);
    const match = /^(ArgumentError|RangeError|IllegalOperationError):(\d+):(.*)$/s.exec(msg);
    if (match) {
      const kind = match[1];
      const code = Number(match[2]) | 0;
      const body = match[3].trim();
      if (kind === 'ArgumentError') return makeArgumentError(body, code);
      if (kind === 'RangeError') return makeRangeError(body, code);
      return makeIllegalOperationError(body, code);
    }
    return raw instanceof Error ? raw : new Error(msg);
  }

  function callKernel(fn) {
    try {
      return fn();
    } catch (error) {
      throw translateKernelError(error);
    }
  }

  function tagType(Type, name, predicate, initFn = null) {
    const check = predicate || (value => value instanceof Type);
    Object.defineProperties(Type, {
      __nfIs: { value: check, configurable: true },
      [Symbol.hasInstance]: { value: check, configurable: true },
      __nfInitialize: {
        value: initFn || (() => {
          throw new TypeError(`${name} cannot be subclassed in user code.`);
        }),
        configurable: true,
      },
    });
    return Type;
  }

  function tagConstantsClass(name, constants) {
    const Holder = {
      [name]: class {
        constructor() {
          throw makeArgumentError(`Error #2012: ${name}$ class cannot be instantiated.`, 2012);
        }
      },
    }[name];
    Object.assign(Holder, constants);
    tagType(Holder, name, () => false);
    return Object.freeze(Holder);
  }

  const stageHookSym = Symbol.for('neoflash.mediaEvents.stageHooks');
  function installStagePrototypeGetter(proto) {
    if (!proto || proto === Object.prototype) return;
    if (!proto[stageHookSym]) {
      Object.defineProperty(proto, stageHookSym, {
        value: { byStage: new WeakMap(), availByStage: new WeakMap(), fallback: null, availFallback: null },
      });
    }
    if (!Object.hasOwn(proto, 'stageVideos')) {
      Object.defineProperty(proto, 'stageVideos', {
        enumerable: true,
        configurable: true,
        get() {
          const store = proto[stageHookSym];
          const fn = store.byStage.get(this) || store.fallback;
          return fn ? fn(this) : [];
        },
      });
    }
  }

  function wrapStageAvailabilityListener(stageInstance, onListenerAdded) {
    if (!stageInstance || typeof stageInstance !== 'object') return;
    const origAdd = stageInstance.addEventListener;
    if (typeof origAdd !== 'function' || origAdd.__nfStageVideoWrapped) return;
    const wrapped = function (type, listener, useCapture = false, priority = 0, useWeakReference = false) {
      const res = Reflect.apply(origAdd, this, [type, listener, useCapture, priority, useWeakReference]);
      if (type === 'stageVideoAvailability' && typeof onListenerAdded === 'function') {
        onListenerAdded(this);
      }
      return res;
    };
    wrapped.__nfStageVideoWrapped = true;
    try {
      Object.defineProperty(stageInstance, 'addEventListener', {
        value: wrapped,
        writable: true,
        configurable: true,
      });
    } catch (_) {}
  }

  function bindStageMediaProperties(StageClass, getVectorForStage, stageInstance = null, onAvailabilityListener = null) {
    const proto =
      StageClass?.prototype ||
      (stageInstance && typeof stageInstance === 'object' ? Object.getPrototypeOf(stageInstance) : null);
    if (!proto || proto === Object.prototype) return;
    installStagePrototypeGetter(proto);
    const store = proto[stageHookSym];
    if (stageInstance && typeof stageInstance === 'object') {
      store.byStage.set(stageInstance, getVectorForStage);
      if (onAvailabilityListener) {
        store.availByStage.set(stageInstance, onAvailabilityListener);
        wrapStageAvailabilityListener(stageInstance, onAvailabilityListener);
      }
    } else {
      store.fallback = getVectorForStage;
      if (onAvailabilityListener) store.availFallback = onAvailabilityListener;
    }
  }

  const loaderUncaughtMap = new WeakMap();
  function bindLoaderInfoProperties(LoaderInfoClass, createUncaughtErrorEvents) {
    if (!LoaderInfoClass || !LoaderInfoClass.prototype) return;
    const prev = Object.getOwnPropertyDescriptor(LoaderInfoClass.prototype, 'uncaughtErrorEvents');
    Object.defineProperty(LoaderInfoClass.prototype, 'uncaughtErrorEvents', {
      enumerable: true,
      configurable: true,
      get() {
        if (prev?.get) {
          const existing = prev.get.call(this);
          if (existing && existing.constructor?.name === 'UncaughtErrorEvents') return existing;
        }
        let target = loaderUncaughtMap.get(this);
        if (!target) {
          target = createUncaughtErrorEvents();
          loaderUncaughtMap.set(this, target);
        }
        return target;
      },
    });
  }

  function create(options = {}) {
    const wasm = options.wasm;
    if (typeof wasm?.As3MediaEventsKernel !== 'function') {
      throw new Error('AS3 Media/Events runtime requires wasm.As3MediaEventsKernel.');
    }

    const events = options.events || global.NeoFlashEvents;
    if (!events?.Event || !events?.EventDispatcher) {
      throw new Error('AS3 Media/Events runtime requires NeoFlashEvents.');
    }
    const BaseEvent = events.Event;
    const BaseErrorEvent = events.ErrorEvent || events.TextEvent || BaseEvent;
    const BaseDispatcher = events.EventDispatcher;
    const StatusEvent = events.StatusEvent;
    const ActivityEvent = events.ActivityEvent;
    const ContextMenuEvent = events.ContextMenuEvent;
    const binary = options.binary;
    const geometry = options.geometry;
    const net = options.net;

    const hostTouchPoints = Number(
      options.maxTouchPoints ?? global.navigator?.maxTouchPoints ?? 5
    );
    let kernel = new wasm.As3MediaEventsKernel(
      Number.isFinite(hostTouchPoints) ? hostTouchPoints | 0 : 5
    );
    let disposed = false;
    let vectorFactory = typeof options.vectorFactory === 'function' ? options.vectorFactory : null;
    let cachedStageVideosVector = null;

    function setVectorFactory(fn) {
      vectorFactory = typeof fn === 'function' ? fn : null;
      cachedStageVideosVector = null;
    }

    function makeTypedVector(elementTypeName, items = [], fixed = false) {
      const list = Array.from(items);
      if (vectorFactory) {
        const vec = vectorFactory(elementTypeName, list);
        if (fixed && vec && typeof vec === 'object') {
          try { vec.fixed = true; } catch (_) {}
        }
        return vec;
      }
      return Object.assign(list, { fixed: Boolean(fixed) });
    }

    function fmtEvent(ev, className, fields = []) {
      try {
        return ev.formatToString(className, 'type', 'bubbles', 'cancelable', 'eventPhase', ...fields);
      } catch (_) {
        const all = ['type', 'bubbles', 'cancelable', 'eventPhase', ...fields];
        const parts = all.map(k => {
          const v = ev[k];
          return `${k}=${typeof v === 'string' ? `"${v}"` : String(v)}`;
        });
        return `[${className} ${parts.join(' ')}]`;
      }
    }

    function makePoint(x, y) {
      return geometry?.Point ? new geometry.Point(x, y) : { x: Number(x), y: Number(y) };
    }

    function makeRectangle(x, y, width, height) {
      return geometry?.Rectangle
        ? new geometry.Rectangle(x, y, width, height)
        : { x: Number(x), y: Number(y), width: Number(width), height: Number(height) };
    }

    function isByteArray(value) {
      if (!value || typeof value !== 'object') return false;
      if (binary?.ByteArray?.__nfIs) return binary.ByteArray.__nfIs(value);
      return typeof value.writeByte === 'function' && typeof value.readUnsignedByte === 'function';
    }

    // =========================================================================
    // 1. flash.events.* (21 media / AV / DRM / shader / stage-video / uncaught classes)
    // =========================================================================

    const avDictStates = new WeakMap();
    class AVDictionaryDataEvent extends BaseEvent {
      constructor(
        type = 'avDictionaryData',
        bubbles = false,
        cancelable = false,
        init_dictionary = null,
        init_dataTime = 0
      ) {
        super(type, bubbles, cancelable);
        avDictStates.set(this, {
          dictionary: init_dictionary ?? null,
          time: Number(init_dataTime),
        });
      }
      get dictionary() { return avDictStates.get(this).dictionary; }
      get time() { return avDictStates.get(this).time; }
      clone() {
        return new AVDictionaryDataEvent(this.type, this.bubbles, this.cancelable, this.dictionary, this.time);
      }
      toString() {
        return fmtEvent(this, 'AVDictionaryDataEvent', ['dictionary', 'time']);
      }
    }
    AVDictionaryDataEvent.AV_DICTIONARY_DATA = 'avDictionaryData';
    tagType(AVDictionaryDataEvent, 'AVDictionaryDataEvent', v => avDictStates.has(v));

    const avHttpStates = new WeakMap();
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
        avHttpStates.set(this, {
          status: Number(status) | 0,
          responseURL: responseUrl == null ? null : String(responseUrl),
          responseHeaders: Array.isArray(responseHeaders) ? responseHeaders : (responseHeaders ?? null),
        });
      }
      get status() { return avHttpStates.get(this).status; }
      get responseURL() { return avHttpStates.get(this).responseURL; }
      set responseURL(v) { avHttpStates.get(this).responseURL = v == null ? null : String(v); }
      get responseHeaders() { return avHttpStates.get(this).responseHeaders; }
      set responseHeaders(v) {
        if (v !== null && v !== undefined && !Array.isArray(v)) {
          throw new TypeError('AVHTTPStatusEvent.responseHeaders requires an Array or null.');
        }
        avHttpStates.get(this).responseHeaders = v ?? null;
      }
      clone() {
        const headers = Array.isArray(this.responseHeaders) ? this.responseHeaders.slice() : this.responseHeaders;
        return new AVHTTPStatusEvent(this.type, this.bubbles, this.cancelable, this.status, this.responseURL, headers);
      }
      toString() {
        return fmtEvent(this, 'AVHTTPStatusEvent', ['status', 'responseURL']);
      }
    }
    AVHTTPStatusEvent.AV_HTTP_RESPONSE_STATUS = 'avHttpResponseStatus';
    tagType(AVHTTPStatusEvent, 'AVHTTPStatusEvent', v => avHttpStates.has(v));

    const audioOutStates = new WeakMap();
    class AudioOutputChangeEvent extends BaseEvent {
      constructor(
        type = 'audioOutputChange',
        bubbles = false,
        cancelable = false,
        reason = null
      ) {
        super(type, bubbles, cancelable);
        audioOutStates.set(this, { reason: reason == null ? null : String(reason) });
      }
      get reason() { return audioOutStates.get(this).reason; }
      clone() {
        return new AudioOutputChangeEvent(this.type, this.bubbles, this.cancelable, this.reason);
      }
      toString() {
        return fmtEvent(this, 'AudioOutputChangeEvent', ['reason']);
      }
    }
    AudioOutputChangeEvent.AUDIO_OUTPUT_CHANGE = 'audioOutputChange';
    tagType(AudioOutputChangeEvent, 'AudioOutputChangeEvent', v => audioOutStates.has(v));

    const drmAuthOkStates = new WeakMap();
    class DRMAuthenticationCompleteEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        inServerURL = null,
        inDomain = null,
        inToken = null
      ) {
        super(type, bubbles, cancelable);
        if (inToken !== null && inToken !== undefined && !isByteArray(inToken)) {
          throw new TypeError('DRMAuthenticationCompleteEvent.token requires a ByteArray or null.');
        }
        drmAuthOkStates.set(this, {
          serverURL: inServerURL == null ? null : String(inServerURL),
          domain: inDomain == null ? null : String(inDomain),
          token: inToken ?? null,
        });
      }
      get serverURL() { return drmAuthOkStates.get(this).serverURL; }
      set serverURL(v) { drmAuthOkStates.get(this).serverURL = v == null ? null : String(v); }
      get domain() { return drmAuthOkStates.get(this).domain; }
      set domain(v) { drmAuthOkStates.get(this).domain = v == null ? null : String(v); }
      get token() { return drmAuthOkStates.get(this).token; }
      set token(v) {
        if (v !== null && v !== undefined && !isByteArray(v)) {
          throw new TypeError('DRMAuthenticationCompleteEvent.token requires a ByteArray or null.');
        }
        drmAuthOkStates.get(this).token = v ?? null;
      }
      clone() {
        let tok = this.token;
        if (tok && binary?.ByteArray) {
          const copy = new binary.ByteArray();
          if (tok.length > 0) copy.writeBytes(tok, 0, tok.length);
          copy.position = 0;
          tok = copy;
        }
        return new DRMAuthenticationCompleteEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.serverURL,
          this.domain,
          tok
        );
      }
      toString() {
        return fmtEvent(this, 'DRMAuthenticationCompleteEvent', ['serverURL', 'domain', 'token']);
      }
    }
    DRMAuthenticationCompleteEvent.AUTHENTICATION_COMPLETE = 'authenticationComplete';
    tagType(DRMAuthenticationCompleteEvent, 'DRMAuthenticationCompleteEvent', v => drmAuthOkStates.has(v));

    const drmAuthErrStates = new WeakMap();
    class DRMAuthenticationErrorEvent extends BaseErrorEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        inDetail = '',
        inErrorID = 0,
        inSubErrorID = 0,
        inServerURL = null,
        inDomain = null
      ) {
        super(type, bubbles, cancelable, String(inDetail ?? ''), Number(inErrorID) | 0);
        drmAuthErrStates.set(this, {
          subErrorID: Number(inSubErrorID) | 0,
          serverURL: inServerURL == null ? null : String(inServerURL),
          domain: inDomain == null ? null : String(inDomain),
        });
      }
      get subErrorID() { return drmAuthErrStates.get(this).subErrorID; }
      set subErrorID(v) { drmAuthErrStates.get(this).subErrorID = Number(v) | 0; }
      get serverURL() { return drmAuthErrStates.get(this).serverURL; }
      set serverURL(v) { drmAuthErrStates.get(this).serverURL = v == null ? null : String(v); }
      get domain() { return drmAuthErrStates.get(this).domain; }
      set domain(v) { drmAuthErrStates.get(this).domain = v == null ? null : String(v); }
      clone() {
        return new DRMAuthenticationErrorEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.text,
          this.errorID,
          this.subErrorID,
          this.serverURL,
          this.domain
        );
      }
      toString() {
        return fmtEvent(this, 'DRMAuthenticationErrorEvent', ['text', 'errorID', 'subErrorID', 'serverURL', 'domain']);
      }
    }
    DRMAuthenticationErrorEvent.AUTHENTICATION_ERROR = 'authenticationError';
    tagType(DRMAuthenticationErrorEvent, 'DRMAuthenticationErrorEvent', v => drmAuthErrStates.has(v));

    const drmGroupErrStates = new WeakMap();
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
        drmGroupErrStates.set(this, {
          subErrorID: Number(subErrorID) | 0,
          deviceGroup: deviceGroup ?? null,
          systemUpdateNeeded: Boolean(systemUpdateNeeded),
          drmUpdateNeeded: Boolean(drmUpdateNeeded),
        });
      }
      get subErrorID() { return drmGroupErrStates.get(this).subErrorID; }
      set subErrorID(v) { drmGroupErrStates.get(this).subErrorID = Number(v) | 0; }
      get deviceGroup() { return drmGroupErrStates.get(this).deviceGroup; }
      set deviceGroup(v) { drmGroupErrStates.get(this).deviceGroup = v ?? null; }
      get systemUpdateNeeded() { return drmGroupErrStates.get(this).systemUpdateNeeded; }
      get drmUpdateNeeded() { return drmGroupErrStates.get(this).drmUpdateNeeded; }
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
    tagType(DRMDeviceGroupErrorEvent, 'DRMDeviceGroupErrorEvent', v => drmGroupErrStates.has(v));

    const drmGroupStates = new WeakMap();
    class DRMDeviceGroupEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, deviceGroup = null) {
        super(type, bubbles, cancelable);
        drmGroupStates.set(this, { deviceGroup: deviceGroup ?? null });
      }
      get deviceGroup() { return drmGroupStates.get(this).deviceGroup; }
      set deviceGroup(v) { drmGroupStates.get(this).deviceGroup = v ?? null; }
      clone() {
        return new DRMDeviceGroupEvent(this.type, this.bubbles, this.cancelable, this.deviceGroup);
      }
      toString() {
        return fmtEvent(this, 'DRMDeviceGroupEvent', ['deviceGroup']);
      }
    }
    Object.assign(DRMDeviceGroupEvent, {
      ADD_TO_DEVICE_GROUP_COMPLETE: 'addToDeviceGroupComplete',
      REMOVE_FROM_DEVICE_GROUP_COMPLETE: 'removeFromDeviceGroupComplete',
    });
    tagType(DRMDeviceGroupEvent, 'DRMDeviceGroupEvent', v => drmGroupStates.has(v));

    const drmErrStates = new WeakMap();
    class DRMErrorEvent extends BaseErrorEvent {
      constructor(
        type = 'drmError',
        bubbles = false,
        cancelable = false,
        inErrorDetail = '',
        inErrorCode = 0,
        insubErrorID = 0,
        inMetadata = null,
        inSystemUpdateNeeded = false,
        inDrmUpdateNeeded = false
      ) {
        super(type, bubbles, cancelable, String(inErrorDetail ?? ''), Number(inErrorCode) | 0);
        drmErrStates.set(this, {
          subErrorID: Number(insubErrorID) | 0,
          contentData: inMetadata ?? null,
          systemUpdateNeeded: Boolean(inSystemUpdateNeeded),
          drmUpdateNeeded: Boolean(inDrmUpdateNeeded),
        });
      }
      get contentData() { return drmErrStates.get(this).contentData; }
      set contentData(v) { drmErrStates.get(this).contentData = v ?? null; }
      get subErrorID() { return drmErrStates.get(this).subErrorID; }
      get systemUpdateNeeded() { return drmErrStates.get(this).systemUpdateNeeded; }
      get drmUpdateNeeded() { return drmErrStates.get(this).drmUpdateNeeded; }
      clone() {
        return new DRMErrorEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.text,
          this.errorID,
          this.subErrorID,
          this.contentData,
          this.systemUpdateNeeded,
          this.drmUpdateNeeded
        );
      }
      toString() {
        return fmtEvent(this, 'DRMErrorEvent', [
          'text',
          'errorID',
          'subErrorID',
          'systemUpdateNeeded',
          'drmUpdateNeeded',
        ]);
      }
    }
    Object.assign(DRMErrorEvent, {
      DRM_ERROR: 'drmError',
      DRM_LOAD_DEVICEID_ERROR: 'drmLoadDeviceIdError',
    });
    tagType(DRMErrorEvent, 'DRMErrorEvent', v => drmErrStates.has(v));

    const drmLicReqStates = new WeakMap();
    class DRMLicenseRequestEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, inServerURL = null) {
        super(type, bubbles, cancelable);
        drmLicReqStates.set(this, { serverURL: inServerURL == null ? null : String(inServerURL) });
      }
      get serverURL() { return drmLicReqStates.get(this).serverURL; }
      set serverURL(v) { drmLicReqStates.get(this).serverURL = v == null ? null : String(v); }
      clone() {
        return new DRMLicenseRequestEvent(this.type, this.bubbles, this.cancelable, this.serverURL);
      }
      toString() {
        return fmtEvent(this, 'DRMLicenseRequestEvent', ['serverURL']);
      }
    }
    DRMLicenseRequestEvent.LICENSE_REQUEST = 'licenseRequest';
    tagType(DRMLicenseRequestEvent, 'DRMLicenseRequestEvent', v => drmLicReqStates.has(v));

    const drmMetaStates = new WeakMap();
    class DRMMetadataEvent extends BaseEvent {
      constructor(
        type = 'drmMetadata',
        bubbles = false,
        cancelable = false,
        inMetadata = null,
        inTimestamp = 0
      ) {
        super(type, bubbles, cancelable);
        drmMetaStates.set(this, {
          drmMetadata: inMetadata ?? null,
          timestamp: Number(inTimestamp),
        });
      }
      get drmMetadata() { return drmMetaStates.get(this).drmMetadata; }
      get timestamp() { return drmMetaStates.get(this).timestamp; }
      clone() {
        return new DRMMetadataEvent(this.type, this.bubbles, this.cancelable, this.drmMetadata, this.timestamp);
      }
      toString() {
        return fmtEvent(this, 'DRMMetadataEvent', ['drmMetadata', 'timestamp']);
      }
    }
    DRMMetadataEvent.DRM_METADATA = 'drmMetadata';
    tagType(DRMMetadataEvent, 'DRMMetadataEvent', v => drmMetaStates.has(v));

    const drmRetOkStates = new WeakMap();
    class DRMReturnVoucherCompleteEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        inServerURL = null,
        inLicenseID = null,
        inPolicyID = null,
        inNumberOfVouchersReturned = 0
      ) {
        super(type, bubbles, cancelable);
        drmRetOkStates.set(this, {
          serverURL: inServerURL == null ? null : String(inServerURL),
          licenseID: inLicenseID == null ? null : String(inLicenseID),
          policyID: inPolicyID == null ? null : String(inPolicyID),
          numberOfVouchersReturned: Number(inNumberOfVouchersReturned) | 0,
        });
      }
      get serverURL() { return drmRetOkStates.get(this).serverURL; }
      set serverURL(v) { drmRetOkStates.get(this).serverURL = v == null ? null : String(v); }
      get licenseID() { return drmRetOkStates.get(this).licenseID; }
      set licenseID(v) { drmRetOkStates.get(this).licenseID = v == null ? null : String(v); }
      get policyID() { return drmRetOkStates.get(this).policyID; }
      set policyID(v) { drmRetOkStates.get(this).policyID = v == null ? null : String(v); }
      get numberOfVouchersReturned() { return drmRetOkStates.get(this).numberOfVouchersReturned; }
      set numberOfVouchersReturned(v) { drmRetOkStates.get(this).numberOfVouchersReturned = Number(v) | 0; }
      clone() {
        return new DRMReturnVoucherCompleteEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.serverURL,
          this.licenseID,
          this.policyID,
          this.numberOfVouchersReturned
        );
      }
      toString() {
        return fmtEvent(this, 'DRMReturnVoucherCompleteEvent', [
          'serverURL',
          'licenseID',
          'policyID',
          'numberOfVouchersReturned',
        ]);
      }
    }
    DRMReturnVoucherCompleteEvent.RETURN_VOUCHER_COMPLETE = 'returnVoucherComplete';
    tagType(DRMReturnVoucherCompleteEvent, 'DRMReturnVoucherCompleteEvent', v => drmRetOkStates.has(v));

    const drmRetErrStates = new WeakMap();
    class DRMReturnVoucherErrorEvent extends BaseErrorEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        inDetail = '',
        inErrorID = 0,
        inSubErrorID = 0,
        inServerURL = null,
        inLicenseID = null,
        inPolicyID = null
      ) {
        super(type, bubbles, cancelable, String(inDetail ?? ''), Number(inErrorID) | 0);
        drmRetErrStates.set(this, {
          subErrorID: Number(inSubErrorID) | 0,
          serverURL: inServerURL == null ? null : String(inServerURL),
          licenseID: inLicenseID == null ? null : String(inLicenseID),
          policyID: inPolicyID == null ? null : String(inPolicyID),
        });
      }
      get subErrorID() { return drmRetErrStates.get(this).subErrorID; }
      set subErrorID(v) { drmRetErrStates.get(this).subErrorID = Number(v) | 0; }
      get serverURL() { return drmRetErrStates.get(this).serverURL; }
      set serverURL(v) { drmRetErrStates.get(this).serverURL = v == null ? null : String(v); }
      get licenseID() { return drmRetErrStates.get(this).licenseID; }
      set licenseID(v) { drmRetErrStates.get(this).licenseID = v == null ? null : String(v); }
      get policyID() { return drmRetErrStates.get(this).policyID; }
      set policyID(v) { drmRetErrStates.get(this).policyID = v == null ? null : String(v); }
      clone() {
        return new DRMReturnVoucherErrorEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.text,
          this.errorID,
          this.subErrorID,
          this.serverURL,
          this.licenseID,
          this.policyID
        );
      }
      toString() {
        return fmtEvent(this, 'DRMReturnVoucherErrorEvent', [
          'text',
          'errorID',
          'subErrorID',
          'serverURL',
          'licenseID',
          'policyID',
        ]);
      }
    }
    DRMReturnVoucherErrorEvent.RETURN_VOUCHER_ERROR = 'returnVoucherError';
    tagType(DRMReturnVoucherErrorEvent, 'DRMReturnVoucherErrorEvent', v => drmRetErrStates.has(v));

    const drmStatusStates = new WeakMap();
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
        drmStatusStates.set(this, {
          contentData: inMetadata ?? null,
          voucher: inVoucher ?? null,
          isLocal: Boolean(inLocal),
        });
      }
      get contentData() { return drmStatusStates.get(this).contentData; }
      set contentData(v) { drmStatusStates.get(this).contentData = v ?? null; }
      get voucher() { return drmStatusStates.get(this).voucher; }
      set voucher(v) { drmStatusStates.get(this).voucher = v ?? null; }
      get isLocal() { return drmStatusStates.get(this).isLocal; }
      set isLocal(v) { drmStatusStates.get(this).isLocal = Boolean(v); }
      get detail() { return 'DRM.voucherObtained'; }
      get isAnonymous() { return false; }
      get isAvailableOffline() { return false; }
      get offlineLeasePeriod() { return 0; }
      get policies() { return null; }
      get voucherEndDate() { return null; }
      clone() {
        return new DRMStatusEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.contentData,
          this.voucher,
          this.isLocal
        );
      }
      toString() {
        return fmtEvent(this, 'DRMStatusEvent', ['isLocal', 'detail']);
      }
    }
    DRMStatusEvent.DRM_STATUS = 'drmStatus';
    tagType(DRMStatusEvent, 'DRMStatusEvent', v => drmStatusStates.has(v));

    const netMonitorStates = new WeakMap();
    class NetMonitorEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, netStream = null) {
        super(type, bubbles, cancelable);
        netMonitorStates.set(this, { netStream: netStream ?? null });
      }
      get netStream() { return netMonitorStates.get(this).netStream; }
      clone() {
        return new NetMonitorEvent(this.type, this.bubbles, this.cancelable, this.netStream);
      }
      toString() {
        return fmtEvent(this, 'NetMonitorEvent', ['netStream']);
      }
    }
    NetMonitorEvent.NET_STREAM_CREATE = 'netStreamCreate';
    tagType(NetMonitorEvent, 'NetMonitorEvent', v => netMonitorStates.has(v));

    const netFilterStates = new WeakMap();
    class NetFilterEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, header = null, data = null) {
        super(type, bubbles, cancelable);
        netFilterStates.set(this, { header: header ?? null, data: data ?? null });
      }
      get header() { return netFilterStates.get(this).header; }
      set header(v) { netFilterStates.get(this).header = v ?? null; }
      get data() { return netFilterStates.get(this).data; }
      set data(v) { netFilterStates.get(this).data = v ?? null; }
      clone() {
        return new NetFilterEvent(this.type, this.bubbles, this.cancelable, this.header, this.data);
      }
      toString() {
        return fmtEvent(this, 'NetFilterEvent', ['header', 'data']);
      }
    }
    tagType(NetFilterEvent, 'NetFilterEvent', v => netFilterStates.has(v));

    const shaderEventStates = new WeakMap();
    class ShaderEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        bitmap = null,
        array = null,
        vector = null
      ) {
        super(type, bubbles, cancelable);
        shaderEventStates.set(this, {
          bitmapData: bitmap ?? null,
          byteArray: array ?? null,
          vector: vector ?? null,
        });
      }
      get bitmapData() { return shaderEventStates.get(this).bitmapData; }
      set bitmapData(v) { shaderEventStates.get(this).bitmapData = v ?? null; }
      get byteArray() { return shaderEventStates.get(this).byteArray; }
      set byteArray(v) { shaderEventStates.get(this).byteArray = v ?? null; }
      get vector() { return shaderEventStates.get(this).vector; }
      set vector(v) { shaderEventStates.get(this).vector = v ?? null; }
      clone() {
        return new ShaderEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.bitmapData,
          this.byteArray,
          this.vector
        );
      }
      toString() {
        return fmtEvent(this, 'ShaderEvent', ['bitmapData', 'byteArray', 'vector']);
      }
    }
    ShaderEvent.COMPLETE = 'complete';
    tagType(ShaderEvent, 'ShaderEvent', v => shaderEventStates.has(v));

    const stageVideoAvailStates = new WeakMap();
    class StageVideoAvailabilityEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        availability = 'unavailable',
        reason = 'unavailable'
      ) {
        super(type, bubbles, cancelable);
        stageVideoAvailStates.set(this, {
          availability: availability == null ? null : String(availability),
          reason: reason == null ? null : String(reason),
          driver: 'Software',
        });
      }
      get availability() { return stageVideoAvailStates.get(this).availability; }
      get reason() { return stageVideoAvailStates.get(this).reason; }
      get driver() { return stageVideoAvailStates.get(this).driver; }
      clone() {
        return new StageVideoAvailabilityEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.availability,
          this.reason
        );
      }
      toString() {
        return fmtEvent(this, 'StageVideoAvailabilityEvent', ['availability', 'reason']);
      }
    }
    StageVideoAvailabilityEvent.STAGE_VIDEO_AVAILABILITY = 'stageVideoAvailability';
    tagType(StageVideoAvailabilityEvent, 'StageVideoAvailabilityEvent', v => stageVideoAvailStates.has(v));

    const stageVideoEvStates = new WeakMap();
    class StageVideoEvent extends BaseEvent {
      constructor(
        type,
        bubbles = false,
        cancelable = false,
        status = null,
        colorSpace = null,
        codecInfo = null
      ) {
        super(type, bubbles, cancelable);
        stageVideoEvStates.set(this, {
          status: status == null ? null : String(status),
          colorSpace: colorSpace == null ? null : String(colorSpace),
          codecInfo: codecInfo == null ? null : String(codecInfo),
        });
      }
      get status() { return stageVideoEvStates.get(this).status; }
      get colorSpace() { return stageVideoEvStates.get(this).colorSpace; }
      get codecInfo() { return stageVideoEvStates.get(this).codecInfo; }
      clone() {
        return new StageVideoEvent(
          this.type,
          this.bubbles,
          this.cancelable,
          this.status,
          this.colorSpace,
          this.codecInfo
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
      codecInfo: 'codecInfo',
    });
    tagType(StageVideoEvent, 'StageVideoEvent', v => stageVideoEvStates.has(v));

    const uncaughtTargets = new WeakSet();
    class UncaughtErrorEvents extends BaseDispatcher {
      constructor() {
        super();
        uncaughtTargets.add(this);
      }
    }
    tagType(UncaughtErrorEvents, 'UncaughtErrorEvents', v => uncaughtTargets.has(v));

    const videoEvStates = new WeakMap();
    class VideoEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, status = null) {
        super(type, bubbles, cancelable);
        videoEvStates.set(this, {
          status: status == null ? null : String(status),
          codecInfo: null,
        });
      }
      get status() { return videoEvStates.get(this).status; }
      get codecInfo() { return videoEvStates.get(this).codecInfo; }
      clone() {
        return new VideoEvent(this.type, this.bubbles, this.cancelable, this.status);
      }
      toString() {
        return fmtEvent(this, 'VideoEvent', ['status']);
      }
    }
    Object.assign(VideoEvent, {
      RENDER_STATE: 'renderState',
      RENDER_STATUS_ACCELERATED: 'accelerated',
      RENDER_STATUS_SOFTWARE: 'software',
      RENDER_STATUS_UNAVAILABLE: 'unavailable',
      codecInfo: 'codecInfo',
    });
    tagType(VideoEvent, 'VideoEvent', v => videoEvStates.has(v));

    const videoTexEvStates = new WeakMap();
    class VideoTextureEvent extends BaseEvent {
      constructor(type, bubbles = false, cancelable = false, status = null, colorSpace = null) {
        super(type, bubbles, cancelable);
        videoTexEvStates.set(this, {
          status: status == null ? null : String(status),
          colorSpace: colorSpace == null ? null : String(colorSpace),
          codecInfo: null,
        });
      }
      get status() { return videoTexEvStates.get(this).status; }
      get colorSpace() { return videoTexEvStates.get(this).colorSpace; }
      get codecInfo() { return videoTexEvStates.get(this).codecInfo; }
      clone() {
        return new VideoTextureEvent(this.type, this.bubbles, this.cancelable, this.status, this.colorSpace);
      }
      toString() {
        return fmtEvent(this, 'VideoTextureEvent', ['status', 'colorSpace']);
      }
    }
    VideoTextureEvent.RENDER_STATE = 'renderState';
    tagType(VideoTextureEvent, 'VideoTextureEvent', v => videoTexEvStates.has(v));

    // =========================================================================
    // 2. flash.media.*
    // =========================================================================

    const AudioDecoder = tagConstantsClass('AudioDecoder', {
      DOLBY_DIGITAL: 'DolbyDigital',
      DOLBY_DIGITAL_PLUS: 'DolbyDigitalPlus',
      DTS: 'DTS',
      DTS_EXPRESS: 'DTSExpress',
      DTS_HD_HIGH_RESOLUTION_AUDIO: 'DTSHDHighResolutionAudio',
      DTS_HD_MASTER_AUDIO: 'DTSHDMasterAudio',
    });

    const AudioOutputChangeReason = tagConstantsClass('AudioOutputChangeReason', {
      DEVICE_CHANGE: 'deviceChange',
      USER_SELECTION: 'userSelection',
    });

    const CameraPosition = tagConstantsClass('CameraPosition', {
      BACK: 'back',
      FRONT: 'front',
      UNKNOWN: 'unknown',
    });

    const H264Level = tagConstantsClass('H264Level', {
      LEVEL_1: '1',
      LEVEL_1B: '1b',
      LEVEL_1_1: '1.1',
      LEVEL_1_2: '1.2',
      LEVEL_1_3: '1.3',
      LEVEL_2: '2',
      LEVEL_2_1: '2.1',
      LEVEL_2_2: '2.2',
      LEVEL_3: '3',
      LEVEL_3_1: '3.1',
      LEVEL_3_2: '3.2',
      LEVEL_4: '4',
      LEVEL_4_1: '4.1',
      LEVEL_4_2: '4.2',
      LEVEL_5: '5',
      LEVEL_5_1: '5.1',
    });

    const H264Profile = tagConstantsClass('H264Profile', {
      BASELINE: 'baseline',
      MAIN: 'main',
    });

    const VideoCodec = tagConstantsClass('VideoCodec', {
      H264AVC: 'H264Avc',
      SORENSON: 'Sorenson',
      VP6: 'VP6',
    });

    const SoundCodec = tagConstantsClass('SoundCodec', {
      NELLYMOSER: 'NellyMoser',
      PCMA: 'pcma',
      PCMU: 'pcmu',
      SPEEX: 'Speex',
    });

    const MicrophoneEnhancedMode = tagConstantsClass('MicrophoneEnhancedMode', {
      FULL_DUPLEX: 'fullDuplex',
      HALF_DUPLEX: 'halfDuplex',
      HEADSET: 'headset',
      OFF: 'off',
      SPEAKER_MUTE: 'speakerMute',
    });

    const StageVideoAvailability = tagConstantsClass('StageVideoAvailability', {
      AVAILABLE: 'available',
      UNAVAILABLE: 'unavailable',
    });

    const StageVideoAvailabilityReason = tagConstantsClass('StageVideoAvailabilityReason', {
      DRIVER_TOO_OLD: 'driverTooOld',
      NO_ERROR: 'noError',
      UNAVAILABLE: 'unavailable',
      USER_DISABLED: 'userDisabled',
      WMODE_INCOMPATIBLE: 'wmodeIncompatible',
    });

    const avTagStates = new WeakMap();
    class AVTagData {
      constructor(data = '', localTime = 0) {
        avTagStates.set(this, {
          data: String(data ?? ''),
          localTime: Number(localTime),
        });
      }
      get data() { return avTagStates.get(this).data; }
      get localTime() { return avTagStates.get(this).localTime; }
    }
    tagType(AVTagData, 'AVTagData', v => avTagStates.has(v));

    const avUrlLoaderStates = new WeakMap();
    class AVURLLoader extends BaseDispatcher {
      constructor(request = null) {
        super();
        const inner = net?.URLLoader ? new net.URLLoader() : null;
        const st = {
          inner,
          cookieHeader: '',
          data: undefined,
          dataFormat: 'text',
          bytesLoaded: 0,
          bytesTotal: 0,
        };
        avUrlLoaderStates.set(this, st);
        if (inner) {
          for (const evName of [
            'open',
            'progress',
            'httpStatus',
            'httpResponseStatus',
            'complete',
            'ioError',
            'securityError',
          ]) {
            inner.addEventListener(evName, ev => {
              st.data = inner.data;
              st.bytesLoaded = inner.bytesLoaded ?? st.bytesLoaded;
              st.bytesTotal = inner.bytesTotal ?? st.bytesTotal;
              this.dispatchEvent(ev.clone());
            });
          }
        }
        if (request !== null && request !== undefined) {
          this.load(request);
        }
      }
      get cookieHeader() { return avUrlLoaderStates.get(this).cookieHeader; }
      set cookieHeader(v) { avUrlLoaderStates.get(this).cookieHeader = v == null ? '' : String(v); }
      get data() {
        const st = avUrlLoaderStates.get(this);
        return st.inner ? st.inner.data : st.data;
      }
      set data(v) {
        const st = avUrlLoaderStates.get(this);
        st.data = v;
        if (st.inner) st.inner.data = v;
      }
      get dataFormat() {
        const st = avUrlLoaderStates.get(this);
        return st.inner ? st.inner.dataFormat : st.dataFormat;
      }
      set dataFormat(v) {
        const st = avUrlLoaderStates.get(this);
        st.dataFormat = String(v);
        if (st.inner) st.inner.dataFormat = st.dataFormat;
      }
      get bytesLoaded() {
        const st = avUrlLoaderStates.get(this);
        return st.inner ? st.inner.bytesLoaded : st.bytesLoaded;
      }
      get bytesTotal() {
        const st = avUrlLoaderStates.get(this);
        return st.inner ? st.inner.bytesTotal : st.bytesTotal;
      }
      load(request) {
        const st = avUrlLoaderStates.get(this);
        if (!st.inner) {
          throw new Error('AVURLLoader.load requires the shared NeoFlashNet service.');
        }
        st.inner.load(request);
      }
      close() {
        const st = avUrlLoaderStates.get(this);
        if (st.inner) st.inner.close();
      }
    }
    tagType(AVURLLoader, 'AVURLLoader', v => avUrlLoaderStates.has(v));

    const avUrlStreamStates = new WeakMap();
    class AVURLStream extends BaseDispatcher {
      constructor() {
        super();
        const inner = net?.URLLoader ? new net.URLLoader() : null;
        if (inner) inner.dataFormat = 'binary';
        const buffer = binary?.ByteArray ? new binary.ByteArray() : null;
        const st = {
          inner,
          buffer,
          cookieHeader: '',
          connected: false,
        };
        avUrlStreamStates.set(this, st);
        if (inner) {
          inner.addEventListener('open', ev => {
            st.connected = true;
            this.dispatchEvent(ev.clone());
          });
          inner.addEventListener('progress', ev => this.dispatchEvent(ev.clone()));
          inner.addEventListener('httpStatus', ev => this.dispatchEvent(ev.clone()));
          inner.addEventListener('httpResponseStatus', ev => this.dispatchEvent(ev.clone()));
          inner.addEventListener('complete', ev => {
            st.connected = true;
            if (isByteArray(inner.data) && st.buffer) {
              st.buffer.clear();
              if (inner.data.length > 0) st.buffer.writeBytes(inner.data, 0, inner.data.length);
              st.buffer.position = 0;
            }
            this.dispatchEvent(ev.clone());
          });
          inner.addEventListener('ioError', ev => {
            st.connected = false;
            this.dispatchEvent(ev.clone());
          });
          inner.addEventListener('securityError', ev => {
            st.connected = false;
            this.dispatchEvent(ev.clone());
          });
        }
      }
      get cookieHeader() { return avUrlStreamStates.get(this).cookieHeader; }
      set cookieHeader(v) { avUrlStreamStates.get(this).cookieHeader = v == null ? '' : String(v); }
      get connected() { return avUrlStreamStates.get(this).connected; }
      get bytesAvailable() {
        const buf = avUrlStreamStates.get(this).buffer;
        return buf ? buf.bytesAvailable : 0;
      }
      get endian() {
        const buf = avUrlStreamStates.get(this).buffer;
        return buf ? buf.endian : 'bigEndian';
      }
      set endian(v) {
        const buf = avUrlStreamStates.get(this).buffer;
        if (buf) buf.endian = v;
      }
      get objectEncoding() {
        const buf = avUrlStreamStates.get(this).buffer;
        return buf ? buf.objectEncoding : 3;
      }
      set objectEncoding(v) {
        const buf = avUrlStreamStates.get(this).buffer;
        if (buf) buf.objectEncoding = v;
      }
      load(request) {
        const st = avUrlStreamStates.get(this);
        if (!st.inner) {
          throw new Error('AVURLStream.load requires the shared NeoFlashNet service.');
        }
        st.inner.dataFormat = 'binary';
        st.inner.load(request);
      }
      close() {
        const st = avUrlStreamStates.get(this);
        st.connected = false;
        if (st.inner) st.inner.close();
      }
      readBoolean() { return avUrlStreamStates.get(this).buffer.readBoolean(); }
      readByte() { return avUrlStreamStates.get(this).buffer.readByte(); }
      readUnsignedByte() { return avUrlStreamStates.get(this).buffer.readUnsignedByte(); }
      readShort() { return avUrlStreamStates.get(this).buffer.readShort(); }
      readUnsignedShort() { return avUrlStreamStates.get(this).buffer.readUnsignedShort(); }
      readInt() { return avUrlStreamStates.get(this).buffer.readInt(); }
      readUnsignedInt() { return avUrlStreamStates.get(this).buffer.readUnsignedInt(); }
      readFloat() { return avUrlStreamStates.get(this).buffer.readFloat(); }
      readDouble() { return avUrlStreamStates.get(this).buffer.readDouble(); }
      readUTF() { return avUrlStreamStates.get(this).buffer.readUTF(); }
      readUTFBytes(length) { return avUrlStreamStates.get(this).buffer.readUTFBytes(length); }
      readMultiByte(length, charSet) { return avUrlStreamStates.get(this).buffer.readMultiByte(length, charSet); }
      readBytes(bytes, offset = 0, length = 0) { return avUrlStreamStates.get(this).buffer.readBytes(bytes, offset, length); }
      readObject() { return avUrlStreamStates.get(this).buffer.readObject(); }
    }
    tagType(AVURLStream, 'AVURLStream', v => avUrlStreamStates.has(v));

    const audioMgrToken = {};
    const audioMgrSet = new WeakSet();
    let audioMgrSingleton = null;
    class AudioDeviceManager extends BaseDispatcher {
      constructor(token) {
        if (token !== audioMgrToken) {
          throw makeArgumentError(
            'Error #2012: AudioDeviceManager class cannot be instantiated directly; use AudioDeviceManager.audioDeviceManager.',
            2012
          );
        }
        super();
        audioMgrSet.add(this);
      }
      static get isSupported() { return true; }
      static get audioDeviceManager() {
        if (!audioMgrSingleton) {
          audioMgrSingleton = new AudioDeviceManager(audioMgrToken);
        }
        return audioMgrSingleton;
      }
      get deviceNames() {
        return JSON.parse(callKernel(() => kernel.audio_device_names_json()));
      }
      get selectedDeviceIndex() {
        return callKernel(() => kernel.selected_audio_device());
      }
      set selectedDeviceIndex(value) {
        const idx = Number(value) | 0;
        const changed = callKernel(() => kernel.select_audio_device(idx));
        if (changed) {
          this.dispatchEvent(
            new AudioOutputChangeEvent(
              AudioOutputChangeEvent.AUDIO_OUTPUT_CHANGE,
              false,
              false,
              AudioOutputChangeReason.USER_SELECTION
            )
          );
        }
      }
    }
    tagType(AudioDeviceManager, 'AudioDeviceManager', v => audioMgrSet.has(v));

    // ---- Camera ----
    const cameraToken = {};
    const cameraStates = new WeakMap();
    const cameraInstances = new Map();

    function readCameraState(cam) {
      const slot = cameraStates.get(cam);
      if (!slot) throw new TypeError('Invalid Camera receiver.');
      slot.cache = JSON.parse(callKernel(() => kernel.camera_state_json(slot.index)));
      return slot;
    }

    class Camera extends BaseDispatcher {
      constructor(token, index) {
        if (token !== cameraToken) {
          throw makeArgumentError(
            'Error #2012: Camera class cannot be instantiated directly; use Camera.getCamera().',
            2012
          );
        }
        super();
        const cache = JSON.parse(callKernel(() => kernel.camera_state_json(index)));
        cameraStates.set(this, { index, cache, videoSource: null });
      }
      static get isSupported() { return true; }
      static get names() {
        return JSON.parse(callKernel(() => kernel.camera_names_json()));
      }
      static getCamera(name = null) {
        const arg = name === undefined || name === null ? undefined : String(name);
        const idx = callKernel(() => kernel.resolve_camera(arg));
        if (idx < 0) return null;
        let inst = cameraInstances.get(idx);
        if (!inst) {
          inst = new Camera(cameraToken, idx);
          cameraInstances.set(idx, inst);
        }
        return inst;
      }
      attachVideoSource(source) {
        const slot = readCameraState(this);
        slot.videoSource = source ?? null;
      }
      get activityLevel() { return readCameraState(this).cache.activity_level; }
      get bandwidth() { return readCameraState(this).cache.bandwidth; }
      get currentFPS() { return readCameraState(this).cache.current_fps; }
      get fps() { return readCameraState(this).cache.fps; }
      get height() { return readCameraState(this).cache.height; }
      get index() { return readCameraState(this).cache.index; }
      get keyFrameInterval() { return readCameraState(this).cache.key_frame_interval; }
      get loopback() { return readCameraState(this).cache.loopback; }
      get motionLevel() { return readCameraState(this).cache.motion_level; }
      get motionTimeout() { return readCameraState(this).cache.motion_timeout; }
      get muted() { return readCameraState(this).cache.muted; }
      get name() { return readCameraState(this).cache.name; }
      get position() { return readCameraState(this).cache.position; }
      get quality() { return readCameraState(this).cache.quality; }
      get width() { return readCameraState(this).cache.width; }
      get permissionStatus() { return readCameraState(this).cache.permission_status; }

      setMode(width, height, fps, favorArea = true) {
        const slot = readCameraState(this);
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.camera_set_mode_json(
              slot.index,
              Number(width) | 0,
              Number(height) | 0,
              Number(fps),
              Boolean(favorArea)
            )
          )
        );
        if (StatusEvent && this.hasEventListener('status')) {
          this.dispatchEvent(new StatusEvent(StatusEvent.STATUS, false, false, 'Camera.Unmuted', 'status'));
        }
        if (this.hasEventListener('videoFrame')) {
          this.dispatchEvent(new BaseEvent(BaseEvent.VIDEO_FRAME || 'videoFrame', false, false));
        }
      }
      setQuality(bandwidth, quality) {
        const slot = readCameraState(this);
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.camera_set_quality_json(slot.index, Number(bandwidth) | 0, Number(quality) | 0)
          )
        );
      }
      setKeyFrameInterval(keyFrameInterval) {
        const slot = readCameraState(this);
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.camera_set_key_frame_interval_json(slot.index, Number(keyFrameInterval) | 0)
          )
        );
      }
      setLoopback(compress = false) {
        const slot = readCameraState(this);
        slot.cache = JSON.parse(
          callKernel(() => kernel.camera_set_loopback_json(slot.index, Boolean(compress)))
        );
      }
      setMotionLevel(motionLevel, timeout = 2000) {
        const slot = readCameraState(this);
        const prevLevel = slot.cache.motion_level;
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.camera_set_motion_level_json(
              slot.index,
              Number(motionLevel) | 0,
              Number(timeout) | 0
            )
          )
        );
        if (ActivityEvent && prevLevel !== slot.cache.motion_level) {
          this.dispatchEvent(new ActivityEvent(ActivityEvent.ACTIVITY, false, false, true));
        }
      }
      setCursor(value) {
        const slot = readCameraState(this);
        slot.cache = JSON.parse(
          callKernel(() => kernel.camera_set_cursor_json(slot.index, Boolean(value)))
        );
      }
      requestPermission() {
        readCameraState(this);
        if (StatusEvent) {
          this.dispatchEvent(new StatusEvent(StatusEvent.STATUS, false, false, 'Camera.Unmuted', 'status'));
        }
      }
      copyToByteArray(rect, destination) {
        const slot = readCameraState(this);
        if (!rect || typeof rect !== 'object') {
          throw new TypeError('Camera.copyToByteArray requires a non-null Rectangle.');
        }
        if (!isByteArray(destination)) {
          throw new TypeError('Camera.copyToByteArray requires a ByteArray destination.');
        }
        const x = Number(rect.x) | 0;
        const y = Number(rect.y) | 0;
        const w = Number(rect.width) | 0;
        const h = Number(rect.height) | 0;
        if (slot.videoSource && typeof slot.videoSource.getPixel32 === 'function') {
          callKernel(() => kernel.camera_frame_argb_u32(slot.index, x, y, w, h));
          for (let py = 0; py < h; py++) {
            for (let px = 0; px < w; px++) {
              const argb = (slot.videoSource.getPixel32(x + px, y + py) >>> 0);
              destination.writeByte((argb >>> 24) & 0xFF);
              destination.writeByte((argb >>> 16) & 0xFF);
              destination.writeByte((argb >>> 8) & 0xFF);
              destination.writeByte(argb & 0xFF);
            }
          }
          return;
        }
        const bytes = callKernel(() => kernel.camera_frame_argb_bytes(slot.index, x, y, w, h));
        for (let i = 0; i < bytes.length; i++) {
          destination.writeByte(bytes[i]);
        }
      }
      copyToVector(rect, destination) {
        const slot = readCameraState(this);
        if (!rect || typeof rect !== 'object') {
          throw new TypeError('Camera.copyToVector requires a non-null Rectangle.');
        }
        if (!destination || typeof destination !== 'object' || typeof destination.length !== 'number') {
          throw new TypeError('Camera.copyToVector requires a Vector.<uint> destination.');
        }
        const x = Number(rect.x) | 0;
        const y = Number(rect.y) | 0;
        const w = Number(rect.width) | 0;
        const h = Number(rect.height) | 0;
        const pixels = callKernel(() => kernel.camera_frame_argb_u32(slot.index, x, y, w, h));
        if (slot.videoSource && typeof slot.videoSource.getPixel32 === 'function') {
          let k = 0;
          for (let py = 0; py < h; py++) {
            for (let px = 0; px < w; px++) {
              pixels[k++] = slot.videoSource.getPixel32(x + px, y + py) >>> 0;
            }
          }
        }
        if (!destination.fixed && destination.length < pixels.length) {
          destination.length = pixels.length;
        }
        if (destination.length < pixels.length) {
          throw makeRangeError('Destination Vector.<uint> is fixed and too small for camera rectangle.', 1125);
        }
        for (let i = 0; i < pixels.length; i++) {
          destination[i] = pixels[i] >>> 0;
        }
      }
      drawToBitmapData(destination) {
        const slot = readCameraState(this);
        if (!destination || typeof destination !== 'object' || typeof destination.setPixel32 !== 'function') {
          throw new TypeError('Camera.drawToBitmapData requires a BitmapData destination.');
        }
        const w = Math.min(slot.cache.width, Number(destination.width) | 0);
        const h = Math.min(slot.cache.height, Number(destination.height) | 0);
        if (w <= 0 || h <= 0) return;
        const pixels = callKernel(() => kernel.camera_frame_argb_u32(slot.index, 0, 0, w, h));
        if (typeof destination.lock === 'function') destination.lock();
        try {
          let k = 0;
          for (let py = 0; py < h; py++) {
            for (let px = 0; px < w; px++) {
              const pxVal =
                slot.videoSource && typeof slot.videoSource.getPixel32 === 'function'
                  ? slot.videoSource.getPixel32(px, py) >>> 0
                  : pixels[k] >>> 0;
              k++;
              destination.setPixel32(px, py, pxVal);
            }
          }
        } finally {
          if (typeof destination.unlock === 'function') destination.unlock();
        }
      }
    }
    tagType(Camera, 'Camera', v => cameraStates.has(v));

    const cameraVideoAdapter = {
      isCamera: value => cameraStates.has(value),
      attachVideo(cam, _onFrame) {
        const slot = readCameraState(cam);
        let active = true;
        return {
          read() {
            if (!active || disposed) return null;
            const cur = readCameraState(cam).cache;
            return {
              image: slot.videoSource || null,
              width: cur.width,
              height: cur.height,
              id: `as3-cam:${slot.index}:${cur.frame_revision}`,
            };
          },
          release() {
            active = false;
          },
        };
      },
    };

    // ---- Microphone & MicrophoneEnhancedOptions ----
    const enhOptStates = new WeakMap();
    class MicrophoneEnhancedOptions {
      constructor() {
        enhOptStates.set(this, {
          mode: 'fullDuplex',
          echo_path: 128,
          non_linear_processing: true,
          is_voice_detected: -1,
          auto_gain: false,
        });
      }
      get mode() { return enhOptStates.get(this).mode; }
      set mode(v) {
        const valid = callKernel(() => kernel.validate_enhanced_mode(String(v)));
        enhOptStates.get(this).mode = valid;
      }
      get echoPath() { return enhOptStates.get(this).echo_path; }
      set echoPath(v) {
        const valid = callKernel(() => kernel.validate_echo_path(Number(v) | 0));
        enhOptStates.get(this).echo_path = valid;
      }
      get nonLinearProcessing() { return enhOptStates.get(this).non_linear_processing; }
      set nonLinearProcessing(v) { enhOptStates.get(this).non_linear_processing = Boolean(v); }
      get isVoiceDetected() { return enhOptStates.get(this).is_voice_detected; }
      set isVoiceDetected(v) { enhOptStates.get(this).is_voice_detected = Number(v) | 0; }
      get autoGain() { return enhOptStates.get(this).auto_gain; }
      set autoGain(v) { enhOptStates.get(this).auto_gain = Boolean(v); }
    }
    tagType(MicrophoneEnhancedOptions, 'MicrophoneEnhancedOptions', v => enhOptStates.has(v));

    const micToken = {};
    const micStates = new WeakMap();
    const micInstances = new Map();

    function readMicState(mic) {
      const slot = micStates.get(mic);
      if (!slot) throw new TypeError('Invalid Microphone receiver.');
      slot.cache = JSON.parse(callKernel(() => kernel.mic_state_json(slot.index)));
      return slot;
    }

    class Microphone extends BaseDispatcher {
      constructor(token, index) {
        if (token !== micToken) {
          throw makeArgumentError(
            'Error #2012: Microphone class cannot be instantiated directly; use Microphone.getMicrophone().',
            2012
          );
        }
        super();
        const cache = JSON.parse(callKernel(() => kernel.mic_state_json(index)));
        micStates.set(this, { index, cache, enhancedWrapper: null });
      }
      static get isSupported() { return true; }
      static get names() {
        return JSON.parse(callKernel(() => kernel.microphone_names_json()));
      }
      static getMicrophone(index = -1) {
        const idx = callKernel(() => kernel.resolve_microphone(Number(index) | 0));
        if (idx < 0) return null;
        let inst = micInstances.get(idx);
        if (!inst) {
          inst = new Microphone(micToken, idx);
          micInstances.set(idx, inst);
        }
        return inst;
      }
      static getEnhancedMicrophone(index = -1) {
        const mic = Microphone.getMicrophone(index);
        if (!mic) return null;
        const slot = readMicState(mic);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_enable_enhanced_json(slot.index)));
        if (!slot.enhancedWrapper) {
          slot.enhancedWrapper = new MicrophoneEnhancedOptions();
        }
        return mic;
      }
      get activityLevel() { return readMicState(this).cache.activity_level; }
      get codec() { return readMicState(this).cache.codec; }
      set codec(v) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_set_codec_json(slot.index, String(v))));
      }
      get enableVAD() { return readMicState(this).cache.enable_vad; }
      set enableVAD(v) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_set_enable_vad_json(slot.index, Boolean(v))));
      }
      get encodeQuality() { return readMicState(this).cache.encode_quality; }
      set encodeQuality(v) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_set_encode_quality_json(slot.index, Number(v) | 0)));
      }
      get enhancedOptions() {
        const slot = readMicState(this);
        if (!slot.cache.enhanced_options) return null;
        if (!slot.enhancedWrapper) {
          slot.enhancedWrapper = new MicrophoneEnhancedOptions();
        }
        const st = enhOptStates.get(slot.enhancedWrapper);
        Object.assign(st, slot.cache.enhanced_options);
        return slot.enhancedWrapper;
      }
      set enhancedOptions(v) {
        const slot = readMicState(this);
        if (v === null || v === undefined) {
          slot.enhancedWrapper = null;
          slot.cache = JSON.parse(callKernel(() => kernel.mic_set_enhanced_options_json(slot.index, 'null')));
          return;
        }
        if (!enhOptStates.has(v)) {
          throw new TypeError('Microphone.enhancedOptions requires a MicrophoneEnhancedOptions or null.');
        }
        const raw = enhOptStates.get(v);
        slot.cache = JSON.parse(
          callKernel(() => kernel.mic_set_enhanced_options_json(slot.index, JSON.stringify(raw)))
        );
        slot.enhancedWrapper = v;
      }
      get framesPerPacket() { return readMicState(this).cache.frames_per_packet; }
      set framesPerPacket(v) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_set_frames_per_packet_json(slot.index, Number(v) | 0)));
      }
      get gain() { return readMicState(this).cache.gain; }
      set gain(v) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_set_gain_json(slot.index, Number(v))));
      }
      get index() { return readMicState(this).cache.index; }
      get muted() { return readMicState(this).cache.muted; }
      get name() { return readMicState(this).cache.name; }
      get noiseSuppressionLevel() { return readMicState(this).cache.noise_suppression_level; }
      set noiseSuppressionLevel(v) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(
          callKernel(() => kernel.mic_set_noise_suppression_level_json(slot.index, Number(v) | 0))
        );
      }
      get rate() { return readMicState(this).cache.rate; }
      set rate(v) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_set_rate_json(slot.index, Number(v) | 0)));
      }
      get silenceLevel() { return readMicState(this).cache.silence_level; }
      get silenceTimeout() { return readMicState(this).cache.silence_timeout; }
      get soundTransform() {
        const slot = readMicState(this);
        const ST = options.getAudio?.()?.SoundTransform;
        return ST
          ? new ST(slot.cache.volume, slot.cache.pan)
          : { volume: slot.cache.volume, pan: slot.cache.pan };
      }
      set soundTransform(v) {
        const slot = readMicState(this);
        if (!v || typeof v !== 'object') {
          throw new TypeError('Microphone.soundTransform requires a SoundTransform.');
        }
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.mic_set_sound_transform_json(slot.index, Number(v.volume ?? 1), Number(v.pan ?? 0))
          )
        );
      }
      get useEchoSuppression() { return readMicState(this).cache.use_echo_suppression; }
      get loopback() { return readMicState(this).cache.loopback; }
      get permissionStatus() { return readMicState(this).cache.permission_status; }

      setLoopBack(state = true) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(callKernel(() => kernel.mic_set_loop_back_json(slot.index, Boolean(state))));
        if (state && this.hasEventListener('sampleData')) {
          this.pollSampleData(512);
        }
      }
      setLoopback(state = true) {
        this.setLoopBack(state);
      }
      setSilenceLevel(silenceLevel, timeout = -1) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.mic_set_silence_level_json(slot.index, Number(silenceLevel), Number(timeout) | 0)
          )
        );
      }
      setUseEchoSuppression(useEchoSuppression) {
        const slot = readMicState(this);
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.mic_set_use_echo_suppression_json(slot.index, Boolean(useEchoSuppression))
          )
        );
      }
      requestPermission() {
        readMicState(this);
        if (StatusEvent) {
          this.dispatchEvent(new StatusEvent(StatusEvent.STATUS, false, false, 'Microphone.Unmuted', 'status'));
        }
      }
      pollSampleData(sampleCount = 2048) {
        const slot = readMicState(this);
        const count = Math.max(1, Math.min(16384, Number(sampleCount) | 0 || 2048));
        const pcm = JSON.parse(callKernel(() => kernel.mic_sample_pcm_json(slot.index, count)));
        slot.cache.activity_level = pcm.activity_level;
        slot.cache.sample_position = pcm.sample_position;
        if (!binary?.ByteArray) return null;
        const ba = new binary.ByteArray();
        for (let i = 0; i < pcm.bytes.length; i++) {
          ba.writeByte(pcm.bytes[i]);
        }
        ba.position = 0;
        const SampleDataEvent = events.SampleDataEvent;
        if (!SampleDataEvent) return null;
        const ev = new SampleDataEvent(
          SampleDataEvent.SAMPLE_DATA || 'sampleData',
          false,
          false,
          pcm.position,
          ba
        );
        if (ActivityEvent && !slot.activityActive && pcm.activity_level > slot.cache.silence_level) {
          slot.activityActive = true;
          if (this.hasEventListener('activity')) {
            this.dispatchEvent(new ActivityEvent(ActivityEvent.ACTIVITY, false, false, true));
          }
        }
        if (this.hasEventListener('sampleData')) {
          this.dispatchEvent(ev);
        }
        return ev;
      }
    }
    tagType(Microphone, 'Microphone', v => micStates.has(v));

    function stepMicrophones(sampleCount = 1024) {
      if (disposed) return 0;
      let dispatched = 0;
      for (const mic of micInstances.values()) {
        if (mic.hasEventListener('sampleData')) {
          mic.pollSampleData(sampleCount);
          dispatched++;
        }
      }
      return dispatched;
    }

    function scanHardware(customDevices = null) {
      if (disposed) return null;
      const cams = Array.isArray(customDevices?.cameras) ? customDevices.cameras.map(String) : [];
      const mics = Array.isArray(customDevices?.microphones) ? customDevices.microphones.map(String) : [];
      const outs = Array.isArray(customDevices?.audioOutputs) ? customDevices.audioOutputs.map(String) : [];
      const prevOuts = JSON.parse(callKernel(() => kernel.audio_device_names_json()));
      const result = JSON.parse(
        callKernel(() =>
          kernel.sync_hardware_devices_json(
            JSON.stringify(cams),
            JSON.stringify(mics),
            JSON.stringify(outs)
          )
        )
      );
      if (
        audioMgrSingleton &&
        outs.length > 0 &&
        JSON.stringify(prevOuts) !== JSON.stringify(result.audio_outputs)
      ) {
        audioMgrSingleton.dispatchEvent(
          new AudioOutputChangeEvent(
            AudioOutputChangeEvent.AUDIO_OUTPUT_CHANGE,
            false,
            false,
            AudioOutputChangeReason.DEVICE_CHANGE
          )
        );
      }
      if (
        !customDevices &&
        typeof global.navigator?.mediaDevices?.enumerateDevices === 'function'
      ) {
        global.navigator.mediaDevices
          .enumerateDevices()
          .then(list => {
            if (disposed || !Array.isArray(list)) return;
            const c = list.filter(d => d.kind === 'videoinput').map((d, i) => d.label || `Camera ${i + 1}`);
            const m = list.filter(d => d.kind === 'audioinput').map((d, i) => d.label || `Microphone ${i + 1}`);
            const o = list.filter(d => d.kind === 'audiooutput').map((d, i) => d.label || `Audio Output ${i + 1}`);
            if (c.length || m.length || o.length) {
              scanHardware({ cameras: c, microphones: m, audioOutputs: o });
            }
          })
          .catch(() => {});
      }
      return result;
    }

    // ---- VideoStreamSettings, H264VideoStreamSettings, VP6VideoStreamSettings ----
    const videoSettingsStates = new WeakMap();
    function initVideoStreamSettings(instance, codecKind) {
      const created = JSON.parse(callKernel(() => kernel.create_video_stream_settings_json(codecKind)));
      videoSettingsStates.set(instance, created);
      return created;
    }
    function readVideoSettings(instance) {
      const slot = videoSettingsStates.get(instance);
      if (!slot) throw new TypeError('Invalid VideoStreamSettings receiver.');
      return slot;
    }

    class VideoStreamSettings {
      constructor() {
        initVideoStreamSettings(this, 'Sorenson');
      }
      get bandwidth() { return readVideoSettings(this).state.bandwidth; }
      get codec() { return readVideoSettings(this).state.codec; }
      get fps() { return readVideoSettings(this).state.fps; }
      get height() { return readVideoSettings(this).state.height; }
      get keyFrameInterval() { return readVideoSettings(this).state.key_frame_interval; }
      get quality() { return readVideoSettings(this).state.quality; }
      get width() { return readVideoSettings(this).state.width; }

      setMode(width, height, fps) {
        const slot = readVideoSettings(this);
        slot.state = JSON.parse(
          callKernel(() =>
            kernel.video_settings_set_mode_json(slot.id, Number(width) | 0, Number(height) | 0, Number(fps))
          )
        );
      }
      setQuality(bandwidth, quality) {
        const slot = readVideoSettings(this);
        slot.state = JSON.parse(
          callKernel(() =>
            kernel.video_settings_set_quality_json(slot.id, Number(bandwidth) | 0, Number(quality) | 0)
          )
        );
      }
      setKeyFrameInterval(keyFrameInterval) {
        const slot = readVideoSettings(this);
        slot.state = JSON.parse(
          callKernel(() =>
            kernel.video_settings_set_key_frame_interval_json(slot.id, Number(keyFrameInterval) | 0)
          )
        );
      }
    }
    tagType(VideoStreamSettings, 'VideoStreamSettings', v => videoSettingsStates.has(v));

    const h264SettingsSet = new WeakSet();
    class H264VideoStreamSettings extends VideoStreamSettings {
      constructor() {
        super();
        initVideoStreamSettings(this, 'H264Avc');
        h264SettingsSet.add(this);
      }
      get codec() { return readVideoSettings(this).state.codec; }
      get profile() { return readVideoSettings(this).state.profile; }
      get level() { return readVideoSettings(this).state.level; }
      setProfileLevel(profile, level) {
        const slot = readVideoSettings(this);
        slot.state = JSON.parse(
          callKernel(() =>
            kernel.video_settings_set_profile_level_json(slot.id, String(profile), String(level))
          )
        );
      }
    }
    tagType(H264VideoStreamSettings, 'H264VideoStreamSettings', v => h264SettingsSet.has(v));

    const vp6SettingsSet = new WeakSet();
    class VP6VideoStreamSettings extends VideoStreamSettings {
      constructor() {
        super();
        initVideoStreamSettings(this, 'VP6');
        vp6SettingsSet.add(this);
      }
      get codec() { return readVideoSettings(this).state.codec; }
    }
    tagType(VP6VideoStreamSettings, 'VP6VideoStreamSettings', v => vp6SettingsSet.has(v));

    // ---- StageVideo ----
    const stageVideoToken = {};
    const stageVideoStates = new WeakMap();
    const stageVideoPool = [];

    function readStageVideo(sv) {
      const slot = stageVideoStates.get(sv);
      if (!slot) throw new TypeError('Invalid StageVideo receiver.');
      slot.cache = JSON.parse(callKernel(() => kernel.stage_video_state_json(slot.index)));
      return slot;
    }

    class StageVideo extends BaseDispatcher {
      constructor(token, slotIndex) {
        if (token !== stageVideoToken) {
          throw makeArgumentError(
            'Error #2012: StageVideo class cannot be instantiated directly; use stage.stageVideos.',
            2012
          );
        }
        super();
        const cache = JSON.parse(callKernel(() => kernel.stage_video_state_json(slotIndex)));
        stageVideoStates.set(this, { index: slotIndex, cache, stream: null, camera: null });
      }
      get viewPort() {
        const vp = readStageVideo(this).cache.view_port;
        return makeRectangle(vp[0], vp[1], vp[2], vp[3]);
      }
      set viewPort(rect) {
        const slot = readStageVideo(this);
        if (!rect || typeof rect !== 'object') {
          throw new TypeError('StageVideo.viewPort requires a non-null Rectangle.');
        }
        slot.cache = JSON.parse(
          callKernel(() =>
            kernel.stage_video_set_viewport_json(
              slot.index,
              Number(rect.x),
              Number(rect.y),
              Number(rect.width),
              Number(rect.height)
            )
          )
        );
      }
      get pan() {
        const p = readStageVideo(this).cache.pan;
        return makePoint(p[0], p[1]);
      }
      set pan(pt) {
        const slot = readStageVideo(this);
        if (!pt || typeof pt !== 'object') {
          throw new TypeError('StageVideo.pan requires a non-null Point.');
        }
        slot.cache = JSON.parse(
          callKernel(() => kernel.stage_video_set_pan_json(slot.index, Number(pt.x), Number(pt.y)))
        );
      }
      get zoom() {
        const z = readStageVideo(this).cache.zoom;
        return makePoint(z[0], z[1]);
      }
      set zoom(pt) {
        const slot = readStageVideo(this);
        if (!pt || typeof pt !== 'object') {
          throw new TypeError('StageVideo.zoom requires a non-null Point.');
        }
        slot.cache = JSON.parse(
          callKernel(() => kernel.stage_video_set_zoom_json(slot.index, Number(pt.x), Number(pt.y)))
        );
      }
      get depth() { return readStageVideo(this).cache.depth; }
      set depth(v) {
        const slot = readStageVideo(this);
        slot.cache = JSON.parse(
          callKernel(() => kernel.stage_video_set_depth_json(slot.index, Number(v) | 0))
        );
      }
      get videoWidth() { return readStageVideo(this).cache.video_width; }
      get videoHeight() { return readStageVideo(this).cache.video_height; }
      get colorSpaces() {
        const spaces = readStageVideo(this).cache.color_spaces;
        return makeTypedVector('String', spaces);
      }
      attachNetStream(netStream) {
        const slot = readStageVideo(this);
        slot.camera = null;
        slot.stream = netStream ?? null;
        const w = netStream ? (Number(netStream.videoWidth || netStream.width) | 0) || 320 : 0;
        const h = netStream ? (Number(netStream.videoHeight || netStream.height) | 0) || 240 : 0;
        slot.cache = JSON.parse(
          callKernel(() => kernel.stage_video_set_dimensions_json(slot.index, w, h))
        );
        if (netStream) {
          this.dispatchEvent(
            new StageVideoEvent(
              StageVideoEvent.RENDER_STATE,
              false,
              false,
              StageVideoEvent.RENDER_STATUS_SOFTWARE,
              'BT.709'
            )
          );
        }
      }
      attachCamera(theCamera) {
        const slot = readStageVideo(this);
        if (theCamera !== null && theCamera !== undefined && !cameraStates.has(theCamera)) {
          throw new TypeError('StageVideo.attachCamera requires a flash.media.Camera or null.');
        }
        slot.stream = null;
        slot.camera = theCamera ?? null;
        const w = theCamera ? theCamera.width : 0;
        const h = theCamera ? theCamera.height : 0;
        slot.cache = JSON.parse(
          callKernel(() => kernel.stage_video_set_dimensions_json(slot.index, w, h))
        );
        if (theCamera) {
          this.dispatchEvent(
            new StageVideoEvent(
              StageVideoEvent.RENDER_STATE,
              false,
              false,
              StageVideoEvent.RENDER_STATUS_SOFTWARE,
              'BT.709'
            )
          );
        }
      }
    }
    tagType(StageVideo, 'StageVideo', v => stageVideoStates.has(v));

    function ensureStageVideoPool() {
      if (stageVideoPool.length === 0) {
        const count = callKernel(() => kernel.stage_video_slot_count());
        for (let i = 0; i < count; i++) {
          stageVideoPool.push(new StageVideo(stageVideoToken, i));
        }
      }
      return stageVideoPool;
    }

    function createStageVideosVector() {
      if (!cachedStageVideosVector) {
        cachedStageVideosVector = makeTypedVector(
          'flash.media.StageVideo',
          ensureStageVideoPool(),
          true
        );
      }
      return cachedStageVideosVector;
    }

    function dispatchStageVideoAvailability(stageTarget) {
      if (!stageTarget || typeof stageTarget.dispatchEvent !== 'function') return;
      stageTarget.dispatchEvent(
        new StageVideoAvailabilityEvent(
          StageVideoAvailabilityEvent.STAGE_VIDEO_AVAILABILITY,
          false,
          false,
          StageVideoAvailability.AVAILABLE,
          StageVideoAvailabilityReason.NO_ERROR
        )
      );
    }

    function paintStageVideos(context, stageWidth = 800, stageHeight = 600) {
      if (disposed || !context) return [];
      const activeSlots = [];
      for (const sv of ensureStageVideoPool()) {
        const slot = readStageVideo(sv);
        if (slot.stream || slot.camera) {
          activeSlots.push({ sv, slot });
        }
      }
      if (!activeSlots.length) return [];
      activeSlots.sort((a, b) => a.slot.cache.depth - b.slot.cache.depth || a.slot.index - b.slot.index);
      const rendered = [];
      for (const { slot } of activeSlots) {
        const vp = slot.cache.view_port;
        const vx = vp[2] > 0 && vp[3] > 0 ? vp[0] : 0;
        const vy = vp[2] > 0 && vp[3] > 0 ? vp[1] : 0;
        const vw = vp[2] > 0 ? vp[2] : stageWidth;
        const vh = vp[3] > 0 ? vp[3] : stageHeight;
        const [panX, panY] = slot.cache.pan;
        const [zoomX, zoomY] = slot.cache.zoom;
        const drawW = vw * zoomX;
        const drawH = vh * zoomY;
        const drawX = vx + (vw - drawW) * 0.5 - panX * (drawW - vw) * 0.5;
        const drawY = vy + (vh - drawH) * 0.5 - panY * (drawH - vh) * 0.5;
        context.save?.();
        try {
          if (typeof context.beginPath === 'function' && typeof context.rect === 'function' && typeof context.clip === 'function') {
            context.beginPath();
            context.rect(vx, vy, vw, vh);
            context.clip();
          }
          const camSlot = slot.camera ? cameraStates.get(slot.camera) : null;
          const drawableSource =
            slot.stream?.videoElement ||
            slot.stream?.canvas ||
            camSlot?.videoSource?.videoElement ||
            camSlot?.videoSource?.canvas ||
            null;
          if (drawableSource && typeof context.drawImage === 'function') {
            context.drawImage(drawableSource, drawX, drawY, drawW, drawH);
          } else if (typeof context.fillRect === 'function') {
            let sampleHex = '#102840';
            if (camSlot) {
              const px = callKernel(() => kernel.camera_frame_argb_u32(camSlot.index, 0, 0, 1, 1));
              if (px.length > 0) {
                const rgb = px[0] & 0xFFFFFF;
                sampleHex = '#' + rgb.toString(16).padStart(6, '0');
              }
            }
            context.fillStyle = sampleHex;
            context.fillRect(drawX, drawY, drawW, drawH);
          }
          const summary = {
            slotIndex: slot.index,
            depth: slot.cache.depth,
            viewPort: { x: vx, y: vy, width: vw, height: vh },
            drawRect: { x: drawX, y: drawY, width: drawW, height: drawH },
            sourceKind: slot.camera ? 'camera' : 'netStream',
          };
          slot.lastRenderedFrame = summary;
          rendered.push(summary);
        } finally {
          context.restore?.();
        }
      }
      return rendered;
    }

    const stageObj = typeof options.getStage === 'function' ? options.getStage() : null;
    if (options.StageClass || stageObj) {
      bindStageMediaProperties(
        options.StageClass,
        () => createStageVideosVector(),
        stageObj,
        dispatchStageVideoAvailability
      );
    }
    if (options.LoaderInfoClass) {
      bindLoaderInfoProperties(options.LoaderInfoClass, () => new UncaughtErrorEvents());
    }

    // =========================================================================
    // 3. flash.ui.*
    // =========================================================================

    const MultitouchInputMode = tagConstantsClass('MultitouchInputMode', {
      GESTURE: 'gesture',
      NONE: 'none',
      TOUCH_POINT: 'touchPoint',
    });

    class Multitouch {
      constructor() {
        throw makeArgumentError('Error #2012: Multitouch$ class cannot be instantiated.', 2012);
      }
      static get inputMode() {
        return callKernel(() => kernel.multitouch_input_mode());
      }
      static set inputMode(value) {
        callKernel(() => kernel.set_multitouch_input_mode(String(value)));
      }
      static get maxTouchPoints() {
        return callKernel(() => kernel.max_touch_points());
      }
      static get supportsGestureEvents() { return true; }
      static get supportsTouchEvents() { return true; }
      static get supportedGestures() {
        const list = JSON.parse(callKernel(() => kernel.supported_gestures_json()));
        return makeTypedVector('String', list);
      }
      static get mapTouchToMouse() {
        return callKernel(() => kernel.map_touch_to_mouse());
      }
      static set mapTouchToMouse(value) {
        callKernel(() => kernel.set_map_touch_to_mouse(Boolean(value)));
      }
    }
    tagType(Multitouch, 'Multitouch', () => false);

    // ---- Multitouch Multi-Pointer Touch & 2-Finger Gesture Recognizer ----
    const activeTouches = new Map();
    let primaryTouchId = null;
    let activeGestureState = null;

    function resolveEventTarget(rawEvent, targetOverride = null) {
      if (targetOverride && typeof targetOverride === 'object') return targetOverride;
      if (typeof options.hitTest === 'function') {
        try {
          const hit = options.hitTest(rawEvent);
          if (hit) return hit;
        } catch (_) {}
      }
      return (typeof options.getStage === 'function' ? options.getStage() : null) || null;
    }

    function resolveStageAndLocalCoords(rawEvent, target) {
      let stageX = Number(rawEvent?.stageX ?? rawEvent?.clientX ?? rawEvent?.x ?? 0);
      let stageY = Number(rawEvent?.stageY ?? rawEvent?.clientY ?? rawEvent?.y ?? 0);
      if (
        rawEvent?.stageX === undefined &&
        typeof options.coordinates === 'function' &&
        rawEvent?.clientX !== undefined
      ) {
        try {
          const pt = options.coordinates(rawEvent);
          if (Array.isArray(pt) && pt.length >= 2) {
            stageX = Number(pt[0]);
            stageY = Number(pt[1]);
          }
        } catch (_) {}
      }
      let localX = Number(rawEvent?.localX ?? stageX);
      let localY = Number(rawEvent?.localY ?? stageY);
      if (rawEvent?.localX === undefined && typeof options.localPoint === 'function' && target) {
        try {
          const lp = options.localPoint(target, stageX, stageY);
          if (Array.isArray(lp) && lp.length >= 2) {
            localX = Number(lp[0]);
            localY = Number(lp[1]);
          }
        } catch (_) {}
      }
      return { stageX, stageY, localX, localY };
    }

    function dispatchOnTarget(target, ev) {
      if (!target || !ev) return false;
      if (options.eventHub && typeof options.eventHub.dispatch === 'function') {
        try {
          return options.eventHub.dispatch(target, ev);
        } catch (_) {}
      }
      if (typeof target.dispatchEvent === 'function') {
        return target.dispatchEvent(ev);
      }
      return false;
    }

    function encodeTouchSamplesByteArray(samplesList) {
      if (!binary?.ByteArray || !Array.isArray(samplesList) || !samplesList.length) return null;
      const ba = new binary.ByteArray();
      for (const s of samplesList) {
        if (typeof ba.writeFloat === 'function') {
          ba.writeFloat(Number(s.x) || 0);
          ba.writeFloat(Number(s.y) || 0);
          ba.writeFloat(Number(s.pressure ?? 1));
        }
      }
      ba.position = 0;
      return ba;
    }

    function makeTouchEventInstance(type, pointState, rawEvent, relatedObject = null) {
      const TouchEvent = events.TouchEvent;
      if (!TouchEvent) return null;
      const ev = new TouchEvent(
        type,
        true,
        false,
        pointState.id,
        pointState.isPrimary,
        pointState.localX,
        pointState.localY,
        pointState.sizeX,
        pointState.sizeY,
        pointState.pressure,
        relatedObject,
        Boolean(rawEvent?.ctrlKey),
        Boolean(rawEvent?.altKey),
        Boolean(rawEvent?.shiftKey)
      );
      if (typeof events.setStagePoint === 'function') {
        events.setStagePoint(ev, pointState.stageX, pointState.stageY);
      }
      const samplesBa = encodeTouchSamplesByteArray(pointState.samples);
      if (samplesBa) {
        try { ev.samples = samplesBa; } catch (_) {}
        Object.defineProperty(ev, 'getSamples', {
          configurable: true,
          value(buffer, append = false) {
            if (!isByteArray(buffer)) {
              throw new TypeError('TouchEvent.getSamples requires a ByteArray buffer.');
            }
            if (!append && typeof buffer.clear === 'function') buffer.clear();
            const src = this.samples || samplesBa;
            if (src && typeof src.length === 'number' && src.length > 0 && typeof buffer.writeBytes === 'function') {
              buffer.writeBytes(src, 0, src.length);
              return Math.floor(src.length / 12);
            }
            return 0;
          },
        });
      }
      return ev;
    }

    function handleTouchPointMode(kind, rawEvent, target) {
      const TouchEvent = events.TouchEvent;
      if (!TouchEvent) return [];
      const id = Number(rawEvent?.pointerId ?? rawEvent?.touchPointID ?? rawEvent?.identifier ?? 1) | 0;
      const { stageX, stageY, localX, localY } = resolveStageAndLocalCoords(rawEvent, target);
      const sizeX = Number(rawEvent?.width ?? rawEvent?.sizeX ?? 10);
      const sizeY = Number(rawEvent?.height ?? rawEvent?.sizeY ?? 10);
      const pressure = Number(rawEvent?.pressure ?? 1);
      const dispatched = [];

      if (kind === 'down' || kind === 'touchstart' || kind === 'pointerdown') {
        if (primaryTouchId === null || activeTouches.size === 0) {
          primaryTouchId = id;
        }
        const pointState = {
          id,
          isPrimary: primaryTouchId === id,
          startX: stageX,
          startY: stageY,
          stageX,
          stageY,
          localX,
          localY,
          sizeX,
          sizeY,
          pressure,
          target,
          moved: false,
          samples: [{ x: localX, y: localY, pressure }],
        };
        activeTouches.set(id, pointState);
        const overEv = makeTouchEventInstance(TouchEvent.TOUCH_OVER || 'touchOver', pointState, rawEvent, null);
        const rollOverEv = makeTouchEventInstance(TouchEvent.TOUCH_ROLL_OVER || 'touchRollOver', pointState, rawEvent, null);
        const beginEv = makeTouchEventInstance(TouchEvent.TOUCH_BEGIN || 'touchBegin', pointState, rawEvent, null);
        if (overEv) { dispatchOnTarget(target, overEv); dispatched.push(overEv); }
        if (rollOverEv) { dispatchOnTarget(target, rollOverEv); dispatched.push(rollOverEv); }
        if (beginEv) { dispatchOnTarget(target, beginEv); dispatched.push(beginEv); }
      } else if (kind === 'move' || kind === 'touchmove' || kind === 'pointermove') {
        const pointState = activeTouches.get(id);
        if (!pointState) return dispatched;
        const prevTarget = pointState.target;
        pointState.stageX = stageX;
        pointState.stageY = stageY;
        pointState.localX = localX;
        pointState.localY = localY;
        pointState.sizeX = sizeX;
        pointState.sizeY = sizeY;
        pointState.pressure = pressure;
        pointState.target = target;
        if (Math.hypot(stageX - pointState.startX, stageY - pointState.startY) > 6) {
          pointState.moved = true;
        }
        pointState.samples.push({ x: localX, y: localY, pressure });
        if (pointState.samples.length > 64) pointState.samples.shift();
        if (prevTarget && prevTarget !== target) {
          const outEv = makeTouchEventInstance(TouchEvent.TOUCH_OUT || 'touchOut', pointState, rawEvent, target);
          const rollOutEv = makeTouchEventInstance(TouchEvent.TOUCH_ROLL_OUT || 'touchRollOut', pointState, rawEvent, target);
          if (outEv) { dispatchOnTarget(prevTarget, outEv); dispatched.push(outEv); }
          if (rollOutEv) { dispatchOnTarget(prevTarget, rollOutEv); dispatched.push(rollOutEv); }
          const overEv = makeTouchEventInstance(TouchEvent.TOUCH_OVER || 'touchOver', pointState, rawEvent, prevTarget);
          const rollOverEv = makeTouchEventInstance(TouchEvent.TOUCH_ROLL_OVER || 'touchRollOver', pointState, rawEvent, prevTarget);
          if (overEv) { dispatchOnTarget(target, overEv); dispatched.push(overEv); }
          if (rollOverEv) { dispatchOnTarget(target, rollOverEv); dispatched.push(rollOverEv); }
        }
        const moveEv = makeTouchEventInstance(TouchEvent.TOUCH_MOVE || 'touchMove', pointState, rawEvent, null);
        if (moveEv) { dispatchOnTarget(target, moveEv); dispatched.push(moveEv); }
      } else if (
        kind === 'up' ||
        kind === 'touchend' ||
        kind === 'pointerup' ||
        kind === 'cancel' ||
        kind === 'pointercancel'
      ) {
        const pointState = activeTouches.get(id) || {
          id,
          isPrimary: primaryTouchId === id,
          startX: stageX,
          startY: stageY,
          stageX,
          stageY,
          localX,
          localY,
          sizeX,
          sizeY,
          pressure,
          target,
          moved: false,
          samples: [{ x: localX, y: localY, pressure }],
        };
        pointState.stageX = stageX;
        pointState.stageY = stageY;
        pointState.localX = localX;
        pointState.localY = localY;
        activeTouches.delete(id);
        if (primaryTouchId === id) {
          const next = activeTouches.keys().next();
          primaryTouchId = next.done ? null : next.value;
          if (primaryTouchId !== null) {
            activeTouches.get(primaryTouchId).isPrimary = true;
          }
        }
        const endEv = makeTouchEventInstance(TouchEvent.TOUCH_END || 'touchEnd', pointState, rawEvent, null);
        if (endEv) { dispatchOnTarget(target, endEv); dispatched.push(endEv); }
        if (!pointState.moved && kind !== 'cancel' && kind !== 'pointercancel') {
          const tapEv = makeTouchEventInstance(TouchEvent.TOUCH_TAP || 'touchTap', pointState, rawEvent, null);
          if (tapEv) { dispatchOnTarget(target, tapEv); dispatched.push(tapEv); }
        }
      }
      return dispatched;
    }

    function handleGestureMode(kind, rawEvent, target) {
      const TransformGestureEvent = events.TransformGestureEvent;
      const GestureEvent = events.GestureEvent;
      const PressAndTapGestureEvent = events.PressAndTapGestureEvent;
      const GesturePhase = events.GesturePhase || {
        ALL: 'all',
        BEGIN: 'begin',
        UPDATE: 'update',
        END: 'end',
      };
      const id = Number(rawEvent?.pointerId ?? rawEvent?.touchPointID ?? rawEvent?.identifier ?? 1) | 0;
      const { stageX, stageY, localX, localY } = resolveStageAndLocalCoords(rawEvent, target);
      const dispatched = [];

      if (kind === 'down' || kind === 'touchstart' || kind === 'pointerdown') {
        activeTouches.set(id, {
          id,
          startX: stageX,
          startY: stageY,
          stageX,
          stageY,
          localX,
          localY,
          target,
          moved: false,
        });
        if (activeTouches.size === 2) {
          const [p1, p2] = [...activeTouches.values()];
          const dx = p2.stageX - p1.stageX;
          const dy = p2.stageY - p1.stageY;
          activeGestureState = {
            p1Id: p1.id,
            p2Id: p2.id,
            initialDist: Math.max(1e-3, Math.hypot(dx, dy)),
            prevDist: Math.max(1e-3, Math.hypot(dx, dy)),
            prevAngle: (Math.atan2(dy, dx) * 180) / Math.PI,
            prevMidX: (p1.stageX + p2.stageX) * 0.5,
            prevMidY: (p1.stageY + p2.stageY) * 0.5,
            began: false,
            moved: false,
            target: p1.target || target,
          };
        }
      } else if (kind === 'move' || kind === 'touchmove' || kind === 'pointermove') {
        const pt = activeTouches.get(id);
        if (!pt) return dispatched;
        pt.stageX = stageX;
        pt.stageY = stageY;
        pt.localX = localX;
        pt.localY = localY;
        if (Math.hypot(stageX - pt.startX, stageY - pt.startY) > 6) {
          pt.moved = true;
        }
        if (activeTouches.size === 2 && activeGestureState && TransformGestureEvent) {
          const p1 = activeTouches.get(activeGestureState.p1Id);
          const p2 = activeTouches.get(activeGestureState.p2Id);
          if (p1 && p2) {
            const dx = p2.stageX - p1.stageX;
            const dy = p2.stageY - p1.stageY;
            const dist = Math.max(1e-3, Math.hypot(dx, dy));
            const angle = (Math.atan2(dy, dx) * 180) / Math.PI;
            const midX = (p1.stageX + p2.stageX) * 0.5;
            const midY = (p1.stageY + p2.stageY) * 0.5;
            const scaleRatio = dist / activeGestureState.prevDist;
            let deltaAngle = angle - activeGestureState.prevAngle;
            while (deltaAngle > 180) deltaAngle -= 360;
            while (deltaAngle < -180) deltaAngle += 360;
            const offsetX = midX - activeGestureState.prevMidX;
            const offsetY = midY - activeGestureState.prevMidY;
            const phase = activeGestureState.began ? GesturePhase.UPDATE : GesturePhase.BEGIN;
            activeGestureState.began = true;
            activeGestureState.moved = true;
            activeGestureState.prevDist = dist;
            activeGestureState.prevAngle = angle;
            activeGestureState.prevMidX = midX;
            activeGestureState.prevMidY = midY;
            const gTarget = activeGestureState.target || target;

            if (Math.abs(scaleRatio - 1) > 0.01) {
              const zoomEv = new TransformGestureEvent(
                TransformGestureEvent.GESTURE_ZOOM || 'gestureZoom',
                true,
                false,
                phase,
                midX,
                midY,
                scaleRatio,
                scaleRatio,
                0,
                0,
                0,
                Boolean(rawEvent?.ctrlKey),
                Boolean(rawEvent?.altKey),
                Boolean(rawEvent?.shiftKey)
              );
              if (typeof events.setStagePoint === 'function') events.setStagePoint(zoomEv, midX, midY);
              dispatchOnTarget(gTarget, zoomEv);
              dispatched.push(zoomEv);
            }
            if (Math.abs(deltaAngle) > 0.5) {
              const rotEv = new TransformGestureEvent(
                TransformGestureEvent.GESTURE_ROTATE || 'gestureRotate',
                true,
                false,
                phase,
                midX,
                midY,
                1,
                1,
                deltaAngle,
                0,
                0,
                Boolean(rawEvent?.ctrlKey),
                Boolean(rawEvent?.altKey),
                Boolean(rawEvent?.shiftKey)
              );
              if (typeof events.setStagePoint === 'function') events.setStagePoint(rotEv, midX, midY);
              dispatchOnTarget(gTarget, rotEv);
              dispatched.push(rotEv);
            }
            if (Math.hypot(offsetX, offsetY) > 0.5) {
              const panEv = new TransformGestureEvent(
                TransformGestureEvent.GESTURE_PAN || 'gesturePan',
                true,
                false,
                phase,
                midX,
                midY,
                1,
                1,
                0,
                offsetX,
                offsetY,
                Boolean(rawEvent?.ctrlKey),
                Boolean(rawEvent?.altKey),
                Boolean(rawEvent?.shiftKey)
              );
              if (typeof events.setStagePoint === 'function') events.setStagePoint(panEv, midX, midY);
              dispatchOnTarget(gTarget, panEv);
              dispatched.push(panEv);
            }
          }
        }
      } else if (kind === 'up' || kind === 'touchend' || kind === 'pointerup') {
        const pt = activeTouches.get(id);
        const remainingCountBeforeDelete = activeTouches.size;
        activeTouches.delete(id);

        if (remainingCountBeforeDelete === 2 && activeGestureState) {
          const gTarget = activeGestureState.target || target;
          if (!activeGestureState.moved) {
            const other = [...activeTouches.values()][0];
            if (other && other.moved === false && PressAndTapGestureEvent && rawEvent?.pressAndTap) {
              const patEv = new PressAndTapGestureEvent(
                PressAndTapGestureEvent.GESTURE_PRESS_AND_TAP || 'gesturePressAndTap',
                true,
                false,
                GesturePhase.ALL,
                other.localX,
                other.localY,
                localX,
                localY,
                Boolean(rawEvent?.ctrlKey),
                Boolean(rawEvent?.altKey),
                Boolean(rawEvent?.shiftKey)
              );
              if (typeof events.setStagePoint === 'function') {
                events.setStagePoint(patEv, other.stageX, other.stageY);
              }
              dispatchOnTarget(gTarget, patEv);
              dispatched.push(patEv);
            } else if (GestureEvent) {
              const tap2Ev = new GestureEvent(
                GestureEvent.GESTURE_TWO_FINGER_TAP || 'gestureTwoFingerTap',
                true,
                false,
                GesturePhase.ALL,
                activeGestureState.prevMidX,
                activeGestureState.prevMidY,
                Boolean(rawEvent?.ctrlKey),
                Boolean(rawEvent?.altKey),
                Boolean(rawEvent?.shiftKey)
              );
              if (typeof events.setStagePoint === 'function') {
                events.setStagePoint(tap2Ev, activeGestureState.prevMidX, activeGestureState.prevMidY);
              }
              dispatchOnTarget(gTarget, tap2Ev);
              dispatched.push(tap2Ev);
            }
          } else if (TransformGestureEvent) {
            const endEv = new TransformGestureEvent(
              TransformGestureEvent.GESTURE_PAN || 'gesturePan',
              true,
              false,
              GesturePhase.END,
              activeGestureState.prevMidX,
              activeGestureState.prevMidY,
              1,
              1,
              0,
              0,
              0,
              Boolean(rawEvent?.ctrlKey),
              Boolean(rawEvent?.altKey),
              Boolean(rawEvent?.shiftKey)
            );
            if (typeof events.setStagePoint === 'function') {
              events.setStagePoint(endEv, activeGestureState.prevMidX, activeGestureState.prevMidY);
            }
            dispatchOnTarget(gTarget, endEv);
            dispatched.push(endEv);
          }
          activeGestureState = null;
        } else if (remainingCountBeforeDelete === 1 && pt && TransformGestureEvent) {
          const dx = stageX - pt.startX;
          const dy = stageY - pt.startY;
          if (Math.hypot(dx, dy) >= 40) {
            const sx = Math.abs(dx) >= Math.abs(dy) ? (dx > 0 ? 1 : -1) : 0;
            const sy = Math.abs(dy) > Math.abs(dx) ? (dy > 0 ? 1 : -1) : 0;
            const swipeEv = new TransformGestureEvent(
              TransformGestureEvent.GESTURE_SWIPE || 'gestureSwipe',
              true,
              false,
              GesturePhase.ALL,
              localX,
              localY,
              1,
              1,
              0,
              sx,
              sy,
              Boolean(rawEvent?.ctrlKey),
              Boolean(rawEvent?.altKey),
              Boolean(rawEvent?.shiftKey)
            );
            if (typeof events.setStagePoint === 'function') {
              events.setStagePoint(swipeEv, stageX, stageY);
            }
            dispatchOnTarget(pt.target || target, swipeEv);
            dispatched.push(swipeEv);
          }
        }
      }
      return dispatched;
    }

    function handlePointerInput(type, rawEvent, targetOverride = null) {
      if (disposed) return [];
      const mode = Multitouch.inputMode;
      if (mode === MultitouchInputMode.NONE) return [];
      const target = resolveEventTarget(rawEvent, targetOverride);
      if (!target) return [];
      if (mode === MultitouchInputMode.TOUCH_POINT) {
        return handleTouchPointMode(type, rawEvent, target);
      }
      if (mode === MultitouchInputMode.GESTURE) {
        return handleGestureMode(type, rawEvent, target);
      }
      return [];
    }

    // ---- ContextMenuBuiltInItems, ContextMenuClipboardItems, ContextMenuItem, ContextMenu ----
    const builtInStates = new WeakMap();
    class ContextMenuBuiltInItems {
      constructor() {
        builtInStates.set(this, {
          forwardAndBack: true,
          loop: true,
          play: true,
          print: true,
          quality: true,
          rewind: true,
          save: true,
          zoom: true,
        });
      }
      get forwardAndBack() { return builtInStates.get(this).forwardAndBack; }
      set forwardAndBack(v) { builtInStates.get(this).forwardAndBack = Boolean(v); }
      get loop() { return builtInStates.get(this).loop; }
      set loop(v) { builtInStates.get(this).loop = Boolean(v); }
      get play() { return builtInStates.get(this).play; }
      set play(v) { builtInStates.get(this).play = Boolean(v); }
      get print() { return builtInStates.get(this).print; }
      set print(v) { builtInStates.get(this).print = Boolean(v); }
      get quality() { return builtInStates.get(this).quality; }
      set quality(v) { builtInStates.get(this).quality = Boolean(v); }
      get rewind() { return builtInStates.get(this).rewind; }
      set rewind(v) { builtInStates.get(this).rewind = Boolean(v); }
      get save() { return builtInStates.get(this).save; }
      set save(v) { builtInStates.get(this).save = Boolean(v); }
      get zoom() { return builtInStates.get(this).zoom; }
      set zoom(v) { builtInStates.get(this).zoom = Boolean(v); }
      clone() {
        const copy = new ContextMenuBuiltInItems();
        Object.assign(builtInStates.get(copy), builtInStates.get(this));
        return copy;
      }
    }
    tagType(ContextMenuBuiltInItems, 'ContextMenuBuiltInItems', v => builtInStates.has(v));

    const clipboardItemStates = new WeakMap();
    class ContextMenuClipboardItems {
      constructor() {
        clipboardItemStates.set(this, {
          clear: false,
          copy: false,
          cut: false,
          paste: false,
          selectAll: true,
        });
      }
      get clear() { return clipboardItemStates.get(this).clear; }
      set clear(v) { clipboardItemStates.get(this).clear = Boolean(v); }
      get copy() { return clipboardItemStates.get(this).copy; }
      set copy(v) { clipboardItemStates.get(this).copy = Boolean(v); }
      get cut() { return clipboardItemStates.get(this).cut; }
      set cut(v) { clipboardItemStates.get(this).cut = Boolean(v); }
      get paste() { return clipboardItemStates.get(this).paste; }
      set paste(v) { clipboardItemStates.get(this).paste = Boolean(v); }
      get selectAll() { return clipboardItemStates.get(this).selectAll; }
      set selectAll(v) { clipboardItemStates.get(this).selectAll = Boolean(v); }
      clone() {
        const copy = new ContextMenuClipboardItems();
        Object.assign(clipboardItemStates.get(copy), clipboardItemStates.get(this));
        return copy;
      }
    }
    tagType(ContextMenuClipboardItems, 'ContextMenuClipboardItems', v => clipboardItemStates.has(v));

    const airBuiltins = options.airBuiltins || null;
    const BaseNativeMenuItem = airBuiltins?.['flash.display.NativeMenuItem'] || BaseDispatcher;
    const BaseNativeMenu = airBuiltins?.['flash.display.NativeMenu'] || BaseDispatcher;

    const cmItemStates = new WeakMap();
    class ContextMenuItem extends BaseNativeMenuItem {
      constructor(caption, separatorBefore = false, enabled = true, visible = true) {
        const cap = String(caption ?? '');
        if (BaseNativeMenuItem !== BaseDispatcher) {
          super(cap, false);
          this.enabled = Boolean(enabled);
        } else {
          super();
        }
        cmItemStates.set(this, {
          caption: cap,
          separatorBefore: Boolean(separatorBefore),
          enabled: Boolean(enabled),
          visible: Boolean(visible),
        });
      }
      get caption() { return cmItemStates.get(this).caption; }
      set caption(v) {
        const cap = String(v ?? '');
        cmItemStates.get(this).caption = cap;
        if (BaseNativeMenuItem !== BaseDispatcher) this.label = cap;
      }
      get separatorBefore() { return cmItemStates.get(this).separatorBefore; }
      set separatorBefore(v) { cmItemStates.get(this).separatorBefore = Boolean(v); }
      get visible() { return cmItemStates.get(this).visible; }
      set visible(v) { cmItemStates.get(this).visible = Boolean(v); }
      get enabled() {
        return BaseNativeMenuItem !== BaseDispatcher ? super.enabled : cmItemStates.get(this).enabled;
      }
      set enabled(v) {
        const flag = Boolean(v);
        if (cmItemStates.has(this)) cmItemStates.get(this).enabled = flag;
        if (BaseNativeMenuItem !== BaseDispatcher) super.enabled = flag;
      }
      clone() {
        return new ContextMenuItem(this.caption, this.separatorBefore, this.enabled, this.visible);
      }
      static systemClearMenuItem() { return new ContextMenuItem('Clear'); }
      static systemCopyLinkMenuItem() { return new ContextMenuItem('Copy Link'); }
      static systemCopyMenuItem() { return new ContextMenuItem('Copy'); }
      static systemCutMenuItem() { return new ContextMenuItem('Cut'); }
      static systemOpenLinkMenuItem() { return new ContextMenuItem('Open Link'); }
      static systemPasteMenuItem() { return new ContextMenuItem('Paste'); }
      static systemSelectAllMenuItem() { return new ContextMenuItem('Select All'); }
    }
    tagType(ContextMenuItem, 'ContextMenuItem', v => cmItemStates.has(v));

    const cmStates = new WeakMap();
    class ContextMenu extends BaseNativeMenu {
      constructor() {
        super();
        cmStates.set(this, {
          builtInItems: new ContextMenuBuiltInItems(),
          clipboardItems: new ContextMenuClipboardItems(),
          clipboardMenu: false,
          customItems: [],
          link: null,
        });
      }
      static get isSupported() { return true; }
      get builtInItems() { return cmStates.get(this).builtInItems; }
      set builtInItems(v) {
        if (!builtInStates.has(v)) {
          throw new TypeError('ContextMenu.builtInItems requires a ContextMenuBuiltInItems instance.');
        }
        cmStates.get(this).builtInItems = v;
      }
      get clipboardItems() { return cmStates.get(this).clipboardItems; }
      set clipboardItems(v) {
        if (!clipboardItemStates.has(v)) {
          throw new TypeError('ContextMenu.clipboardItems requires a ContextMenuClipboardItems instance.');
        }
        cmStates.get(this).clipboardItems = v;
      }
      get clipboardMenu() { return cmStates.get(this).clipboardMenu; }
      set clipboardMenu(v) { cmStates.get(this).clipboardMenu = Boolean(v); }
      get customItems() { return cmStates.get(this).customItems; }
      set customItems(v) {
        if (!Array.isArray(v)) {
          throw new TypeError('ContextMenu.customItems requires an Array.');
        }
        cmStates.get(this).customItems = v;
      }
      get link() { return cmStates.get(this).link; }
      set link(v) {
        if (v !== null && v !== undefined && net?.URLRequest && !net.URLRequest.__nfIs(v)) {
          throw new TypeError('ContextMenu.link requires a URLRequest or null.');
        }
        cmStates.get(this).link = v ?? null;
      }
      hideBuiltInItems() {
        const b = this.builtInItems;
        b.forwardAndBack = false;
        b.loop = false;
        b.play = false;
        b.print = false;
        b.quality = false;
        b.rewind = false;
        b.save = false;
        b.zoom = false;
      }
      clone() {
        const copy = new ContextMenu();
        copy.builtInItems = this.builtInItems.clone();
        copy.clipboardItems = this.clipboardItems.clone();
        copy.clipboardMenu = this.clipboardMenu;
        copy.link = this.link;
        copy.customItems = this.customItems.map(item =>
          cmItemStates.has(item) ? item.clone() : item
        );
        return copy;
      }
    }
    tagType(ContextMenu, 'ContextMenu', v => cmStates.has(v));

    // ---- Interactive HTML5 DOM ContextMenu Overlay ----
    let activeMenuElement = null;
    let clipboardTextBuffer = '';

    function closeContextMenu() {
      if (activeMenuElement) {
        try { activeMenuElement.remove(); } catch (_) {}
        activeMenuElement = null;
      }
    }

    function getActiveContextMenu() {
      return activeMenuElement;
    }

    function executeClipboardMenuAction(action, target) {
      if (!target || typeof target !== 'object') return;
      const text = typeof target.text === 'string' ? target.text : '';
      const begin = Math.max(0, Math.min(text.length, Number(target.selectionBeginIndex ?? 0) | 0));
      const end = Math.max(begin, Math.min(text.length, Number(target.selectionEndIndex ?? text.length) | 0));
      if (action === 'selectAll') {
        if (typeof target.setSelection === 'function') {
          target.setSelection(0, text.length);
        } else {
          target.selectionBeginIndex = 0;
          target.selectionEndIndex = text.length;
        }
      } else if (action === 'copy') {
        clipboardTextBuffer = begin < end ? text.slice(begin, end) : text;
      } else if (action === 'cut') {
        clipboardTextBuffer = begin < end ? text.slice(begin, end) : text;
        target.text = begin < end ? text.slice(0, begin) + text.slice(end) : '';
      } else if (action === 'paste') {
        target.text = text.slice(0, begin) + clipboardTextBuffer + text.slice(end);
      } else if (action === 'clear') {
        target.text = begin < end ? text.slice(0, begin) + text.slice(end) : '';
      }
    }

    function findContextMenuOwner(mouseTarget) {
      let cur = mouseTarget;
      const seen = new Set();
      while (cur && typeof cur === 'object' && !seen.has(cur)) {
        seen.add(cur);
        try {
          if (cur.contextMenu && cmStates.has(cur.contextMenu)) {
            return { owner: cur, menu: cur.contextMenu };
          }
        } catch (_) {}
        cur = cur.parent || null;
      }
      const stg = typeof options.getStage === 'function' ? options.getStage() : null;
      if (stg && stg.contextMenu && cmStates.has(stg.contextMenu)) {
        return { owner: stg, menu: stg.contextMenu };
      }
      return null;
    }

    function createFallbackDomNode(tagName, doc) {
      if (doc && typeof doc.createElement === 'function') {
        return doc.createElement(tagName);
      }
      const attrs = new Map();
      const children = [];
      const listeners = new Map();
      return {
        tagName: String(tagName).toUpperCase(),
        className: '',
        textContent: '',
        disabled: false,
        style: {},
        dataset: {},
        children,
        setAttribute(k, v) { attrs.set(String(k), String(v)); },
        getAttribute(k) { return attrs.has(String(k)) ? attrs.get(String(k)) : null; },
        appendChild(child) { children.push(child); return child; },
        addEventListener(type, fn) {
          const list = listeners.get(type) || [];
          list.push(fn);
          listeners.set(type, list);
        },
        click() {
          if (this.disabled) return;
          for (const fn of listeners.get('click') || []) fn({ type: 'click', target: this });
        },
        querySelectorAll(selector) {
          if (selector === '[role="menuitem"]') {
            return children.filter(c => c.getAttribute?.('role') === 'menuitem');
          }
          if (selector === '[role="separator"]') {
            return children.filter(c => c.getAttribute?.('role') === 'separator');
          }
          return children;
        },
        remove() {},
      };
    }

    function openContextMenu(rawEvent, targetOverride = null) {
      if (disposed) return null;
      closeContextMenu();
      const mouseTarget = resolveEventTarget(rawEvent, targetOverride);
      const found = findContextMenuOwner(mouseTarget);
      if (!found) return null;
      const { owner: contextMenuOwner, menu } = found;
      rawEvent?.preventDefault?.();

      if (ContextMenuEvent) {
        const selectEv = new ContextMenuEvent(
          ContextMenuEvent.MENU_SELECT || 'menuSelect',
          false,
          false,
          mouseTarget,
          contextMenuOwner
        );
        menu.dispatchEvent(selectEv);
      }

      const doc = options.document || global.document || null;
      const container =
        options.menuContainer ||
        options.canvas?.parentElement ||
        doc?.body ||
        null;
      const root = createFallbackDomNode('div', doc);
      root.className = 'nf-as3-context-menu';
      root.setAttribute('role', 'menu');
      Object.assign(root.style, {
        position: 'absolute',
        left: `${Number(rawEvent?.clientX ?? rawEvent?.stageX ?? 0)}px`,
        top: `${Number(rawEvent?.clientY ?? rawEvent?.stageY ?? 0)}px`,
        zIndex: '10000',
        display: 'flex',
        flexDirection: 'column',
      });

      const appendSeparator = () => {
        const sep = createFallbackDomNode('div', doc);
        sep.className = 'nf-as3-context-menu-separator';
        sep.setAttribute('role', 'separator');
        root.appendChild(sep);
      };

      const appendButton = (caption, enabled, kind, extraAttrs, onClick) => {
        const btn = createFallbackDomNode('button', doc);
        btn.type = 'button';
        btn.className = 'nf-as3-context-menu-item';
        btn.setAttribute('role', 'menuitem');
        btn.setAttribute('data-kind', kind);
        btn.setAttribute('aria-disabled', String(!enabled));
        for (const [k, v] of Object.entries(extraAttrs || {})) {
          btn.setAttribute(k, String(v));
        }
        btn.textContent = String(caption);
        btn.disabled = !enabled;
        btn.addEventListener('click', () => {
          if (btn.disabled) return;
          try {
            onClick();
          } finally {
            closeContextMenu();
          }
        });
        root.appendChild(btn);
        return btn;
      };

      if (menu.link && menu.link.url) {
        appendButton('Open Link', true, 'link', { 'data-url': menu.link.url }, () => {
          if (typeof net?.navigateToURL === 'function') {
            net.navigateToURL(menu.link, '_blank');
          }
        });
      }

      let renderedCustomCount = 0;
      for (const item of menu.customItems) {
        if (!item || !cmItemStates.has(item)) continue;
        if (!item.visible || !item.caption) continue;
        if (item.separatorBefore || (renderedCustomCount === 0 && root.children?.length > 0)) {
          appendSeparator();
        }
        renderedCustomCount++;
        appendButton(item.caption, Boolean(item.enabled), 'custom', {}, () => {
          if (ContextMenuEvent) {
            const itemEv = new ContextMenuEvent(
              ContextMenuEvent.MENU_ITEM_SELECT || 'menuItemSelect',
              false,
              false,
              mouseTarget,
              contextMenuOwner
            );
            item.dispatchEvent(itemEv);
          }
        });
      }

      const clip = menu.clipboardItems;
      const clipSpecs = [
        ['cut', 'Cut'],
        ['copy', 'Copy'],
        ['paste', 'Paste'],
        ['clear', 'Clear'],
        ['selectAll', 'Select All'],
      ];
      const anyClipEnabled =
        menu.clipboardMenu ||
        clip.cut ||
        clip.copy ||
        clip.paste ||
        clip.clear;
      if (anyClipEnabled && clip) {
        if (renderedCustomCount > 0) appendSeparator();
        for (const [key, label] of clipSpecs) {
          if (!menu.clipboardMenu && !clip[key]) continue;
          appendButton(label, Boolean(clip[key]), 'clipboard', { 'data-action': key }, () => {
            executeClipboardMenuAction(key, mouseTarget);
          });
        }
      }

      const builtIn = menu.builtInItems;
      const builtInSpecs = [
        ['zoom', 'Zoom'],
        ['quality', 'Quality'],
        ['play', 'Play'],
        ['loop', 'Loop'],
        ['rewind', 'Rewind'],
        ['forwardAndBack', 'Forward / Back'],
        ['print', 'Print...'],
        ['save', 'Save'],
      ];
      if (!menu.clipboardMenu && builtIn) {
        let addedBuiltInSeparator = false;
        for (const [key, label] of builtInSpecs) {
          if (!builtIn[key]) continue;
          if (!addedBuiltInSeparator && (renderedCustomCount > 0 || anyClipEnabled)) {
            appendSeparator();
            addedBuiltInSeparator = true;
          }
          appendButton(label, true, 'builtin', { 'data-builtin': key }, () => {
            if (typeof options.onBuiltInMenuAction === 'function') {
              options.onBuiltInMenuAction(key, mouseTarget, contextMenuOwner);
            }
          });
        }
      }

      if (container && typeof container.appendChild === 'function') {
        container.appendChild(root);
      }
      activeMenuElement = root;
      return root;
    }

    // ---- GameInput, GameInputDevice, GameInputControl ----
    const gameCtrlToken = {};
    const gameCtrlStates = new WeakMap();
    class GameInputControl extends BaseDispatcher {
      constructor(token, device, devIndex, ctrlIndex) {
        if (token !== gameCtrlToken) {
          throw makeArgumentError('Error #2012: GameInputControl cannot be instantiated directly.', 2012);
        }
        super();
        gameCtrlStates.set(this, { device, devIndex, ctrlIndex });
      }
      get device() { return gameCtrlStates.get(this).device; }
      get id() {
        const s = gameCtrlStates.get(this);
        const dev = JSON.parse(callKernel(() => kernel.game_device_state_json(s.devIndex)));
        return dev.controls[s.ctrlIndex].id;
      }
      get minValue() {
        const s = gameCtrlStates.get(this);
        const dev = JSON.parse(callKernel(() => kernel.game_device_state_json(s.devIndex)));
        return dev.controls[s.ctrlIndex].min_value;
      }
      get maxValue() {
        const s = gameCtrlStates.get(this);
        const dev = JSON.parse(callKernel(() => kernel.game_device_state_json(s.devIndex)));
        return dev.controls[s.ctrlIndex].max_value;
      }
      get value() {
        const s = gameCtrlStates.get(this);
        const dev = JSON.parse(callKernel(() => kernel.game_device_state_json(s.devIndex)));
        return dev.controls[s.ctrlIndex].value;
      }
    }
    tagType(GameInputControl, 'GameInputControl', v => gameCtrlStates.has(v));

    const gameDevToken = {};
    const gameDevStates = new WeakMap();
    const gameDevInstances = new Map();
    class GameInputDevice {
      constructor(token, index) {
        if (token !== gameDevToken) {
          throw makeArgumentError('Error #2012: GameInputDevice cannot be instantiated directly.', 2012);
        }
        callKernel(() => kernel.game_device_state_json(index));
        gameDevStates.set(this, { index, controls: new Map() });
      }
      get id() {
        const s = gameDevStates.get(this);
        return JSON.parse(callKernel(() => kernel.game_device_state_json(s.index))).id;
      }
      get name() {
        const s = gameDevStates.get(this);
        return JSON.parse(callKernel(() => kernel.game_device_state_json(s.index))).name;
      }
      get enabled() {
        const s = gameDevStates.get(this);
        return JSON.parse(callKernel(() => kernel.game_device_state_json(s.index))).enabled;
      }
      set enabled(v) {
        const s = gameDevStates.get(this);
        callKernel(() => kernel.game_device_set_enabled(s.index, Boolean(v)));
      }
      get sampleInterval() {
        const s = gameDevStates.get(this);
        return JSON.parse(callKernel(() => kernel.game_device_state_json(s.index))).sample_interval;
      }
      set sampleInterval(v) {
        const s = gameDevStates.get(this);
        callKernel(() => kernel.game_device_set_sample_interval(s.index, Number(v) | 0));
      }
      get numControls() {
        const s = gameDevStates.get(this);
        return JSON.parse(callKernel(() => kernel.game_device_state_json(s.index))).controls.length;
      }
      getControlAt(i) {
        const s = gameDevStates.get(this);
        const idx = Number(i) | 0;
        const dev = JSON.parse(callKernel(() => kernel.game_device_state_json(s.index)));
        if (idx < 0 || idx >= dev.controls.length) {
          throw makeRangeError(`GameInputDevice.getControlAt index ${idx} is out of bounds.`, 2006);
        }
        let ctrl = s.controls.get(idx);
        if (!ctrl) {
          ctrl = new GameInputControl(gameCtrlToken, this, s.index, idx);
          s.controls.set(idx, ctrl);
        }
        return ctrl;
      }
      startCachingSamples(numSamples, controls) {
        const s = gameDevStates.get(this);
        const ids = Array.from(controls || [], x => String(x));
        callKernel(() =>
          kernel.game_device_start_caching(s.index, Number(numSamples) | 0, JSON.stringify(ids))
        );
      }
      stopCachingSamples() {
        const s = gameDevStates.get(this);
        callKernel(() => kernel.game_device_stop_caching(s.index));
      }
      getCachedSamples(data, append = false) {
        const s = gameDevStates.get(this);
        if (!isByteArray(data)) {
          throw new TypeError('GameInputDevice.getCachedSamples requires a ByteArray.');
        }
        if (!append && typeof data.clear === 'function') data.clear();
        const drained = JSON.parse(callKernel(() => kernel.game_device_drain_samples_json(s.index)));
        for (let i = 0; i < drained.bytes.length; i++) {
          data.writeByte(drained.bytes[i]);
        }
        return drained.rows;
      }
    }
    GameInputDevice.MAX_BUFFER_SIZE = 32000;
    tagType(GameInputDevice, 'GameInputDevice', v => gameDevStates.has(v));

    const gameInputSet = new WeakSet();
    const liveGameInputs = new Set();
    class GameInput extends BaseDispatcher {
      constructor() {
        super();
        gameInputSet.add(this);
        liveGameInputs.add(this);
      }
      static get isSupported() { return true; }
      static get numDevices() {
        return callKernel(() => kernel.game_device_count());
      }
      static getDeviceAt(index) {
        const idx = Number(index) | 0;
        callKernel(() => kernel.game_device_state_json(idx));
        let inst = gameDevInstances.get(idx);
        if (!inst) {
          inst = new GameInputDevice(gameDevToken, idx);
          gameDevInstances.set(idx, inst);
        }
        return inst;
      }
    }
    tagType(GameInput, 'GameInput', v => gameInputSet.has(v));

    const GameInputEvent =
      events.GameInputEvent ||
      airBuiltins?.['flash.events.GameInputEvent'] ||
      (() => {
        const giEvStates = new WeakMap();
        class FallbackGameInputEvent extends BaseEvent {
          constructor(type = 'deviceAdded', bubbles = false, cancelable = false, device = null) {
            super(type, bubbles, cancelable);
            giEvStates.set(this, { device: device ?? null });
          }
          get device() { return giEvStates.get(this).device; }
          clone() {
            return new FallbackGameInputEvent(this.type, this.bubbles, this.cancelable, this.device);
          }
          toString() {
            return fmtEvent(this, 'GameInputEvent', ['device']);
          }
        }
        Object.assign(FallbackGameInputEvent, {
          DEVICE_ADDED: 'deviceAdded',
          DEVICE_REMOVED: 'deviceRemoved',
          DEVICE_UNUSABLE: 'deviceUnusable',
        });
        tagType(FallbackGameInputEvent, 'GameInputEvent', v => giEvStates.has(v));
        return FallbackGameInputEvent;
      })();

    function syncGamepads(gamepadList) {
      if (disposed || !Array.isArray(gamepadList)) return [];
      const results = [];
      for (let i = 0; i < gamepadList.length && i < 16; i++) {
        const gp = gamepadList[i];
        if (!gp) continue;
        const id = String(gp.id || `gamepad-${i}`);
        const name = String(gp.name || gp.id || `Gamepad ${i + 1}`);
        const axes = Array.isArray(gp.axes) ? gp.axes.map(Number) : [];
        const buttons = Array.isArray(gp.buttons)
          ? gp.buttons.map(b => (typeof b === 'object' && b !== null ? Number(b.value ?? (b.pressed ? 1 : 0)) : Number(b)))
          : [];
        const syncRes = JSON.parse(
          callKernel(() =>
            kernel.game_device_sync_json(
              i,
              id,
              name,
              JSON.stringify(axes),
              JSON.stringify(buttons)
            )
          )
        );
        const deviceObj = GameInput.getDeviceAt(i);
        if (syncRes.is_new && GameInputEvent) {
          for (const gi of liveGameInputs) {
            if (gi.hasEventListener(GameInputEvent.DEVICE_ADDED || 'deviceAdded')) {
              gi.dispatchEvent(
                new GameInputEvent(
                  GameInputEvent.DEVICE_ADDED || 'deviceAdded',
                  false,
                  false,
                  deviceObj
                )
              );
            }
          }
        }
        if (Array.isArray(syncRes.changed_controls) && syncRes.changed_controls.length > 0 && deviceObj.enabled) {
          const devState = syncRes.device;
          for (const cid of syncRes.changed_controls) {
            const ctrlIdx = devState.controls.findIndex(c => c.id === cid);
            if (ctrlIdx >= 0) {
              const ctrlObj = deviceObj.getControlAt(ctrlIdx);
              if (ctrlObj.hasEventListener('change')) {
                ctrlObj.dispatchEvent(new BaseEvent(BaseEvent.CHANGE || 'change', false, false));
              }
            }
          }
        }
        results.push({ index: i, device: deviceObj, isNew: syncRes.is_new, changedControls: syncRes.changed_controls });
      }
      return results;
    }

    function removeGamepad(index) {
      if (disposed) return null;
      const idx = Number(index) | 0;
      const deviceObj = gameDevInstances.get(idx) || null;
      callKernel(() => kernel.game_device_remove_json(idx));
      gameDevInstances.delete(idx);
      if (GameInputEvent) {
        for (const gi of liveGameInputs) {
          if (gi.hasEventListener(GameInputEvent.DEVICE_REMOVED || 'deviceRemoved')) {
            gi.dispatchEvent(
              new GameInputEvent(
                GameInputEvent.DEVICE_REMOVED || 'deviceRemoved',
                false,
                false,
                deviceObj
              )
            );
          }
        }
      }
      return deviceObj;
    }

    function pollNavigatorGamepads() {
      if (disposed || typeof global.navigator?.getGamepads !== 'function') return [];
      try {
        const pads = Array.from(global.navigator.getGamepads() || []).filter(Boolean);
        return pads.length ? syncGamepads(pads) : [];
      } catch (_) {
        return [];
      }
    }

    // =========================================================================
    // 4. flash.accessibility.*
    // =========================================================================

    const accPropsStates = new WeakMap();
    class AccessibilityProperties {
      constructor() {
        accPropsStates.set(this, {
          name: '',
          description: '',
          shortcut: '',
          silent: false,
          forceSimple: false,
          noAutoLabeling: false,
        });
      }
      get name() { return accPropsStates.get(this).name; }
      set name(v) {
        const valid = callKernel(() => kernel.validate_acc_string(String(v ?? ''), 'name'));
        accPropsStates.get(this).name = valid;
      }
      get description() { return accPropsStates.get(this).description; }
      set description(v) {
        const valid = callKernel(() => kernel.validate_acc_string(String(v ?? ''), 'description'));
        accPropsStates.get(this).description = valid;
      }
      get shortcut() { return accPropsStates.get(this).shortcut; }
      set shortcut(v) {
        const valid = callKernel(() => kernel.validate_acc_string(String(v ?? ''), 'shortcut'));
        accPropsStates.get(this).shortcut = valid;
      }
      get silent() { return accPropsStates.get(this).silent; }
      set silent(v) { accPropsStates.get(this).silent = Boolean(v); }
      get forceSimple() { return accPropsStates.get(this).forceSimple; }
      set forceSimple(v) { accPropsStates.get(this).forceSimple = Boolean(v); }
      get noAutoLabeling() { return accPropsStates.get(this).noAutoLabeling; }
      set noAutoLabeling(v) { accPropsStates.get(this).noAutoLabeling = Boolean(v); }
    }
    tagType(AccessibilityProperties, 'AccessibilityProperties', v => accPropsStates.has(v));

    const accImplStates = new WeakMap();
    function ensureAccImplState(receiver) {
      let st = accImplStates.get(receiver);
      if (!st) {
        st = { errno: 0, stub: false };
        accImplStates.set(receiver, st);
      }
      return st;
    }

    class AccessibilityImplementation {
      constructor() {
        ensureAccImplState(this);
      }
      get errno() { return ensureAccImplState(this).errno; }
      set errno(v) { ensureAccImplState(this).errno = Number(v) >>> 0; }
      get stub() { return ensureAccImplState(this).stub; }
      set stub(v) { ensureAccImplState(this).stub = Boolean(v); }

      accDoDefaultAction(_childID) {}
      accLocation(_childID) { return null; }
      accSelect(_operation, _childID) {}
      getChildIDArray() { return null; }
      get_accDefaultAction(_childID) { return null; }
      get_accFocus() { return 0; }
      get_accName(_childID) { return null; }
      get_accRole(_childID) {
        const err = new Error('Error #2143: AccessibilityImplementation.get_accRole() must be overridden from its default.');
        err.errorID = 2143;
        throw err;
      }
      get_accSelection() { return null; }
      get_accState(_childID) {
        const err = new Error('Error #2144: AccessibilityImplementation.get_accState() must be overridden from its default.');
        err.errorID = 2144;
        throw err;
      }
      get_accValue(_childID) { return null; }
      isLabeledBy(_labelBounds) { return false; }
      get_selectionActiveIndex() { return null; }
      get_selectionAnchorIndex() { return null; }
    }
    tagType(
      AccessibilityImplementation,
      'AccessibilityImplementation',
      v => v !== null && typeof v === 'object' && (accImplStates.has(v) || v instanceof AccessibilityImplementation),
      (receiver, args) => {
        if (args && args.length > 0) {
          throw makeArgumentError('AccessibilityImplementation constructor expects 0 arguments.', 1063);
        }
        ensureAccImplState(receiver);
      }
    );

    function applyAccessibilityPropertiesToSemanticNodes(nodes, resolveDisplayObject) {
      if (!Array.isArray(nodes)) return [];
      const out = [];
      for (const node of nodes) {
        if (!node || typeof node !== 'object') continue;
        const baseAcc = { ...(node.accessibility || {}) };
        let disp = null;
        if (typeof resolveDisplayObject === 'function') {
          try { disp = resolveDisplayObject(node); } catch (_) {}
        }
        const props = disp?.accessibilityProperties;
        if (props && typeof props === 'object') {
          if (typeof props.name === 'string' && props.name.length > 0) baseAcc.name = props.name;
          if (typeof props.description === 'string') baseAcc.description = props.description;
          if (typeof props.shortcut === 'string') baseAcc.shortcut = props.shortcut;
          if (typeof props.silent === 'boolean') baseAcc.silent = props.silent;
          if (typeof props.forceSimple === 'boolean') baseAcc.forceSimple = props.forceSimple;
          if (typeof props.noAutoLabeling === 'boolean') baseAcc.noAutoLabeling = props.noAutoLabeling;
        }
        const impl = disp?.accessibilityImplementation;
        if (impl && typeof impl === 'object' && !impl.stub) {
          try {
            const implName = impl.get_accName?.(0);
            if (typeof implName === 'string' && implName.length > 0) baseAcc.name = implName;
          } catch (_) {}
          try {
            const implVal = impl.get_accValue?.(0);
            if (typeof implVal === 'string' && implVal.length > 0 && !baseAcc.description) {
              baseAcc.description = implVal;
            }
          } catch (_) {}
        }
        if (baseAcc.silent) continue;
        out.push({ ...node, accessibility: baseAcc });
      }
      return out;
    }

    class Accessibility {
      constructor() {
        throw makeArgumentError('Error #2012: Accessibility$ class cannot be instantiated.', 2012);
      }
      static get active() {
        if (typeof options.getAccessibilityActive === 'function') {
          return Boolean(options.getAccessibilityActive());
        }
        return false;
      }
      static updateProperties() {
        callKernel(() => kernel.record_accessibility_update());
        if (typeof options.onAccessibilityUpdate === 'function') {
          options.onAccessibilityUpdate();
        }
      }
      static sendEvent(source, childID, eventType, nonHTML = false) {
        if (!source || typeof source !== 'object') {
          throw new TypeError('Accessibility.sendEvent requires a DisplayObject source.');
        }
        void (Number(childID) >>> 0);
        void (Number(eventType) >>> 0);
        void Boolean(nonHTML);
        if (typeof options.onAccessibilityEvent === 'function') {
          options.onAccessibilityEvent(source, Number(childID) >>> 0, Number(eventType) >>> 0, Boolean(nonHTML));
        }
      }
    }
    tagType(Accessibility, 'Accessibility', () => false);

    function ISearchableText() {
      throw new TypeError('ISearchableText is an interface and cannot be instantiated.');
    }
    ISearchableText.__nfIs = value =>
      value !== null && typeof value === 'object' && typeof value.searchText === 'string';
    Object.defineProperty(ISearchableText, Symbol.hasInstance, {
      value: ISearchableText.__nfIs,
      configurable: true,
    });

    function ISimpleTextSelection() {
      throw new TypeError('ISimpleTextSelection is an interface and cannot be instantiated.');
    }
    ISimpleTextSelection.__nfIs = value =>
      value !== null &&
      typeof value === 'object' &&
      typeof value.selectionActiveIndex === 'number' &&
      typeof value.selectionAnchorIndex === 'number';
    Object.defineProperty(ISimpleTextSelection, Symbol.hasInstance, {
      value: ISimpleTextSelection.__nfIs,
      configurable: true,
    });

    // =========================================================================
    // Canvas DOM Listeners & Lifecycle
    // =========================================================================
    const canvasEl = options.canvas || null;
    const onCanvasContextMenu = ev => {
      try {
        openContextMenu(ev);
      } catch (err) {
        options.onError?.(err);
      }
    };
    const onCanvasPointerDown = ev => {
      closeContextMenu();
      if (Multitouch.inputMode !== MultitouchInputMode.NONE) {
        try {
          handlePointerInput('pointerdown', ev);
        } catch (err) {
          options.onError?.(err);
        }
      }
    };
    const onCanvasPointerMove = ev => {
      if (Multitouch.inputMode !== MultitouchInputMode.NONE && activeTouches.size > 0) {
        try {
          handlePointerInput('pointermove', ev);
        } catch (err) {
          options.onError?.(err);
        }
      }
    };
    const onCanvasPointerUp = ev => {
      if (Multitouch.inputMode !== MultitouchInputMode.NONE && activeTouches.size > 0) {
        try {
          handlePointerInput('pointerup', ev);
        } catch (err) {
          options.onError?.(err);
        }
      }
    };

    if (canvasEl && typeof canvasEl.addEventListener === 'function') {
      canvasEl.addEventListener('contextmenu', onCanvasContextMenu);
      canvasEl.addEventListener('pointerdown', onCanvasPointerDown);
      canvasEl.addEventListener('pointermove', onCanvasPointerMove);
      canvasEl.addEventListener('pointerup', onCanvasPointerUp);
    }

    function reset() {
      if (disposed) return;
      closeContextMenu();
      activeTouches.clear();
      primaryTouchId = null;
      activeGestureState = null;
      liveGameInputs.clear();
      cameraInstances.clear();
      micInstances.clear();
      stageVideoPool.length = 0;
      cachedStageVideosVector = null;
      gameDevInstances.clear();
      audioMgrSingleton = null;
      kernel.reset();
    }

    function dispose() {
      if (disposed) return;
      disposed = true;
      closeContextMenu();
      if (canvasEl && typeof canvasEl.removeEventListener === 'function') {
        canvasEl.removeEventListener('contextmenu', onCanvasContextMenu);
        canvasEl.removeEventListener('pointerdown', onCanvasPointerDown);
        canvasEl.removeEventListener('pointermove', onCanvasPointerMove);
        canvasEl.removeEventListener('pointerup', onCanvasPointerUp);
      }
      activeTouches.clear();
      primaryTouchId = null;
      activeGestureState = null;
      liveGameInputs.clear();
      cameraInstances.clear();
      micInstances.clear();
      stageVideoPool.length = 0;
      cachedStageVideosVector = null;
      gameDevInstances.clear();
      audioMgrSingleton = null;
      kernel.free();
      kernel = null;
    }

    const builtins = Object.freeze({
      // flash.accessibility.*
      'flash.accessibility.Accessibility': Accessibility,
      'flash.accessibility.AccessibilityImplementation': AccessibilityImplementation,
      'flash.accessibility.AccessibilityProperties': AccessibilityProperties,
      'flash.accessibility.ISearchableText': ISearchableText,
      'flash.accessibility.ISimpleTextSelection': ISimpleTextSelection,

      // flash.events.*
      'flash.events.GameInputEvent': GameInputEvent,
      'flash.events.AVDictionaryDataEvent': AVDictionaryDataEvent,
      'flash.events.AVHTTPStatusEvent': AVHTTPStatusEvent,
      'flash.events.AudioOutputChangeEvent': AudioOutputChangeEvent,
      'flash.events.DRMAuthenticationCompleteEvent': DRMAuthenticationCompleteEvent,
      'flash.events.DRMAuthenticationErrorEvent': DRMAuthenticationErrorEvent,
      'flash.events.DRMDeviceGroupErrorEvent': DRMDeviceGroupErrorEvent,
      'flash.events.DRMDeviceGroupEvent': DRMDeviceGroupEvent,
      'flash.events.DRMErrorEvent': DRMErrorEvent,
      'flash.events.DRMLicenseRequestEvent': DRMLicenseRequestEvent,
      'flash.events.DRMMetadataEvent': DRMMetadataEvent,
      'flash.events.DRMReturnVoucherCompleteEvent': DRMReturnVoucherCompleteEvent,
      'flash.events.DRMReturnVoucherErrorEvent': DRMReturnVoucherErrorEvent,
      'flash.events.DRMStatusEvent': DRMStatusEvent,
      'flash.events.NetFilterEvent': NetFilterEvent,
      'flash.events.NetMonitorEvent': NetMonitorEvent,
      'flash.events.ShaderEvent': ShaderEvent,
      'flash.events.StageVideoAvailabilityEvent': StageVideoAvailabilityEvent,
      'flash.events.StageVideoEvent': StageVideoEvent,
      'flash.events.UncaughtErrorEvents': UncaughtErrorEvents,
      'flash.events.VideoEvent': VideoEvent,
      'flash.events.VideoTextureEvent': VideoTextureEvent,

      // flash.media.*
      'flash.media.scanHardware': scanHardware,
      'flash.media.AVTagData': AVTagData,
      'flash.media.AVURLLoader': AVURLLoader,
      'flash.media.AVURLStream': AVURLStream,
      'flash.media.AudioDecoder': AudioDecoder,
      'flash.media.AudioDeviceManager': AudioDeviceManager,
      'flash.media.AudioOutputChangeReason': AudioOutputChangeReason,
      'flash.media.Camera': Camera,
      'flash.media.CameraPosition': CameraPosition,
      'flash.media.H264Level': H264Level,
      'flash.media.H264Profile': H264Profile,
      'flash.media.H264VideoStreamSettings': H264VideoStreamSettings,
      'flash.media.Microphone': Microphone,
      'flash.media.MicrophoneEnhancedMode': MicrophoneEnhancedMode,
      'flash.media.MicrophoneEnhancedOptions': MicrophoneEnhancedOptions,
      'flash.media.SoundCodec': SoundCodec,
      'flash.media.StageVideo': StageVideo,
      'flash.media.StageVideoAvailability': StageVideoAvailability,
      'flash.media.StageVideoAvailabilityReason': StageVideoAvailabilityReason,
      'flash.media.VP6VideoStreamSettings': VP6VideoStreamSettings,
      'flash.media.VideoCodec': VideoCodec,
      'flash.media.VideoStreamSettings': VideoStreamSettings,

      // flash.ui.*
      'flash.ui.ContextMenu': ContextMenu,
      'flash.ui.ContextMenuBuiltInItems': ContextMenuBuiltInItems,
      'flash.ui.ContextMenuClipboardItems': ContextMenuClipboardItems,
      'flash.ui.ContextMenuItem': ContextMenuItem,
      'flash.ui.GameInput': GameInput,
      'flash.ui.GameInputControl': GameInputControl,
      'flash.ui.GameInputDevice': GameInputDevice,
      'flash.ui.Multitouch': Multitouch,
      'flash.ui.MultitouchInputMode': MultitouchInputMode,
    });

    return Object.freeze({
      builtins,
      cameraVideoAdapter,
      getStageVideos: () => createStageVideosVector(),
      paintStageVideos,
      stepMicrophones,
      scanHardware,
      handlePointerInput,
      openContextMenu,
      closeContextMenu,
      getActiveContextMenu,
      syncGamepads,
      removeGamepad,
      pollNavigatorGamepads,
      applyAccessibilityPropertiesToSemanticNodes,
      setVectorFactory,
      bindStage: (StageClass, stageInst = null) =>
        bindStageMediaProperties(StageClass, () => createStageVideosVector(), stageInst, dispatchStageVideoAvailability),
      bindLoaderInfo: LoaderInfoClass => bindLoaderInfoProperties(LoaderInfoClass, () => new UncaughtErrorEvents()),
      reset,
      dispose,
    });
  }

  global.NeoFlashAS3MediaEvents = Object.freeze({ create });
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = global.NeoFlashAS3MediaEvents;
  }
})(typeof globalThis !== 'undefined' ? globalThis : window);
