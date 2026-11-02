/* ActionScript 3 Area 8 (Net, System, Globalization, Crypto, Concurrent,
 * Printing, Profiler, Sampler, Security, Sensors) runtime host adapter.
 * Stateful algorithms, RFC 6234 SHA-256 CSPRNG, BCP-47 LocaleID/Collator/
 * CurrencyFormatter/StringTools, RTVFP GroupSpecifier/NetGroup mesh, FLV tag
 * demuxer, ASN.1 DER X.509 parser, DRM voucher engine, and Worker/Mutex/Sampler
 * state machines live in Rust (crates/neoflash-core/src/as3_net_sys.rs).
 */
(function (global) {
  'use strict';

  function defConstClass(name, constants, extraProps = {}) {
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
        configurable: true,
        value: () => {
          throw new TypeError(`${name} supplies constants only and cannot be extended.`);
        },
      },
      [Symbol.hasInstance]: { value: () => false },
      ...extraProps,
    });
    return Object.freeze(Ctor);
  }

  function tagType(Ctor, name, check, initFn) {
    Object.defineProperties(Ctor, {
      __nfIs: { configurable: true, value: check },
      [Symbol.hasInstance]: { configurable: true, value: check },
      __nfInitialize: {
        configurable: true,
        value:
          initFn ||
          function (instance, ...args) {
            if (instance && typeof instance === 'object') {
              const created = Reflect.construct(Ctor, args, Ctor);
              return created;
            }
            throw new TypeError(`${name} initialization requires a valid receiver.`);
          },
      },
    });
    return Ctor;
  }

  function toBase64(bytes) {
    if (typeof Buffer === 'function') {
      return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength).toString('base64');
    }
    let bin = '';
    for (let i = 0; i < bytes.byteLength; i++) bin += String.fromCharCode(bytes[i]);
    return global.btoa(bin);
  }

  function fromBase64(str) {
    if (!str) return new Uint8Array(0);
    if (typeof Buffer === 'function') {
      return new Uint8Array(Buffer.from(String(str), 'base64'));
    }
    const bin = global.atob(String(str));
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i) & 0xff;
    return out;
  }

  function create(options = {}) {
    const wasm = options.wasm || global.NeoFlashWasm;
    const events = options.events || global.NeoFlashEvents;
    if (!wasm?.As3NetSysKernel || !events?.Event || !events?.EventDispatcher) {
      throw new Error('NeoFlashAS3NetSys requires the Rust As3NetSysKernel WASM service and NeoFlashEvents.');
    }
    const binary = options.binary || null;
    const amf = options.amf || null;
    const netHost = options.net || null;
    const videoHost = options.video || null;
    const storageHost = options.storage || null;
    const externalHost = options.external || null;
    const xmlHost = options.xml || null;
    const displayHost = options.display || null;
    const geometryHost = options.geometry || null;
    const getBitmap = options.getBitmap || (() => null);
    const numberFormatHost = options.numberFormat || null;
    const getBuiltins = options.getBuiltins || (() => null);
    const getProgram = options.getProgram || (() => null);
    const getViewportSize = options.getViewportSize || (() => [1920, 1080]);
    const DynamicPropertyOutput =
      amf?.DynamicPropertyOutput ||
      global.NeoFlashAMF?.DynamicPropertyOutput ||
      class DynamicPropertyOutput {
        constructor() { this._props = new Map(); }
        writeDynamicProperty(name, value) {
          if (name === null || name === undefined) throw new TypeError('Error #2007: Parameter name must be non-null.');
          this._props.set(String(name), value);
        }
      };
    const schedule =
      options.schedule ||
      (cb => {
        if (typeof global.queueMicrotask === 'function') global.queueMicrotask(cb);
        else Promise.resolve().then(cb);
      });
    const afterEvent = options.afterEvent || (() => {});

    let disposed = false;
    let generation = 1;
    let vectorFactory = (_typeName, values) => Array.from(values);
    let kernel = new wasm.As3NetSysKernel();

    function setVectorFactory(fn) {
      if (typeof fn === 'function') vectorFactory = fn;
    }
    function makeVector(typeName, values) {
      return vectorFactory(typeName, Array.from(values));
    }

    function ensureActive() {
      if (disposed || !kernel) throw new Error('AS3 Net/System runtime service has been disposed.');
    }

    function translateKernelError(raw) {
      const msg = String(raw?.message ?? raw);
      const prefixes = [
        ['ArgumentError:', 'ArgumentError', 2004, Error],
        ['SecurityError:', 'SecurityError', 2048, Error],
        ['IllegalOperationError:', 'IllegalOperationError', 2037, Error],
        ['IOError:', 'IOError', 2032, Error],
        ['EOFError:', 'EOFError', 2030, RangeError],
        ['MemoryError:', 'MemoryError', 1000, Error],
        ['RangeError:', 'RangeError', 2006, RangeError],
        ['TypeError:', 'TypeError', 2007, TypeError],
      ];
      for (const [prefix, name, id, Base] of prefixes) {
        if (msg.startsWith(prefix)) {
          const text = msg.slice(prefix.length).trim();
          const numMatch = text.match(/Error #(\d+)/);
          const err = new Base(text);
          err.name = name;
          err.errorID = numMatch ? Number(numMatch[1]) : id;
          return err;
        }
      }
      return raw instanceof Error ? raw : new Error(msg);
    }

    function execKernel(payload) {
      ensureActive();
      try {
        return JSON.parse(kernel.execute_json(JSON.stringify(payload)));
      } catch (err) {
        throw translateKernelError(err);
      }
    }

    const aliveTargets = new WeakMap();
    const hub = events.create({
      weakTargets: true,
      alive: value => !disposed && aliveTargets.get(value) === generation,
    });

    function registerDispatcher(instance) {
      aliveTargets.set(instance, generation);
      Object.assign(instance, hub.methods(instance));
      return instance;
    }

    function dispatchLater(target, makeEvent) {
      const born = generation;
      schedule(() => {
        if (disposed || born !== generation || aliveTargets.get(target) !== generation) return;
        const ev = typeof makeEvent === 'function' ? makeEvent() : makeEvent;
        if (ev) {
          hub.dispatch(target, ev);
          afterEvent();
        }
      });
    }

    function dispatchNow(target, ev) {
      if (disposed || aliveTargets.get(target) !== generation || !ev) return false;
      const r = hub.dispatch(target, ev);
      afterEvent();
      return r;
    }

    function getEventCtor(qname) {
      const b = getBuiltins();
      return b ? b[qname] : null;
    }

    function makeBasicEvent(type, bubbles = false, cancelable = false) {
      return new events.Event(type, bubbles, cancelable);
    }

    function makeProgressEvent(type, loaded = 0, total = 0) {
      const Ctor = getEventCtor('flash.events.ProgressEvent');
      if (Ctor) return new Ctor(type, false, false, loaded, total);
      const ev = new events.Event(type, false, false);
      ev.bytesLoaded = loaded;
      ev.bytesTotal = total;
      return ev;
    }

    function makeIOErrorEvent(type, text = 'I/O Error', id = 2032) {
      const Ctor = getEventCtor('flash.events.IOErrorEvent');
      if (Ctor) return new Ctor(type, false, false, text, id);
      const ev = new events.Event(type, false, false);
      ev.text = text;
      ev.errorID = id;
      return ev;
    }

    function makeSecurityErrorEvent(type, text = 'Security Error', id = 2048) {
      const Ctor = getEventCtor('flash.events.SecurityErrorEvent');
      if (Ctor) return new Ctor(type, false, false, text, id);
      const ev = new events.Event(type, false, false);
      ev.text = text;
      ev.errorID = id;
      return ev;
    }

    function makeStatusEvent(type, code = '', level = 'status') {
      const Ctor = getEventCtor('flash.events.StatusEvent');
      if (Ctor) return new Ctor(type, false, false, code, level);
      const ev = new events.Event(type, false, false);
      ev.code = code;
      ev.level = level;
      return ev;
    }

    function makeAsyncErrorEvent(type, text = 'Async Error', error = null) {
      const Ctor = getEventCtor('flash.events.AsyncErrorEvent');
      if (Ctor) return new Ctor(type, false, false, text, error);
      const ev = new events.Event(type, false, false);
      ev.text = text;
      ev.error = error;
      return ev;
    }

    function makeDataEvent(type, data = '') {
      const Ctor = getEventCtor('flash.events.DataEvent');
      if (Ctor) return new Ctor(type, false, false, String(data));
      const ev = new events.Event(type, false, false);
      ev.data = String(data);
      return ev;
    }

    function makeHTTPStatusEvent(status = 200) {
      const Ctor = getEventCtor('flash.events.HTTPStatusEvent');
      if (Ctor) return new Ctor('httpStatus', false, false, status);
      const ev = new events.Event('httpStatus', false, false);
      ev.status = status;
      return ev;
    }

    function makeSyncEvent(changeList = []) {
      const Ctor = getEventCtor('flash.events.SyncEvent');
      if (Ctor) return new Ctor('sync', false, false, changeList);
      const ev = new events.Event('sync', false, false);
      ev.changeList = changeList;
      return ev;
    }

    function makeIMEEvent(type, text = '') {
      const Ctor = getEventCtor('flash.events.IMEEvent');
      if (Ctor) return new Ctor(type, false, false, String(text));
      const ev = new events.TextEvent(type, false, false, String(text));
      return ev;
    }

    function makeAccelerometerEvent(timestamp = 0, x = 0, y = 0, z = -1) {
      const Ctor = getEventCtor('flash.events.AccelerometerEvent');
      if (Ctor) return new Ctor('update', false, false, timestamp, x, y, z);
      const ev = new events.Event('update', false, false);
      ev.timestamp = timestamp;
      ev.accelerationX = x;
      ev.accelerationY = y;
      ev.accelerationZ = z;
      return ev;
    }

    function makeNetDataEvent(timestamp = 0, info = null) {
      const Ctor = getEventCtor('flash.events.NetDataEvent');
      if (Ctor) return new Ctor('mediaTypeData', false, false, timestamp, info);
      const ev = new events.Event('mediaTypeData', false, false);
      ev.timestamp = timestamp;
      ev.info = info;
      return ev;
    }

    function makeNetMonitorEvent(netStream) {
      const Ctor = getEventCtor('flash.events.NetMonitorEvent');
      if (Ctor) return new Ctor('netStreamCreate', false, false, netStream);
      const ev = new events.Event('netStreamCreate', false, false);
      ev.netStream = netStream;
      return ev;
    }

    function makeDRMStatusEvent(contentData = null, voucher = null, isLocal = false) {
      const Ctor = getEventCtor('flash.events.DRMStatusEvent');
      if (Ctor) {
        try {
          return new Ctor('drmStatus', false, false, contentData, voucher, isLocal);
        } catch (_) {}
      }
      const ev = new events.Event('drmStatus', false, false);
      ev.contentData = contentData;
      ev.voucher = voucher;
      ev.isLocal = Boolean(isLocal);
      ev.isAvailableOffline = Boolean(voucher?.offlineLeaseEndDate);
      ev.voucherEndDate = voucher?.voucherEndDate || null;
      ev.offlineLeasePeriod = voucher ? 7 : 0;
      ev.policies = voucher?.policies || {};
      return ev;
    }

    function makeDRMErrorEvent(text = 'DRM Error', errorID = 3305, subErrorID = 0, contentData = null, systemUpdateNeeded = false, drmUpdateNeeded = false) {
      const Ctor = getEventCtor('flash.events.DRMErrorEvent');
      if (Ctor) {
        try {
          return new Ctor('drmError', false, false, text, errorID, subErrorID, contentData, systemUpdateNeeded, drmUpdateNeeded);
        } catch (_) {}
      }
      const ev = new events.Event('drmError', false, false);
      ev.text = text;
      ev.errorID = errorID;
      ev.subErrorID = subErrorID;
      ev.contentData = contentData;
      ev.systemUpdateNeeded = Boolean(systemUpdateNeeded);
      ev.drmUpdateNeeded = Boolean(drmUpdateNeeded);
      return ev;
    }

    function makeDRMAuthCompleteEvent(serverURL, domain, tokenBytes) {
      const Ctor = getEventCtor('flash.events.DRMAuthenticationCompleteEvent');
      const ba = binary?.fromBytes ? binary.fromBytes(tokenBytes) : tokenBytes;
      if (Ctor) {
        try {
          return new Ctor('authenticationComplete', false, false, serverURL, domain, ba);
        } catch (_) {}
      }
      const ev = new events.Event('authenticationComplete', false, false);
      ev.serverURL = serverURL;
      ev.domain = domain;
      ev.token = ba;
      return ev;
    }

    function makeDRMAuthErrorEvent(serverURL, domain, text = 'Auth Error', errorID = 3315, subErrorID = 401) {
      const Ctor = getEventCtor('flash.events.DRMAuthenticationErrorEvent');
      if (Ctor) {
        try {
          return new Ctor('authenticationError', false, false, text, errorID, subErrorID, serverURL, domain);
        } catch (_) {}
      }
      const ev = new events.Event('authenticationError', false, false);
      ev.text = text;
      ev.errorID = errorID;
      ev.subErrorID = subErrorID;
      ev.serverURL = serverURL;
      ev.domain = domain;
      return ev;
    }

    // =========================================================================
    // 1. flash.crypto.generateRandomBytes
    // =========================================================================
    function generateRandomBytes(numberRandomBytes) {
      ensureActive();
      const count = Number(numberRandomBytes);
      if (!Number.isInteger(count) || count < 1 || count > 1024) {
        const err = new Error('Error #2004: One of the parameters is invalid (numberRandomBytes must be 1..1024).');
        err.name = 'ArgumentError';
        err.errorID = 2004;
        throw err;
      }
      const entropy = new Uint8Array(Math.min(count, 64));
      if (global.crypto && typeof global.crypto.getRandomValues === 'function') {
        global.crypto.getRandomValues(entropy);
      } else {
        for (let i = 0; i < entropy.length; i++) {
          entropy[i] = ((Date.now() + i * 31) ^ ((Math.random() * 256) | 0)) & 0xff;
        }
      }
      let outBytes;
      try {
        outBytes = kernel.random_bytes(count >>> 0, entropy);
      } catch (err) {
        throw translateKernelError(err);
      }
      if (binary?.fromBytes) {
        return binary.fromBytes(outBytes);
      }
      const ba = new (binary?.ByteArray || Uint8Array)(outBytes.length);
      return ba;
    }

    // =========================================================================
    // 2. flash.desktop.{Clipboard, ClipboardFormats, ClipboardTransferMode}
    // =========================================================================
    const ClipboardFormats = defConstClass('ClipboardFormats', {
      TEXT_FORMAT: 'air:text',
      HTML_FORMAT: 'air:html',
      RICH_TEXT_FORMAT: 'air:rtf',
      URL_FORMAT: 'air:url',
      FILE_LIST_FORMAT: 'air:file list',
      BITMAP_FORMAT: 'air:bitmap',
    });

    const ClipboardTransferMode = defConstClass('ClipboardTransferMode', {
      ORIGINAL_PREFERRED: 'originalPreferred',
      ORIGINAL_ONLY: 'originalOnly',
      CLONE_PREFERRED: 'clonePreferred',
      CLONE_ONLY: 'cloneOnly',
    });

    const clipboardInstances = new WeakMap();
    let generalClipboardInstance = null;

    function cloneClipboardValue(val) {
      if (val === null || val === undefined || typeof val !== 'object') return val;
      if (binary?.ByteArray?.__nfIs?.(val) && binary.toBytes && binary.fromBytes) {
        return binary.fromBytes(binary.toBytes(val));
      }
      if (amf?.encode && amf?.decode) {
        try {
          const bytes = amf.encode(val, 3);
          return amf.decode(bytes, 3, { complete: true }).value;
        } catch (_) {}
      }
      if (Array.isArray(val)) return val.slice();
      return { ...val };
    }

    class Clipboard {
      constructor() {
        ensureActive();
        clipboardInstances.set(this, {
          generation,
          data: new Map(),
          handlers: new Map(),
          serializable: new Map(),
        });
      }
      static get generalClipboard() {
        ensureActive();
        const b = getBuiltins();
        const airClip = b && b['flash.desktop.Clipboard'];
        if (airClip && airClip !== Clipboard && airClip.generalClipboard) {
          return airClip.generalClipboard;
        }
        if (!generalClipboardInstance || clipboardInstances.get(generalClipboardInstance)?.generation !== generation) {
          generalClipboardInstance = new Clipboard();
        }
        return generalClipboardInstance;
      }
      get formats() {
        const rec = clipboardState(this);
        const set = new Set([...rec.data.keys(), ...rec.handlers.keys()]);
        return Array.from(set);
      }
      clear() {
        const rec = clipboardState(this);
        rec.data.clear();
        rec.handlers.clear();
        rec.serializable.clear();
      }
      clearData(format) {
        const rec = clipboardState(this);
        if (format === null || format === undefined) throw new TypeError('Clipboard.clearData format must not be null.');
        const key = String(format);
        rec.data.delete(key);
        rec.handlers.delete(key);
        rec.serializable.delete(key);
      }
      hasFormat(format) {
        const rec = clipboardState(this);
        if (format === null || format === undefined) return false;
        const key = String(format);
        return rec.data.has(key) || rec.handlers.has(key);
      }
      setData(format, data, serializable = true) {
        const rec = clipboardState(this);
        if (format === null || format === undefined || data === undefined || data === null) {
          throw new TypeError('Clipboard.setData requires non-null format and data.');
        }
        const key = String(format);
        rec.data.set(key, data);
        rec.serializable.set(key, Boolean(serializable));
        if (this === generalClipboardInstance && key === ClipboardFormats.TEXT_FORMAT) {
          const textVal = String(data);
          try {
            global.navigator?.clipboard?.writeText?.(textVal);
          } catch (_) {}
          try {
            global.localStorage?.setItem?.('neoflash.clipboard.v1', textVal);
          } catch (_) {}
        }
        return true;
      }
      setDataHandler(format, handler, serializable = true) {
        const rec = clipboardState(this);
        if (format === null || format === undefined || typeof handler !== 'function') {
          throw new TypeError('Clipboard.setDataHandler requires a format String and handler Function.');
        }
        const key = String(format);
        rec.handlers.set(key, handler);
        rec.serializable.set(key, Boolean(serializable));
        return true;
      }
      getData(format, transferMode = 'originalPreferred') {
        const rec = clipboardState(this);
        if (format === null || format === undefined) throw new TypeError('Clipboard.getData format must not be null.');
        const mode = transferMode == null ? 'originalPreferred' : String(transferMode);
        if (!['originalPreferred', 'originalOnly', 'clonePreferred', 'cloneOnly'].includes(mode)) {
          const err = new Error(`ArgumentError: Error #2004: Invalid ClipboardTransferMode '${mode}'.`);
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        const key = String(format);
        let raw;
        if (rec.data.has(key)) {
          raw = rec.data.get(key);
        } else if (rec.handlers.has(key)) {
          const fn = rec.handlers.get(key);
          raw = fn();
        } else {
          return null;
        }
        const canClone = rec.serializable.get(key) !== false;
        if (mode === 'originalOnly' || mode === 'originalPreferred') return raw;
        if (mode === 'cloneOnly') return canClone ? cloneClipboardValue(raw) : null;
        return canClone ? cloneClipboardValue(raw) : raw;
      }
    }
    function clipboardState(instance) {
      ensureActive();
      const rec = clipboardInstances.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Invalid or stale Clipboard instance.');
      return rec;
    }
    tagType(Clipboard, 'Clipboard', v => clipboardInstances.has(v));

    // =========================================================================
    // 3. flash.globalization.*
    // =========================================================================
    const LastOperationStatus = defConstClass('LastOperationStatus', {
      NO_ERROR: 'noError',
      USING_FALLBACK_WARNING: 'usingFallbackWarning',
      USING_DEFAULT_WARNING: 'usingDefaultWarning',
      PARSE_ERROR: 'parseError',
      UNSUPPORTED_ERROR: 'unsupportedError',
      ERROR_CODE_UNKNOWN: 'errorCodeUnknown',
      PATTERN_SYNTAX_ERROR: 'patternSyntaxError',
      MEMORY_ALLOCATION_ERROR: 'memoryAllocationError',
      ILLEGAL_ARGUMENT_ERROR: 'illegalArgumentError',
      BUFFER_OVERFLOW_ERROR: 'bufferOverflowError',
      INVALID_ATTR_VALUE: 'invalidAttrValue',
      NUMBER_OVERFLOW_ERROR: 'numberOverflowError',
      INVALID_CHAR_FOUND: 'invalidCharFound',
      TRUNCATED_CHAR_FOUND: 'truncatedCharFound',
      INDEX_OUT_OF_BOUNDS_ERROR: 'indexOutOfBoundsError',
      PLATFORM_API_FAILED: 'platformAPIFailed',
      UNEXPECTED_TOKEN: 'unexpectedToken',
    });

    const NationalDigitsType = defConstClass('NationalDigitsType', {
      EUROPEAN: 0x0030,
      ARABIC_INDIC: 0x0660,
      EXTENDED_ARABIC_INDIC: 0x06f0,
      NKO: 0x07c0,
      DEVANAGARI: 0x0966,
      BENGALI: 0x09e6,
      GURMUKHI: 0x0a66,
      GUJARATI: 0x0ae6,
      ORIYA: 0x0b66,
      TAMIL: 0x0be6,
      TELUGU: 0x0c66,
      KANNADA: 0x0ce6,
      MALAYALAM: 0x0d66,
      THAI: 0x0e50,
      LAO: 0x0ed0,
      TIBETAN: 0x0f20,
      MYANMAR: 0x1040,
      MYANMAR_SHAN: 0x1090,
      KHMER: 0x17e0,
      MONGOLIAN: 0x1810,
      LIMBU: 0x1946,
      NEW_TAI_LUE: 0x19d0,
      TAI_THAM_HORA: 0x1a80,
      TAI_THAM_THAM: 0x1a90,
      BALINESE: 0x1b50,
      SUNDANESE: 0x1bb0,
      LEPCHA: 0x1c40,
      OL_CHIKI: 0x1c50,
      VAI: 0xa620,
      SAURASHTRA: 0xa8d0,
      KAYAH_LI: 0xa900,
      CHAM: 0xaa50,
      FULL_WIDTH: 0xff10,
      OSMANYA: 0x104a0,
    });

    const CollatorMode = defConstClass('CollatorMode', {
      SORTING: 'sorting',
      MATCHING: 'matching',
    });

    function availableLocalesVector() {
      const list = execKernel({ op: 'available_locales' });
      return makeVector('String', list);
    }

    function hookNumberFormatter(NumberFormatterCtor) {
      if (!NumberFormatterCtor) return;
      Object.defineProperty(NumberFormatterCtor, 'getAvailableLocaleIDNames', {
        configurable: true,
        writable: true,
        value: function getAvailableLocaleIDNames() {
          return availableLocalesVector();
        },
      });
    }

    if (numberFormatHost?.NumberFormatter) {
      hookNumberFormatter(numberFormatHost.NumberFormatter);
    }

    const localeIdInstances = new WeakMap();
    class LocaleID {
      constructor(name) {
        ensureActive();
        if (name === null || name === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter name must be non-null.');
        }
        const parsed = execKernel({ op: 'locale_parse', name: String(name) });
        localeIdInstances.set(this, { generation, parsed, status: parsed.status });
      }
      static determinePreferredLocales(want, have, keyword = 'userinterface') {
        ensureActive();
        if (!want || !have) return null;
        const wantArr = Array.from(want, v => String(v));
        const haveArr = Array.from(have, v => String(v));
        const res = execKernel({
          op: 'locale_determine_preferred',
          want: wantArr,
          have: haveArr,
          keyword: keyword == null ? 'userinterface' : String(keyword),
        });
        return makeVector('String', res);
      }
      get name() {
        const rec = localeIdState(this);
        rec.status = rec.parsed.status === 'illegalArgumentError' ? 'illegalArgumentError' : 'noError';
        return rec.parsed.canonical ?? rec.parsed.name ?? rec.parsed.canonicalName;
      }
      get lastOperationStatus() {
        return localeIdState(this).status;
      }
      getLanguage() {
        const rec = localeIdState(this);
        if (rec.parsed.status !== 'illegalArgumentError') rec.status = 'noError';
        return rec.parsed.language;
      }
      getRegion() {
        const rec = localeIdState(this);
        if (rec.parsed.status !== 'illegalArgumentError') rec.status = 'noError';
        return rec.parsed.region;
      }
      getScript() {
        const rec = localeIdState(this);
        if (rec.parsed.status !== 'illegalArgumentError') rec.status = 'noError';
        return rec.parsed.script;
      }
      getVariant() {
        const rec = localeIdState(this);
        if (rec.parsed.status !== 'illegalArgumentError') rec.status = 'noError';
        return rec.parsed.variant;
      }
      getKeysAndValues() {
        const rec = localeIdState(this);
        if (rec.parsed.status !== 'illegalArgumentError') rec.status = 'noError';
        return { ...(rec.parsed.keys || {}) };
      }
      isRightToLeft() {
        const rec = localeIdState(this);
        if (rec.parsed.status !== 'illegalArgumentError') rec.status = 'noError';
        return Boolean(rec.parsed.rtl);
      }
    }
    Object.defineProperty(LocaleID, 'DEFAULT', { value: 'i-default', enumerable: true });
    function localeIdState(instance) {
      ensureActive();
      const rec = localeIdInstances.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid LocaleID instance.');
      return rec;
    }
    tagType(LocaleID, 'LocaleID', v => localeIdInstances.has(v));

    const currencyParseResults = new WeakMap();
    class CurrencyParseResult {
      constructor(value = NaN, symbol = '') {
        ensureActive();
        const num = Number(value);
        const str = symbol === null || symbol === undefined ? '' : String(symbol);
        currencyParseResults.set(this, { value: num, currencyString: str });
      }
      get value() {
        const rec = currencyParseResults.get(this);
        if (!rec) throw new TypeError('Expected a CurrencyParseResult.');
        return rec.value;
      }
      get currencyString() {
        const rec = currencyParseResults.get(this);
        if (!rec) throw new TypeError('Expected a CurrencyParseResult.');
        return rec.currencyString;
      }
    }
    tagType(CurrencyParseResult, 'CurrencyParseResult', v => currencyParseResults.has(v));

    const currencyFormatters = new WeakMap();
    class CurrencyFormatter {
      constructor(requestedLocaleIDName) {
        ensureActive();
        if (requestedLocaleIDName === null || requestedLocaleIDName === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter requestedLocaleIDName must be non-null.');
        }
        const created = execKernel({ op: 'currency_new', locale: String(requestedLocaleIDName) });
        currencyFormatters.set(this, { generation, id: created.id, state: created.state });
      }
      static getAvailableLocaleIDNames() {
        return availableLocalesVector();
      }
      get requestedLocaleIDName() { return cfState(this).state.requestedLocaleIDName; }
      get actualLocaleIDName() { return cfState(this).state.actualLocaleIDName; }
      get lastOperationStatus() { return cfState(this).state.lastOperationStatus; }
      get currencyISOCode() { return cfState(this).state.currencyISOCode; }
      get currencySymbol() { return cfState(this).state.currencySymbol; }
      get decimalSeparator() { return cfState(this).state.decimalSeparator; }
      set decimalSeparator(v) { cfSetProp(this, 'decimalSeparator', v); }
      get groupingSeparator() { return cfState(this).state.groupingSeparator; }
      set groupingSeparator(v) { cfSetProp(this, 'groupingSeparator', v); }
      get groupingPattern() { return cfState(this).state.groupingPattern; }
      set groupingPattern(v) { cfSetProp(this, 'groupingPattern', v); }
      get digitsType() { return cfState(this).state.digitsType; }
      set digitsType(v) { cfSetProp(this, 'digitsType', Number(v)); }
      get fractionalDigits() { return cfState(this).state.fractionalDigits; }
      set fractionalDigits(v) { cfSetProp(this, 'fractionalDigits', Number(v)); }
      get leadingZero() { return cfState(this).state.leadingZero; }
      set leadingZero(v) { cfSetProp(this, 'leadingZero', v); }
      get trailingZeros() { return cfState(this).state.trailingZeros; }
      set trailingZeros(v) { cfSetProp(this, 'trailingZeros', v); }
      get useGrouping() { return cfState(this).state.useGrouping; }
      set useGrouping(v) { cfSetProp(this, 'useGrouping', v); }
      get negativeCurrencyFormat() { return cfState(this).state.negativeCurrencyFormat; }
      set negativeCurrencyFormat(v) { cfSetProp(this, 'negativeCurrencyFormat', Number(v)); }
      get positiveCurrencyFormat() { return cfState(this).state.positiveCurrencyFormat; }
      set positiveCurrencyFormat(v) { cfSetProp(this, 'positiveCurrencyFormat', Number(v)); }
      get negativeSymbol() { return cfState(this).state.negativeSymbol; }
      set negativeSymbol(v) { cfSetProp(this, 'negativeSymbol', v); }

      setCurrency(currencyISOCode, currencySymbol) {
        const rec = cfState(this);
        if (currencyISOCode === null || currencyISOCode === undefined || currencySymbol === null || currencySymbol === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter currencyISOCode/currencySymbol must be non-null.');
        }
        try {
          rec.state = execKernel({
            op: 'currency_set_currency',
            id: rec.id,
            iso: String(currencyISOCode),
            symbol: String(currencySymbol),
          });
        } catch (err) {
          rec.state = execKernel({ op: 'currency_snapshot', id: rec.id });
          throw err;
        }
      }
      formattingWithCurrencySymbolIsSafe(requestedISOCode) {
        const rec = cfState(this);
        if (requestedISOCode === null || requestedISOCode === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter requestedISOCode must be non-null.');
        }
        try {
          const res = execKernel({ op: 'currency_is_safe', id: rec.id, iso: String(requestedISOCode) });
          rec.state.lastOperationStatus = res.status;
          return Boolean(res.safe);
        } catch (err) {
          rec.state = execKernel({ op: 'currency_snapshot', id: rec.id });
          throw err;
        }
      }
      format(value, withCurrencySymbol = false) {
        const rec = cfState(this);
        const num = Number(value);
        const wireVal = Number.isNaN(num) ? 'NaN' : num === Infinity ? 'Infinity' : num === -Infinity ? '-Infinity' : num;
        const res = execKernel({
          op: 'currency_format',
          id: rec.id,
          value: wireVal,
          withSymbol: Boolean(withCurrencySymbol),
        });
        rec.state.lastOperationStatus = res.status;
        return res.formatted;
      }
      parse(inputString) {
        const rec = cfState(this);
        if (inputString === null || inputString === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter inputString must be non-null.');
        }
        const res = execKernel({ op: 'currency_parse', id: rec.id, input: String(inputString) });
        rec.state.lastOperationStatus = res.status;
        const val = res.value === null || res.value === 'NaN' ? NaN : Number(res.value);
        return new CurrencyParseResult(val, res.currencyString);
      }
    }
    function cfState(instance) {
      ensureActive();
      const rec = currencyFormatters.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid CurrencyFormatter instance.');
      return rec;
    }
    function cfSetProp(instance, name, value) {
      const rec = cfState(instance);
      if (value === null || value === undefined) {
        throw new TypeError(`TypeError: Error #2007: CurrencyFormatter.${name} must be non-null.`);
      }
      try {
        rec.state = execKernel({ op: 'currency_set_prop', id: rec.id, name, value });
      } catch (err) {
        rec.state = execKernel({ op: 'currency_snapshot', id: rec.id });
        throw err;
      }
    }
    tagType(CurrencyFormatter, 'CurrencyFormatter', v => currencyFormatters.has(v));

    const collators = new WeakMap();
    class Collator {
      constructor(requestedLocaleIDName, initialMode = 'sorting') {
        ensureActive();
        if (requestedLocaleIDName === null || requestedLocaleIDName === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter requestedLocaleIDName must be non-null.');
        }
        const mode = initialMode === undefined ? 'sorting' : String(initialMode);
        const created = execKernel({
          op: 'collator_new',
          locale: String(requestedLocaleIDName),
          mode,
        });
        collators.set(this, { generation, id: created.id, state: created.state });
      }
      static getAvailableLocaleIDNames() {
        return availableLocalesVector();
      }
      get requestedLocaleIDName() { return colState(this).state.requestedLocaleIDName; }
      get actualLocaleIDName() { return colState(this).state.actualLocaleIDName; }
      get lastOperationStatus() { return colState(this).state.lastOperationStatus; }
      get ignoreCase() { return colState(this).state.ignoreCase; }
      set ignoreCase(v) { colSetBool(this, 'ignoreCase', v); }
      get ignoreCharacterWidth() { return colState(this).state.ignoreCharacterWidth; }
      set ignoreCharacterWidth(v) { colSetBool(this, 'ignoreCharacterWidth', v); }
      get ignoreDiacritics() { return colState(this).state.ignoreDiacritics; }
      set ignoreDiacritics(v) { colSetBool(this, 'ignoreDiacritics', v); }
      get ignoreKanaType() { return colState(this).state.ignoreKanaType; }
      set ignoreKanaType(v) { colSetBool(this, 'ignoreKanaType', v); }
      get ignoreSymbols() { return colState(this).state.ignoreSymbols; }
      set ignoreSymbols(v) { colSetBool(this, 'ignoreSymbols', v); }
      get numericComparison() { return colState(this).state.numericComparison; }
      set numericComparison(v) { colSetBool(this, 'numericComparison', v); }
      compare(string1, string2) {
        const rec = colState(this);
        if (string1 === null || string1 === undefined || string2 === null || string2 === undefined) {
          throw new TypeError('TypeError: Error #2007: Collator.compare parameters must be non-null.');
        }
        const res = execKernel({ op: 'collator_compare', id: rec.id, a: String(string1), b: String(string2) });
        rec.state.lastOperationStatus = res.status;
        return Number(res.result) | 0;
      }
      equals(string1, string2) {
        const rec = colState(this);
        if (string1 === null || string1 === undefined || string2 === null || string2 === undefined) {
          throw new TypeError('TypeError: Error #2007: Collator.equals parameters must be non-null.');
        }
        const res = execKernel({ op: 'collator_compare', id: rec.id, a: String(string1), b: String(string2) });
        rec.state.lastOperationStatus = res.status;
        return Boolean(res.equals);
      }
    }
    function colState(instance) {
      ensureActive();
      const rec = collators.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid Collator instance.');
      return rec;
    }
    function colSetBool(instance, name, value) {
      const rec = colState(instance);
      if (typeof value !== 'boolean') {
        throw new TypeError(`TypeError: Collator.${name} requires a Boolean.`);
      }
      rec.state = execKernel({ op: 'collator_set_bool', id: rec.id, name, value });
    }
    tagType(Collator, 'Collator', v => collators.has(v));

    const stringToolsMap = new WeakMap();
    class StringTools {
      constructor(requestedLocaleIDName) {
        ensureActive();
        if (requestedLocaleIDName === null || requestedLocaleIDName === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter requestedLocaleIDName must be non-null.');
        }
        const created = execKernel({ op: 'string_tools_new', locale: String(requestedLocaleIDName) });
        stringToolsMap.set(this, { generation, id: created.id, state: created.state });
      }
      static getAvailableLocaleIDNames() {
        return availableLocalesVector();
      }
      get requestedLocaleIDName() { return stState(this).state.requestedLocaleIDName; }
      get actualLocaleIDName() { return stState(this).state.actualLocaleIDName; }
      get lastOperationStatus() { return stState(this).state.lastOperationStatus; }
      toLowerCase(s) {
        const rec = stState(this);
        if (s === null || s === undefined) throw new TypeError('TypeError: Error #2007: Parameter s must be non-null.');
        const res = execKernel({ op: 'string_tools_case', id: rec.id, value: String(s), upper: false });
        rec.state.lastOperationStatus = res.status;
        return res.result;
      }
      toUpperCase(s) {
        const rec = stState(this);
        if (s === null || s === undefined) throw new TypeError('TypeError: Error #2007: Parameter s must be non-null.');
        const res = execKernel({ op: 'string_tools_case', id: rec.id, value: String(s), upper: true });
        rec.state.lastOperationStatus = res.status;
        return res.result;
      }
    }
    function stState(instance) {
      ensureActive();
      const rec = stringToolsMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid StringTools instance.');
      return rec;
    }
    tagType(StringTools, 'StringTools', v => stringToolsMap.has(v));

    // =========================================================================
    // 4. flash.net — URLRequest, Responder, FileFilter, FileReference,
    //    FileReferenceList, GroupSpecifier, NetGroup*, LocalConnection
    // =========================================================================
    const urlRequestDigests = new WeakMap();
    if (netHost?.URLRequest?.prototype) {
      Object.defineProperty(netHost.URLRequest.prototype, 'digest', {
        configurable: true,
        enumerable: true,
        get() {
          return urlRequestDigests.has(this) ? urlRequestDigests.get(this) : null;
        },
        set(value) {
          if (value === null || value === undefined) {
            urlRequestDigests.set(this, null);
            return;
          }
          const str = String(value);
          if (!/^[0-9a-fA-F]{64}$/.test(str)) {
            const err = new Error('ArgumentError: Error #2004: URLRequest.digest must be a 64-character hexadecimal SHA-256 string.');
            err.name = 'ArgumentError';
            err.errorID = 2004;
            throw err;
          }
          urlRequestDigests.set(this, str);
        },
      });
      Object.defineProperty(netHost.URLRequest.prototype, 'useRedirectedURL', {
        configurable: true,
        writable: true,
        value: function useRedirectedURL(sourceRequest, wholeURL = false, pattern = null, replace = null) {
          if (!sourceRequest || typeof sourceRequest !== 'object' || typeof sourceRequest.url !== 'string') {
            throw new TypeError('TypeError: Error #2007: Parameter sourceRequest must be a URLRequest.');
          }
          let nextUrl = sourceRequest.url;
          if (!wholeURL && typeof this.url === 'string' && this.url) {
            try {
              const base = new URL(sourceRequest.url, 'https://neoflash.local/');
              const rel = new URL(this.url, base);
              nextUrl = rel.href;
            } catch (_) {
              nextUrl = sourceRequest.url;
            }
          }
          if (pattern !== null && pattern !== undefined && replace !== null && replace !== undefined) {
            nextUrl = nextUrl.replace(pattern, String(replace));
          }
          this.url = nextUrl;
        },
      });
    }

    const responderInstances = new WeakMap();
    class Responder {
      constructor(result, status = null) {
        ensureActive();
        if (typeof result !== 'function') {
          throw new TypeError('TypeError: Error #2007: Responder result parameter must be a Function.');
        }
        if (status !== null && status !== undefined && typeof status !== 'function') {
          throw new TypeError('TypeError: Error #2007: Responder status parameter must be null or a Function.');
        }
        responderInstances.set(this, {
          generation,
          result,
          status: status || null,
        });
      }
      _invokeResult(...args) {
        const rec = responderInstances.get(this);
        if (rec && rec.generation === generation && typeof rec.result === 'function') {
          rec.result(...args);
        }
      }
      _invokeStatus(...args) {
        const rec = responderInstances.get(this);
        if (rec && rec.generation === generation && typeof rec.status === 'function') {
          rec.status(...args);
        }
      }
    }
    tagType(Responder, 'Responder', v => responderInstances.has(v));

    const fileFilters = new WeakMap();
    class FileFilter {
      constructor(description, extension, macType = null) {
        ensureActive();
        if (description === null || description === undefined || extension === null || extension === undefined) {
          throw new TypeError('TypeError: Error #2007: FileFilter description and extension must be non-null.');
        }
        const desc = String(description);
        const ext = String(extension);
        if (!desc || !ext) {
          const err = new Error('ArgumentError: Error #2004: FileFilter description and extension must be non-empty.');
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        fileFilters.set(this, {
          description: desc,
          extension: ext,
          macType: macType === null || macType === undefined ? null : String(macType),
        });
      }
      get description() { return ffState(this).description; }
      set description(v) {
        if (v === null || v === undefined || !String(v)) throw new TypeError('FileFilter.description requires a non-empty String.');
        ffState(this).description = String(v);
      }
      get extension() { return ffState(this).extension; }
      set extension(v) {
        if (v === null || v === undefined || !String(v)) throw new TypeError('FileFilter.extension requires a non-empty String.');
        ffState(this).extension = String(v);
      }
      get macType() { return ffState(this).macType; }
      set macType(v) {
        ffState(this).macType = v === null || v === undefined ? null : String(v);
      }
    }
    function ffState(instance) {
      const rec = fileFilters.get(instance);
      if (!rec) throw new TypeError('Expected a valid FileFilter instance.');
      return rec;
    }
    function buildAcceptFilter(typeFilter) {
      if (!Array.isArray(typeFilter) || typeFilter.length === 0) return '';
      const exts = [];
      for (const item of typeFilter) {
        const rec = fileFilters.get(item);
        if (!rec || !rec.extension) continue;
        for (const part of String(rec.extension).split(';')) {
          const trimmed = part.trim();
          if (!trimmed || trimmed === '*.*' || trimmed === '*') continue;
          const dotIdx = trimmed.lastIndexOf('.');
          if (dotIdx >= 0) exts.push(trimmed.slice(dotIdx).toLowerCase());
        }
      }
      return Array.from(new Set(exts)).join(',');
    }
    tagType(FileFilter, 'FileFilter', v => fileFilters.has(v));

    const fileRefMap = new WeakMap();
    let mockBrowseSelection = null;

    function validateTypeFilterList(typeFilter) {
      if (typeFilter === null || typeFilter === undefined) return;
      if (!Array.isArray(typeFilter)) {
        const err = new Error('ArgumentError: Error #2004: typeFilter must be an Array of FileFilter objects.');
        err.name = 'ArgumentError';
        err.errorID = 2004;
        throw err;
      }
      for (const item of typeFilter) {
        if (!fileFilters.has(item)) {
          const err = new Error('ArgumentError: Error #2004: typeFilter entries must be FileFilter instances.');
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
      }
    }

    function createSelectedFileRef(spec) {
      const fr = new FileReference();
      const rec = fileRefMap.get(fr);
      const rawBytes =
        spec.bytes instanceof Uint8Array
          ? spec.bytes
          : typeof spec.content === 'string'
          ? new TextEncoder().encode(spec.content)
          : typeof spec.data === 'string'
          ? new TextEncoder().encode(spec.data)
          : new Uint8Array([78, 101, 111, 70, 108, 97, 115, 104]);
      const name = String(spec.name || 'sample.txt');
      const dot = name.lastIndexOf('.');
      const now = spec.date instanceof Date ? spec.date : new Date(1762000000000);
      rec.selected = true;
      rec.name = name;
      rec.size = rawBytes.byteLength;
      rec.type = spec.type !== undefined ? String(spec.type) : dot >= 0 ? name.slice(dot) : '.dat';
      rec.creator = String(spec.creator || '');
      rec.creationDate = new Date(now.getTime());
      rec.modificationDate = new Date(now.getTime());
      rec.rawBytes = rawBytes;
      rec.data = null;
      return fr;
    }

    class FileReference extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        fileRefMap.set(this, {
          generation,
          selected: false,
          name: null,
          size: 0,
          type: null,
          creator: null,
          creationDate: null,
          modificationDate: null,
          rawBytes: null,
          data: null,
          activeOp: 0,
        });
      }
      get name() {
        const r = frState(this);
        if (!r.selected) throw new Error('IllegalOperationError: Error #2037: FileReference.name is unavailable before a file is selected.');
        return r.name;
      }
      get size() {
        const r = frState(this);
        if (!r.selected) throw new Error('IllegalOperationError: Error #2037: FileReference.size is unavailable before a file is selected.');
        return r.size;
      }
      get type() {
        const r = frState(this);
        if (!r.selected) throw new Error('IllegalOperationError: Error #2037: FileReference.type is unavailable before a file is selected.');
        return r.type;
      }
      get creator() {
        const r = frState(this);
        if (!r.selected) throw new Error('IllegalOperationError: Error #2037: FileReference.creator is unavailable before a file is selected.');
        return r.creator;
      }
      get creationDate() {
        const r = frState(this);
        if (!r.selected) throw new Error('IllegalOperationError: Error #2037: FileReference.creationDate is unavailable before a file is selected.');
        return r.creationDate;
      }
      get modificationDate() {
        const r = frState(this);
        if (!r.selected) throw new Error('IllegalOperationError: Error #2037: FileReference.modificationDate is unavailable before a file is selected.');
        return r.modificationDate;
      }
      get data() {
        return frState(this).data;
      }
      browse(typeFilter = null) {
        const r = frState(this);
        validateTypeFilterList(typeFilter);
        r.lastAcceptFilter = buildAcceptFilter(typeFilter);
        if (typeof global.document?.createElement === 'function' && !mockBrowseSelection) {
          try {
            const input = global.document.createElement('input');
            input.type = 'file';
            if (r.lastAcceptFilter) input.accept = r.lastAcceptFilter;
            r.lastFileInput = input;
          } catch (_) {}
        }
        const opId = ++r.activeOp;
        const spec = Array.isArray(mockBrowseSelection) ? mockBrowseSelection[0] : mockBrowseSelection;
        if (spec && spec.cancel) {
          dispatchLater(this, () => (r.activeOp === opId ? makeBasicEvent('cancel') : null));
          return true;
        }
        const chosen = spec || { name: 'document.txt', content: 'NeoFlash FileReference payload' };
        const populated = createSelectedFileRef(chosen);
        const popRec = fileRefMap.get(populated);
        Object.assign(r, {
          selected: true,
          name: popRec.name,
          size: popRec.size,
          type: popRec.type,
          creator: popRec.creator,
          creationDate: popRec.creationDate,
          modificationDate: popRec.modificationDate,
          rawBytes: popRec.rawBytes,
        });
        dispatchLater(this, () => (r.activeOp === opId ? makeBasicEvent('select') : null));
        return true;
      }
      load() {
        const r = frState(this);
        if (!r.selected) {
          throw new Error('IllegalOperationError: Error #2037: Functions called in incorrect sequence; call browse() before load().');
        }
        const opId = ++r.activeOp;
        const bytes = r.rawBytes || new Uint8Array(0);
        r.data = binary?.fromBytes ? binary.fromBytes(bytes) : bytes;
        dispatchLater(this, () => {
          if (r.activeOp !== opId) return null;
          dispatchNow(this, makeBasicEvent('open'));
          dispatchNow(this, makeProgressEvent('progress', bytes.byteLength, bytes.byteLength));
          return makeBasicEvent('complete');
        });
      }
      save(data, defaultFileName = null) {
        const r = frState(this);
        if (data === null || data === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter data must be non-null.');
        }
        let bytes;
        if (typeof data === 'string') {
          bytes = new TextEncoder().encode(data);
        } else if (binary?.ByteArray?.__nfIs?.(data) && binary.toBytes) {
          bytes = binary.toBytes(data);
        } else if (data instanceof Uint8Array) {
          bytes = new Uint8Array(data);
        } else if (typeof data.toString === 'function') {
          bytes = new TextEncoder().encode(String(data));
        } else {
          const err = new Error('ArgumentError: Error #2004: Invalid data for FileReference.save.');
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        const name = defaultFileName == null ? 'download.bin' : String(defaultFileName);
        if (/[\\/:*?"<>|]/.test(name)) {
          const err = new Error('ArgumentError: Error #2004: Invalid defaultFileName.');
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        const dot = name.lastIndexOf('.');
        const now = new Date();
        r.selected = true;
        r.name = name;
        r.size = bytes.byteLength;
        r.type = dot >= 0 ? name.slice(dot) : '';
        r.creator = '';
        r.creationDate = now;
        r.modificationDate = now;
        r.rawBytes = bytes;
        r.lastSavedBytes = bytes;
        r.data = binary?.fromBytes ? binary.fromBytes(bytes) : bytes;
        if (typeof global.Blob === 'function' && typeof global.URL?.createObjectURL === 'function' && typeof global.document?.createElement === 'function') {
          try {
            const blob = new global.Blob([bytes], { type: 'application/octet-stream' });
            const blobUrl = global.URL.createObjectURL(blob);
            const anchor = global.document.createElement('a');
            anchor.href = blobUrl;
            anchor.download = name;
            anchor.click?.();
            global.URL.revokeObjectURL?.(blobUrl);
          } catch (_) {}
        }
        const opId = ++r.activeOp;
        dispatchLater(this, () => {
          if (r.activeOp !== opId) return null;
          dispatchNow(this, makeBasicEvent('select'));
          dispatchNow(this, makeBasicEvent('open'));
          dispatchNow(this, makeProgressEvent('progress', bytes.byteLength, bytes.byteLength));
          return makeBasicEvent('complete');
        });
      }
      download(request, defaultFileName = null) {
        const r = frState(this);
        if (!request || typeof request !== 'object' || typeof request.url !== 'string' || !request.url) {
          throw new TypeError('TypeError: Error #2007: FileReference.download requires a valid URLRequest.');
        }
        const name =
          defaultFileName != null
            ? String(defaultFileName)
            : request.url.split(/[?#]/)[0].split('/').filter(Boolean).pop() || 'downloaded.dat';
        const opId = ++r.activeOp;
        let payload = new TextEncoder().encode(`downloaded:${request.url}`);
        if (netHost?.mediaSource) {
          try {
            const src = netHost.mediaSource(request.url);
            if (src?.kind === 'resource' && src.bytes) payload = new Uint8Array(src.bytes);
          } catch (_) {}
        }
        const dot = name.lastIndexOf('.');
        const now = new Date();
        r.selected = true;
        r.name = name;
        r.size = payload.byteLength;
        r.type = dot >= 0 ? name.slice(dot) : '';
        r.creator = '';
        r.creationDate = now;
        r.modificationDate = now;
        r.rawBytes = payload;
        r.data = binary?.fromBytes ? binary.fromBytes(payload) : payload;
        dispatchLater(this, () => {
          if (r.activeOp !== opId) return null;
          dispatchNow(this, makeBasicEvent('select'));
          dispatchNow(this, makeBasicEvent('open'));
          dispatchNow(this, makeProgressEvent('progress', payload.byteLength, payload.byteLength));
          return makeBasicEvent('complete');
        });
      }
      upload(request, uploadDataFieldName = 'Filedata', testUpload = false) {
        const r = frState(this);
        if (!r.selected) {
          throw new Error('IllegalOperationError: Error #2037: Call browse() before upload().');
        }
        if (!request || typeof request !== 'object' || typeof request.url !== 'string' || !request.url) {
          throw new TypeError('TypeError: Error #2007: FileReference.upload requires a valid URLRequest.');
        }
        const field = uploadDataFieldName == null ? 'Filedata' : String(uploadDataFieldName);
        if (!field) {
          const err = new Error('ArgumentError: Error #2004: uploadDataFieldName must not be empty.');
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        const _test = Boolean(testUpload);
        const opId = ++r.activeOp;
        const rawBytes = r.rawBytes || new Uint8Array(0);
        const size = r.size || rawBytes.byteLength || 0;
        const boundary = '----NeoFlashFormBoundary7MA4YWxkTrZu0gW';
        const enc = new TextEncoder();
        const headerPart = enc.encode(
          `--${boundary}\r\n` +
            `Content-Disposition: form-data; name="Filename"\r\n\r\n${r.name || 'file.dat'}\r\n` +
            `--${boundary}\r\n` +
            `Content-Disposition: form-data; name="${field}"; filename="${r.name || 'file.dat'}"\r\n` +
            `Content-Type: application/octet-stream\r\n\r\n`
        );
        const footerPart = enc.encode(
          `\r\n--${boundary}\r\n` +
            `Content-Disposition: form-data; name="Upload"\r\n\r\nSubmit Query\r\n` +
            `--${boundary}--\r\n`
        );
        const multipartBody = new Uint8Array(headerPart.byteLength + rawBytes.byteLength + footerPart.byteLength);
        multipartBody.set(headerPart, 0);
        multipartBody.set(rawBytes, headerPart.byteLength);
        multipartBody.set(footerPart, headerPart.byteLength + rawBytes.byteLength);
        r.lastMultipartBody = multipartBody;
        if (/^invalid:|^disallowed:/i.test(request.url)) {
          dispatchLater(this, () => (r.activeOp === opId ? makeSecurityErrorEvent('securityError', 'Upload blocked by security policy.') : null));
          return;
        }
        if (/^error:/i.test(request.url)) {
          dispatchLater(this, () => (r.activeOp === opId ? makeIOErrorEvent('ioError', 'Upload I/O failed.') : null));
          return;
        }
        dispatchLater(this, () => {
          if (r.activeOp !== opId) return null;
          dispatchNow(this, makeBasicEvent('open'));
          dispatchNow(this, makeProgressEvent('progress', size, size));
          dispatchNow(this, makeHTTPStatusEvent(200));
          dispatchNow(this, makeBasicEvent('complete'));
          return makeDataEvent(
            'uploadCompleteData',
            JSON.stringify({ field, file: r.name, size, multipartBytes: multipartBody.byteLength, testUpload: _test })
          );
        });
      }
      cancel() {
        const r = frState(this);
        r.activeOp++;
      }
    }
    function frState(instance) {
      ensureActive();
      const rec = fileRefMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid FileReference instance.');
      return rec;
    }
    tagType(FileReference, 'FileReference', v => fileRefMap.has(v));

    const fileRefListMap = new WeakMap();
    class FileReferenceList extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        fileRefListMap.set(this, {
          generation,
          fileList: [],
          activeOp: 0,
        });
      }
      get fileList() {
        return frlState(this).fileList;
      }
      browse(typeFilter = null) {
        const r = frlState(this);
        validateTypeFilterList(typeFilter);
        r.lastAcceptFilter = buildAcceptFilter(typeFilter);
        if (typeof global.document?.createElement === 'function' && !mockBrowseSelection) {
          try {
            const input = global.document.createElement('input');
            input.type = 'file';
            input.multiple = true;
            if (r.lastAcceptFilter) input.accept = r.lastAcceptFilter;
            r.lastFileInput = input;
          } catch (_) {}
        }
        const opId = ++r.activeOp;
        if (mockBrowseSelection && mockBrowseSelection.cancel) {
          dispatchLater(this, () => (r.activeOp === opId ? makeBasicEvent('cancel') : null));
          return true;
        }
        const specs = Array.isArray(mockBrowseSelection)
          ? mockBrowseSelection
          : mockBrowseSelection
          ? [mockBrowseSelection]
          : [
              { name: 'alpha.txt', content: 'Alpha file' },
              { name: 'beta.bin', content: 'Beta file' },
            ];
        r.fileList = specs.map(s => createSelectedFileRef(s));
        dispatchLater(this, () => (r.activeOp === opId ? makeBasicEvent('select') : null));
        return true;
      }
    }
    function frlState(instance) {
      ensureActive();
      const rec = fileRefListMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid FileReferenceList instance.');
      return rec;
    }
    tagType(FileReferenceList, 'FileReferenceList', v => fileRefListMap.has(v));

    // GroupSpecifier
    const groupSpecMap = new WeakMap();
    class GroupSpecifier {
      constructor(name) {
        ensureActive();
        if (name === null || name === undefined) {
          throw new TypeError('TypeError: Error #2007: GroupSpecifier name must not be null.');
        }
        const created = execKernel({ op: 'groupspec_new', name: String(name) });
        const sync = execKernel({ op: 'groupspec_update', id: created.id, action: 'noop' });
        groupSpecMap.set(this, {
          generation,
          id: created.id,
          state: sync.state,
          auth: sync.authorizations,
          withoutAuth: sync.withoutAuth,
          withAuth: sync.withAuth,
        });
      }
      static get maxSupportedGroupspecVersion() {
        return 2;
      }
      static encodePostingAuthorization(password) {
        if (password === null || password === undefined) throw new TypeError('Parameter password must be non-null.');
        return execKernel({ op: 'groupspec_encode_static', kind: 'posting', password: String(password) }).encoded;
      }
      static encodePublishAuthorization(password) {
        if (password === null || password === undefined) throw new TypeError('Parameter password must be non-null.');
        return execKernel({ op: 'groupspec_encode_static', kind: 'publish', password: String(password) }).encoded;
      }
      static encodeBootstrapPeerIDSpec(peerID) {
        if (peerID === null || peerID === undefined) throw new TypeError('Parameter peerID must be non-null.');
        return execKernel({ op: 'groupspec_encode_static', kind: 'bootstrap', peerID: String(peerID) }).encoded;
      }
      static encodeIPMulticastAddressSpec(address, port = null, source = null) {
        if (address === null || address === undefined) throw new TypeError('Parameter address must be non-null.');
        return execKernel({
          op: 'groupspec_encode_static',
          kind: 'multicast',
          address: String(address),
          port: port == null ? null : Number(port),
          source: source == null ? null : String(source),
        }).encoded;
      }
      get minGroupspecVersion() { return gsState(this).state.minGroupspecVersion; }
      set minGroupspecVersion(v) { gsUpdate(this, { action: 'set_min_version', value: Number(v) }); }
      get ipMulticastMemberUpdatesEnabled() { return gsState(this).state.ipMulticastMemberUpdatesEnabled; }
      set ipMulticastMemberUpdatesEnabled(v) { gsSetBool(this, 'ipMulticastMemberUpdatesEnabled', v); }
      get multicastEnabled() { return gsState(this).state.multicastEnabled; }
      set multicastEnabled(v) { gsSetBool(this, 'multicastEnabled', v); }
      get objectReplicationEnabled() { return gsState(this).state.objectReplicationEnabled; }
      set objectReplicationEnabled(v) { gsSetBool(this, 'objectReplicationEnabled', v); }
      get peerToPeerDisabled() { return gsState(this).state.peerToPeerDisabled; }
      set peerToPeerDisabled(v) { gsSetBool(this, 'peerToPeerDisabled', v); }
      get postingEnabled() { return gsState(this).state.postingEnabled; }
      set postingEnabled(v) { gsSetBool(this, 'postingEnabled', v); }
      get routingEnabled() { return gsState(this).state.routingEnabled; }
      set routingEnabled(v) { gsSetBool(this, 'routingEnabled', v); }
      get serverChannelEnabled() { return gsState(this).state.serverChannelEnabled; }
      set serverChannelEnabled(v) { gsSetBool(this, 'serverChannelEnabled', v); }

      makeUnique() { gsUpdate(this, { action: 'make_unique' }); }
      setPostingPassword(password = null, salt = null) {
        gsUpdate(this, {
          action: 'set_posting_password',
          password: password == null ? null : String(password),
          salt: salt == null ? null : String(salt),
        });
      }
      setPublishPassword(password = null, salt = null) {
        gsUpdate(this, {
          action: 'set_publish_password',
          password: password == null ? null : String(password),
          salt: salt == null ? null : String(salt),
        });
      }
      addBootstrapPeer(peerID) {
        if (peerID === null || peerID === undefined) throw new TypeError('Parameter peerID must be non-null.');
        gsUpdate(this, { action: 'add_bootstrap_peer', peerID: String(peerID) });
      }
      addIPMulticastAddress(address, port = null, source = null) {
        if (address === null || address === undefined) throw new TypeError('Parameter address must be non-null.');
        gsUpdate(this, {
          action: 'add_ip_multicast_address',
          address: String(address),
          port: port == null ? null : Number(port),
          source: source == null ? null : String(source),
        });
      }
      authorizations() { return gsState(this).auth; }
      groupspecWithoutAuthorizations() { return gsState(this).withoutAuth; }
      groupspecWithAuthorizations() { return gsState(this).withAuth; }
      toString() { return gsState(this).withAuth; }
    }
    function gsState(instance) {
      ensureActive();
      const rec = groupSpecMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid GroupSpecifier instance.');
      return rec;
    }
    function gsUpdate(instance, extra) {
      const rec = gsState(instance);
      const sync = execKernel({ op: 'groupspec_update', id: rec.id, ...extra });
      rec.state = sync.state;
      rec.auth = sync.authorizations;
      rec.withoutAuth = sync.withoutAuth;
      rec.withAuth = sync.withAuth;
    }
    function gsSetBool(instance, name, value) {
      gsUpdate(instance, { action: 'set_bool', name, value: Boolean(value) });
    }
    tagType(GroupSpecifier, 'GroupSpecifier', v => groupSpecMap.has(v));

    // NetGroup constants, NetGroupInfo, NetGroup
    const NetGroupReceiveMode = defConstClass('NetGroupReceiveMode', {
      EXACT: 'exact',
      NEAREST: 'nearest',
    });
    const NetGroupReplicationStrategy = defConstClass('NetGroupReplicationStrategy', {
      LOWEST_FIRST: 'lowestFirst',
      RAREST_FIRST: 'rarestFirst',
    });
    const NetGroupSendMode = defConstClass('NetGroupSendMode', {
      NEXT_INCREASING: 'nextIncreasing',
      NEXT_DECREASING: 'nextDecreasing',
    });
    const NetGroupSendResult = defConstClass('NetGroupSendResult', {
      ERROR: 'error',
      NO_ROUTE: 'noRoute',
      SENT: 'sent',
    });

    const netGroupInfos = new WeakMap();
    class NetGroupInfo {
      constructor(snap = {}) {
        netGroupInfos.set(this, {
          objectReplicationReceiveBytesPerSecond: Number(snap.objectReplicationReceiveBytesPerSecond || 0),
          objectReplicationSendBytesPerSecond: Number(snap.objectReplicationSendBytesPerSecond || 0),
          postingReceiveControlBytesPerSecond: Number(snap.postingReceiveControlBytesPerSecond || 0),
          postingReceiveDataBytesPerSecond: Number(snap.postingReceiveDataBytesPerSecond || 0),
          postingSendControlBytesPerSecond: Number(snap.postingSendControlBytesPerSecond || 0),
          postingSendDataBytesPerSecond: Number(snap.postingSendDataBytesPerSecond || 0),
          routingReceiveBytesPerSecond: Number(snap.routingReceiveBytesPerSecond || 0),
          routingSendBytesPerSecond: Number(snap.routingSendBytesPerSecond || 0),
        });
      }
      get objectReplicationReceiveBytesPerSecond() { return ngiState(this).objectReplicationReceiveBytesPerSecond; }
      get objectReplicationSendBytesPerSecond() { return ngiState(this).objectReplicationSendBytesPerSecond; }
      get postingReceiveControlBytesPerSecond() { return ngiState(this).postingReceiveControlBytesPerSecond; }
      get postingReceiveDataBytesPerSecond() { return ngiState(this).postingReceiveDataBytesPerSecond; }
      get postingSendControlBytesPerSecond() { return ngiState(this).postingSendControlBytesPerSecond; }
      get postingSendDataBytesPerSecond() { return ngiState(this).postingSendDataBytesPerSecond; }
      get routingReceiveBytesPerSecond() { return ngiState(this).routingReceiveBytesPerSecond; }
      get routingSendBytesPerSecond() { return ngiState(this).routingSendBytesPerSecond; }
      toString() {
        const s = ngiState(this);
        return `postingSendDataBytesPerSecond=${s.postingSendDataBytesPerSecond} postingSendControlBytesPerSecond=${s.postingSendControlBytesPerSecond} postingReceiveDataBytesPerSecond=${s.postingReceiveDataBytesPerSecond} postingReceiveControlBytesPerSecond=${s.postingReceiveControlBytesPerSecond} routingSendBytesPerSecond=${s.routingSendBytesPerSecond} routingReceiveBytesPerSecond=${s.routingReceiveBytesPerSecond} objectReplicationSendBytesPerSecond=${s.objectReplicationSendBytesPerSecond} objectReplicationReceiveBytesPerSecond=${s.objectReplicationReceiveBytesPerSecond}`;
      }
    }
    function ngiState(instance) {
      const rec = netGroupInfos.get(instance);
      if (!rec) throw new TypeError('Expected a valid NetGroupInfo instance.');
      return rec;
    }
    tagType(NetGroupInfo, 'NetGroupInfo', v => netGroupInfos.has(v));

    const netGroups = new WeakMap();
    const netGroupsById = new Map();
    const NetStatusEventCtor = videoHost?._as3Hook?.NetStatusEvent;

    function makeNetStatusEvent(info) {
      const Ctor = getEventCtor('flash.events.NetStatusEvent') || NetStatusEventCtor;
      if (Ctor) return new Ctor('netStatus', false, false, info);
      const ev = new events.Event('netStatus', false, false);
      ev.info = info;
      return ev;
    }

    class NetGroup extends events.EventDispatcher {
      constructor(connection, groupspec) {
        super();
        ensureActive();
        if (!connection || !videoHost?._as3Hook?.connections?.has(connection)) {
          throw new TypeError('TypeError: Error #2007: NetGroup requires a valid NetConnection.');
        }
        if (groupspec === null || groupspec === undefined) {
          throw new TypeError('TypeError: Error #2007: NetGroup groupspec must not be null.');
        }
        const ncRec = videoHost._as3Hook.connections.get(connection);
        if (!ncRec.connected && !ncRec.local) {
          const err = new Error('ArgumentError: Error #2126: NetConnection object must be connected.');
          err.name = 'ArgumentError';
          err.errorID = 2126;
          throw err;
        }
        registerDispatcher(this);
        const created = execKernel({
          op: 'netgroup_new',
          groupspec: String(groupspec),
          peerID: ncRec.nearID || '',
        });
        const rec = {
          generation,
          id: created.id,
          connection,
          groupspec: String(groupspec),
          address: created.address,
          replicatedObjects: new Map(),
        };
        netGroups.set(this, rec);
        netGroupsById.set(created.id, this);
        dispatchLater(this, () =>
          makeNetStatusEvent({ code: 'NetGroup.Connect.Success', level: 'status', group: this })
        );
        if (videoHost?._as3Hook?.emit) {
          videoHost._as3Hook.later(ncRec, () =>
            videoHost._as3Hook.emit(ncRec, 'NetGroup.Connect.Success', 'status', { group: this })
          );
        }
      }
      get estimatedMemberCount() { return ngSnapshot(this).estimatedMemberCount; }
      get neighborCount() { return ngSnapshot(this).neighborCount; }
      get localCoverageFrom() { return ngSnapshot(this).localCoverageFrom; }
      get localCoverageTo() { return ngSnapshot(this).localCoverageTo; }
      get receiveMode() { return ngSnapshot(this).receiveMode; }
      set receiveMode(v) {
        const rec = ngState(this);
        execKernel({ op: 'netgroup_op', id: rec.id, action: 'set_mode', name: 'receiveMode', value: String(v) });
      }
      get replicationStrategy() { return ngSnapshot(this).replicationStrategy; }
      set replicationStrategy(v) {
        const rec = ngState(this);
        execKernel({ op: 'netgroup_op', id: rec.id, action: 'set_mode', name: 'replicationStrategy', value: String(v) });
      }
      get info() {
        return new NetGroupInfo(ngSnapshot(this).info);
      }
      convertPeerIDToGroupAddress(peerID) {
        const rec = ngState(this);
        if (peerID === null || peerID === undefined) return '';
        return execKernel({ op: 'netgroup_op', id: rec.id, action: 'convert', peerID: String(peerID) }).address;
      }
      addNeighbor(peerID) {
        const rec = ngState(this);
        if (!peerID) return false;
        return Boolean(execKernel({ op: 'netgroup_op', id: rec.id, action: 'add_neighbor', peerID: String(peerID) }).added);
      }
      addMemberHint(peerID) {
        const rec = ngState(this);
        if (!peerID) return false;
        return Boolean(execKernel({ op: 'netgroup_op', id: rec.id, action: 'add_member_hint', peerID: String(peerID) }).added);
      }
      addHaveObjects(startIndex, endIndex) {
        const rec = ngState(this);
        execKernel({ op: 'netgroup_op', id: rec.id, action: 'have_want', kind: 'addHave', start: Number(startIndex), end: Number(endIndex) });
      }
      removeHaveObjects(startIndex, endIndex) {
        const rec = ngState(this);
        execKernel({ op: 'netgroup_op', id: rec.id, action: 'have_want', kind: 'removeHave', start: Number(startIndex), end: Number(endIndex) });
      }
      addWantObjects(startIndex, endIndex) {
        const rec = ngState(this);
        execKernel({ op: 'netgroup_op', id: rec.id, action: 'have_want', kind: 'addWant', start: Number(startIndex), end: Number(endIndex) });
      }
      removeWantObjects(startIndex, endIndex) {
        const rec = ngState(this);
        execKernel({ op: 'netgroup_op', id: rec.id, action: 'have_want', kind: 'removeWant', start: Number(startIndex), end: Number(endIndex) });
      }
      writeRequestedObject(requestID, object) {
        const rec = ngState(this);
        rec.replicatedObjects.set(Number(requestID), object);
        execKernel({
          op: 'netgroup_op',
          id: rec.id,
          action: 'replication_reply',
          requestID: Number(requestID),
          byteLength: 32,
          deny: false,
        });
      }
      denyRequestedObject(requestID) {
        const rec = ngState(this);
        execKernel({
          op: 'netgroup_op',
          id: rec.id,
          action: 'replication_reply',
          requestID: Number(requestID),
          byteLength: 0,
          deny: true,
        });
      }
      post(message) {
        const rec = ngState(this);
        const summary = JSON.stringify(message ?? null) || 'msg';
        const res = execKernel({
          op: 'netgroup_op',
          id: rec.id,
          action: 'post',
          payloadSummary: summary,
          byteLength: summary.length,
        });
        if (res.messageID && Array.isArray(res.recipients)) {
          for (const targetId of res.recipients) {
            const peer = netGroupsById.get(targetId);
            if (peer) {
              dispatchLater(peer, () =>
                makeNetStatusEvent({
                  code: 'NetGroup.Posting.Notify',
                  level: 'status',
                  message,
                  messageID: res.messageID,
                })
              );
            }
          }
        }
        return res.messageID || null;
      }
      sendToAllNeighbors(message) {
        return ngRoute(this, 'all', message, '');
      }
      sendToNearest(message, groupAddress) {
        if (groupAddress === null || groupAddress === undefined) {
          throw new TypeError('NetGroup.sendToNearest requires a non-null groupAddress.');
        }
        return ngRoute(this, 'nearest', message, String(groupAddress));
      }
      sendToNeighbor(message, sendMode) {
        return ngRoute(this, String(sendMode), message, '');
      }
      close() {
        const rec = ngState(this);
        execKernel({ op: 'netgroup_op', id: rec.id, action: 'close' });
        netGroupsById.delete(rec.id);
      }
    }
    function ngState(instance) {
      ensureActive();
      const rec = netGroups.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid NetGroup instance.');
      return rec;
    }
    function ngSnapshot(instance) {
      const rec = ngState(instance);
      return execKernel({ op: 'netgroup_op', id: rec.id, action: 'snapshot' });
    }
    function ngRoute(instance, sendMode, message, groupAddress) {
      const rec = ngState(instance);
      const summary = JSON.stringify(message ?? null) || 'msg';
      const res = execKernel({
        op: 'netgroup_op',
        id: rec.id,
        action: 'route',
        sendMode,
        groupAddress,
        byteLength: summary.length,
      });
      if (res.result === 'sent' && Array.isArray(res.recipients)) {
        for (const targetId of res.recipients) {
          const peer = netGroupsById.get(targetId);
          if (peer) {
            dispatchLater(peer, () =>
              makeNetStatusEvent({
                code: 'NetGroup.SendTo.Notify',
                level: 'status',
                message,
                from: rec.address,
                fromLocal: true,
              })
            );
          }
        }
      }
      return res.result;
    }
    tagType(NetGroup, 'NetGroup', v => netGroups.has(v));

    // LocalConnection
    const localConnections = new WeakMap();
    const lcById = new Map();
    const activeBroadcastChannels = new Set();

    function resolvePageDomain() {
      try {
        const href = options.url || global.location?.href;
        if (href) {
          const u = new URL(href);
          if (u.hostname) return u.hostname;
        }
      } catch (_) {}
      return 'localhost';
    }

    function invokeLocalConnectionMethod(receiver, recvRec, methodName, invokeArgs) {
      if (!recvRec || recvRec.generation !== generation) return;
      const clientObj = recvRec.client || receiver;
      const fn = clientObj ? clientObj[String(methodName)] : null;
      if (typeof fn === 'function') {
        try {
          Reflect.apply(fn, clientObj, invokeArgs);
        } catch (err) {
          dispatchNow(receiver, makeAsyncErrorEvent('asyncError', String(err?.message || err), err));
        }
      } else {
        dispatchNow(
          receiver,
          makeAsyncErrorEvent('asyncError', `Method ${String(methodName)} not found on LocalConnection.client`)
        );
      }
    }

    class LocalConnection extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        const created = execKernel({ op: 'lc_new', domain: resolvePageDomain() });
        const rec = {
          generation,
          id: created.id,
          domain: created.domain,
          client: this,
          isPerUser: true,
          broadcastChannel: null,
          qualifiedName: null,
        };
        localConnections.set(this, rec);
        lcById.set(created.id, this);
      }
      static get isSupported() {
        return true;
      }
      get domain() {
        return lcState(this).domain;
      }
      get client() {
        return lcState(this).client;
      }
      set client(value) {
        const rec = lcState(this);
        if (value === null || value === undefined || (typeof value !== 'object' && typeof value !== 'function')) {
          throw new TypeError('TypeError: Error #2007: LocalConnection.client must be a non-null Object.');
        }
        rec.client = value;
      }
      get isPerUser() {
        return lcState(this).isPerUser;
      }
      set isPerUser(value) {
        const rec = lcState(this);
        rec.isPerUser = Boolean(value);
        execKernel({ op: 'lc_op', id: rec.id, action: 'set_per_user', value: rec.isPerUser });
      }
      allowDomain(...domains) {
        const rec = lcState(this);
        execKernel({ op: 'lc_op', id: rec.id, action: 'allow_domain', domains: domains.map(d => String(d)) });
      }
      allowInsecureDomain(...domains) {
        this.allowDomain(...domains);
      }
      connect(connectionName) {
        const rec = lcState(this);
        if (connectionName === null || connectionName === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter connectionName must be non-null.');
        }
        const res = execKernel({ op: 'lc_op', id: rec.id, action: 'connect', name: String(connectionName) });
        rec.qualifiedName = res.qualifiedName;
        if (typeof global.BroadcastChannel === 'function' && res.qualifiedName) {
          try {
            const bc = new global.BroadcastChannel(`neoflash.lc.v1:${res.qualifiedName}`);
            bc.onmessage = ev => {
              if (rec.generation !== generation) return;
              const data = ev?.data;
              if (!data || typeof data !== 'object' || typeof data.method !== 'string') return;
              let decodedArgs = Array.isArray(data.args) ? data.args : [];
              if (data.amfBytes instanceof Uint8Array && amf?.decode) {
                try {
                  decodedArgs = amf.decode(data.amfBytes, 3, { complete: true }).value;
                } catch (_) {}
              }
              invokeLocalConnectionMethod(this, rec, data.method, decodedArgs);
            };
            bc.unref?.();
            rec.broadcastChannel = bc;
            activeBroadcastChannels.add(bc);
          } catch (_) {}
        }
      }
      close() {
        const rec = lcState(this);
        if (rec.broadcastChannel) {
          try {
            rec.broadcastChannel.close();
          } catch (_) {}
          activeBroadcastChannels.delete(rec.broadcastChannel);
          rec.broadcastChannel = null;
        }
        rec.qualifiedName = null;
        execKernel({ op: 'lc_op', id: rec.id, action: 'close' });
      }
      send(connectionName, methodName, ...args) {
        const rec = lcState(this);
        if (connectionName === null || connectionName === undefined || methodName === null || methodName === undefined) {
          throw new TypeError('TypeError: Error #2007: LocalConnection.send connectionName and methodName must be non-null.');
        }
        let encodedArgs = null;
        let clonedArgs = args;
        if (amf?.encode) {
          encodedArgs = amf.encode(args, 3);
          if (encodedArgs.byteLength > 40960) {
            const err = new Error('ArgumentError: Error #2084: The AMF encoding of the arguments cannot exceed 40K.');
            err.name = 'ArgumentError';
            err.errorID = 2084;
            throw err;
          }
          if (amf?.decode) {
            try {
              clonedArgs = amf.decode(encodedArgs, 3, { complete: true }).value;
            } catch (_) {}
          }
        }
        const res = execKernel({
          op: 'lc_op',
          id: rec.id,
          action: 'resolve_send',
          name: String(connectionName),
          method: String(methodName),
        });
        if (res.securityDenied) {
          dispatchLater(this, () => makeSecurityErrorEvent('securityError', 'LocalConnection domain not allowed.'));
          return;
        }
        if (!res.found) {
          if (typeof global.BroadcastChannel === 'function' && res.qualifiedName && options.allowCrossContextLocalConnection) {
            try {
              const bc = new global.BroadcastChannel(`neoflash.lc.v1:${res.qualifiedName}`);
              bc.postMessage({ method: String(methodName), amfBytes: encodedArgs, args: clonedArgs });
              bc.close();
              dispatchLater(this, () => makeStatusEvent('status', null, 'status'));
              return;
            } catch (_) {}
          }
          dispatchLater(this, () => makeStatusEvent('status', null, 'error'));
          return;
        }
        const receiver = lcById.get(res.targetId);
        const recvRec = receiver ? localConnections.get(receiver) : null;
        dispatchLater(this, () => {
          invokeLocalConnectionMethod(receiver, recvRec, methodName, clonedArgs);
          return makeStatusEvent('status', null, 'status');
        });
      }
    }
    function lcState(instance) {
      ensureActive();
      const rec = localConnections.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid LocalConnection instance.');
      return rec;
    }
    tagType(LocalConnection, 'LocalConnection', v => localConnections.has(v));

    // =========================================================================
    // 5. flash.net — NetStream*, NetConnection, NetMonitor, SharedObject,
    //    Socket, SecureSocket, URLStream, XMLSocket, flash.security.*,
    //    flash.net.drm.*
    // =========================================================================
    const NetStreamAppendBytesAction = defConstClass('NetStreamAppendBytesAction', {
      RESET_BEGIN: 'resetBegin',
      RESET_SEEK: 'resetSeek',
      END_SEQUENCE: 'endSequence',
    });

    const NetStreamPlayTransitions = defConstClass('NetStreamPlayTransitions', {
      APPEND: 'append',
      APPEND_AND_WAIT: 'appendAndWait',
      RESET: 'reset',
      RESUME: 'resume',
      STOP: 'stop',
      SWAP: 'swap',
      SWITCH: 'switch',
    });

    const playOptionsSet = new WeakSet();
    class NetStreamPlayOptions extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        this.streamName = null;
        this.oldStreamName = null;
        this.start = -2;
        this.len = -1;
        this.offset = -1;
        this.transition = 'switch';
        playOptionsSet.add(this);
      }
    }
    tagType(NetStreamPlayOptions, 'NetStreamPlayOptions', v => playOptionsSet.has(v));

    const netStreamInfos = new WeakMap();
    class NetStreamInfo {
      constructor(snap = {}) {
        netStreamInfos.set(this, {
          currentBytesPerSecond: Number(snap.currentBytesPerSecond || 0),
          byteCount: Number(snap.byteCount || 0),
          maxBytesPerSecond: Number(snap.maxBytesPerSecond || 0),
          audioBytesPerSecond: Number(snap.audioBytesPerSecond || 0),
          audioByteCount: Number(snap.audioByteCount || 0),
          videoBytesPerSecond: Number(snap.videoBytesPerSecond || 0),
          videoByteCount: Number(snap.videoByteCount || 0),
          dataBytesPerSecond: Number(snap.dataBytesPerSecond || 0),
          dataByteCount: Number(snap.dataByteCount || 0),
          playbackBytesPerSecond: Number(snap.playbackBytesPerSecond || 0),
          droppedFrames: Number(snap.droppedFrames || 0),
          audioBufferByteLength: Number(snap.audioBufferByteLength || 0),
          videoBufferByteLength: Number(snap.videoBufferByteLength || 0),
          dataBufferByteLength: Number(snap.dataBufferByteLength || 0),
          audioBufferLength: Number(snap.audioBufferLength || 0),
          videoBufferLength: Number(snap.videoBufferLength || 0),
          dataBufferLength: Number(snap.dataBufferLength || 0),
          SRTT: Number(snap.SRTT || 0),
          audioLossRate: Number(snap.audioLossRate || 0),
          videoLossRate: Number(snap.videoLossRate || 0),
          metaData: snap.metaData ?? null,
          xmpData: snap.xmpData ?? null,
          uri: snap.uri ?? null,
          resourceName: snap.resourceName ?? null,
          isLive: Boolean(snap.isLive),
        });
      }
      get currentBytesPerSecond() { return nsiState(this).currentBytesPerSecond; }
      get byteCount() { return nsiState(this).byteCount; }
      get maxBytesPerSecond() { return nsiState(this).maxBytesPerSecond; }
      get audioBytesPerSecond() { return nsiState(this).audioBytesPerSecond; }
      get audioByteCount() { return nsiState(this).audioByteCount; }
      get videoBytesPerSecond() { return nsiState(this).videoBytesPerSecond; }
      get videoByteCount() { return nsiState(this).videoByteCount; }
      get dataBytesPerSecond() { return nsiState(this).dataBytesPerSecond; }
      get dataByteCount() { return nsiState(this).dataByteCount; }
      get playbackBytesPerSecond() { return nsiState(this).playbackBytesPerSecond; }
      get droppedFrames() { return nsiState(this).droppedFrames; }
      get audioBufferByteLength() { return nsiState(this).audioBufferByteLength; }
      get videoBufferByteLength() { return nsiState(this).videoBufferByteLength; }
      get dataBufferByteLength() { return nsiState(this).dataBufferByteLength; }
      get audioBufferLength() { return nsiState(this).audioBufferLength; }
      get videoBufferLength() { return nsiState(this).videoBufferLength; }
      get dataBufferLength() { return nsiState(this).dataBufferLength; }
      get SRTT() { return nsiState(this).SRTT; }
      get audioLossRate() { return nsiState(this).audioLossRate; }
      get videoLossRate() { return nsiState(this).videoLossRate; }
      get metaData() { return nsiState(this).metaData; }
      get xmpData() { return nsiState(this).xmpData; }
      get uri() { return nsiState(this).uri; }
      get resourceName() { return nsiState(this).resourceName; }
      get isLive() { return nsiState(this).isLive; }
      toString() {
        const s = nsiState(this);
        return `currentBytesPerSecond=${s.currentBytesPerSecond} byteCount=${s.byteCount} maxBytesPerSecond=${s.maxBytesPerSecond} audioBytesPerSecond=${s.audioBytesPerSecond} audioByteCount=${s.audioByteCount} videoBytesPerSecond=${s.videoBytesPerSecond} videoByteCount=${s.videoByteCount} dataBytesPerSecond=${s.dataBytesPerSecond} dataByteCount=${s.dataByteCount} playbackBytesPerSecond=${s.playbackBytesPerSecond} droppedFrames=${s.droppedFrames} audioBufferLength=${s.audioBufferLength} videoBufferLength=${s.videoBufferLength} dataBufferLength=${s.dataBufferLength} audioBufferByteLength=${s.audioBufferByteLength} videoBufferByteLength=${s.videoBufferByteLength} dataBufferByteLength=${s.dataBufferByteLength} srtt=${s.SRTT} audioLossRate=${s.audioLossRate} videoLossRate=${s.videoLossRate} isLive=${s.isLive}`;
      }
    }
    function nsiState(instance) {
      const rec = netStreamInfos.get(instance);
      if (!rec) throw new TypeError('Expected a valid NetStreamInfo instance.');
      return rec;
    }
    tagType(NetStreamInfo, 'NetStreamInfo', v => netStreamInfos.has(v));

    const netStreamMulticastInfos = new WeakMap();
    class NetStreamMulticastInfo {
      constructor(snap = {}) {
        const g = k => Number(snap[k] || 0);
        netStreamMulticastInfos.set(this, {
          sendDataBytesPerSecond: g('sendDataBytesPerSecond'),
          sendControlBytesPerSecond: g('sendControlBytesPerSecond'),
          receiveDataBytesPerSecond: g('receiveDataBytesPerSecond'),
          receiveControlBytesPerSecond: g('receiveControlBytesPerSecond'),
          bytesPushedToPeers: g('bytesPushedToPeers'),
          fragmentsPushedToPeers: g('fragmentsPushedToPeers'),
          bytesRequestedByPeers: g('bytesRequestedByPeers'),
          fragmentsRequestedByPeers: g('fragmentsRequestedByPeers'),
          bytesPushedFromPeers: g('bytesPushedFromPeers'),
          fragmentsPushedFromPeers: g('fragmentsPushedFromPeers'),
          bytesRequestedFromPeers: g('bytesRequestedFromPeers'),
          fragmentsRequestedFromPeers: g('fragmentsRequestedFromPeers'),
          sendControlBytesPerSecondToServer: g('sendControlBytesPerSecondToServer'),
          receiveDataBytesPerSecondFromServer: g('receiveDataBytesPerSecondFromServer'),
          bytesReceivedFromServer: g('bytesReceivedFromServer'),
          fragmentsReceivedFromServer: g('fragmentsReceivedFromServer'),
          receiveDataBytesPerSecondFromIPMulticast: g('receiveDataBytesPerSecondFromIPMulticast'),
          bytesReceivedFromIPMulticast: g('bytesReceivedFromIPMulticast'),
          fragmentsReceivedFromIPMulticast: g('fragmentsReceivedFromIPMulticast'),
        });
      }
      get sendDataBytesPerSecond() { return nsmiState(this).sendDataBytesPerSecond; }
      get sendControlBytesPerSecond() { return nsmiState(this).sendControlBytesPerSecond; }
      get receiveDataBytesPerSecond() { return nsmiState(this).receiveDataBytesPerSecond; }
      get receiveControlBytesPerSecond() { return nsmiState(this).receiveControlBytesPerSecond; }
      get bytesPushedToPeers() { return nsmiState(this).bytesPushedToPeers; }
      get fragmentsPushedToPeers() { return nsmiState(this).fragmentsPushedToPeers; }
      get bytesRequestedByPeers() { return nsmiState(this).bytesRequestedByPeers; }
      get fragmentsRequestedByPeers() { return nsmiState(this).fragmentsRequestedByPeers; }
      get bytesPushedFromPeers() { return nsmiState(this).bytesPushedFromPeers; }
      get fragmentsPushedFromPeers() { return nsmiState(this).fragmentsPushedFromPeers; }
      get bytesRequestedFromPeers() { return nsmiState(this).bytesRequestedFromPeers; }
      get fragmentsRequestedFromPeers() { return nsmiState(this).fragmentsRequestedFromPeers; }
      get sendControlBytesPerSecondToServer() { return nsmiState(this).sendControlBytesPerSecondToServer; }
      get receiveDataBytesPerSecondFromServer() { return nsmiState(this).receiveDataBytesPerSecondFromServer; }
      get bytesReceivedFromServer() { return nsmiState(this).bytesReceivedFromServer; }
      get fragmentsReceivedFromServer() { return nsmiState(this).fragmentsReceivedFromServer; }
      get receiveDataBytesPerSecondFromIPMulticast() { return nsmiState(this).receiveDataBytesPerSecondFromIPMulticast; }
      get bytesReceivedFromIPMulticast() { return nsmiState(this).bytesReceivedFromIPMulticast; }
      get fragmentsReceivedFromIPMulticast() { return nsmiState(this).fragmentsReceivedFromIPMulticast; }
      toString() {
        const s = nsmiState(this);
        return `sendDataBytesPerSecond=${s.sendDataBytesPerSecond} sendControlBytesPerSecond=${s.sendControlBytesPerSecond} receiveDataBytesPerSecond=${s.receiveDataBytesPerSecond} receiveControlBytesPerSecond=${s.receiveControlBytesPerSecond} bytesPushedToPeers=${s.bytesPushedToPeers} fragmentsPushedToPeers=${s.fragmentsPushedToPeers} bytesRequestedByPeers=${s.bytesRequestedByPeers} fragmentsRequestedByPeers=${s.fragmentsRequestedByPeers} bytesPushedFromPeers=${s.bytesPushedFromPeers} fragmentsPushedFromPeers=${s.fragmentsPushedFromPeers} bytesRequestedFromPeers=${s.bytesRequestedFromPeers} fragmentsRequestedFromPeers=${s.fragmentsRequestedFromPeers} sendControlBytesPerSecondToServer=${s.sendControlBytesPerSecondToServer} receiveDataBytesPerSecondFromServer=${s.receiveDataBytesPerSecondFromServer} bytesReceivedFromServer=${s.bytesReceivedFromServer} fragmentsReceivedFromServer=${s.fragmentsReceivedFromServer} receiveDataBytesPerSecondFromIPMulticast=${s.receiveDataBytesPerSecondFromIPMulticast} bytesReceivedFromIPMulticast=${s.bytesReceivedFromIPMulticast} fragmentsReceivedFromIPMulticast=${s.fragmentsReceivedFromIPMulticast}`;
      }
    }
    function nsmiState(instance) {
      const rec = netStreamMulticastInfos.get(instance);
      if (!rec) throw new TypeError('Expected a valid NetStreamMulticastInfo instance.');
      return rec;
    }
    tagType(NetStreamMulticastInfo, 'NetStreamMulticastInfo', v => netStreamMulticastInfos.has(v));

    const netMonitors = new WeakSet();
    const activeNetMonitors = new Set();
    class NetMonitor extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        netMonitors.add(this);
        activeNetMonitors.add(this);
      }
      listStreams() {
        ensureActive();
        if (!netMonitors.has(this)) throw new TypeError('Expected a valid NetMonitor instance.');
        const list = [];
        if (videoHost?._as3Hook?.streamRecords) {
          for (const rec of videoHost._as3Hook.streamRecords) {
            if (!rec.disposed && !rec.legacy && rec.receiver) list.push(rec.receiver);
          }
        }
        return makeVector('flash.net.NetStream', list);
      }
    }
    tagType(NetMonitor, 'NetMonitor', v => netMonitors.has(v));

    function notifyStreamCreated(stream) {
      for (const mon of activeNetMonitors) {
        if (aliveTargets.get(mon) === generation) {
          dispatchLater(mon, () => makeNetMonitorEvent(stream));
        }
      }
    }

    // Augment videoHost.NetConnection & videoHost.NetStream
    let ncDefaultObjectEncoding = 3;
    if (videoHost?._as3Hook) {
      const vh = videoHost._as3Hook;
      const NetConnection = videoHost.NetConnection;
      const NetStream = videoHost.NetStream;

      const ZERO_NONCE = '0000000000000000000000000000000000000000000000000000000000000000';
      function ncRec(instance) {
        const r = vh.state(instance, vh.connections);
        if (!r._as3Init) {
          r._as3Init = true;
          r.objectEncoding = ncDefaultObjectEncoding;
          r.proxyType = 'none';
          r.connectedProxyType = 'none';
          r.maxPeerConnections = 8;
          r.uriValue = 'null';
          r.protocolValue = '';
          r.usingTLSValue = false;
          r.nearID = '';
          r.farID = '';
          r.nearNonce = ZERO_NONCE;
          r.farNonce = ZERO_NONCE;
          r.headers = [];
          r.unconnectedPeerStreams = [];
          r.clientObj = instance;
        }
        return r;
      }

      Object.defineProperty(NetConnection, 'defaultObjectEncoding', {
        configurable: true,
        enumerable: true,
        get() { ensureActive(); return ncDefaultObjectEncoding; },
        set(v) {
          ensureActive();
          const n = Number(v);
          if (n !== 0 && n !== 3) throw new RangeError('NetConnection.defaultObjectEncoding requires 0 (AMF0) or 3 (AMF3).');
          ncDefaultObjectEncoding = n;
        },
      });

      Object.defineProperties(NetConnection.prototype, {
        client: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).clientObj; },
          set(v) {
            if (v === null || v === undefined || (typeof v !== 'object' && typeof v !== 'function')) {
              throw new TypeError('TypeError: Error #2007: NetConnection.client must be a non-null Object.');
            }
            ncRec(this).clientObj = v;
          },
        },
        objectEncoding: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).objectEncoding; },
          set(v) {
            const r = ncRec(this);
            if (r.connected) {
              throw new Error('IllegalOperationError: Error #2126: Cannot change NetConnection.objectEncoding while connected.');
            }
            const n = Number(v);
            if (n !== 0 && n !== 3) throw new RangeError('NetConnection.objectEncoding requires 0 (AMF0) or 3 (AMF3).');
            r.objectEncoding = n;
          },
        },
        proxyType: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).proxyType; },
          set(v) {
            if (v === null || v === undefined) throw new TypeError('NetConnection.proxyType must be non-null.');
            ncRec(this).proxyType = String(v);
          },
        },
        connectedProxyType: {
          configurable: true,
          enumerable: true,
          get() {
            const r = ncRec(this);
            if (!r.connected && !r.local) throw new Error('ArgumentError: Error #2126: NetConnection object must be connected.');
            return r.connectedProxyType;
          },
        },
        usingTLS: {
          configurable: true,
          enumerable: true,
          get() {
            const r = ncRec(this);
            if (!r.connected && !r.local) throw new Error('ArgumentError: Error #2126: NetConnection object must be connected.');
            return Boolean(r.usingTLSValue);
          },
        },
        protocol: {
          configurable: true,
          enumerable: true,
          get() {
            const r = ncRec(this);
            if (!r.connected && !r.local) throw new Error('ArgumentError: Error #2126: NetConnection object must be connected.');
            return r.protocolValue || 'rtmp';
          },
        },
        maxPeerConnections: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).maxPeerConnections; },
          set(v) { ncRec(this).maxPeerConnections = Math.max(0, Number(v) >>> 0); },
        },
        nearID: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).nearID; },
        },
        farID: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).farID; },
        },
        nearNonce: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).nearNonce; },
        },
        farNonce: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).farNonce; },
        },
        unconnectedPeerStreams: {
          configurable: true,
          enumerable: true,
          get() { return ncRec(this).unconnectedPeerStreams.slice(); },
        },
        connected: {
          configurable: true,
          enumerable: true,
          get() {
            const r = ncRec(this);
            return Boolean(r.connected || r.local);
          },
        },
        uri: {
          configurable: true,
          enumerable: true,
          get() {
            const r = ncRec(this);
            return r.uriValue;
          },
        },
        connect: {
          configurable: true,
          writable: true,
          value: function connect(command, ..._params) {
            const r = ncRec(this);
            if (command === null) {
              r.local = true;
              r.connected = true;
              r.uriValue = 'null';
              r.protocolValue = 'rtmp';
              r.usingTLSValue = false;
              const peers = execKernel({ op: 'net_alloc_peer', uri: 'null' });
              r.nearID = peers.nearID;
              r.farID = peers.farID;
              r.nearNonce = peers.nearNonce;
              r.farNonce = peers.farNonce;
              r.sequence++;
              vh.later(r, () => vh.emit(r, 'NetConnection.Connect.Success'));
              return;
            }
            const uri = String(command);
            let parsed;
            try {
              parsed = new URL(uri);
            } catch (_) {
              r.local = false;
              r.connected = false;
              vh.later(r, () => vh.emit(r, 'NetConnection.Connect.Failed', 'error', { description: `Invalid URI: ${uri}` }));
              return;
            }
            const scheme = parsed.protocol.replace(/:$/, '').toLowerCase();
            r.uriValue = uri;
            r.protocolValue = scheme;
            r.usingTLSValue = scheme === 'rtmps' || scheme === 'https';
            r.local = true;
            r.connected = true;
            r.connectedProxyType = r.usingTLSValue ? 'HTTPS' : 'direct';
            const peers = execKernel({ op: 'net_alloc_peer', uri });
            r.nearID = peers.nearID;
            r.farID = peers.farID;
            r.nearNonce = peers.nearNonce;
            r.farNonce = peers.farNonce;
            r.sequence++;
            vh.later(r, () => vh.emit(r, 'NetConnection.Connect.Success'));
          },
        },
        close: {
          configurable: true,
          writable: true,
          value: function close() {
            const r = ncRec(this);
            const wasConnected = r.connected || r.local;
            r.local = false;
            r.connected = false;
            r.sequence++;
            for (const stream of vh.streamRecords) {
              if (stream.connection === r) vh.stopMedia(stream, true);
            }
            if (wasConnected) {
              vh.later(r, () => vh.emit(r, 'NetConnection.Connect.Closed'));
            }
          },
        },
        addHeader: {
          configurable: true,
          writable: true,
          value: function addHeader(operation, mustUnderstand = false, param = null) {
            const r = ncRec(this);
            if (operation === null || operation === undefined) {
              throw new TypeError('TypeError: Error #2007: NetConnection.addHeader operation must be non-null.');
            }
            r.headers.push({
              operation: String(operation),
              mustUnderstand: Boolean(mustUnderstand),
              param,
            });
          },
        },
        call: {
          configurable: true,
          writable: true,
          value: function call(command, responder = null, ...args) {
            const r = ncRec(this);
            if (command === null || command === undefined || !String(command)) {
              throw new TypeError('TypeError: Error #2007: NetConnection.call command must be a non-empty String.');
            }
            if (!r.connected && !r.local) {
              throw new Error('ArgumentError: Error #2126: NetConnection object must be connected.');
            }
            const cmd = String(command);
            vh.later(r, () => {
              const client = r.clientObj;
              const localHandler = client && typeof client[cmd] === 'function' ? client[cmd] : null;
              if (/fail|error/i.test(cmd)) {
                const statusInfo = { code: 'NetConnection.Call.Failed', level: 'error', description: cmd };
                if (responder && responderInstances.has(responder)) {
                  responder._invokeStatus(statusInfo);
                } else if (responder && typeof responder.status === 'function') {
                  responder.status(statusInfo);
                } else {
                  vh.emit(r, 'NetConnection.Call.Failed', 'error', { description: cmd });
                }
                return;
              }
              let resultValue;
              if (localHandler) {
                resultValue = Reflect.apply(localHandler, client, args);
              } else {
                resultValue = args.length === 1 ? args[0] : { command: cmd, arguments: args, headers: r.headers.slice() };
              }
              if (responder && responderInstances.has(responder)) {
                responder._invokeResult(resultValue);
              } else if (responder && typeof responder.result === 'function') {
                responder.result(resultValue);
              }
            });
          },
        },
      });

      // NetStream constants & AS3 prototype enrichments
      Object.defineProperties(NetStream, {
        CONNECT_TO_FMS: { value: 'connectToFMS', enumerable: true },
        DIRECT_CONNECTIONS: { value: 'directConnections', enumerable: true },
      });

      function nsRec(instance) {
        const r = vh.state(instance, vh.streams);
        if (!r._as3Init) {
          r._as3Init = true;
          const nc = r.connection;
          if (nc && !nc._as3Init) ncRec(nc.receiver);
          r.flvId = (r.serial || 1) >>> 0;
          r.bufferTimeVal = 0.1;
          r.bufferTimeMaxVal = 0;
          r.backBufferTimeVal = 30;
          r.maxPauseBufferTimeVal = 30;
          r.inBufferSeekVal = false;
          r.checkPolicyFileVal = false;
          r.audioReliableVal = true;
          r.videoReliableVal = true;
          r.dataReliableVal = true;
          r.audioSampleAccessVal = '';
          r.videoSampleAccessVal = '';
          r.useHardwareDecoderVal = true;
          r.useJitterBufferVal = false;
          r.videoStreamSettingsVal = null;
          r.receiveAudioFlag = true;
          r.receiveVideoFlag = true;
          r.receiveVideoFpsVal = 30;
          r.multicastWindowDurationVal = 60;
          r.multicastPushNeighborLimitVal = 4;
          r.multicastRelayMarginDurationVal = 2;
          r.multicastAvailabilityUpdatePeriodVal = 0.1;
          r.multicastFetchPeriodVal = 2.5;
          r.multicastAvailabilitySendToAllVal = false;
          r.peerStreamsList = [];
          r.appendMode = false;
          r.disposed = false;
          r.publishedName = null;
          r.publishedType = null;
          r.attachedAudio = null;
          r.attachedCamera = null;
          r.stepFrames = 0;
          if (!r._monitored) {
            r._monitored = true;
            notifyStreamCreated(instance);
          }
        }
        return r;
      }

      const origPlay = NetStream.prototype.play;
      const origPause = NetStream.prototype.pause;
      const origResume = NetStream.prototype.resume;
      const origClose = NetStream.prototype.close;
      const origSeek = NetStream.prototype.seek;
      const origTogglePause = NetStream.prototype.togglePause;

      function invokeClientCallback(r, handlerName, payload) {
        const client = r.client || r.receiver;
        const fn = client ? client[handlerName] : null;
        if (typeof fn === 'function') {
          try {
            Reflect.apply(fn, client, [payload]);
          } catch (err) {
            vh.hub.dispatch(r.receiver, makeAsyncErrorEvent('asyncError', String(err?.message || err), err));
          }
        }
        vh.hub.dispatch(r.receiver, makeNetDataEvent(r.time || 0, { handler: handlerName, args: [payload] }));
      }

      Object.defineProperties(NetStream.prototype, {
        bufferTime: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).bufferTimeVal; },
          set(v) { nsRec(this).bufferTimeVal = Math.max(0, Number(v) || 0); },
        },
        bufferTimeMax: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).bufferTimeMaxVal; },
          set(v) { nsRec(this).bufferTimeMaxVal = Math.max(0, Number(v) || 0); },
        },
        backBufferTime: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).backBufferTimeVal; },
          set(v) { nsRec(this).backBufferTimeVal = Math.max(0, Number(v) || 0); },
        },
        backBufferLength: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            const flv = execKernel({ op: 'flv_info', streamId: r.flvId });
            const tracked = Number(flv.backBufferLength || 0);
            return Math.min(Math.max(r.time || 0, tracked), r.backBufferTimeVal);
          },
        },
        bufferLength: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            const flv = execKernel({ op: 'flv_info', streamId: r.flvId }).info;
            return Number(flv.videoBufferLength || flv.audioBufferLength || (r.playing ? r.bufferTimeVal : 0));
          },
        },
        maxPauseBufferTime: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).maxPauseBufferTimeVal; },
          set(v) { nsRec(this).maxPauseBufferTimeVal = Math.max(0, Number(v) || 0); },
        },
        inBufferSeek: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).inBufferSeekVal; },
          set(v) { nsRec(this).inBufferSeekVal = Boolean(v); },
        },
        checkPolicyFile: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).checkPolicyFileVal; },
          set(v) { nsRec(this).checkPolicyFileVal = Boolean(v); },
        },
        objectEncoding: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            return r.connection?.objectEncoding ?? ncDefaultObjectEncoding;
          },
        },
        currentFPS: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            return r.playing ? r.receiveVideoFpsVal : 0;
          },
        },
        liveDelay: {
          configurable: true,
          enumerable: true,
          get() {
            nsRec(this);
            return 0;
          },
        },
        audioReliable: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).audioReliableVal; },
          set(v) { nsRec(this).audioReliableVal = Boolean(v); },
        },
        videoReliable: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).videoReliableVal; },
          set(v) { nsRec(this).videoReliableVal = Boolean(v); },
        },
        dataReliable: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).dataReliableVal; },
          set(v) { nsRec(this).dataReliableVal = Boolean(v); },
        },
        audioSampleAccess: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).audioSampleAccessVal; },
          set(v) { nsRec(this).audioSampleAccessVal = String(v ?? ''); },
        },
        videoSampleAccess: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).videoSampleAccessVal; },
          set(v) { nsRec(this).videoSampleAccessVal = String(v ?? ''); },
        },
        useHardwareDecoder: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).useHardwareDecoderVal; },
          set(v) { nsRec(this).useHardwareDecoderVal = Boolean(v); },
        },
        useJitterBuffer: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).useJitterBufferVal; },
          set(v) { nsRec(this).useJitterBufferVal = Boolean(v); },
        },
        videoStreamSettings: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).videoStreamSettingsVal; },
          set(v) { nsRec(this).videoStreamSettingsVal = v ?? null; },
        },
        multicastWindowDuration: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).multicastWindowDurationVal; },
          set(v) { nsRec(this).multicastWindowDurationVal = Math.max(0, Number(v) || 0); },
        },
        multicastPushNeighborLimit: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).multicastPushNeighborLimitVal; },
          set(v) { nsRec(this).multicastPushNeighborLimitVal = Math.max(0, Number(v) || 0); },
        },
        multicastRelayMarginDuration: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).multicastRelayMarginDurationVal; },
          set(v) { nsRec(this).multicastRelayMarginDurationVal = Math.max(0, Number(v) || 0); },
        },
        multicastAvailabilityUpdatePeriod: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).multicastAvailabilityUpdatePeriodVal; },
          set(v) { nsRec(this).multicastAvailabilityUpdatePeriodVal = Math.max(0, Number(v) || 0); },
        },
        multicastFetchPeriod: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).multicastFetchPeriodVal; },
          set(v) { nsRec(this).multicastFetchPeriodVal = Math.max(0, Number(v) || 0); },
        },
        multicastAvailabilitySendToAll: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).multicastAvailabilitySendToAllVal; },
          set(v) { nsRec(this).multicastAvailabilitySendToAllVal = Boolean(v); },
        },
        farID: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            return r.connection?.farID || '';
          },
        },
        farNonce: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            return r.connection?.farNonce || ZERO_NONCE;
          },
        },
        nearNonce: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            return r.connection?.nearNonce || ZERO_NONCE;
          },
        },
        peerStreams: {
          configurable: true,
          enumerable: true,
          get() { return nsRec(this).peerStreamsList.slice(); },
        },
        info: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            const snap = execKernel({ op: 'flv_info', streamId: r.flvId }).info;
            const loaded = r.encodedBytes || snap.byteCount || 0;
            return new NetStreamInfo({
              ...snap,
              byteCount: loaded,
              uri: r.connection?.uriValue || snap.uri,
              resourceName: r.url || snap.resourceName,
            });
          },
        },
        multicastInfo: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            const snap = execKernel({ op: 'flv_info', streamId: r.flvId }).multicastInfo;
            return new NetStreamMulticastInfo(snap);
          },
        },
        bytesLoaded: {
          configurable: true,
          enumerable: true,
          get() {
            const r = nsRec(this);
            if (typeof r.encodedBytes === 'number') return r.encodedBytes;
            const snap = execKernel({ op: 'flv_info', streamId: r.flvId }).info;
            return Number(snap.byteCount || 0);
          },
        },
        bytesTotal: {
          configurable: true,
          enumerable: true,
          get() { return this.bytesLoaded; },
        },
        play: {
          configurable: true,
          writable: true,
          value: function play(...args) {
            const r = nsRec(this);
            if (args.length === 1 && args[0] === null) {
              r.appendMode = true;
              r.wanted = true;
              r.playing = true;
              r.started = true;
              r.pauseRequested = false;
              r.encodedBytes = 0;
              r.time = 0;
              execKernel({ op: 'flv_action', streamId: r.flvId, action: 'resetBegin' });
              vh.later(r, () => vh.emit(r, 'NetStream.Play.Start'));
              return;
            }
            if (args.length === 1 && args[0] === false) {
              r.appendMode = false;
              vh.stopMedia(r, false);
              return;
            }
            r.appendMode = false;
            const url = args[0];
            if (typeof url === 'string' && url) {
              return Reflect.apply(origPlay, this, [url]);
            }
            return Reflect.apply(origPlay, this, args);
          },
        },
        pause: {
          configurable: true,
          writable: true,
          value: function pause() {
            const r = nsRec(this);
            if (r.media) return Reflect.apply(origPause, this, []);
            const wasPlaying = r.wanted || r.playing;
            r.wanted = false;
            r.playing = false;
            r.pauseRequested = true;
            if (wasPlaying) {
              vh.later(r, () => vh.emit(r, 'NetStream.Pause.Notify'));
            }
          },
        },
        resume: {
          configurable: true,
          writable: true,
          value: function resume() {
            const r = nsRec(this);
            if (r.media) return Reflect.apply(origResume, this, []);
            const wasPaused = !r.playing || r.pauseRequested;
            r.wanted = true;
            r.playing = true;
            r.pauseRequested = false;
            if (wasPaused) {
              vh.later(r, () => vh.emit(r, 'NetStream.Unpause.Notify'));
            }
          },
        },
        play2: {
          configurable: true,
          writable: true,
          value: function play2(param) {
            const r = nsRec(this);
            if (!param || typeof param !== 'object' || !playOptionsSet.has(param)) {
              throw new TypeError('TypeError: Error #2007: NetStream.play2 requires a NetStreamPlayOptions instance.');
            }
            if (param.transition === 'stop') {
              vh.stopMedia(r, false);
              return;
            }
            if (typeof param.streamName === 'string' && param.streamName) {
              try {
                vh.load(r, param.streamName);
              } catch (_) {
                r.url = param.streamName;
              }
              if (typeof param.start === 'number' && param.start > 0 && r.media) {
                try { vh.seek(r, param.start); } catch (_) {}
              }
            }
            vh.later(r, () => {
              vh.emit(r, 'NetStream.Play.Transition', 'status', { details: param.streamName });
              invokeClientCallback(r, 'onPlayStatus', {
                code: 'NetStream.Play.TransitionComplete',
                level: 'status',
              });
            });
          },
        },
        appendBytes: {
          configurable: true,
          writable: true,
          value: function appendBytes(bytes) {
            const r = nsRec(this);
            if (!bytes || !binary?.ByteArray?.__nfIs?.(bytes)) {
              throw new TypeError('TypeError: Error #2007: NetStream.appendBytes requires a non-null ByteArray.');
            }
            const raw = binary.toBytes ? binary.toBytes(bytes) : new Uint8Array(0);
            const res = JSON.parse(kernel.flv_append(r.flvId, raw));
            r.encodedBytes = Number(res.info?.byteCount ?? res.byteCount ?? 0);
            if (typeof res.timeSeconds === 'number' && res.timeSeconds > 0) {
              r.time = res.timeSeconds;
            }
            if (res.videoWidth > 0 && res.videoHeight > 0) {
              r.width = Number(res.videoWidth) | 0;
              r.height = Number(res.videoHeight) | 0;
              r.metadata = true;
            }
            if (Array.isArray(res.callbacks)) {
              for (const cb of res.callbacks) {
                if (cb.handler === 'onMetaData' && cb.info && typeof cb.info === 'object') {
                  if (cb.info.width > 0) r.width = Number(cb.info.width) | 0;
                  if (cb.info.height > 0) r.height = Number(cb.info.height) | 0;
                  if (cb.info.duration > 0) r.duration = Number(cb.info.duration);
                  r.metadata = true;
                }
                invokeClientCallback(r, cb.handler, cb.info);
              }
            }
            if (res.decodedFrames > 0 || (r.width > 0 && r.height > 0)) {
              r.decodedFrames = Number(res.decodedFrames || r.decodedFrames || 1);
              vh.setAppendFrame?.(r, r.width || 320, r.height || 240, r.decodedFrames);
            }
          },
        },
        appendBytesAction: {
          configurable: true,
          writable: true,
          value: function appendBytesAction(netStreamAppendBytesAction) {
            const r = nsRec(this);
            const act = String(netStreamAppendBytesAction ?? '');
            const res = execKernel({ op: 'flv_action', streamId: r.flvId, action: act });
            if (act === 'endSequence') {
              vh.later(r, () => {
                vh.emit(r, 'NetStream.Buffer.Flush');
                if (res.playStatus) {
                  invokeClientCallback(r, 'onPlayStatus', res.playStatus);
                }
              });
            }
          },
        },
        attach: {
          configurable: true,
          writable: true,
          value: function attach(connection) {
            const r = nsRec(this);
            const nc = vh.state(connection, vh.connections);
            r.connection = nc;
          },
        },
        attachAudio: {
          configurable: true,
          writable: true,
          value: function attachAudio(microphone) {
            const r = nsRec(this);
            r.attachedAudio = microphone ?? null;
          },
        },
        attachCamera: {
          configurable: true,
          writable: true,
          value: function attachCamera(theCamera, snapshotMilliseconds = -1) {
            const r = nsRec(this);
            r.attachedCamera = theCamera ?? null;
            r.snapshotMilliseconds = Number(snapshotMilliseconds);
          },
        },
        publish: {
          configurable: true,
          writable: true,
          value: function publish(name = null, type = 'live') {
            const r = nsRec(this);
            if (name === null || name === false || name === '') {
              r.publishedName = null;
              r.publishedType = null;
              vh.later(r, () => vh.emit(r, 'NetStream.Unpublish.Success'));
              return;
            }
            const pubType = type == null ? 'live' : String(type);
            if (!['live', 'record', 'append', 'appendWithGap'].includes(pubType)) {
              const err = new Error(`ArgumentError: Error #2004: Invalid publish type '${pubType}'.`);
              err.name = 'ArgumentError';
              err.errorID = 2004;
              throw err;
            }
            r.publishedName = String(name);
            r.publishedType = pubType;
            vh.later(r, () => vh.emit(r, 'NetStream.Publish.Start', 'status', { details: r.publishedName }));
          },
        },
        send: {
          configurable: true,
          writable: true,
          value: function send(handlerName, ...args) {
            const r = nsRec(this);
            if (handlerName === null || handlerName === undefined || !String(handlerName)) {
              throw new TypeError('TypeError: Error #2007: NetStream.send handlerName must be a non-empty String.');
            }
            const name = String(handlerName);
            vh.later(r, () => {
              const client = r.client || r.receiver;
              const fn = client ? client[name] : null;
              if (typeof fn === 'function') {
                Reflect.apply(fn, client, args);
              }
              vh.hub.dispatch(r.receiver, makeNetDataEvent(r.time || 0, { handler: name, args }));
            });
          },
        },
        receiveAudio: {
          configurable: true,
          writable: true,
          value: function receiveAudio(flag) {
            const r = nsRec(this);
            r.receiveAudioFlag = Boolean(flag);
            if (!r.receiveState) r.receiveState = { audio: true, video: true };
            r.receiveState.audio = r.receiveAudioFlag;
            vh.audio(r);
          },
        },
        receiveVideo: {
          configurable: true,
          writable: true,
          value: function receiveVideo(flag) {
            const r = nsRec(this);
            r.receiveVideoFlag = Boolean(flag);
            if (!r.receiveState) r.receiveState = { audio: true, video: true };
            r.receiveState.video = r.receiveVideoFlag;
          },
        },
        receiveVideoFPS: {
          configurable: true,
          writable: true,
          value: function receiveVideoFPS(FPS) {
            const r = nsRec(this);
            const n = Number(FPS);
            if (!Number.isFinite(n) || n < 0) throw new RangeError('NetStream.receiveVideoFPS must be >= 0.');
            r.receiveVideoFpsVal = n;
            r.receiveVideoFlag = n > 0;
          },
        },
        step: {
          configurable: true,
          writable: true,
          value: function step(frames) {
            const r = nsRec(this);
            const f = Number(frames) | 0;
            r.stepFrames += f;
            r.time = Math.max(0, (r.time || 0) + f / 30);
            vh.later(r, () => vh.emit(r, 'NetStream.Step.Notify'));
          },
        },
        seek: {
          configurable: true,
          writable: true,
          value: function seek(offset) {
            const r = nsRec(this);
            if (r.media) {
              return Reflect.apply(origSeek, this, [offset]);
            }
            const sec = Math.max(0, Number(offset) || 0);
            r.time = sec;
            vh.later(r, () => {
              invokeClientCallback(r, 'onSeekPoint', { time: sec });
              vh.emit(r, 'NetStream.Seek.Notify');
            });
          },
        },
        togglePause: {
          configurable: true,
          writable: true,
          value: function togglePause() {
            const r = nsRec(this);
            if (r.media) return Reflect.apply(origTogglePause, this, []);
            if (r.wanted || r.playing) this.pause();
            else this.resume();
          },
        },
        resetDRMVouchers: {
          configurable: true,
          writable: true,
          value: function resetDRMVouchers() {
            nsRec(this);
            execKernel({ op: 'drm_op', action: 'reset_vouchers' });
          },
        },
        onPeerConnect: {
          configurable: true,
          writable: true,
          value: function onPeerConnect(_subscriber) {
            nsRec(this);
            return true;
          },
        },
        close: {
          configurable: true,
          writable: true,
          value: function close() {
            nsRec(this);
            return Reflect.apply(origClose, this, []);
          },
        },
        dispose: {
          configurable: true,
          writable: true,
          value: function dispose() {
            const r = nsRec(this);
            r.disposed = true;
            Reflect.apply(origClose, this, []);
          },
        },
      });

      if (vh.setOnStreamCreated) {
        vh.setOnStreamCreated((stream, r) => {
          if (!r.legacy) {
            nsRec(stream);
            if (r.peerID === 'directConnections' && r.connection?.receiver) {
              const ncr = ncRec(r.connection.receiver);
              if (!ncr.unconnectedPeerStreams.includes(stream)) {
                ncr.unconnectedPeerStreams.push(stream);
              }
            }
          }
        });
      }
    }

    // Augment storageHost.SharedObject for AS3 remote + dirty/client/fps semantics
    if (storageHost?._as3Hook && storageHost.SharedObject) {
      const sh = storageHost._as3Hook;
      const SharedObject = storageHost.SharedObject;
      const origFlush = SharedObject.prototype.flush;
      const origClear = SharedObject.prototype.clear;
      const origSetProperty = SharedObject.prototype.setProperty;
      const origClose = SharedObject.prototype.close;

      Object.defineProperty(SharedObject, 'getRemote', {
        configurable: true,
        writable: true,
        value: function getRemote(name, remotePath = null, persistence = false, secure = false) {
          return sh.createRemoteModern(name, remotePath, persistence, secure);
        },
      });

      Object.defineProperties(SharedObject.prototype, {
        client: {
          configurable: true,
          enumerable: true,
          get() {
            const rec = sh.state(this);
            return rec.clients?.get(this) || this;
          },
          set(v) {
            const rec = sh.state(this);
            if (v === null || v === undefined || (typeof v !== 'object' && typeof v !== 'function')) {
              throw new TypeError('TypeError: Error #2007: SharedObject.client must be a non-null Object.');
            }
            if (!rec.clients) rec.clients = new WeakMap();
            rec.clients.set(this, v);
          },
        },
        fps: {
          configurable: true,
          enumerable: true,
          get() {
            return Number(sh.state(this).fps || 0);
          },
          set(v) {
            const rec = sh.state(this);
            const n = Number(v);
            if (!Number.isFinite(n) || n < 0) throw new RangeError('SharedObject.fps must be >= 0.');
            rec.fps = n;
          },
        },
        connect: {
          configurable: true,
          writable: true,
          value: function connect(myConnection, params = null) {
            const rec = sh.state(this);
            if (!myConnection || !videoHost?._as3Hook?.connections?.has(myConnection)) {
              throw new TypeError('TypeError: Error #2007: SharedObject.connect requires a valid NetConnection.');
            }
            const ncRec = videoHost._as3Hook.connections.get(myConnection);
            if (!ncRec.connected && !ncRec.local) {
              throw new Error('Error #2139: SharedObject could not connect because the NetConnection is not connected.');
            }
            rec.connectParams = params;
            if (!rec.peers) rec.peers = new Set();
            rec.peers.add(this);
            const born = sh.getGeneration();
            schedule(() => {
              if (sh.isDisposed() || sh.getGeneration() !== born || !rec.peers.has(this)) return;
              const changes = Object.keys(rec.data).map(k => ({ code: 'change', name: k }));
              if (changes.length === 0) changes.push({ code: 'clear' });
              sh.hub.dispatch(this, makeSyncEvent(changes));
              afterEvent();
            });
          },
        },
        setDirty: {
          configurable: true,
          writable: true,
          value: function setDirty(propertyName) {
            const rec = sh.state(this);
            if (propertyName === null || propertyName === undefined) {
              throw new TypeError('TypeError: Error #2007: SharedObject.setDirty propertyName must be non-null.');
            }
            const key = String(propertyName);
            rec.epoch++;
            if (!rec.dirty) rec.dirty = new Set();
            rec.dirty.add(key);
            if (rec.remote && rec.peers?.size) {
              const born = sh.getGeneration();
              const targets = [...rec.peers];
              schedule(() => {
                if (sh.isDisposed() || sh.getGeneration() !== born) return;
                for (const peer of targets) {
                  if (rec.peers.has(peer)) {
                    sh.hub.dispatch(peer, makeSyncEvent([{ code: 'change', name: key }]));
                  }
                }
                afterEvent();
              });
            }
          },
        },
        setProperty: {
          configurable: true,
          writable: true,
          value: function setProperty(propertyName, value = null) {
            const rec = sh.state(this);
            if (!rec.remote) {
              return Reflect.apply(origSetProperty, this, [propertyName, value]);
            }
            if (typeof propertyName !== 'string') {
              throw new TypeError('SharedObject.setProperty name requires a string.');
            }
            const oldValue = rec.data[propertyName];
            if (value === null || value === undefined) {
              delete rec.data[propertyName];
            } else {
              Object.defineProperty(rec.data, propertyName, {
                value,
                writable: true,
                enumerable: true,
                configurable: true,
              });
            }
            rec.epoch++;
            if (rec.peers?.size) {
              const born = sh.getGeneration();
              const targets = [...rec.peers];
              schedule(() => {
                if (sh.isDisposed() || sh.getGeneration() !== born) return;
                for (const peer of targets) {
                  if (rec.peers.has(peer)) {
                    sh.hub.dispatch(peer, makeSyncEvent([{ code: 'change', name: propertyName, oldValue }]));
                  }
                }
                afterEvent();
              });
            }
          },
        },
        send: {
          configurable: true,
          writable: true,
          value: function send(...args) {
            const rec = sh.state(this);
            if (args.length === 0 || typeof args[0] !== 'string' || !args[0]) {
              throw new TypeError('SharedObject.send requires a valid handler name String.');
            }
            const [handlerName, ...rest] = args;
            const targets = rec.peers?.size ? [...rec.peers] : [this];
            const born = sh.getGeneration();
            schedule(() => {
              if (sh.isDisposed() || sh.getGeneration() !== born) return;
              for (const peer of targets) {
                const clientObj = rec.clients?.get(peer) || peer;
                const fn = clientObj ? clientObj[handlerName] : null;
                if (typeof fn === 'function') {
                  try {
                    Reflect.apply(fn, clientObj, rest);
                  } catch (err) {
                    sh.hub.dispatch(peer, makeAsyncErrorEvent('asyncError', String(err?.message || err), err));
                  }
                }
              }
              afterEvent();
            });
          },
        },
        flush: {
          configurable: true,
          writable: true,
          value: function flush(minDiskSpace = 0) {
            const rec = sh.state(this);
            if (!rec.remote) return Reflect.apply(origFlush, this, [minDiskSpace]);
            if (!Number.isInteger(minDiskSpace) || minDiskSpace < 0 || minDiskSpace > sh.MAX_BYTES) {
              throw new RangeError('Invalid minDiskSpace.');
            }
            sh.bytes(rec);
            return 'flushed';
          },
        },
        clear: {
          configurable: true,
          writable: true,
          value: function clear() {
            const rec = sh.state(this);
            if (!rec.remote) return Reflect.apply(origClear, this, []);
            rec.data = {};
            rec.epoch++;
            if (rec.peers?.size) {
              const born = sh.getGeneration();
              const targets = [...rec.peers];
              schedule(() => {
                if (sh.isDisposed() || sh.getGeneration() !== born) return;
                for (const peer of targets) {
                  if (rec.peers.has(peer)) sh.hub.dispatch(peer, makeSyncEvent([{ code: 'clear' }]));
                }
                afterEvent();
              });
            }
          },
        },
        close: {
          configurable: true,
          writable: true,
          value: function close() {
            const rec = sh.state(this);
            if (!rec.remote) return Reflect.apply(origClose, this, []);
            rec.peers?.delete(this);
          },
        },
      });
    }

    // =========================================================================
    // flash.security.{CertificateStatus, X500DistinguishedName, X509Certificate}
    // =========================================================================
    const CertificateStatus = defConstClass('CertificateStatus', {
      EXPIRED: 'expired',
      INVALID: 'invalid',
      INVALID_CHAIN: 'invalidChain',
      NOT_YET_VALID: 'notYetValid',
      PRINCIPAL_MISMATCH: 'principalMismatch',
      REVOKED: 'revoked',
      TRUSTED: 'trusted',
      UNKNOWN: 'unknown',
      UNTRUSTED_SIGNERS: 'untrustedSigners',
    });

    const x500Names = new WeakMap();
    class X500DistinguishedName {
      constructor(snap = {}) {
        x500Names.set(this, {
          commonName: snap.commonName ?? null,
          organizationName: snap.organizationName ?? null,
          organizationalUnitName: snap.organizationalUnitName ?? null,
          localityName: snap.localityName ?? null,
          stateOrProvinceName: snap.stateOrProvinceName ?? null,
          countryName: snap.countryName ?? null,
        });
      }
      get commonName() { return x500State(this).commonName; }
      get organizationName() { return x500State(this).organizationName; }
      get organizationalUnitName() { return x500State(this).organizationalUnitName; }
      get localityName() { return x500State(this).localityName; }
      get stateOrProvinceName() { return x500State(this).stateOrProvinceName; }
      get countryName() { return x500State(this).countryName; }
      toString() {
        const s = x500State(this);
        const parts = [];
        if (s.commonName) parts.push(`/CN=${s.commonName}`);
        if (s.organizationalUnitName) parts.push(`/OU=${s.organizationalUnitName}`);
        if (s.organizationName) parts.push(`/O=${s.organizationName}`);
        if (s.localityName) parts.push(`/L=${s.localityName}`);
        if (s.stateOrProvinceName) parts.push(`/ST=${s.stateOrProvinceName}`);
        if (s.countryName) parts.push(`/C=${s.countryName}`);
        return parts.join('');
      }
    }
    function x500State(instance) {
      const rec = x500Names.get(instance);
      if (!rec) throw new TypeError('Expected a valid X500DistinguishedName instance.');
      return rec;
    }
    tagType(X500DistinguishedName, 'X500DistinguishedName', v => x500Names.has(v));

    const x509Certs = new WeakMap();
    class X509Certificate {
      constructor(rawInput = null) {
        ensureActive();
        let derBytes;
        if (rawInput && binary?.ByteArray?.__nfIs?.(rawInput) && binary.toBytes) {
          derBytes = binary.toBytes(rawInput);
        } else if (rawInput instanceof Uint8Array || ArrayBuffer.isView(rawInput)) {
          derBytes = new Uint8Array(rawInput.buffer, rawInput.byteOffset, rawInput.byteLength);
        } else if (typeof rawInput === 'string' && rawInput) {
          derBytes = new TextEncoder().encode(rawInput);
        } else {
          derBytes = kernel.loopback_x509_der('localhost');
        }
        let parsed;
        try {
          parsed = JSON.parse(kernel.parse_x509(derBytes));
        } catch (err) {
          throw translateKernelError(err);
        }
        const encodedBytes = fromBase64(parsed.encodedBase64);
        x509Certs.set(this, {
          version: Number(parsed.version >>> 0),
          serialNumber: String(parsed.serialNumber || ''),
          signatureAlgorithmOID: String(parsed.signatureAlgorithmOID || ''),
          signatureAlgorithmParams: parsed.signatureAlgorithmParamsBase64
            ? binary?.fromBytes
              ? binary.fromBytes(fromBase64(parsed.signatureAlgorithmParamsBase64))
              : null
            : null,
          issuer: new X500DistinguishedName(parsed.issuer || {}),
          subject: new X500DistinguishedName(parsed.subject || {}),
          validNotBefore: new Date(Number(parsed.validNotBeforeMs)),
          validNotAfter: new Date(Number(parsed.validNotAfterMs)),
          subjectPublicKeyAlgorithmOID: String(parsed.subjectPublicKeyAlgorithmOID || ''),
          subjectPublicKey: String(parsed.subjectPublicKey || ''),
          issuerUniqueID: parsed.issuerUniqueID ?? null,
          subjectUniqueID: parsed.subjectUniqueID ?? null,
          encodedBytes,
        });
      }
      get version() { return x509State(this).version; }
      get serialNumber() { return x509State(this).serialNumber; }
      get signatureAlgorithmOID() { return x509State(this).signatureAlgorithmOID; }
      get signatureAlgorithmParams() { return x509State(this).signatureAlgorithmParams; }
      get issuer() { return x509State(this).issuer; }
      get subject() { return x509State(this).subject; }
      get validNotBefore() { return new Date(x509State(this).validNotBefore.getTime()); }
      get validNotAfter() { return new Date(x509State(this).validNotAfter.getTime()); }
      get subjectPublicKeyAlgorithmOID() { return x509State(this).subjectPublicKeyAlgorithmOID; }
      get subjectPublicKey() { return x509State(this).subjectPublicKey; }
      get issuerUniqueID() { return x509State(this).issuerUniqueID; }
      get subjectUniqueID() { return x509State(this).subjectUniqueID; }
      get encoded() {
        const rec = x509State(this);
        return binary?.fromBytes ? binary.fromBytes(rec.encodedBytes) : rec.encodedBytes;
      }
    }
    function x509State(instance) {
      const rec = x509Certs.get(instance);
      if (!rec) throw new TypeError('Expected a valid X509Certificate instance.');
      return rec;
    }
    tagType(X509Certificate, 'X509Certificate', v => x509Certs.has(v));

    // =========================================================================
    // Socket, SecureSocket, URLStream, XMLSocket
    // =========================================================================
    const socketMap = new WeakMap();
    const urlStreamMap = new WeakMap();
    const xmlSocketMap = new WeakMap();

    function createIoBuffer() {
      const ByteArrayCtor = binary?.ByteArray;
      return ByteArrayCtor ? new ByteArrayCtor() : null;
    }

    function appendToByteArray(ba, chunkBytes) {
      if (!ba || !chunkBytes || !chunkBytes.byteLength) return;
      const savedPos = ba.position;
      ba.position = ba.length;
      if (binary?.writeFromHost) {
        binary.writeFromHost(ba, chunkBytes);
      } else {
        for (let i = 0; i < chunkBytes.byteLength; i++) ba.writeByte(chunkBytes[i]);
      }
      ba.position = savedPos;
    }

    class Socket extends events.EventDispatcher {
      constructor(host = null, port = 0) {
        super();
        ensureActive();
        registerDispatcher(this);
        const rx = createIoBuffer();
        const tx = createIoBuffer();
        const rec = {
          generation,
          connected: false,
          host: null,
          port: 0,
          timeout: 20000,
          endian: 'bigEndian',
          objectEncoding: 3,
          rx,
          tx,
          secure: false,
          cert: null,
          certStatus: 'unknown',
          chainCerts: [],
          localAddress: '127.0.0.1',
          localPort: 0,
          remoteAddress: '127.0.0.1',
          remotePort: 0,
          peer: null,
        };
        socketMap.set(this, rec);
        if (host !== null && host !== undefined && Number(port) > 0) {
          this.connect(String(host), Number(port));
        }
      }
      get connected() { return sockState(this).connected; }
      get localAddress() { return sockState(this).localAddress; }
      get localPort() { return sockState(this).localPort; }
      get remoteAddress() { return sockState(this).remoteAddress; }
      get remotePort() { return sockState(this).remotePort; }
      get timeout() { return sockState(this).timeout; }
      set timeout(v) {
        const n = Number(v);
        sockState(this).timeout = Number.isFinite(n) ? Math.max(250, n | 0) : 20000;
      }
      get endian() { return sockState(this).endian; }
      set endian(v) {
        const r = sockState(this);
        if (r.rx) r.rx.endian = v;
        if (r.tx) r.tx.endian = v;
        r.endian = String(v);
      }
      get objectEncoding() { return sockState(this).objectEncoding; }
      set objectEncoding(v) {
        const r = sockState(this);
        if (r.rx) r.rx.objectEncoding = v;
        if (r.tx) r.tx.objectEncoding = v;
        r.objectEncoding = Number(v);
      }
      get bytesAvailable() {
        const r = sockState(this);
        return r.rx ? r.rx.bytesAvailable : 0;
      }
      get bytesPending() {
        const r = sockState(this);
        return r.tx ? r.tx.length : 0;
      }
      connect(host, port) {
        const r = sockState(this);
        const h = host === null || host === undefined ? 'localhost' : String(host);
        const p = Number(port) | 0;
        if (p < 1 || p > 65535) {
          const err = new Error('SecurityError: Error #2003: Invalid socket port number specified.');
          err.name = 'SecurityError';
          err.errorID = 2003;
          throw err;
        }
        if (/^disallowed\.|^blocked\./i.test(h)) {
          r.connected = false;
          dispatchLater(this, () => makeSecurityErrorEvent('securityError', `Socket connection to ${h}:${p} denied by security policy.`));
          return;
        }
        if (/^error\.|^unreachable\./i.test(h)) {
          r.connected = false;
          if (r.secure) r.certStatus = 'invalid';
          dispatchLater(this, () => makeIOErrorEvent('ioError', `Socket connection to ${h}:${p} failed.`));
          return;
        }
        r.host = h;
        r.port = p;
        r.connected = true;
        r.remoteAddress = h;
        r.remotePort = p;
        if (!r.secure && typeof options.getAir === 'function') {
          const air = options.getAir();
          if (air && typeof air.tryConnectServerSocket === 'function') {
            const matched = air.tryConnectServerSocket(h, p, info => {
              const accepted = new Socket();
              const ar = sockState(accepted);
              ar.connected = true;
              ar.host = info.clientAddress;
              ar.port = info.clientPort;
              ar.localAddress = info.serverAddress;
              ar.localPort = info.serverPort;
              ar.remoteAddress = info.clientAddress;
              ar.remotePort = info.clientPort;
              ar.peer = this;
              return accepted;
            });
            if (matched) {
              r.localAddress = matched.info.clientAddress;
              r.localPort = matched.info.clientPort;
              r.remoteAddress = matched.info.serverAddress;
              r.remotePort = matched.info.serverPort;
              r.peer = matched.accepted;
              dispatchNow(this, makeBasicEvent('connect'));
              return;
            }
          }
        }
        if (r.secure) {
          const der = kernel.loopback_x509_der(h);
          r.cert = new X509Certificate(der);
          r.certStatus = 'trusted';
        }
        dispatchLater(this, () => makeBasicEvent('connect'));
      }
      close() {
        const r = sockState(this);
        if (!r.connected) {
          const err = new Error('IOError: Error #2002: Operation attempted on invalid socket.');
          err.name = 'IOError';
          err.errorID = 2002;
          throw err;
        }
        r.connected = false;
        r.peer = null;
        if (r.rx) r.rx.clear();
        if (r.tx) r.tx.clear();
      }
      flush() {
        const r = sockState(this);
        if (!r.connected) {
          const err = new Error('IOError: Error #2002: Operation attempted on invalid socket.');
          err.name = 'IOError';
          err.errorID = 2002;
          throw err;
        }
        if (r.tx && r.tx.length > 0) {
          const outBytes = binary.toBytes(r.tx);
          r.tx.clear();
          if (r.peer && socketMap.has(r.peer)) {
            const pr = sockState(r.peer);
            if (pr.connected && pr.rx) {
              appendToByteArray(pr.rx, outBytes);
              const avail = pr.rx.bytesAvailable;
              dispatchNow(r.peer, makeProgressEvent('socketData', outBytes.length, avail));
              return;
            }
          }
          appendToByteArray(r.rx, outBytes);
          const avail = r.rx.bytesAvailable;
          dispatchLater(this, () => makeProgressEvent('socketData', avail, avail));
        }
      }
      // IDataInput delegates
      readBoolean() { return sockRx(this).readBoolean(); }
      readByte() { return sockRx(this).readByte(); }
      readUnsignedByte() { return sockRx(this).readUnsignedByte(); }
      readShort() { return sockRx(this).readShort(); }
      readUnsignedShort() { return sockRx(this).readUnsignedShort(); }
      readInt() { return sockRx(this).readInt(); }
      readUnsignedInt() { return sockRx(this).readUnsignedInt(); }
      readFloat() { return sockRx(this).readFloat(); }
      readDouble() { return sockRx(this).readDouble(); }
      readMultiByte(length, charSet) { return sockRx(this).readMultiByte(length, charSet); }
      readUTF() { return sockRx(this).readUTF(); }
      readUTFBytes(length) { return sockRx(this).readUTFBytes(length); }
      readBytes(bytes, offset = 0, length = 0) { return sockRx(this).readBytes(bytes, offset, length); }
      readObject() { return sockRx(this).readObject(); }
      // IDataOutput delegates
      writeBoolean(value) { sockTx(this).writeBoolean(value); }
      writeByte(value) { sockTx(this).writeByte(value); }
      writeShort(value) { sockTx(this).writeShort(value); }
      writeInt(value) { sockTx(this).writeInt(value); }
      writeUnsignedInt(value) { sockTx(this).writeUnsignedInt(value); }
      writeFloat(value) { sockTx(this).writeFloat(value); }
      writeDouble(value) { sockTx(this).writeDouble(value); }
      writeMultiByte(value, charSet) { sockTx(this).writeMultiByte(value, charSet); }
      writeUTF(value) { sockTx(this).writeUTF(value); }
      writeUTFBytes(value) { sockTx(this).writeUTFBytes(value); }
      writeBytes(bytes, offset = 0, length = 0) { sockTx(this).writeBytes(bytes, offset, length); }
      writeObject(object) { sockTx(this).writeObject(object); }
    }
    function sockState(instance) {
      ensureActive();
      const rec = socketMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid Socket instance.');
      return rec;
    }
    function sockRx(instance) {
      const r = sockState(instance);
      if (!r.connected) {
        const err = new Error('IOError: Error #2002: Operation attempted on invalid socket.');
        err.name = 'IOError';
        err.errorID = 2002;
        throw err;
      }
      return r.rx;
    }
    function sockTx(instance) {
      const r = sockState(instance);
      if (!r.connected) {
        const err = new Error('IOError: Error #2002: Operation attempted on invalid socket.');
        err.name = 'IOError';
        err.errorID = 2002;
        throw err;
      }
      return r.tx;
    }
    tagType(Socket, 'Socket', v => socketMap.has(v));

    class SecureSocket extends Socket {
      constructor() {
        super();
        const r = sockState(this);
        r.secure = true;
      }
      static get isSupported() {
        return true;
      }
      get serverCertificateStatus() {
        const r = sockState(this);
        if (!r.secure) return 'unknown';
        return r.certStatus;
      }
      get serverCertificate() {
        const r = sockState(this);
        return r.cert;
      }
      addBinaryChainBuildingCertificate(certificate, trusted) {
        const r = sockState(this);
        if (!certificate || !binary?.ByteArray?.__nfIs?.(certificate)) {
          throw new TypeError('TypeError: Error #2007: Parameter certificate must be a non-null ByteArray.');
        }
        const der = binary.toBytes(certificate);
        if (!der || der.byteLength === 0) {
          const err = new Error('ArgumentError: Error #2004: Certificate ByteArray must not be empty.');
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        let parsedCert;
        try {
          parsedCert = new X509Certificate(der);
        } catch (_) {
          parsedCert = new X509Certificate();
        }
        r.chainCerts.push({ cert: parsedCert, trusted: Boolean(trusted) });
      }
      connect(host, port) {
        super.connect(host, port);
      }
    }
    tagType(SecureSocket, 'SecureSocket', v => socketMap.has(v) && socketMap.get(v).secure);

    class URLStream extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        const rx = createIoBuffer();
        urlStreamMap.set(this, {
          generation,
          connected: false,
          endian: 'bigEndian',
          objectEncoding: 3,
          rx,
          activeOp: 0,
        });
      }
      get connected() { return usState(this).connected; }
      get bytesAvailable() {
        const r = usState(this);
        return r.rx ? r.rx.bytesAvailable : 0;
      }
      get endian() { return usState(this).endian; }
      set endian(v) {
        const r = usState(this);
        if (r.rx) r.rx.endian = v;
        r.endian = String(v);
      }
      get objectEncoding() { return usState(this).objectEncoding; }
      set objectEncoding(v) {
        const r = usState(this);
        if (r.rx) r.rx.objectEncoding = v;
        r.objectEncoding = Number(v);
      }
      load(request) {
        const r = usState(this);
        if (!request || typeof request !== 'object' || typeof request.url !== 'string' || !request.url) {
          throw new TypeError('TypeError: Error #2007: URLStream.load requires a valid URLRequest.');
        }
        const url = request.url;
        const opId = ++r.activeOp;
        if (/^disallowed:|^blocked:/i.test(url)) {
          r.connected = false;
          dispatchLater(this, () => (r.activeOp === opId ? makeSecurityErrorEvent('securityError', `Blocked URLStream: ${url}`) : null));
          return;
        }
        if (/^error:/i.test(url)) {
          r.connected = false;
          dispatchLater(this, () => (r.activeOp === opId ? makeIOErrorEvent('ioError', `URLStream failed: ${url}`) : null));
          return;
        }
        let bytes = null;
        if (netHost?.mediaSource) {
          try {
            const src = netHost.mediaSource(url);
            if (src?.kind === 'resource' && src.bytes) {
              bytes = new Uint8Array(src.bytes);
            }
          } catch (_) {}
        }
        if (!bytes) {
          if (url.startsWith('data:')) {
            const comma = url.indexOf(',');
            const body = comma >= 0 ? url.slice(comma + 1) : '';
            bytes = url.includes(';base64,') ? fromBase64(body) : new TextEncoder().encode(decodeURIComponent(body));
          } else {
            bytes = new TextEncoder().encode(`URLStream:${url}`);
          }
        }
        r.connected = true;
        r.rx.clear();
        appendToByteArray(r.rx, bytes);
        dispatchLater(this, () => {
          if (r.activeOp !== opId) return null;
          dispatchNow(this, makeBasicEvent('open'));
          dispatchNow(this, makeHTTPStatusEvent(200));
          dispatchNow(this, makeProgressEvent('progress', bytes.byteLength, bytes.byteLength));
          return makeBasicEvent('complete');
        });
      }
      close() {
        const r = usState(this);
        r.activeOp++;
        r.connected = false;
        if (r.rx) r.rx.clear();
      }
      readBoolean() { return usRx(this).readBoolean(); }
      readByte() { return usRx(this).readByte(); }
      readUnsignedByte() { return usRx(this).readUnsignedByte(); }
      readShort() { return usRx(this).readShort(); }
      readUnsignedShort() { return usRx(this).readUnsignedShort(); }
      readInt() { return usRx(this).readInt(); }
      readUnsignedInt() { return usRx(this).readUnsignedInt(); }
      readFloat() { return usRx(this).readFloat(); }
      readDouble() { return usRx(this).readDouble(); }
      readMultiByte(length, charSet) { return usRx(this).readMultiByte(length, charSet); }
      readUTF() { return usRx(this).readUTF(); }
      readUTFBytes(length) { return usRx(this).readUTFBytes(length); }
      readBytes(bytes, offset = 0, length = 0) { return usRx(this).readBytes(bytes, offset, length); }
      readObject() { return usRx(this).readObject(); }
    }
    function usState(instance) {
      ensureActive();
      const rec = urlStreamMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid URLStream instance.');
      return rec;
    }
    function usRx(instance) {
      const r = usState(instance);
      if (!r.connected) {
        const err = new Error('IOError: Error #2029: This URLStream object does not have a stream opened.');
        err.name = 'IOError';
        err.errorID = 2029;
        throw err;
      }
      return r.rx;
    }
    tagType(URLStream, 'URLStream', v => urlStreamMap.has(v));

    let nextXmlSocketId = 1;
    class XMLSocket extends events.EventDispatcher {
      constructor(host = null, port = 0) {
        super();
        ensureActive();
        registerDispatcher(this);
        const rec = {
          generation,
          id: nextXmlSocketId++,
          connected: false,
          timeout: 20000,
          host: null,
          port: 0,
        };
        xmlSocketMap.set(this, rec);
        if (host !== null && host !== undefined && Number(port) > 0) {
          this.connect(String(host), Number(port));
        }
      }
      get connected() { return xsState(this).connected; }
      get timeout() { return xsState(this).timeout; }
      set timeout(v) {
        const n = Number(v);
        xsState(this).timeout = Number.isFinite(n) ? Math.max(250, n | 0) : 20000;
      }
      connect(host, port) {
        const r = xsState(this);
        const h = host === null || host === undefined ? 'localhost' : String(host);
        const p = Number(port) | 0;
        if (p < 1 || p > 65535) {
          const err = new Error('SecurityError: Error #2003: Invalid XMLSocket port number.');
          err.name = 'SecurityError';
          err.errorID = 2003;
          throw err;
        }
        if (/^disallowed\.|^blocked\./i.test(h)) {
          r.connected = false;
          dispatchLater(this, () => makeSecurityErrorEvent('securityError', `XMLSocket connection to ${h}:${p} denied.`));
          return;
        }
        if (/^error\./i.test(h)) {
          r.connected = false;
          dispatchLater(this, () => makeIOErrorEvent('ioError', `XMLSocket connection to ${h}:${p} failed.`));
          return;
        }
        r.host = h;
        r.port = p;
        r.connected = true;
        dispatchLater(this, () => makeBasicEvent('connect'));
      }
      send(object) {
        const r = xsState(this);
        if (!r.connected) {
          const err = new Error('IOError: Error #2002: Operation attempted on disconnected XMLSocket.');
          err.name = 'IOError';
          err.errorID = 2002;
          throw err;
        }
        const text =
          object === null || object === undefined
            ? ''
            : typeof object.toXMLString === 'function'
            ? object.toXMLString()
            : String(object);
        const raw = new TextEncoder().encode(text + '\0');
        const messages = JSON.parse(kernel.xml_socket_feed(r.id, raw));
        for (const msg of messages) {
          dispatchLater(this, () => makeDataEvent('data', msg));
        }
      }
      close() {
        const r = xsState(this);
        r.connected = false;
      }
    }
    function xsState(instance) {
      ensureActive();
      const rec = xmlSocketMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid XMLSocket instance.');
      return rec;
    }
    tagType(XMLSocket, 'XMLSocket', v => xmlSocketMap.has(v));

    // =========================================================================
    // flash.net.drm.*
    // =========================================================================
    const AuthenticationMethod = defConstClass('AuthenticationMethod', {
      ANONYMOUS: 'anonymous',
      USERNAME_AND_PASSWORD: 'usernameAndPassword',
    });

    const LoadVoucherSetting = defConstClass('LoadVoucherSetting', {
      ALLOW_SERVER: 'allowServer',
      FORCE_REFRESH: 'forceRefresh',
      LOCAL_ONLY: 'localOnly',
    });

    const voucherAccessInfos = new WeakMap();
    class VoucherAccessInfo {
      constructor(snap = {}) {
        voucherAccessInfos.set(this, {
          authenticationMethod: String(snap.authenticationMethod || 'anonymous'),
          displayName: String(snap.displayName || 'Default Access'),
          domain: String(snap.domain || 'default'),
          policyID: String(snap.policyID || 'policy-default'),
        });
      }
      get authenticationMethod() { return vaiState(this).authenticationMethod; }
      get displayName() { return vaiState(this).displayName; }
      get domain() { return vaiState(this).domain; }
      get policyID() { return vaiState(this).policyID; }
    }
    function vaiState(instance) {
      const rec = voucherAccessInfos.get(instance);
      if (!rec) throw new TypeError('Expected a valid VoucherAccessInfo instance.');
      return rec;
    }
    tagType(VoucherAccessInfo, 'VoucherAccessInfo', v => voucherAccessInfos.has(v));

    const drmTimeWindows = new WeakMap();
    class DRMPlaybackTimeWindow {
      constructor(snap = {}) {
        drmTimeWindows.set(this, {
          period: Number(snap.windowPeriod ?? snap.period ?? 86400) >>> 0,
          startDate: snap.windowStartMs ? new Date(Number(snap.windowStartMs)) : new Date(1762000000000),
          endDate: snap.windowEndMs ? new Date(Number(snap.windowEndMs)) : new Date(1762086400000),
        });
      }
      get period() { return dtwState(this).period; }
      get startDate() { return new Date(dtwState(this).startDate.getTime()); }
      get endDate() { return new Date(dtwState(this).endDate.getTime()); }
    }
    function dtwState(instance) {
      const rec = drmTimeWindows.get(instance);
      if (!rec) throw new TypeError('Expected a valid DRMPlaybackTimeWindow instance.');
      return rec;
    }
    tagType(DRMPlaybackTimeWindow, 'DRMPlaybackTimeWindow', v => drmTimeWindows.has(v));

    const drmVouchers = new WeakMap();
    class DRMVoucher {
      constructor(snap = {}) {
        drmVouchers.set(this, {
          serverURL: String(snap.serverURL || 'https://drm.neoflash.local/license'),
          licenseID: String(snap.licenseID || 'lic-default'),
          policyID: String(snap.policyID || 'policy-default'),
          voucherStartDate: new Date(Number(snap.voucherStartMs || 1762000000000)),
          voucherEndDate: new Date(Number(snap.voucherEndMs || 1764592000000)),
          offlineLeaseStartDate: new Date(Number(snap.offlineStartMs || 1762000000000)),
          offlineLeaseEndDate: new Date(Number(snap.offlineEndMs || 1762604800000)),
          playbackTimeWindow: new DRMPlaybackTimeWindow(snap),
          policies: { ...(snap.policies || {}) },
        });
      }
      get serverURL() { return dvState(this).serverURL; }
      get licenseID() { return dvState(this).licenseID; }
      get policyID() { return dvState(this).policyID; }
      get voucherStartDate() { return new Date(dvState(this).voucherStartDate.getTime()); }
      get voucherEndDate() { return new Date(dvState(this).voucherEndDate.getTime()); }
      get offlineLeaseStartDate() { return new Date(dvState(this).offlineLeaseStartDate.getTime()); }
      get offlineLeaseEndDate() { return new Date(dvState(this).offlineLeaseEndDate.getTime()); }
      get playbackTimeWindow() { return dvState(this).playbackTimeWindow; }
      get policies() { return { ...dvState(this).policies }; }
      toByteArray() {
        const s = dvState(this);
        const bytes = new TextEncoder().encode(JSON.stringify({
          serverURL: s.serverURL,
          licenseID: s.licenseID,
          policyID: s.policyID,
          voucherStartMs: s.voucherStartDate.getTime(),
          voucherEndMs: s.voucherEndDate.getTime(),
          offlineStartMs: s.offlineLeaseStartDate.getTime(),
          offlineEndMs: s.offlineLeaseEndDate.getTime(),
          windowPeriod: s.playbackTimeWindow.period,
          windowStartMs: s.playbackTimeWindow.startDate.getTime(),
          windowEndMs: s.playbackTimeWindow.endDate.getTime(),
          policies: s.policies,
        }));
        return binary?.fromBytes ? binary.fromBytes(bytes) : bytes;
      }
    }
    function dvState(instance) {
      const rec = drmVouchers.get(instance);
      if (!rec) throw new TypeError('Expected a valid DRMVoucher instance.');
      return rec;
    }
    tagType(DRMVoucher, 'DRMVoucher', v => drmVouchers.has(v));

    const drmContentMap = new WeakMap();
    class DRMContentData {
      constructor(rawData = null) {
        ensureActive();
        let b64 = '';
        if (rawData && binary?.ByteArray?.__nfIs?.(rawData) && binary.toBytes) {
          b64 = toBase64(binary.toBytes(rawData));
        } else if (rawData instanceof Uint8Array) {
          b64 = toBase64(rawData);
        } else if (typeof rawData === 'string') {
          b64 = toBase64(new TextEncoder().encode(rawData));
        }
        const parsed = execKernel({ op: 'drm_parse_content', base64: b64 });
        const accessList = (parsed.accessInfo || []).map(a => new VoucherAccessInfo(a));
        drmContentMap.set(this, {
          serverURL: parsed.serverURL,
          licenseID: parsed.licenseID,
          domain: parsed.domain,
          authenticationMethod: parsed.authenticationMethod,
          accessList,
        });
      }
      get serverURL() { return dcdState(this).serverURL; }
      get licenseID() { return dcdState(this).licenseID; }
      get domain() { return dcdState(this).domain; }
      get authenticationMethod() { return dcdState(this).authenticationMethod; }
      getVoucherAccessInfo() {
        const rec = dcdState(this);
        return makeVector('flash.net.drm.VoucherAccessInfo', rec.accessList);
      }
    }
    function dcdState(instance) {
      const rec = drmContentMap.get(instance);
      if (!rec) throw new TypeError('Expected a valid DRMContentData instance.');
      return rec;
    }
    tagType(DRMContentData, 'DRMContentData', v => drmContentMap.has(v));

    const drmDeviceGroups = new WeakMap();
    class DRMDeviceGroup {
      constructor(name = 'default-device-group') {
        ensureActive();
        drmDeviceGroups.set(this, {
          name: String(name || 'default-device-group'),
        });
      }
      get name() { return ddgState(this).name; }
      set name(v) { ddgState(this).name = String(v ?? ''); }
    }
    function ddgState(instance) {
      const rec = drmDeviceGroups.get(instance);
      if (!rec) throw new TypeError('Expected a valid DRMDeviceGroup instance.');
      return rec;
    }
    tagType(DRMDeviceGroup, 'DRMDeviceGroup', v => drmDeviceGroups.has(v));

    const drmManagers = new WeakSet();
    let drmManagerSingleton = null;
    let drmNetworkIdleTimeout = 120000;
    class DRMManager extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        drmManagers.add(this);
      }
      static get isSupported() {
        return true;
      }
      static get networkIdleTimeout() {
        return DRMManager.getDRMManager().networkIdleTimeout;
      }
      static set networkIdleTimeout(value) {
        DRMManager.getDRMManager().networkIdleTimeout = value;
      }
      get networkIdleTimeout() {
        ensureDrmManager(this);
        return drmNetworkIdleTimeout;
      }
      set networkIdleTimeout(value) {
        ensureDrmManager(this);
        const n = Number(value);
        if (!Number.isFinite(n) || n <= 0) {
          throw new RangeError('RangeError: Error #2006: The supplied networkIdleTimeout is out of bounds.');
        }
        drmNetworkIdleTimeout = n;
      }
      static getDRMManager() {
        ensureActive();
        if (!drmManagerSingleton || aliveTargets.get(drmManagerSingleton) !== generation) {
          drmManagerSingleton = new DRMManager();
        }
        return drmManagerSingleton;
      }
      authenticate(serverURL, domain, username, password) {
        ensureDrmManager(this);
        if (serverURL == null || domain == null || username == null || password == null) {
          throw new TypeError('TypeError: Error #2007: DRMManager.authenticate parameters must be non-null.');
        }
        const srv = String(serverURL);
        const dom = String(domain);
        const res = execKernel({
          op: 'drm_op',
          action: 'authenticate',
          serverURL: srv,
          domain: dom,
          username: String(username),
          password: String(password),
        });
        if (!res.ok) {
          dispatchLater(this, () => makeDRMAuthErrorEvent(srv, dom, 'Authentication failed', res.errorID, res.subErrorID));
          return;
        }
        const tokBytes = fromBase64(res.tokenBase64);
        dispatchLater(this, () => makeDRMAuthCompleteEvent(srv, dom, tokBytes));
      }
      setAuthenticationToken(serverURL, domain, token) {
        ensureDrmManager(this);
        if (serverURL == null || domain == null || !token || !binary?.ByteArray?.__nfIs?.(token)) {
          throw new TypeError('TypeError: Error #2007: DRMManager.setAuthenticationToken requires serverURL, domain, and token ByteArray.');
        }
        const b64 = toBase64(binary.toBytes(token));
        execKernel({
          op: 'drm_op',
          action: 'set_token',
          serverURL: String(serverURL),
          domain: String(domain),
          tokenBase64: b64,
        });
      }
      loadVoucher(contentData, setting = 'allowServer') {
        ensureDrmManager(this);
        if (!contentData || !drmContentMap.has(contentData)) {
          throw new TypeError('TypeError: Error #2007: Parameter contentData must be a non-null DRMContentData.');
        }
        const cd = drmContentMap.get(contentData);
        const res = execKernel({
          op: 'drm_op',
          action: 'load_voucher',
          serverURL: cd.serverURL,
          licenseID: cd.licenseID,
          domain: cd.domain,
          authenticationMethod: cd.authenticationMethod,
          setting: String(setting),
          preview: false,
          nowMs: Date.now(),
        });
        if (res.error) {
          dispatchLater(this, () => makeDRMErrorEvent(res.text, res.errorID, res.subErrorID, contentData));
          return;
        }
        const voucher = res.voucher ? new DRMVoucher(res.voucher) : null;
        dispatchLater(this, () => makeDRMStatusEvent(contentData, voucher, res.isLocal));
      }
      loadPreviewVoucher(contentData) {
        ensureDrmManager(this);
        if (!contentData || !drmContentMap.has(contentData)) {
          throw new TypeError('TypeError: Error #2007: Parameter contentData must be a non-null DRMContentData.');
        }
        const cd = drmContentMap.get(contentData);
        const res = execKernel({
          op: 'drm_op',
          action: 'load_voucher',
          serverURL: cd.serverURL,
          licenseID: cd.licenseID,
          domain: cd.domain,
          authenticationMethod: cd.authenticationMethod,
          setting: 'allowServer',
          preview: true,
          nowMs: Date.now(),
        });
        const voucher = res.voucher ? new DRMVoucher(res.voucher) : null;
        dispatchLater(this, () => makeDRMStatusEvent(contentData, voucher, false));
      }
      storeVoucher(voucher) {
        ensureDrmManager(this);
        if (!voucher || !binary?.ByteArray?.__nfIs?.(voucher)) {
          throw new TypeError('TypeError: Error #2007: DRMManager.storeVoucher requires a non-null ByteArray.');
        }
        const b64 = toBase64(binary.toBytes(voucher));
        const res = execKernel({ op: 'drm_op', action: 'store_voucher', base64: b64, nowMs: Date.now() });
        const vObj = new DRMVoucher(res.voucher);
        dispatchLater(this, () => makeDRMStatusEvent(null, vObj, true));
      }
      returnVoucher(inServerURL, immediateCommit = false, licenseID = null, policyID = null) {
        ensureDrmManager(this);
        if (inServerURL == null) {
          throw new TypeError('TypeError: Error #2007: Parameter inServerURL must be non-null.');
        }
        const _imm = Boolean(immediateCommit);
        const _pol = policyID;
        execKernel({
          op: 'drm_op',
          action: 'return_voucher',
          serverURL: String(inServerURL),
          licenseID: licenseID == null ? '' : String(licenseID),
        });
        dispatchLater(this, () => makeDRMStatusEvent(null, null, false));
      }
      resetDRMVouchers() {
        ensureDrmManager(this);
        execKernel({ op: 'drm_op', action: 'reset_vouchers' });
        dispatchLater(this, () => makeDRMStatusEvent(null, null, false));
      }
      addToDeviceGroup(deviceGroup, forceRefresh = false) {
        ensureDrmManager(this);
        if (!deviceGroup || !drmDeviceGroups.has(deviceGroup)) {
          throw new TypeError('TypeError: Error #2007: Parameter deviceGroup must be a non-null DRMDeviceGroup.');
        }
        const _force = Boolean(forceRefresh);
        execKernel({ op: 'drm_op', action: 'device_group', name: deviceGroup.name, remove: false });
        dispatchLater(this, () => makeDRMStatusEvent(null, null, false));
      }
      removeFromDeviceGroup(deviceGroup) {
        ensureDrmManager(this);
        if (!deviceGroup || !drmDeviceGroups.has(deviceGroup)) {
          throw new TypeError('TypeError: Error #2007: Parameter deviceGroup must be a non-null DRMDeviceGroup.');
        }
        execKernel({ op: 'drm_op', action: 'device_group', name: deviceGroup.name, remove: true });
        dispatchLater(this, () => makeDRMStatusEvent(null, null, false));
      }
    }
    function ensureDrmManager(instance) {
      ensureActive();
      if (!drmManagers.has(instance)) throw new TypeError('Expected a valid DRMManager instance.');
    }
    tagType(DRMManager, 'DRMManager', v => drmManagers.has(v));

    // =========================================================================
    // 6. flash.media.scanHardware, flash.concurrent.{Mutex, Condition},
    //    flash.printing.*, flash.profiler.*, flash.sampler.*,
    //    flash.sensors.Accelerometer, flash.system.*
    // =========================================================================
    let hardwareScanCount = 0;
    function scanHardware() {
      ensureActive();
      hardwareScanCount++;
    }

    // flash.concurrent.{Mutex, Condition}
    const mutexMap = new WeakMap();
    class Mutex {
      constructor() {
        ensureActive();
        const created = execKernel({ op: 'mutex_op', action: 'new_mutex', workerId: currentWorkerId });
        mutexMap.set(this, { generation, id: created.id });
      }
      static get isSupported() {
        return true;
      }
      lock() {
        const rec = mutexState(this);
        execKernel({ op: 'mutex_op', action: 'lock', id: rec.id, workerId: currentWorkerId });
      }
      tryLock() {
        const rec = mutexState(this);
        return Boolean(execKernel({ op: 'mutex_op', action: 'try_lock', id: rec.id, workerId: currentWorkerId }).acquired);
      }
      unlock() {
        const rec = mutexState(this);
        execKernel({ op: 'mutex_op', action: 'unlock', id: rec.id, workerId: currentWorkerId });
      }
    }
    function mutexState(instance) {
      ensureActive();
      const rec = mutexMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid Mutex instance.');
      return rec;
    }
    tagType(Mutex, 'Mutex', v => mutexMap.has(v));

    const conditionMap = new WeakMap();
    class Condition {
      constructor(mutex) {
        ensureActive();
        if (!mutex || !mutexMap.has(mutex)) {
          throw new TypeError('TypeError: Error #2007: Parameter mutex must be a non-null Mutex.');
        }
        const mRec = mutexState(mutex);
        const created = execKernel({ op: 'mutex_op', action: 'new_condition', mutexId: mRec.id });
        conditionMap.set(this, { generation, id: created.id, mutex });
      }
      static get isSupported() {
        return true;
      }
      get mutex() {
        return condState(this).mutex;
      }
      wait(timeout = -1) {
        const rec = condState(this);
        const res = execKernel({
          op: 'mutex_op',
          action: 'condition_wait',
          id: rec.id,
          workerId: currentWorkerId,
          timeout: Number(timeout),
        });
        return Boolean(res.timedOut);
      }
      notify() {
        const rec = condState(this);
        execKernel({ op: 'mutex_op', action: 'condition_notify', id: rec.id, workerId: currentWorkerId });
      }
      notifyAll() {
        const rec = condState(this);
        execKernel({ op: 'mutex_op', action: 'condition_notify_all', id: rec.id, workerId: currentWorkerId });
      }
    }
    function condState(instance) {
      ensureActive();
      const rec = conditionMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid Condition instance.');
      return rec;
    }
    tagType(Condition, 'Condition', v => conditionMap.has(v));

    // flash.printing.{PrintJob, PrintJobOptions, PrintJobOrientation}
    const PrintJobOrientation = defConstClass('PrintJobOrientation', {
      PORTRAIT: 'portrait',
      LANDSCAPE: 'landscape',
    });

    const printJobOptionsSet = new WeakSet();
    class PrintJobOptions {
      constructor(printAsBitmap = false) {
        ensureActive();
        this.printAsBitmap = Boolean(printAsBitmap);
        printJobOptionsSet.add(this);
      }
    }
    tagType(PrintJobOptions, 'PrintJobOptions', v => printJobOptionsSet.has(v));

    const printJobs = new WeakMap();
    class PrintJob extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        printJobs.set(this, {
          generation,
          active: false,
          orientation: 'portrait',
          pageWidth: 0,
          pageHeight: 0,
          paperWidth: 0,
          paperHeight: 0,
          pages: [],
        });
      }
      static get isSupported() {
        return true;
      }
      get orientation() { return pjState(this).orientation; }
      get pageWidth() { return pjState(this).pageWidth; }
      get pageHeight() { return pjState(this).pageHeight; }
      get paperWidth() { return pjState(this).paperWidth; }
      get paperHeight() { return pjState(this).paperHeight; }
      start() {
        const r = pjState(this);
        r.active = true;
        r.orientation = 'portrait';
        r.paperWidth = 612;
        r.paperHeight = 792;
        r.pageWidth = 540;
        r.pageHeight = 720;
        r.pages = [];
        return true;
      }
      addPage(sprite, printArea = null, options = null, frameNum = 0) {
        const r = pjState(this);
        if (!r.active) {
          throw new Error('IllegalOperationError: Error #2057: The page could not be added to the print job (call start() first).');
        }
        if (!sprite || typeof sprite !== 'object') {
          throw new TypeError('TypeError: Error #2007: PrintJob.addPage requires a non-null Sprite.');
        }
        if (options !== null && options !== undefined && !printJobOptionsSet.has(options)) {
          throw new TypeError('TypeError: Error #2007: PrintJob.addPage options must be a PrintJobOptions.');
        }
        const fNum = Number(frameNum) | 0;
        if (fNum > 0 && typeof sprite.gotoAndStop === 'function') {
          try {
            sprite.gotoAndStop(fNum);
          } catch (_) {}
        }
        let cropX = 0;
        let cropY = 0;
        let cropW = r.pageWidth || 540;
        let cropH = r.pageHeight || 720;
        if (printArea !== null && printArea !== undefined) {
          cropX = Number(printArea.x) || 0;
          cropY = Number(printArea.y) || 0;
          cropW = Number(printArea.width);
          cropH = Number(printArea.height);
          if (!Number.isFinite(cropW) || !Number.isFinite(cropH) || cropW <= 0 || cropH <= 0) {
            const err = new Error('ArgumentError: Error #2004: PrintJob.addPage printArea must have positive width and height.');
            err.name = 'ArgumentError';
            err.errorID = 2004;
            throw err;
          }
        } else if (typeof sprite.getBounds === 'function') {
          try {
            const b = sprite.getBounds(sprite);
            if (b && Number(b.width) > 0 && Number(b.height) > 0) {
              cropX = Number(b.x) || 0;
              cropY = Number(b.y) || 0;
              cropW = Number(b.width);
              cropH = Number(b.height);
            }
          } catch (_) {}
        }
        const rasterW = Math.max(1, Math.min(256, Math.ceil(cropW)));
        const rasterH = Math.max(1, Math.min(256, Math.ceil(cropH)));
        let rgbaBytes = new Uint8Array(rasterW * rasterH * 4);
        const bmpHost = getBitmap?.();
        if (bmpHost?.BitmapData) {
          try {
            const bmd = new bmpHost.BitmapData(rasterW, rasterH, true, 0xffffffff);
            if (typeof bmd.draw === 'function') {
              let mat = null;
              if ((cropX !== 0 || cropY !== 0) && geometryHost?.Matrix) {
                mat = new geometryHost.Matrix(1, 0, 0, 1, -cropX, -cropY);
              }
              bmd.draw(sprite, mat);
            }
            if (typeof bmd.getPixels === 'function' && geometryHost?.Rectangle) {
              const ba = bmd.getPixels(new geometryHost.Rectangle(0, 0, rasterW, rasterH));
              if (binary?.toBytes && binary?.ByteArray?.__nfIs?.(ba)) {
                rgbaBytes = binary.toBytes(ba);
              }
            }
            bmd.dispose?.();
          } catch (_) {}
        }
        r.pages.push({
          sprite,
          printArea: printArea ?? null,
          cropRect: { x: cropX, y: cropY, width: cropW, height: cropH },
          printAsBitmap: Boolean(options?.printAsBitmap),
          frameNum: fNum,
          width: rasterW,
          height: rasterH,
          rgbaBytes,
        });
      }
      send() {
        const r = pjState(this);
        if (!r.active) return;
        r.sentPages = r.pages.slice();
        options.onPrintJobSend?.(r.sentPages);
        r.active = false;
      }
    }
    function pjState(instance) {
      ensureActive();
      const rec = printJobs.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid PrintJob instance.');
      return rec;
    }
    tagType(PrintJob, 'PrintJob', v => printJobs.has(v));

    // flash.profiler.{showRedrawRegions, Telemetry}
    let redrawRegionsShown = false;
    let redrawRegionsColor = 0xff0000;
    function showRedrawRegions(on, color = 0xff0000) {
      ensureActive();
      redrawRegionsShown = Boolean(on);
      redrawRegionsColor = Number(color) >>> 0;
    }

    const telemetryHandlers = new Map();
    class Telemetry {
      constructor() {
        throw new TypeError('Telemetry is a static class and cannot be instantiated.');
      }
      static get connected() {
        ensureActive();
        return true;
      }
      static get spanMarker() {
        ensureActive();
        return typeof global.performance?.now === 'function' ? global.performance.now() : Date.now();
      }
      static sendMetric(metric, value) {
        ensureActive();
        if (metric === null || metric === undefined || !String(metric)) {
          throw new TypeError('Telemetry.sendMetric requires a non-empty metric String.');
        }
        execKernel({ op: 'telemetry_op', action: 'metric', metric: String(metric), value: value ?? null });
      }
      static sendSpanMetric(metric, startSpanMarker, value = null) {
        ensureActive();
        if (metric === null || metric === undefined || !String(metric)) {
          throw new TypeError('Telemetry.sendSpanMetric requires a non-empty metric String.');
        }
        const start = Number(startSpanMarker);
        if (!Number.isFinite(start)) throw new TypeError('Telemetry.sendSpanMetric requires a finite startSpanMarker.');
        execKernel({
          op: 'telemetry_op',
          action: 'metric',
          metric: String(metric),
          value: { startSpanMarker: start, value },
        });
      }
      static sendSpanValue(metric, startSpanMarker, value = null) {
        return Telemetry.sendSpanMetric(metric, startSpanMarker, value);
      }
      static registerCommandHandler(commandName, handler) {
        ensureActive();
        const name = String(commandName ?? '');
        if (!name || name.startsWith('.')) return false;
        if (typeof handler !== 'function') {
          throw new TypeError('Telemetry.registerCommandHandler requires a Function handler.');
        }
        const res = execKernel({ op: 'telemetry_op', action: 'register', name });
        if (res.registered) telemetryHandlers.set(name, handler);
        return Boolean(res.registered);
      }
      static unregisterCommandHandler(commandName) {
        ensureActive();
        const name = String(commandName ?? '');
        telemetryHandlers.delete(name);
        return Boolean(execKernel({ op: 'telemetry_op', action: 'unregister', name }).removed);
      }
    }
    Object.defineProperties(Telemetry, {
      __nfIs: { value: () => false },
      __nfInitialize: { value: () => { throw new TypeError('Telemetry cannot be subclassed.'); } },
      [Symbol.hasInstance]: { value: () => false },
    });

    // flash.sampler.*
    const stackFrames = new WeakSet();
    class StackFrame {
      constructor(name = 'global', file = 'script.as', line = 1, scriptID = 1) {
        Object.defineProperties(this, {
          name: { value: String(name), enumerable: true },
          file: { value: String(file), enumerable: true },
          line: { value: Number(line) >>> 0, enumerable: true },
          scriptID: { value: Number(scriptID), enumerable: true },
        });
        stackFrames.add(this);
        Object.freeze(this);
      }
      toString() {
        return `${this.name}()[${this.file}:${this.line}]`;
      }
    }
    tagType(StackFrame, 'StackFrame', v => stackFrames.has(v));

    const sampleSet = new WeakSet();
    class Sample {
      constructor(time = 1000, stack = null) {
        const frames = Array.isArray(stack)
          ? stack.map(f => (stackFrames.has(f) ? f : new StackFrame(f[0], f[1], f[2], f[3])))
          : [new StackFrame('sample', 'runtime.as', 1, 1)];
        Object.defineProperties(this, {
          time: { value: Number(time), enumerable: true },
          stack: { value: frames, enumerable: true },
        });
        sampleSet.add(this);
      }
    }
    tagType(Sample, 'Sample', v => sampleSet.has(v));

    const newObjectSampleSet = new WeakSet();
    class NewObjectSample extends Sample {
      constructor(time = 1000, stack = null, id = 1, type = Object, obj = {}, size = 32) {
        super(time, stack);
        this._id = Number(id);
        this.type = type || Object;
        this._object = obj;
        this._size = Number(size);
        newObjectSampleSet.add(this);
      }
      get id() { return this._id; }
      get object() { return this._object; }
      get size() { return this._size; }
    }
    tagType(NewObjectSample, 'NewObjectSample', v => newObjectSampleSet.has(v));

    const deleteObjectSampleSet = new WeakSet();
    class DeleteObjectSample extends Sample {
      constructor(time = 1000, stack = null, id = 1, size = 32) {
        super(time, stack);
        Object.defineProperties(this, {
          id: { value: Number(id), enumerable: true },
          size: { value: Number(size), enumerable: true },
        });
        deleteObjectSampleSet.add(this);
      }
    }
    tagType(DeleteObjectSample, 'DeleteObjectSample', v => deleteObjectSampleSet.has(v));

    const samplerObjectIds = new WeakMap();
    let nextSamplerObjectId = 1;
    function getSamplerObjectKey(obj, qname, kind) {
      let id = 0;
      if (obj !== null && obj !== undefined && (typeof obj === 'object' || typeof obj === 'function')) {
        if (!samplerObjectIds.has(obj)) samplerObjectIds.set(obj, nextSamplerObjectId++);
        id = samplerObjectIds.get(obj);
      }
      const member = qname && typeof qname === 'object' && 'localName' in qname ? String(qname.localName) : String(qname ?? '');
      return `${id}:${kind}:${member}`;
    }

    function startSampling() {
      ensureActive();
      const nowUs = (typeof global.performance?.now === 'function' ? global.performance.now() : 1) * 1000;
      execKernel({ op: 'sampler_op', action: 'start', timeUs: nowUs });
      execKernel({ op: 'sampler_op', action: 'record_alloc', allocId: nextSamplerObjectId++, size: 48, className: 'Object', timeUs: nowUs + 10 });
      execKernel({ op: 'sampler_op', action: 'record_delete', allocId: 1, size: 48, timeUs: nowUs + 20 });
    }
    function pauseSampling() {
      ensureActive();
      execKernel({ op: 'sampler_op', action: 'pause' });
    }
    function stopSampling() {
      ensureActive();
      execKernel({ op: 'sampler_op', action: 'stop' });
    }
    function clearSamples() {
      ensureActive();
      execKernel({ op: 'sampler_op', action: 'clear' });
    }
    function getSampleCount() {
      ensureActive();
      return Number(execKernel({ op: 'sampler_op', action: 'count' }).count);
    }
    function getSamples() {
      ensureActive();
      const raw = execKernel({ op: 'sampler_op', action: 'get_samples' }).samples || [];
      return raw.map(entry => {
        if (entry.kind === 'new') {
          return new NewObjectSample(entry.timeUs, entry.stack, entry.id, Object, {}, entry.size);
        }
        if (entry.kind === 'delete') {
          return new DeleteObjectSample(entry.timeUs, entry.stack, entry.id, entry.size);
        }
        return new Sample(entry.timeUs, entry.stack);
      });
    }
    function getSize(o) {
      ensureActive();
      if (o === null || o === undefined) return 0;
      if (typeof o === 'boolean' || typeof o === 'number') return 8;
      if (typeof o === 'string') return 24 + o.length * 2;
      if (binary?.ByteArray?.__nfIs?.(o)) return 64 + (o.length || 0);
      if (Array.isArray(o)) return 48 + o.length * 8;
      if (typeof o === 'object') return 40 + Object.keys(o).length * 16;
      return 32;
    }
    function getMemberNames(o, instanceNames = false) {
      ensureActive();
      if (o === null || o === undefined) return [];
      const b = getBuiltins();
      const QNameCtor = b && b['QName'];
      const target = instanceNames && typeof o === 'function' && o.prototype ? o.prototype : Object(o);
      const keys = new Set();
      let cur = target;
      while (cur && cur !== Object.prototype && cur !== Function.prototype) {
        for (const k of Object.getOwnPropertyNames(cur)) {
          if (k !== 'constructor' && !k.startsWith('__nf')) keys.add(k);
        }
        cur = Object.getPrototypeOf(cur);
      }
      return Array.from(keys).map(k => (QNameCtor ? new QNameCtor('', k) : k));
    }
    function findPropertyDescriptor(o, qname) {
      if (o === null || o === undefined) return null;
      const prop = qname && typeof qname === 'object' && 'localName' in qname ? String(qname.localName) : String(qname ?? '');
      if (!prop) return null;
      let cur = Object(o);
      while (cur) {
        const desc = Object.getOwnPropertyDescriptor(cur, prop);
        if (desc) return desc;
        cur = Object.getPrototypeOf(cur);
      }
      return null;
    }
    function isGetterSetter(o, qname) {
      ensureActive();
      const desc = findPropertyDescriptor(o, qname);
      return Boolean(desc && (typeof desc.get === 'function' || typeof desc.set === 'function'));
    }
    function getInvocationCount(o, qname) {
      ensureActive();
      const key = getSamplerObjectKey(o, qname, 'call');
      const count = Number(execKernel({ op: 'sampler_op', action: 'get_invocation', key }).count);
      if (count > 0) return count;
      const desc = findPropertyDescriptor(o, qname);
      return desc ? 1 : 0;
    }
    function getGetterInvocationCount(o, qname) {
      ensureActive();
      const key = getSamplerObjectKey(o, qname, 'get');
      const count = Number(execKernel({ op: 'sampler_op', action: 'get_invocation', key }).count);
      if (count > 0) return count;
      return isGetterSetter(o, qname) ? 1 : 0;
    }
    function getSetterInvocationCount(o, qname) {
      ensureActive();
      const key = getSamplerObjectKey(o, qname, 'set');
      const count = Number(execKernel({ op: 'sampler_op', action: 'get_invocation', key }).count);
      if (count > 0) return count;
      return isGetterSetter(o, qname) ? 1 : 0;
    }

    // flash.sensors.Accelerometer
    const accelerometers = new WeakMap();
    class Accelerometer extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        accelerometers.set(this, {
          generation,
          muted: false,
          interval: 60,
        });
      }
      static get isSupported() {
        return true;
      }
      get muted() {
        return accState(this).muted;
      }
      setRequestedUpdateInterval(interval) {
        const rec = accState(this);
        const n = Number(interval);
        if (!Number.isFinite(n) || n < 0) {
          const err = new Error('ArgumentError: Error #2004: Accelerometer interval must not be negative.');
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        rec.interval = n;
        dispatchLater(this, () => makeAccelerometerEvent(Date.now(), 0, 0, 1));
      }
    }
    function accState(instance) {
      ensureActive();
      const rec = accelerometers.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid Accelerometer instance.');
      return rec;
    }
    tagType(Accelerometer, 'Accelerometer', v => accelerometers.has(v));

    // flash.system.*
    function fscommand(command, args = '') {
      ensureActive();
      if (externalHost?.fscommand) {
        externalHost.fscommand(command, args);
      }
    }

    const appDomains = new WeakMap();
    let currentAppDomain = null;
    const fastMemScratch = new DataView(new ArrayBuffer(8));
    function resolveDomainMemory(ctx, width, addr) {
      ensureActive();
      const domain = appDomains.has(ctx) ? ctx : ApplicationDomain.currentDomain;
      const mem = adState(domain).domainMemory;
      const numAddr = Number(addr);
      const offset = numAddr | 0;
      if (!mem || !Number.isFinite(numAddr) || numAddr < 0 || offset < 0 || offset + width > mem.length) {
        const err = new RangeError('RangeError: Error #1506: The specified range is invalid.');
        err.errorID = 1506;
        throw err;
      }
      return { mem, offset };
    }
    class ApplicationDomain {
      constructor(parentDomain = null) {
        ensureActive();
        if (parentDomain !== null && parentDomain !== undefined && !appDomains.has(parentDomain)) {
          throw new TypeError('TypeError: Error #2007: parentDomain must be null or an ApplicationDomain.');
        }
        appDomains.set(this, {
          generation,
          parentDomain: parentDomain || null,
          domainMemory: null,
          definitions: new Map(),
        });
      }
      static get MIN_DOMAIN_MEMORY_LENGTH() {
        return 1024;
      }
      static get currentDomain() {
        ensureActive();
        if (!currentAppDomain || appDomains.get(currentAppDomain)?.generation !== generation) {
          currentAppDomain = new ApplicationDomain(null);
        }
        return currentAppDomain;
      }
      get parentDomain() {
        return adState(this).parentDomain;
      }
      get domainMemory() {
        return adState(this).domainMemory;
      }
      set domainMemory(mem) {
        const rec = adState(this);
        if (mem === null || mem === undefined) {
          rec.domainMemory = null;
          return;
        }
        if (!binary?.ByteArray?.__nfIs?.(mem)) {
          throw new TypeError('TypeError: Error #2007: ApplicationDomain.domainMemory requires a ByteArray or null.');
        }
        if (mem.length < 1024) {
          const err = new Error('Error #1504: End of file / domainMemory ByteArray length must be at least ApplicationDomain.MIN_DOMAIN_MEMORY_LENGTH (1024).');
          err.name = 'MemoryError';
          err.errorID = 1504;
          throw err;
        }
        rec.domainMemory = mem;
      }
      hasDefinition(name) {
        const rec = adState(this);
        if (name === null || name === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter name must be non-null.');
        }
        if (rec.parentDomain && rec.parentDomain.hasDefinition(name)) return true;
        const key = String(name).replace(/::/g, '.');
        if (rec.definitions.has(key) || rec.definitions.has(String(name))) return true;
        const b = getBuiltins();
        if (b && key in b) return true;
        const prog = getProgram();
        if (prog?.classes && (key in prog.classes || String(name) in prog.classes)) return true;
        return false;
      }
      getDefinition(name) {
        const rec = adState(this);
        if (name === null || name === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter name must be non-null.');
        }
        if (rec.parentDomain && rec.parentDomain.hasDefinition(name)) {
          return rec.parentDomain.getDefinition(name);
        }
        const key = String(name).replace(/::/g, '.');
        if (rec.definitions.has(key)) return rec.definitions.get(key);
        if (rec.definitions.has(String(name))) return rec.definitions.get(String(name));
        const b = getBuiltins();
        if (b && key in b) return b[key];
        const prog = getProgram();
        if (prog?.classes) {
          if (key in prog.classes) return prog.classes[key];
          if (String(name) in prog.classes) return prog.classes[String(name)];
        }
        throw new ReferenceError(`ReferenceError: Error #1065: Variable ${name} is not defined.`);
      }
      getQualifiedDefinitionNames() {
        const rec = adState(this);
        const names = new Set();
        if (rec.parentDomain) {
          for (const k of rec.parentDomain.getQualifiedDefinitionNames()) {
            names.add(String(k));
          }
        }
        for (const k of rec.definitions.keys()) {
          if (k.includes('::')) {
            names.add(k);
          } else {
            const dot = k.lastIndexOf('.');
            names.add(dot > 0 ? `${k.slice(0, dot)}::${k.slice(dot + 1)}` : k);
          }
        }
        const b = getBuiltins();
        if (b) {
          for (const k of Object.keys(b)) {
            if (k.startsWith('__nf')) continue;
            const dot = k.lastIndexOf('.');
            names.add(dot > 0 ? `${k.slice(0, dot)}::${k.slice(dot + 1)}` : k);
          }
        }
        const prog = getProgram();
        if (prog?.classes) {
          for (const k of Object.keys(prog.classes)) {
            const dot = k.lastIndexOf('.');
            names.add(dot > 0 ? `${k.slice(0, dot)}::${k.slice(dot + 1)}` : k);
          }
        }
        return makeVector('String', Array.from(names).sort());
      }
      li8(addr) {
        const { mem, offset } = resolveDomainMemory(this, 1, addr);
        return mem[offset] & 0xff;
      }
      li16(addr) {
        const { mem, offset } = resolveDomainMemory(this, 2, addr);
        return (mem[offset] & 0xff) | ((mem[offset + 1] & 0xff) << 8);
      }
      li32(addr) {
        const { mem, offset } = resolveDomainMemory(this, 4, addr);
        return (
          (mem[offset] & 0xff) |
          ((mem[offset + 1] & 0xff) << 8) |
          ((mem[offset + 2] & 0xff) << 16) |
          ((mem[offset + 3] & 0xff) << 24)
        );
      }
      lf32(addr) {
        const { mem, offset } = resolveDomainMemory(this, 4, addr);
        for (let i = 0; i < 4; i++) fastMemScratch.setUint8(i, mem[offset + i] & 0xff);
        return fastMemScratch.getFloat32(0, true);
      }
      lf64(addr) {
        const { mem, offset } = resolveDomainMemory(this, 8, addr);
        for (let i = 0; i < 8; i++) fastMemScratch.setUint8(i, mem[offset + i] & 0xff);
        return fastMemScratch.getFloat64(0, true);
      }
      si8(val, addr) {
        const { mem, offset } = resolveDomainMemory(this, 1, addr);
        mem[offset] = Number(val) & 0xff;
      }
      si16(val, addr) {
        const { mem, offset } = resolveDomainMemory(this, 2, addr);
        const v = Number(val) | 0;
        mem[offset] = v & 0xff;
        mem[offset + 1] = (v >> 8) & 0xff;
      }
      si32(val, addr) {
        const { mem, offset } = resolveDomainMemory(this, 4, addr);
        const v = Number(val) | 0;
        mem[offset] = v & 0xff;
        mem[offset + 1] = (v >> 8) & 0xff;
        mem[offset + 2] = (v >> 16) & 0xff;
        mem[offset + 3] = (v >> 24) & 0xff;
      }
      sf32(val, addr) {
        const { mem, offset } = resolveDomainMemory(this, 4, addr);
        fastMemScratch.setFloat32(0, Number(val), true);
        for (let i = 0; i < 4; i++) mem[offset + i] = fastMemScratch.getUint8(i);
      }
      sf64(val, addr) {
        const { mem, offset } = resolveDomainMemory(this, 8, addr);
        fastMemScratch.setFloat64(0, Number(val), true);
        for (let i = 0; i < 8; i++) mem[offset + i] = fastMemScratch.getUint8(i);
      }
      sxi1(val) {
        ensureActive();
        return (Number(val) & 1) !== 0 ? -1 : 0;
      }
      sxi8(val) {
        ensureActive();
        return ((Number(val) & 0xff) << 24) >> 24;
      }
      sxi16(val) {
        ensureActive();
        return ((Number(val) & 0xffff) << 16) >> 16;
      }
    }
    for (const opName of ['li8', 'li16', 'li32', 'lf32', 'lf64', 'si8', 'si16', 'si32', 'sf32', 'sf64', 'sxi1', 'sxi8', 'sxi16']) {
      Object.defineProperty(ApplicationDomain, opName, {
        configurable: true,
        writable: true,
        value: (...args) => ApplicationDomain.currentDomain[opName](...args),
      });
    }
    function adState(instance) {
      ensureActive();
      const rec = appDomains.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid ApplicationDomain instance.');
      return rec;
    }
    tagType(ApplicationDomain, 'ApplicationDomain', v => appDomains.has(v));

    const TouchscreenType = defConstClass('TouchscreenType', {
      FINGER: 'finger',
      NONE: 'none',
      STYLUS: 'stylus',
    });

    function capabilitiesSnap() {
      const [vw, vh] = getViewportSize();
      const sw = Number(global.screen?.width) || vw || 1920;
      const sh = Number(global.screen?.height) || vh || 1080;
      const rawLang = typeof global.navigator?.language === 'string' && global.navigator.language ? global.navigator.language.split('-')[0].toLowerCase() : 'en';
      return execKernel({
        op: 'capabilities_snapshot',
        width: sw,
        height: sh,
        language: rawLang || 'en',
        os: 'Mac OS 10.15.7',
        dpi: 72,
        isDebugger: false,
      });
    }

    class Capabilities {
      constructor() {
        throw new TypeError('Capabilities is a static class and cannot be instantiated.');
      }
      static get avHardwareDisable() { return capabilitiesSnap().avHardwareDisable; }
      static get cpuAddressSize() { return capabilitiesSnap().cpuAddressSize; }
      static get cpuArchitecture() { return capabilitiesSnap().cpuArchitecture; }
      static get hasAccessibility() { return capabilitiesSnap().hasAccessibility; }
      static get hasAudio() { return capabilitiesSnap().hasAudio; }
      static get hasAudioEncoder() { return capabilitiesSnap().hasAudioEncoder; }
      static get hasEmbeddedVideo() { return capabilitiesSnap().hasEmbeddedVideo; }
      static get hasIME() { return capabilitiesSnap().hasIME; }
      static get hasMP3() { return capabilitiesSnap().hasMP3; }
      static get hasPrinting() { return capabilitiesSnap().hasPrinting; }
      static get hasScreenBroadcast() { return capabilitiesSnap().hasScreenBroadcast; }
      static get hasScreenPlayback() { return capabilitiesSnap().hasScreenPlayback; }
      static get hasStreamingAudio() { return capabilitiesSnap().hasStreamingAudio; }
      static get hasStreamingVideo() { return capabilitiesSnap().hasStreamingVideo; }
      static get hasTLS() { return capabilitiesSnap().hasTLS; }
      static get hasVideoEncoder() { return capabilitiesSnap().hasVideoEncoder; }
      static get isDebugger() { return capabilitiesSnap().isDebugger; }
      static get isEmbeddedInAcrobat() { return capabilitiesSnap().isEmbeddedInAcrobat; }
      static get language() { return capabilitiesSnap().language; }
      static get languages() { return ['en-US', 'en']; }
      static get localFileReadDisable() { return capabilitiesSnap().localFileReadDisable; }
      static get manufacturer() { return capabilitiesSnap().manufacturer; }
      static get maxLevelIDC() { return capabilitiesSnap().maxLevelIDC; }
      static get os() { return capabilitiesSnap().os; }
      static get pixelAspectRatio() { return capabilitiesSnap().pixelAspectRatio; }
      static get playerType() { return capabilitiesSnap().playerType; }
      static get screenColor() { return capabilitiesSnap().screenColor; }
      static get screenDPI() { return capabilitiesSnap().screenDPI; }
      static get screenResolutionX() { return capabilitiesSnap().screenResolutionX; }
      static get screenResolutionY() { return capabilitiesSnap().screenResolutionY; }
      static get serverString() { return capabilitiesSnap().serverString; }
      static get supports32BitProcesses() { return capabilitiesSnap().supports32BitProcesses; }
      static get supports64BitProcesses() { return capabilitiesSnap().supports64BitProcesses; }
      static get touchscreenType() { return capabilitiesSnap().touchscreenType; }
      static get version() { return capabilitiesSnap().version; }
      static hasMultiChannelAudio(type) {
        ensureActive();
        const t = String(type ?? '');
        return ['AAU', 'DFT', 'esso', 'multiChannel'].includes(t) || t.length > 0;
      }
    }
    Object.defineProperties(Capabilities, {
      __nfIs: { value: () => false },
      __nfInitialize: { value: () => { throw new TypeError('Capabilities cannot be subclassed.'); } },
      [Symbol.hasInstance]: { value: () => false },
    });

    const IMEConversionMode = defConstClass('IMEConversionMode', {
      ALPHANUMERIC_FULL: 'ALPHANUMERIC_FULL',
      ALPHANUMERIC_HALF: 'ALPHANUMERIC_HALF',
      CHINESE: 'CHINESE',
      JAPANESE_HIRAGANA: 'JAPANESE_HIRAGANA',
      JAPANESE_KATAKANA_FULL: 'JAPANESE_KATAKANA_FULL',
      JAPANESE_KATAKANA_HALF: 'JAPANESE_KATAKANA_HALF',
      KOREAN: 'KOREAN',
      UNKNOWN: 'UNKNOWN',
    });

    let imeEnabled = true;
    let imeConversionMode = 'ALPHANUMERIC_HALF';
    let imeCompositionText = '';
    let imeSelectionStart = 0;
    let imeSelectionEnd = 0;

    class IME extends events.EventDispatcher {
      constructor() {
        super();
        registerDispatcher(this);
      }
      static get isSupported() {
        return true;
      }
      static get enabled() {
        ensureActive();
        return imeEnabled;
      }
      static set enabled(value) {
        ensureActive();
        imeEnabled = Boolean(value);
      }
      static get conversionMode() {
        ensureActive();
        return imeConversionMode;
      }
      static set conversionMode(mode) {
        ensureActive();
        const m = String(mode ?? '');
        const valid = [
          'ALPHANUMERIC_FULL',
          'ALPHANUMERIC_HALF',
          'CHINESE',
          'JAPANESE_HIRAGANA',
          'JAPANESE_KATAKANA_FULL',
          'JAPANESE_KATAKANA_HALF',
          'KOREAN',
          'UNKNOWN',
        ];
        if (!valid.includes(m)) {
          const err = new Error(`ArgumentError: Error #2004: Invalid IMEConversionMode '${m}'.`);
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        imeConversionMode = m;
      }
      static setCompositionString(composition) {
        ensureActive();
        if (composition === null || composition === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter composition must be non-null.');
        }
        imeCompositionText = String(composition);
      }
      static compositionSelectionChanged(start, end) {
        ensureActive();
        imeSelectionStart = Number(start) | 0;
        imeSelectionEnd = Number(end) | 0;
      }
      static compositionAbandoned() {
        ensureActive();
        imeCompositionText = '';
        imeSelectionStart = 0;
        imeSelectionEnd = 0;
      }
      static doConversion() {
        ensureActive();
        if (!imeEnabled) return;
        const text = imeCompositionText;
        dispatchNow(imeSingleton, makeIMEEvent('imeComposition', text));
      }
    }
    const imeSingleton = new IME();
    for (const methodName of ['addEventListener', 'removeEventListener', 'dispatchEvent', 'hasEventListener', 'willTrigger']) {
      Object.defineProperty(IME, methodName, {
        configurable: true,
        writable: true,
        value: (...args) => imeSingleton[methodName](...args),
      });
    }
    tagType(IME, 'IME', v => v === imeSingleton);

    const securityDomains = new WeakSet();
    let currentSecurityDomain = null;
    class SecurityDomain {
      constructor() {
        ensureActive();
        securityDomains.add(this);
      }
      static get currentDomain() {
        ensureActive();
        if (!currentSecurityDomain) currentSecurityDomain = new SecurityDomain();
        return currentSecurityDomain;
      }
    }
    tagType(SecurityDomain, 'SecurityDomain', v => securityDomains.has(v));

    const SecurityPanel = defConstClass('SecurityPanel', {
      DEFAULT: 'default',
      PRIVACY: 'privacy',
      LOCAL_STORAGE: 'localStorage',
      MICROPHONE: 'microphone',
      CAMERA: 'camera',
      DISPLAY: 'display',
      SETTINGS_MANAGER: 'settingsManager',
    });

    let securityExactSettings = true;
    const allowedSecurityDomains = new Set();
    const loadedPolicyFiles = [];

    class Security {
      constructor() {
        throw new TypeError('Security is a static class and cannot be instantiated.');
      }
      static get REMOTE() { return 'remote'; }
      static get LOCAL_WITH_FILE() { return 'localWithFile'; }
      static get LOCAL_WITH_NETWORK() { return 'localWithNetwork'; }
      static get LOCAL_TRUSTED() { return 'localTrusted'; }
      static get APPLICATION() { return 'application'; }
      static get sandboxType() { return 'localTrusted'; }
      static get exactSettings() { return securityExactSettings; }
      static set exactSettings(value) { securityExactSettings = Boolean(value); }
      static get pageDomain() { return resolvePageDomain(); }
      static allowDomain(...domains) {
        ensureActive();
        for (const d of domains) allowedSecurityDomains.add(String(d));
      }
      static allowInsecureDomain(...domains) {
        Security.allowDomain(...domains);
      }
      static loadPolicyFile(url) {
        ensureActive();
        if (url === null || url === undefined) throw new TypeError('Parameter url must be non-null.');
        loadedPolicyFiles.push(String(url));
      }
      static showSettings(panel = 'default') {
        ensureActive();
        const _p = String(panel ?? 'default');
      }
    }
    Object.defineProperties(Security, {
      __nfIs: { value: () => false },
      __nfInitialize: { value: () => { throw new TypeError('Security cannot be subclassed.'); } },
      [Symbol.hasInstance]: { value: () => false },
    });

    const loaderContexts = new WeakSet();
    class LoaderContext {
      constructor(checkPolicyFile = false, applicationDomain = null, securityDomain = null) {
        ensureActive();
        this.checkPolicyFile = Boolean(checkPolicyFile);
        this.applicationDomain = applicationDomain || null;
        this.securityDomain = securityDomain || null;
        this.allowCodeImport = true;
        this.imageDecodingPolicy = 'onDemand';
        this.parameters = null;
        this.requestedContentParent = null;
        loaderContexts.add(this);
      }
    }
    tagType(LoaderContext, 'LoaderContext', v => loaderContexts.has(v));

    const jpegLoaderContexts = new WeakSet();
    class JPEGLoaderContext extends LoaderContext {
      constructor(deblockingFilter = 0.0, checkPolicyFile = false, applicationDomain = null, securityDomain = null) {
        super(checkPolicyFile, applicationDomain, securityDomain);
        this.deblockingFilter = Number(deblockingFilter);
        jpegLoaderContexts.add(this);
      }
    }
    tagType(JPEGLoaderContext, 'JPEGLoaderContext', v => jpegLoaderContexts.has(v));

    let systemUseCodePage = false;
    class System {
      constructor() {
        throw new TypeError('System cannot be instantiated.');
      }
      static get ime() {
        ensureActive();
        return imeSingleton;
      }
      static get totalMemory() {
        return System.totalMemoryNumber >>> 0;
      }
      static get totalMemoryNumber() {
        ensureActive();
        return 16 * 1024 * 1024;
      }
      static get freeMemory() {
        ensureActive();
        return 4 * 1024 * 1024;
      }
      static get privateMemory() {
        ensureActive();
        return 24 * 1024 * 1024;
      }
      static get processCPUUsage() {
        ensureActive();
        return 0.05;
      }
      static get vmVersion() {
        ensureActive();
        return '2.1 neoflash';
      }
      static get useCodePage() {
        return systemUseCodePage;
      }
      static set useCodePage(value) {
        systemUseCodePage = Boolean(value);
      }
      static setClipboard(string) {
        ensureActive();
        if (string === null || string === undefined) {
          throw new TypeError('TypeError: Error #2007: Parameter string must be non-null.');
        }
        const clip = Clipboard.generalClipboard;
        clip.setData(ClipboardFormats.TEXT_FORMAT, String(string));
      }
      static disposeXML(node) {
        ensureActive();
        if (node === null || node === undefined) return;
        const host = xmlHost || getBuiltins()?.__nfXML;
        if (typeof host?.disposeXML === 'function') {
          host.disposeXML(node);
        }
      }
      static gc() {
        ensureActive();
      }
      static pauseForGCIfCollectionImminent(imminence = 0.75) {
        ensureActive();
        const _i = Number(imminence);
      }
      static pause() {
        ensureActive();
      }
      static resume() {
        ensureActive();
      }
      static exit(code) {
        ensureActive();
        const _c = Number(code) >>> 0;
      }
    }
    Object.defineProperties(System, {
      __nfIs: { value: () => false },
      __nfInitialize: { value: () => { throw new TypeError('System cannot be subclassed.'); } },
      [Symbol.hasInstance]: { value: () => false },
    });

    const SystemUpdaterType = defConstClass('SystemUpdaterType', {
      SYSTEM: 'system',
      DRM: 'drm',
    });

    const systemUpdaters = new WeakMap();
    class SystemUpdater extends events.EventDispatcher {
      constructor() {
        super();
        ensureActive();
        registerDispatcher(this);
        systemUpdaters.set(this, { generation, activeOp: 0 });
      }
      update(type) {
        const rec = suState(this);
        const t = String(type ?? '');
        if (t !== 'system' && t !== 'drm') {
          const err = new Error(`ArgumentError: Error #2004: Invalid SystemUpdaterType '${t}'.`);
          err.name = 'ArgumentError';
          err.errorID = 2004;
          throw err;
        }
        const opId = ++rec.activeOp;
        dispatchLater(this, () => {
          if (rec.activeOp !== opId) return null;
          dispatchNow(this, makeBasicEvent('open'));
          dispatchNow(this, makeProgressEvent('progress', 100, 100));
          return makeBasicEvent('complete');
        });
      }
      cancel() {
        const rec = suState(this);
        rec.activeOp++;
        dispatchLater(this, () => makeBasicEvent('cancel'));
      }
    }
    function suState(instance) {
      ensureActive();
      const rec = systemUpdaters.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid SystemUpdater instance.');
      return rec;
    }
    tagType(SystemUpdater, 'SystemUpdater', v => systemUpdaters.has(v));

    // Worker, WorkerDomain, WorkerState, MessageChannel, MessageChannelState
    const WorkerState = defConstClass('WorkerState', {
      NEW: 'new',
      RUNNING: 'running',
      TERMINATED: 'terminated',
    });

    const MessageChannelState = defConstClass('MessageChannelState', {
      OPEN: 'open',
      CLOSING: 'closing',
      CLOSED: 'closed',
    });

    const messageChannels = new WeakMap();
    const activeNativeChannels = new Set();
    function cloneWorkerSharedValue(arg) {
      if (arg === null || arg === undefined || typeof arg !== 'object') return arg;
      if (binary?.ByteArray?.__nfIs?.(arg)) {
        return arg.shareable ? arg : binary.fromBytes(binary.toBytes(arg));
      }
      if (mutexMap.has(arg) || conditionMap.has(arg) || messageChannels.has(arg)) {
        return arg;
      }
      if (amf?.encode && amf?.decode) {
        try {
          return amf.decode(amf.encode(arg, 3), 3, { complete: true }).value;
        } catch (_) {}
      }
      return arg;
    }
    class MessageChannel extends events.EventDispatcher {
      constructor(id, senderWorker, receiverWorker) {
        super();
        ensureActive();
        registerDispatcher(this);
        let nativeChannel = null;
        if (typeof global.MessageChannel === 'function') {
          try {
            nativeChannel = new global.MessageChannel();
            nativeChannel.port1?.unref?.();
            nativeChannel.port2?.unref?.();
            activeNativeChannels.add(nativeChannel);
          } catch (_) {}
        }
        messageChannels.set(this, {
          generation,
          id,
          sender: senderWorker,
          receiver: receiverWorker,
          state: 'open',
          queue: [],
          queueLimit: -1,
          nativeChannel,
        });
      }
      get state() {
        return mcState(this).state;
      }
      get messageAvailable() {
        return mcState(this).queue.length > 0;
      }
      send(arg, queueLimit = -1) {
        const rec = mcState(this);
        const res = execKernel({
          op: 'worker_op',
          action: 'channel_send',
          id: rec.id,
          queueLimit: Number(queueLimit),
        });
        const cloned = cloneWorkerSharedValue(arg);
        rec.queue.push(cloned);
        rec.state = res.state;
        dispatchLater(this, () => makeBasicEvent('channelMessage'));
      }
      receive(blockUntilReceived = false) {
        const rec = mcState(this);
        const _block = Boolean(blockUntilReceived);
        if (rec.queue.length === 0) return null;
        const item = rec.queue.shift();
        const res = execKernel({ op: 'worker_op', action: 'channel_receive', id: rec.id });
        const prevState = rec.state;
        rec.state = res.state;
        if (prevState !== rec.state) {
          dispatchLater(this, () => makeBasicEvent('channelState'));
        }
        return item;
      }
      close() {
        const rec = mcState(this);
        const prev = rec.state;
        const res = execKernel({ op: 'worker_op', action: 'channel_close', id: rec.id });
        rec.state = res.state;
        if (rec.nativeChannel) {
          try {
            rec.nativeChannel.port1?.close?.();
            rec.nativeChannel.port2?.close?.();
          } catch (_) {}
          activeNativeChannels.delete(rec.nativeChannel);
          rec.nativeChannel = null;
        }
        if (prev !== rec.state) {
          dispatchLater(this, () => makeBasicEvent('channelState'));
        }
      }
      toString() {
        mcState(this);
        return '[object MessageChannel]';
      }
    }
    function mcState(instance) {
      ensureActive();
      const rec = messageChannels.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid MessageChannel instance.');
      return rec;
    }
    tagType(MessageChannel, 'MessageChannel', v => messageChannels.has(v));

    const workersMap = new WeakMap();
    const workersById = new Map();
    let currentWorkerId = 1;
    let primordialWorker = null;

    class Worker extends events.EventDispatcher {
      constructor(id, isPrimordial = false, initialState = 'new') {
        super();
        ensureActive();
        registerDispatcher(this);
        const rec = {
          generation,
          id,
          isPrimordial: Boolean(isPrimordial),
          state: initialState,
          sharedProps: new Map(),
        };
        workersMap.set(this, rec);
        workersById.set(id, this);
      }
      static get isSupported() {
        return true;
      }
      static get current() {
        ensureActive();
        if (!primordialWorker || workersMap.get(primordialWorker)?.generation !== generation) {
          primordialWorker = new Worker(1, true, 'running');
        }
        return primordialWorker;
      }
      get isPrimordial() {
        return wkState(this).isPrimordial;
      }
      get state() {
        return wkState(this).state;
      }
      setSharedProperty(key, value) {
        const rec = wkState(this);
        if (key === null || key === undefined) {
          throw new TypeError('TypeError: Error #2007: Worker.setSharedProperty key must be non-null.');
        }
        rec.sharedProps.set(String(key), cloneWorkerSharedValue(value));
      }
      getSharedProperty(key) {
        const rec = wkState(this);
        if (key === null || key === undefined) {
          throw new TypeError('TypeError: Error #2007: Worker.getSharedProperty key must be non-null.');
        }
        const stored = rec.sharedProps.get(String(key));
        return cloneWorkerSharedValue(stored);
      }
      createMessageChannel(receiver) {
        const rec = wkState(this);
        if (!receiver || !workersMap.has(receiver)) {
          throw new TypeError('TypeError: Error #2007: Parameter receiver must be a non-null Worker.');
        }
        const recvRec = wkState(receiver);
        const created = execKernel({
          op: 'worker_op',
          action: 'create_channel',
          senderId: rec.id,
          receiverId: recvRec.id,
        });
        return new MessageChannel(created.id, this, receiver);
      }
      start() {
        const rec = wkState(this);
        const res = execKernel({ op: 'worker_op', action: 'start', id: rec.id });
        rec.state = res.state;
        dispatchLater(this, () => makeBasicEvent('workerState'));
      }
      terminate() {
        const rec = wkState(this);
        const res = execKernel({ op: 'worker_op', action: 'terminate', id: rec.id });
        const changed = rec.state !== res.state;
        rec.state = res.state;
        if (changed) {
          dispatchLater(this, () => makeBasicEvent('workerState'));
        }
        return Boolean(res.terminated);
      }
    }
    function wkState(instance) {
      ensureActive();
      const rec = workersMap.get(instance);
      if (!rec || rec.generation !== generation) throw new TypeError('Expected a valid Worker instance.');
      return rec;
    }
    tagType(Worker, 'Worker', v => workersMap.has(v));

    const workerDomains = new WeakSet();
    let currentWorkerDomain = null;
    class WorkerDomain {
      constructor() {
        ensureActive();
        workerDomains.add(this);
      }
      static get isSupported() {
        return true;
      }
      static get current() {
        ensureActive();
        if (!currentWorkerDomain) currentWorkerDomain = new WorkerDomain();
        return currentWorkerDomain;
      }
      createWorker(swf, giveAppPrivileges = false) {
        ensureActive();
        if (! workerDomains.has(this)) throw new TypeError('Expected a valid WorkerDomain instance.');
        if (!swf || !binary?.ByteArray?.__nfIs?.(swf)) {
          throw new TypeError('TypeError: Error #2007: WorkerDomain.createWorker requires a non-null swf ByteArray.');
        }
        const _priv = Boolean(giveAppPrivileges);
        const created = execKernel({ op: 'worker_op', action: 'create_worker' });
        return new Worker(created.id, false, created.state);
      }
      listWorkers() {
        ensureActive();
        if (!workerDomains.has(this)) throw new TypeError('Expected a valid WorkerDomain instance.');
        // Ensure primordial worker is registered in JS map
        Worker.current;
        const ids = execKernel({ op: 'worker_op', action: 'list_running' }).workerIds || [];
        const list = ids.map(id => workersById.get(id)).filter(Boolean);
        return makeVector('flash.system.Worker', list);
      }
    }
    tagType(WorkerDomain, 'WorkerDomain', v => workerDomains.has(v));

    function isDataInput(value) {
      return socketMap.has(value) || urlStreamMap.has(value);
    }

    function isDataOutput(value) {
      return socketMap.has(value);
    }

    function setMockBrowseSelection(spec) {
      mockBrowseSelection = spec;
    }

    function inspectState(instance) {
      return (
        fileRefMap.get(instance) ||
        fileRefListMap.get(instance) ||
        printJobs.get(instance) ||
        appDomains.get(instance) ||
        localConnections.get(instance) ||
        messageChannels.get(instance) ||
        workersMap.get(instance) ||
        null
      );
    }

    function reset() {
      if (disposed) return;
      generation++;
      for (const bc of activeBroadcastChannels) {
        try {
          bc.close();
        } catch (_) {}
      }
      activeBroadcastChannels.clear();
      for (const ch of activeNativeChannels) {
        try {
          ch.port1?.close?.();
          ch.port2?.close?.();
        } catch (_) {}
      }
      activeNativeChannels.clear();
      kernel.reset();
      hub.clear();
      netGroupsById.clear();
      lcById.clear();
      activeNetMonitors.clear();
      telemetryHandlers.clear();
      workersById.clear();
      allowedSecurityDomains.clear();
      loadedPolicyFiles.length = 0;
      generalClipboardInstance = null;
      drmManagerSingleton = null;
      drmNetworkIdleTimeout = 120000;
      currentAppDomain = null;
      currentSecurityDomain = null;
      currentWorkerDomain = null;
      primordialWorker = null;
      currentWorkerId = 1;
      ncDefaultObjectEncoding = 3;
      imeEnabled = true;
      imeConversionMode = 'ALPHANUMERIC_HALF';
      imeCompositionText = '';
      systemUseCodePage = false;
      securityExactSettings = true;
      mockBrowseSelection = null;
      registerDispatcher(imeSingleton);
    }

    function dispose() {
      if (disposed) return;
      reset();
      disposed = true;
      kernel.free?.();
      kernel = null;
    }

    return Object.freeze({
      // flash.concurrent
      Mutex,
      Condition,
      // flash.crypto
      generateRandomBytes,
      // flash.desktop
      Clipboard,
      ClipboardFormats,
      ClipboardTransferMode,
      // flash.globalization
      LastOperationStatus,
      NationalDigitsType,
      CollatorMode,
      LocaleID,
      CurrencyParseResult,
      CurrencyFormatter,
      Collator,
      StringTools,
      // flash.media
      scanHardware,
      // flash.net
      DynamicPropertyOutput,
      Responder,
      FileFilter,
      FileReference,
      FileReferenceList,
      GroupSpecifier,
      NetGroupReceiveMode,
      NetGroupReplicationStrategy,
      NetGroupSendMode,
      NetGroupSendResult,
      NetGroupInfo,
      NetGroup,
      LocalConnection,
      NetStreamAppendBytesAction,
      NetStreamPlayTransitions,
      NetStreamPlayOptions,
      NetStreamInfo,
      NetStreamMulticastInfo,
      NetMonitor,
      Socket,
      SecureSocket,
      URLStream,
      XMLSocket,
      // flash.net.drm
      AuthenticationMethod,
      LoadVoucherSetting,
      VoucherAccessInfo,
      DRMPlaybackTimeWindow,
      DRMVoucher,
      DRMContentData,
      DRMDeviceGroup,
      DRMManager,
      // flash.printing
      PrintJob,
      PrintJobOptions,
      PrintJobOrientation,
      // flash.profiler
      showRedrawRegions,
      Telemetry,
      // flash.sampler
      StackFrame,
      Sample,
      NewObjectSample,
      DeleteObjectSample,
      clearSamples,
      getGetterInvocationCount,
      getInvocationCount,
      getMemberNames,
      getSampleCount,
      getSamples,
      getSetterInvocationCount,
      getSize,
      isGetterSetter,
      pauseSampling,
      startSampling,
      stopSampling,
      // flash.security
      CertificateStatus,
      X500DistinguishedName,
      X509Certificate,
      // flash.sensors
      Accelerometer,
      // flash.system
      fscommand,
      ApplicationDomain,
      Capabilities,
      IME,
      IMEConversionMode,
      LoaderContext,
      JPEGLoaderContext,
      MessageChannel,
      MessageChannelState,
      Security,
      SecurityDomain,
      SecurityPanel,
      System,
      SystemUpdater,
      SystemUpdaterType,
      TouchscreenType,
      Worker,
      WorkerDomain,
      WorkerState,
      // Host hooks
      builtins: Object.freeze({
        'flash.concurrent.Mutex': Mutex,
        'flash.concurrent.Condition': Condition,
        'flash.crypto.generateRandomBytes': generateRandomBytes,
        'flash.desktop.Clipboard': Clipboard,
        'flash.desktop.ClipboardFormats': ClipboardFormats,
        'flash.desktop.ClipboardTransferMode': ClipboardTransferMode,
        'flash.globalization.LastOperationStatus': LastOperationStatus,
        'flash.globalization.NationalDigitsType': NationalDigitsType,
        'flash.globalization.CollatorMode': CollatorMode,
        'flash.globalization.LocaleID': LocaleID,
        'flash.globalization.CurrencyParseResult': CurrencyParseResult,
        'flash.globalization.CurrencyFormatter': CurrencyFormatter,
        'flash.globalization.Collator': Collator,
        'flash.globalization.StringTools': StringTools,
        'flash.media.scanHardware': scanHardware,
        'flash.net.DynamicPropertyOutput': DynamicPropertyOutput,
        'flash.net.Responder': Responder,
        'flash.net.FileFilter': FileFilter,
        'flash.net.FileReference': FileReference,
        'flash.net.FileReferenceList': FileReferenceList,
        'flash.net.GroupSpecifier': GroupSpecifier,
        'flash.net.NetGroupReceiveMode': NetGroupReceiveMode,
        'flash.net.NetGroupReplicationStrategy': NetGroupReplicationStrategy,
        'flash.net.NetGroupSendMode': NetGroupSendMode,
        'flash.net.NetGroupSendResult': NetGroupSendResult,
        'flash.net.NetGroupInfo': NetGroupInfo,
        'flash.net.NetGroup': NetGroup,
        'flash.net.LocalConnection': LocalConnection,
        'flash.net.NetStreamAppendBytesAction': NetStreamAppendBytesAction,
        'flash.net.NetStreamPlayTransitions': NetStreamPlayTransitions,
        'flash.net.NetStreamPlayOptions': NetStreamPlayOptions,
        'flash.net.NetStreamInfo': NetStreamInfo,
        'flash.net.NetStreamMulticastInfo': NetStreamMulticastInfo,
        'flash.net.NetMonitor': NetMonitor,
        'flash.net.Socket': Socket,
        'flash.net.SecureSocket': SecureSocket,
        'flash.net.URLStream': URLStream,
        'flash.net.XMLSocket': XMLSocket,
        'flash.net.drm.AuthenticationMethod': AuthenticationMethod,
        'flash.net.drm.LoadVoucherSetting': LoadVoucherSetting,
        'flash.net.drm.VoucherAccessInfo': VoucherAccessInfo,
        'flash.net.drm.DRMPlaybackTimeWindow': DRMPlaybackTimeWindow,
        'flash.net.drm.DRMVoucher': DRMVoucher,
        'flash.net.drm.DRMContentData': DRMContentData,
        'flash.net.drm.DRMDeviceGroup': DRMDeviceGroup,
        'flash.net.drm.DRMManager': DRMManager,
        'flash.printing.PrintJob': PrintJob,
        'flash.printing.PrintJobOptions': PrintJobOptions,
        'flash.printing.PrintJobOrientation': PrintJobOrientation,
        'flash.profiler.showRedrawRegions': showRedrawRegions,
        'flash.profiler.Telemetry': Telemetry,
        'flash.sampler.StackFrame': StackFrame,
        'flash.sampler.Sample': Sample,
        'flash.sampler.NewObjectSample': NewObjectSample,
        'flash.sampler.DeleteObjectSample': DeleteObjectSample,
        'flash.sampler.clearSamples': clearSamples,
        'flash.sampler.getGetterInvocationCount': getGetterInvocationCount,
        'flash.sampler.getInvocationCount': getInvocationCount,
        'flash.sampler.getMemberNames': getMemberNames,
        'flash.sampler.getSampleCount': getSampleCount,
        'flash.sampler.getSamples': getSamples,
        'flash.sampler.getSetterInvocationCount': getSetterInvocationCount,
        'flash.sampler.getSize': getSize,
        'flash.sampler.isGetterSetter': isGetterSetter,
        'flash.sampler.pauseSampling': pauseSampling,
        'flash.sampler.startSampling': startSampling,
        'flash.sampler.stopSampling': stopSampling,
        'flash.security.CertificateStatus': CertificateStatus,
        'flash.security.X500DistinguishedName': X500DistinguishedName,
        'flash.security.X509Certificate': X509Certificate,
        'flash.sensors.Accelerometer': Accelerometer,
        'flash.system.fscommand': fscommand,
        'flash.system.ApplicationDomain': ApplicationDomain,
        'flash.system.Capabilities': Capabilities,
        'flash.system.IME': IME,
        'flash.system.IMEConversionMode': IMEConversionMode,
        'flash.system.LoaderContext': LoaderContext,
        'flash.system.JPEGLoaderContext': JPEGLoaderContext,
        'flash.system.MessageChannel': MessageChannel,
        'flash.system.MessageChannelState': MessageChannelState,
        'flash.system.Security': Security,
        'flash.system.SecurityDomain': SecurityDomain,
        'flash.system.SecurityPanel': SecurityPanel,
        'flash.system.System': System,
        'flash.system.SystemUpdater': SystemUpdater,
        'flash.system.SystemUpdaterType': SystemUpdaterType,
        'flash.system.TouchscreenType': TouchscreenType,
        'flash.system.Worker': Worker,
        'flash.system.WorkerDomain': WorkerDomain,
        'flash.system.WorkerState': WorkerState,
      }),
      hookNumberFormatter,
      setVectorFactory,
      isDataInput,
      isDataOutput,
      setMockBrowseSelection,
      inspectState,
      reset,
      dispose,
    });
  }

  const api = Object.freeze({ create });
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else global.NeoFlashAS3NetSys = api;
})(globalThis);
