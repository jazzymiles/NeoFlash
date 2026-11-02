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
        value: { byStage: new WeakMap(), fallback: null },
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

  function bindStageMediaProperties(StageClass, getVectorForStage, stageInstance = null) {
    const proto =
      StageClass?.prototype ||
      (stageInstance && typeof stageInstance === 'object' ? Object.getPrototypeOf(stageInstance) : null);
    if (!proto || proto === Object.prototype) return;
    installStagePrototypeGetter(proto);
    const store = proto[stageHookSym];
    if (stageInstance && typeof stageInstance === 'object') {
      store.byStage.set(stageInstance, getVectorForStage);
    } else {
      store.fallback = getVectorForStage;
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
        cameraStates.set(this, { index, cache });
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
              destination.setPixel32(px, py, pixels[k++] >>> 0);
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
              image: null,
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
    }
    tagType(Microphone, 'Microphone', v => micStates.has(v));

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

    const stageObj = typeof options.getStage === 'function' ? options.getStage() : null;
    if (options.StageClass || stageObj) {
      bindStageMediaProperties(options.StageClass, () => createStageVideosVector(), stageObj);
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
    class GameInput extends BaseDispatcher {
      constructor() {
        super();
        gameInputSet.add(this);
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
    // Lifecycle & Builtin Map
    // =========================================================================

    function reset() {
      if (disposed) return;
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
      setVectorFactory,
      bindStage: StageClass => bindStageMediaProperties(StageClass, () => createStageVideosVector()),
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
