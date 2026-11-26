/* Stage/input adapter for the shared event hub. Logical dimensions are supplied by the host;
 * browser preview zoom must not silently change stage coordinates. Fullscreen, IME, touch
 * gestures, Stage3D and Flash-specific keyboard defaults remain outside this adapter.
 */
(function (global) {
  'use strict';
  const eventsAPI = global.NeoFlashEvents || (typeof require === 'function' ? require('./events.js') : null);
  const stageToken = Symbol('stage host construction');
  const stages = new WeakSet();
  class Stage extends (eventsAPI?.EventDispatcher || class {}) {
    constructor(token) {
      if (token !== stageToken) throw new TypeError('Stage is created by the player. Use a display object\'s stage property.');
      super(); stages.add(this);
    }
  }
  Stage.__nfIs = value => stages.has(value);
  Stage.__nfInitialize = receiver => {
    if (!stages.has(receiver)) throw new TypeError('Stage cannot be constructed or subclassed by project code.');
  };
  function argumentError(message) { const error = new RangeError(message); error.name = 'ArgumentError'; return error; }
  function securityError(message) { const error = new Error(message); error.name = 'SecurityError'; return error; }
  function illegalOperationError(message = 'Error #2071: The Stage class does not implement this property or method.') {
    const error = new Error(message); error.name = 'IllegalOperationError'; error.errorID = 2071; return error;
  }
  let cursorVectorFactory = values => Array.isArray(values) ? values.slice() : Array.from(values || []);
  let cursorPointFactory = (x = 0, y = 0) => ({ x, y });
  let isBitmapData = value => Boolean(value && typeof value === 'object' && typeof value.width === 'number' && typeof value.height === 'number');
  let isPoint = value => Boolean(value && typeof value === 'object' && Number.isFinite(value.x) && Number.isFinite(value.y));
  let isRectangle = value => Boolean(value && typeof value === 'object' && ['x', 'y', 'width', 'height'].every(k => Number.isFinite(value[k])));
  let makeRectangle = (x, y, width, height) => ({ x, y, width, height });
  let encodeCursorPngAdapter = null;
  function setStageAdapters(adapters = {}) {
    if (typeof adapters.cursorVector === 'function') cursorVectorFactory = adapters.cursorVector;
    if (typeof adapters.makePoint === 'function') cursorPointFactory = adapters.makePoint;
    if (typeof adapters.isBitmapData === 'function') isBitmapData = adapters.isBitmapData;
    if (typeof adapters.isPoint === 'function') isPoint = adapters.isPoint;
    if (typeof adapters.isRectangle === 'function') isRectangle = adapters.isRectangle;
    if (typeof adapters.makeRectangle === 'function') makeRectangle = adapters.makeRectangle;
    if (typeof adapters.encodeCursorPng === 'function') encodeCursorPngAdapter = adapters.encodeCursorPng;
  }
  const CRC32_TABLE = (() => {
    const t = new Uint32Array(256);
    for (let i = 0; i < 256; i++) {
      let c = i;
      for (let k = 0; k < 8; k++) c = (c & 1) ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1);
      t[i] = c >>> 0;
    }
    return t;
  })();
  function crc32Bytes(buf, start = 0, end = buf.length) {
    let c = 0xffffffff;
    for (let i = start; i < end; i++) c = CRC32_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  }
  function adler32Bytes(buf) {
    let a = 1, b = 0;
    for (let i = 0; i < buf.length; i++) {
      a = (a + buf[i]) % 65521;
      b = (b + a) % 65521;
    }
    return ((b << 16) | a) >>> 0;
  }
  function encodeRawRgbaPng(width, height, getPixel32) {
    const w = Math.max(1, Math.min(128, Math.trunc(Number(width)) || 1));
    const h = Math.max(1, Math.min(128, Math.trunc(Number(height)) || 1));
    const rawLen = h * (1 + w * 4);
    const raw = new Uint8Array(rawLen);
    let pos = 0;
    for (let y = 0; y < h; y++) {
      raw[pos++] = 0;
      for (let x = 0; x < w; x++) {
        const argb = typeof getPixel32 === 'function' ? (Number(getPixel32(x, y)) >>> 0) : 0xff000000;
        raw[pos++] = (argb >>> 16) & 0xff;
        raw[pos++] = (argb >>> 8) & 0xff;
        raw[pos++] = argb & 0xff;
        raw[pos++] = (argb >>> 24) & 0xff;
      }
    }
    const blocks = Math.max(1, Math.ceil(rawLen / 65535));
    const zlen = 2 + blocks * 5 + rawLen + 4;
    const z = new Uint8Array(zlen);
    let zp = 0;
    z[zp++] = 0x78; z[zp++] = 0x01;
    for (let b = 0, off = 0; b < blocks; b++) {
      const rem = rawLen - off, chunk = Math.min(65535, rem), last = off + chunk >= rawLen ? 1 : 0;
      z[zp++] = last;
      z[zp++] = chunk & 0xff; z[zp++] = (chunk >>> 8) & 0xff;
      const nlen = (~chunk) & 0xffff;
      z[zp++] = nlen & 0xff; z[zp++] = (nlen >>> 8) & 0xff;
      z.set(raw.subarray(off, off + chunk), zp);
      zp += chunk; off += chunk;
    }
    const adler = adler32Bytes(raw);
    z[zp++] = (adler >>> 24) & 0xff; z[zp++] = (adler >>> 16) & 0xff; z[zp++] = (adler >>> 8) & 0xff; z[zp++] = adler & 0xff;
    const out = new Uint8Array(8 + 25 + (12 + zlen) + 12);
    const view = new DataView(out.buffer);
    out.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 0);
    let p = 8;
    const writeChunk = (fourcc, payload) => {
      view.setUint32(p, payload.length); p += 4;
      const typeStart = p;
      for (let i = 0; i < 4; i++) out[p++] = fourcc.charCodeAt(i);
      out.set(payload, p); p += payload.length;
      view.setUint32(p, crc32Bytes(out, typeStart, p)); p += 4;
    };
    const ihdr = new Uint8Array(13);
    const iv = new DataView(ihdr.buffer);
    iv.setUint32(0, w); iv.setUint32(4, h); ihdr[8] = 8; ihdr[9] = 6;
    writeChunk('IHDR', ihdr);
    writeChunk('IDAT', z);
    writeChunk('IEND', new Uint8Array(0));
    return out;
  }
  function bytesToBase64(bytes) {
    const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes || []);
    if (typeof Buffer === 'function') return Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength).toString('base64');
    let bin = '';
    for (let i = 0; i < u8.length; i++) bin += String.fromCharCode(u8[i]);
    return typeof global.btoa === 'function' ? global.btoa(bin) : '';
  }
  function encodeCursorFrameCss(bmp, hotSpot) {
    const w = Math.max(1, Math.min(128, Math.trunc(Number(bmp?.width)) || 32));
    const h = Math.max(1, Math.min(128, Math.trunc(Number(bmp?.height)) || 32));
    const hx = Math.max(0, Math.min(w - 1, Math.round(Number(hotSpot?.x) || 0)));
    const hy = Math.max(0, Math.min(h - 1, Math.round(Number(hotSpot?.y) || 0)));
    let pngBytes = null;
    if (encodeCursorPngAdapter) {
      try { pngBytes = encodeCursorPngAdapter(bmp); } catch (_) { pngBytes = null; }
    }
    if (!pngBytes) {
      const rgbaBuf = typeof bmp?.getPixelsRgba === 'function'
        ? bmp.getPixelsRgba()
        : ((bmp?.rgba instanceof Uint8Array || bmp?.rgba instanceof Uint8ClampedArray || Array.isArray(bmp?.rgba)) ? bmp.rgba : null);
      const getter = typeof bmp?.getPixel32 === 'function'
        ? (x, y) => bmp.getPixel32(x, y)
        : rgbaBuf
          ? (x, y) => {
              const idx = (y * w + x) * 4;
              return (((rgbaBuf[idx + 3] ?? 255) << 24) | ((rgbaBuf[idx] ?? 0) << 16) | ((rgbaBuf[idx + 1] ?? 0) << 8) | (rgbaBuf[idx + 2] ?? 0)) >>> 0;
            }
          : null;
      pngBytes = encodeRawRgbaPng(w, h, getter);
    }
    return `url("data:image/png;base64,${bytesToBase64(pngBytes)}") ${hx} ${hy}, auto`;
  }
  const mouseCursorRecords = new WeakMap();
  class MouseCursorData {
    constructor() {
      if (new.target !== MouseCursorData) throw new TypeError('MouseCursorData is final and cannot be subclassed.');
      mouseCursorRecords.set(this, { data: null, hotSpot: cursorPointFactory(0, 0), frameRate: 0 });
      return Object.seal(this);
    }
    get data() { return mouseCursorRecords.get(this).data; }
    set data(value) {
      const rec = mouseCursorRecords.get(this);
      if (value === null) { rec.data = null; return; }
      if (!value || typeof value[Symbol.iterator] !== 'function') throw new TypeError('MouseCursorData.data requires a Vector.<BitmapData> or null.');
      const items = Array.from(value);
      for (const item of items) if (!isBitmapData(item)) throw new TypeError('MouseCursorData.data elements must be BitmapData instances.');
      rec.data = cursorVectorFactory(items);
    }
    get hotSpot() {
      const rec = mouseCursorRecords.get(this);
      return cursorPointFactory(rec.hotSpot.x, rec.hotSpot.y);
    }
    set hotSpot(value) {
      if (!isPoint(value)) throw new TypeError('MouseCursorData.hotSpot requires a Point.');
      mouseCursorRecords.get(this).hotSpot = cursorPointFactory(Number(value.x), Number(value.y));
    }
    get frameRate() { return mouseCursorRecords.get(this).frameRate; }
    set frameRate(value) {
      const next = Number(value);
      if (!Number.isFinite(next) || next < 0) throw new RangeError('MouseCursorData.frameRate must be a finite nonnegative Number.');
      mouseCursorRecords.get(this).frameRate = next;
    }
  }
  MouseCursorData.__nfIs = value => mouseCursorRecords.has(value);
  MouseCursorData.__nfInitialize = receiver => {
    if (!mouseCursorRecords.has(receiver)) throw new TypeError('MouseCursorData is final and cannot be subclassed.');
  };
  Object.defineProperty(MouseCursorData, Symbol.hasInstance, { value: MouseCursorData.__nfIs });
  const accessibilityPropRecords = new WeakMap();
  class AccessibilityProperties {
    constructor() {
      const rec = { description: '', forceSimple: false, live: 'off', name: '', noAutoLabeling: false, role: '', shortcut: '', silent: false };
      accessibilityPropRecords.set(this, rec);
    }
    get description() { return accessibilityPropRecords.get(this).description; }
    set description(value) { accessibilityPropRecords.get(this).description = value == null ? '' : String(value); }
    get forceSimple() { return accessibilityPropRecords.get(this).forceSimple; }
    set forceSimple(value) { accessibilityPropRecords.get(this).forceSimple = Boolean(value); }
    get live() { return accessibilityPropRecords.get(this).live; }
    set live(value) {
      const next = value == null ? 'off' : String(value).trim().toLowerCase();
      accessibilityPropRecords.get(this).live = (next === 'polite' || next === 'assertive') ? next : 'off';
    }
    get name() { return accessibilityPropRecords.get(this).name; }
    set name(value) { accessibilityPropRecords.get(this).name = value == null ? '' : String(value); }
    get noAutoLabeling() { return accessibilityPropRecords.get(this).noAutoLabeling; }
    set noAutoLabeling(value) { accessibilityPropRecords.get(this).noAutoLabeling = Boolean(value); }
    get role() { return accessibilityPropRecords.get(this).role; }
    set role(value) { accessibilityPropRecords.get(this).role = value == null ? '' : String(value).trim(); }
    get shortcut() { return accessibilityPropRecords.get(this).shortcut; }
    set shortcut(value) { accessibilityPropRecords.get(this).shortcut = value == null ? '' : String(value); }
    get silent() { return accessibilityPropRecords.get(this).silent; }
    set silent(value) { accessibilityPropRecords.get(this).silent = Boolean(value); }
  }
  AccessibilityProperties.__nfIs = value => accessibilityPropRecords.has(value);
  AccessibilityProperties.__nfInitialize = receiver => {
    if (!accessibilityPropRecords.has(receiver)) {
      accessibilityPropRecords.set(receiver, { description: '', forceSimple: false, live: 'off', name: '', noAutoLabeling: false, role: '', shortcut: '', silent: false });
    }
  };
  Object.defineProperty(AccessibilityProperties, Symbol.hasInstance, { value: AccessibilityProperties.__nfIs });

  const accessibilityImplRecords = new WeakMap();
  function ensureStageAccImpl(receiver) {
    let rec = accessibilityImplRecords.get(receiver);
    if (!rec) {
      rec = { errno: 0, stub: false };
      accessibilityImplRecords.set(receiver, rec);
    }
    return rec;
  }
  class AccessibilityImplementation {
    constructor(...args) {
      if (args.length > 0) {
        const err = new Error('ArgumentError: Error #1063: AccessibilityImplementation constructor expects 0 arguments.');
        err.name = 'ArgumentError';
        err.errorID = 1063;
        throw err;
      }
      ensureStageAccImpl(this);
    }
    get errno() { return ensureStageAccImpl(this).errno; }
    set errno(v) { ensureStageAccImpl(this).errno = Number(v) >>> 0; }
    get stub() { return ensureStageAccImpl(this).stub; }
    set stub(v) { ensureStageAccImpl(this).stub = Boolean(v); }
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
  }
  AccessibilityImplementation.__nfIs = v => v !== null && typeof v === 'object' && (accessibilityImplRecords.has(v) || v instanceof AccessibilityImplementation);
  AccessibilityImplementation.__nfInitialize = (receiver, args = []) => {
    if (args.length > 0) {
      const err = new Error('ArgumentError: Error #1063: AccessibilityImplementation constructor expects 0 arguments.');
      err.name = 'ArgumentError';
      err.errorID = 1063;
      throw err;
    }
    ensureStageAccImpl(receiver);
  };
  Object.defineProperty(AccessibilityImplementation, Symbol.hasInstance, { value: AccessibilityImplementation.__nfIs });

  function msaaRoleToAria(role) {
    if (typeof global.NeoFlashAccessibility?.msaaRoleToAria === 'function') {
      return global.NeoFlashAccessibility.msaaRoleToAria(role);
    }
    switch (Number(role) >>> 0) {
      case 0x09: case 0x10: return 'region';
      case 0x0a: case 0x14: return 'group';
      case 0x0b: return 'menu';
      case 0x0c: return 'menuitem';
      case 0x0d: return 'tooltip';
      case 0x0e: return 'application';
      case 0x0f: return 'document';
      case 0x12: return 'dialog';
      case 0x15: return 'separator';
      case 0x16: return 'toolbar';
      case 0x17: return 'status';
      case 0x18: return 'table';
      case 0x19: return 'columnheader';
      case 0x1a: return 'rowheader';
      case 0x1c: return 'row';
      case 0x1d: return 'cell';
      case 0x1e: return 'link';
      case 0x21: return 'listbox';
      case 0x22: return 'option';
      case 0x23: return 'tree';
      case 0x24: return 'treeitem';
      case 0x25: return 'tab';
      case 0x26: return 'tabpanel';
      case 0x28: return 'img';
      case 0x29: return 'note';
      case 0x2a: return 'textbox';
      case 0x2b: return 'button';
      case 0x2c: return 'checkbox';
      case 0x2d: return 'radio';
      case 0x2e: return 'combobox';
      case 0x30: return 'progressbar';
      case 0x33: return 'slider';
      case 0x34: return 'spinbutton';
      case 0x3c: return 'tablist';
      default: return 'group';
    }
  }

  function decodeMsaaState(state, ariaRole = '') {
    if (typeof global.NeoFlashAccessibility?.decodeMsaaState === 'function') {
      return global.NeoFlashAccessibility.decodeMsaaState(state, ariaRole);
    }
    const bits = Number(state) >>> 0;
    const r = String(ariaRole || '').toLowerCase();
    const selectable = (bits & 0x00200000) !== 0 || ['option', 'tab', 'treeitem', 'row', 'cell'].includes(r);
    return {
      disabled: (bits & 0x00000001) !== 0,
      selected: (bits & 0x00000002) !== 0 ? true : (selectable ? false : null),
      focused: (bits & 0x00000004) !== 0,
      pressed: (bits & 0x00000008) !== 0,
      checked: (bits & 0x00000020) !== 0 ? 'mixed' : ((bits & 0x00000010) !== 0 ? 'true' : (['checkbox', 'radio', 'menuitemcheckbox', 'menuitemradio'].includes(r) ? 'false' : null)),
      readonly: (bits & 0x00000040) !== 0,
      expanded: (bits & 0x00000200) !== 0 ? true : ((bits & 0x00000400) !== 0 ? false : null),
      invisible: (bits & 0x00008000) !== 0,
      focusable: (bits & 0x00100000) !== 0,
      selectable,
    };
  }

  function extractAccImplDetails(impl, baseX, baseY, baseW, baseH, forceSimple = false, labelCandidates = null) {
    if (!impl || typeof impl !== 'object' || impl.stub) return null;
    let role = null;
    try {
      const r = impl.get_accRole?.(0);
      if (r !== undefined && r !== null && Number.isFinite(Number(r))) role = msaaRoleToAria(Number(r) >>> 0);
    } catch (_) {}
    let state = null;
    try {
      const s = impl.get_accState?.(0);
      if (s !== undefined && s !== null && Number.isFinite(Number(s))) state = decodeMsaaState(Number(s) >>> 0, role || 'group');
    } catch (_) {}
    let name = null;
    try {
      const n = impl.get_accName?.(0);
      if (typeof n === 'string' && n.length > 0) name = n;
    } catch (_) {}
    if (!name && Array.isArray(labelCandidates) && typeof impl.isLabeledBy === 'function') {
      for (const cand of labelCandidates) {
        if (!cand || typeof cand.text !== 'string' || !cand.text.trim()) continue;
        try {
          const rect = stageAdapters.makeRectangle(Number(cand.x) || 0, Number(cand.y) || 0, Number(cand.width) || 0, Number(cand.height) || 0);
          if (impl.isLabeledBy(rect)) {
            name = cand.text.trim();
            break;
          }
        } catch (_) {}
      }
    }
    let valueText = null;
    try {
      const v = impl.get_accValue?.(0);
      if (typeof v === 'string' && v.length > 0) valueText = v;
    } catch (_) {}
    let defaultAction = null;
    try {
      const d = impl.get_accDefaultAction?.(0);
      if (typeof d === 'string' && d.length > 0) defaultAction = d;
    } catch (_) {}
    let focusChildId = 0;
    try {
      const f = impl.get_accFocus?.();
      if (Number.isFinite(Number(f))) focusChildId = Number(f) >>> 0;
    } catch (_) {}
    const selectedSet = new Set();
    try {
      const sel = impl.get_accSelection?.();
      if (Array.isArray(sel)) {
        for (const id of sel) if (Number.isFinite(Number(id))) selectedSet.add(Number(id) >>> 0);
      }
    } catch (_) {}
    const children = [];
    if (!forceSimple) {
      let childIds = null;
      try { childIds = impl.getChildIDArray?.(); } catch (_) {}
      if (Array.isArray(childIds)) {
        const limit = Math.min(childIds.length, 512);
        for (let i = 0; i < limit; i++) {
          const cid = Number(childIds[i]) >>> 0;
          if (cid === 0) continue;
          let cRole = 'option';
          try {
            const cr = impl.get_accRole?.(cid);
            if (cr !== undefined && cr !== null && Number.isFinite(Number(cr))) cRole = msaaRoleToAria(Number(cr) >>> 0);
          } catch (_) {}
          let cState = decodeMsaaState(0, cRole);
          try {
            const cs = impl.get_accState?.(cid);
            if (cs !== undefined && cs !== null && Number.isFinite(Number(cs))) cState = decodeMsaaState(Number(cs) >>> 0, cRole);
          } catch (_) {}
          if (selectedSet.has(cid)) cState = { ...cState, selected: true, selectable: true };
          if (cState.invisible) continue;
          let cName = '';
          try {
            const cn = impl.get_accName?.(cid);
            if (typeof cn === 'string') cName = cn;
          } catch (_) {}
          let cVal = null;
          try {
            const cv = impl.get_accValue?.(cid);
            if (typeof cv === 'string' && cv.length > 0) cVal = cv;
          } catch (_) {}
          let cDefAct = null;
          try {
            const cd = impl.get_accDefaultAction?.(cid);
            if (typeof cd === 'string' && cd.length > 0) cDefAct = cd;
          } catch (_) {}
          let cx = baseX, cy = baseY, cw = baseW, ch = baseH;
          try {
            const loc = impl.accLocation?.(cid);
            if (loc && typeof loc === 'object' && Number.isFinite(Number(loc.width)) && Number.isFinite(Number(loc.height))) {
              cx = baseX + (Number(loc.x) || 0);
              cy = baseY + (Number(loc.y) || 0);
              cw = Math.max(0, Number(loc.width));
              ch = Math.max(0, Number(loc.height));
            }
          } catch (_) {}
          children.push({
            childID: cid,
            role: cRole,
            ariaLabel: cName || `child-${cid}`,
            valueText: cVal,
            defaultAction: cDefAct,
            ariaDisabled: Boolean(cState.disabled),
            selected: cState.selected,
            checked: cState.checked,
            pressed: Boolean(cState.pressed),
            expanded: cState.expanded,
            readonly: Boolean(cState.readonly),
            selectable: Boolean(cState.selectable),
            focusable: Boolean(cState.focusable || cState.selectable || cDefAct || focusChildId === cid),
            x: cx,
            y: cy,
            width: cw,
            height: ch,
            hooks: {
              onDefaultAction: () => impl.accDoDefaultAction?.(cid),
              onSelect: op => impl.accSelect?.(Number(op) >>> 0, cid),
            },
          });
        }
      }
    }
    return {
      role,
      state,
      name,
      valueText,
      defaultAction,
      focusChildId,
      children,
      hooks: {
        onDefaultAction: () => impl.accDoDefaultAction?.(0),
        onSelect: op => impl.accSelect?.(Number(op) >>> 0, 0),
      },
    };
  }

  function extractAccessibilityEntry(raw, index = 0) {
    const target = raw && typeof raw === 'object' && raw.target ? raw.target : raw;
    const acc = (raw && raw.accessibilityProperties) || (target && target.accessibilityProperties) || null;
    const impl = (raw && raw.accessibilityImplementation) || (target && target.accessibilityImplementation) || null;
    const id = String((raw && raw.id) ?? (target && (target.id ?? target.name)) ?? `node-${index + 1}`);
    const forceSimple = Boolean((raw && raw.forceSimple) ?? (acc && acc.forceSimple) ?? false);
    const x = Number.isFinite(Number(raw && raw.x)) ? Number(raw.x) : (Number.isFinite(Number(target && target.x)) ? Number(target.x) : 0);
    const y = Number.isFinite(Number(raw && raw.y)) ? Number(raw.y) : (Number.isFinite(Number(target && target.y)) ? Number(target.y) : 0);
    const width = Number.isFinite(Number(raw && raw.width)) ? Math.max(0, Number(raw.width)) : (Number.isFinite(Number(target && target.width)) ? Math.max(0, Number(target.width)) : 0);
    const height = Number.isFinite(Number(raw && raw.height)) ? Math.max(0, Number(raw.height)) : (Number.isFinite(Number(target && target.height)) ? Math.max(0, Number(target.height)) : 0);
    const implDetails = extractAccImplDetails(impl, x, y, width, height, forceSimple, raw?.labelCandidates || null);
    const roleRaw = (implDetails && implDetails.role) ?? (raw && raw.role) ?? (acc && acc.role) ?? (target && target.accessibilityRole) ?? '';
    const role = String(roleRaw || '').trim() || 'group';
    const ariaLabelRaw = (implDetails && implDetails.name) || (raw && raw.ariaLabel) || (acc && acc.name) || (target && target.accessibilityName) || (raw && raw.name) || (target && target.name) || '';
    const ariaLabel = String(ariaLabelRaw || '').trim() || id;
    const ariaDescRaw = (raw && (raw.ariaDescription ?? raw.description)) ?? (acc && acc.description) ?? (target && target.accessibilityDescription) ?? (implDetails && implDetails.valueText) ?? '';
    const ariaDescription = String(ariaDescRaw || '').trim();
    const liveRaw = String((raw && (raw.ariaLive ?? raw.live)) ?? (acc && acc.live) ?? (target && target.accessibilityLive) ?? 'off').trim().toLowerCase();
    const ariaLive = (liveRaw === 'polite' || liveRaw === 'assertive') ? liveRaw : 'off';
    const shortcut = String((raw && raw.shortcut) ?? (acc && acc.shortcut) ?? (target && target.accessibilityShortcut) ?? '').trim();
    const silent = Boolean((raw && (raw.silent ?? raw.ariaHidden)) ?? (acc && acc.silent) ?? (implDetails?.state?.invisible) ?? false);
    const disabled = Boolean((raw && (raw.disabled ?? raw.ariaDisabled)) ?? (target && target.enabled === false) ?? (implDetails?.state?.disabled) ?? false);
    const tabEnabled = raw && raw.tabEnabled !== undefined ? Boolean(raw.tabEnabled) : (target && target.tabEnabled !== undefined ? Boolean(target.tabEnabled) : true);
    const rawTab = Number((raw && raw.tabIndex) ?? (target && target.tabIndex));
    const explicitTab = Number.isInteger(rawTab) && rawTab > 0 ? rawTab : null;
    const rawOrder = Number((raw && (raw.readingOrder ?? raw.tabOrder)) ?? explicitTab);
    const explicitOrder = Number.isInteger(rawOrder) && rawOrder > 0 ? rawOrder : null;
    return {
      index,
      id,
      target,
      role,
      ariaLabel,
      ariaDescription,
      valueText: (raw && raw.valueText) ?? (implDetails ? implDetails.valueText : null),
      defaultAction: (raw && raw.defaultAction) ?? (implDetails ? implDetails.defaultAction : null),
      selected: (raw && typeof raw.selected === 'boolean') ? raw.selected : (implDetails?.state ? implDetails.state.selected : null),
      checked: (raw && raw.checked !== undefined) ? raw.checked : (implDetails?.state ? implDetails.state.checked : null),
      pressed: Boolean((raw && raw.pressed) ?? implDetails?.state?.pressed ?? false),
      expanded: (raw && typeof raw.expanded === 'boolean') ? raw.expanded : (implDetails?.state ? implDetails.state.expanded : null),
      readonly: Boolean((raw && raw.readonly) ?? implDetails?.state?.readonly ?? false),
      selectable: Boolean((raw && raw.selectable) ?? implDetails?.state?.selectable ?? false),
      focusChildId: implDetails ? implDetails.focusChildId : 0,
      accChildren: implDetails ? implDetails.children : [],
      hooks: (raw && raw.hooks) || (implDetails ? implDetails.hooks : null),
      ariaLive,
      shortcut,
      ariaHidden: silent,
      ariaDisabled: disabled,
      tabEnabled,
      explicitTab,
      explicitOrder,
      x,
      y,
      width,
      height,
    };
  }

  function validateAccessibilityReadOrder(entries = []) {
    const list = Array.isArray(entries) ? entries.map((item, idx) => extractAccessibilityEntry(item, idx)) : [];
    const byTab = new Map();
    for (const item of list) {
      if (item.ariaHidden || item.explicitTab == null) continue;
      if (!byTab.has(item.explicitTab)) byTab.set(item.explicitTab, []);
      byTab.get(item.explicitTab).push(item.ariaLabel || item.id);
    }
    const diagnostics = [];
    const sortedTabs = Array.from(byTab.keys()).sort((a, b) => a - b);
    for (const tab of sortedTabs) {
      const names = byTab.get(tab);
      if (names.length > 1) {
        diagnostics.push(`Accessibility read-order conflict: tabIndex ${tab} is shared by ${names.join(', ')}.`);
      }
    }
    let expected = 1;
    for (const tab of sortedTabs) {
      if (tab > expected) {
        if (tab - expected === 1) {
          diagnostics.push(`Accessibility read-order gap: missing tabIndex ${expected} before ${tab}.`);
        } else {
          diagnostics.push(`Accessibility read-order gap: missing tabIndex ${expected}..${tab - 1} before ${tab}.`);
        }
      }
      expected = tab + 1;
    }
    return diagnostics;
  }

  function resolveAccessibilityOverlayNodes(entries = [], options = {}) {
    const parsed = Array.isArray(entries) ? entries.map((item, idx) => extractAccessibilityEntry(item, idx)) : [];
    const active = parsed.filter(item => !item.ariaHidden);
    const hidden = parsed.filter(item => item.ariaHidden);
    active.sort((a, b) => {
      const aTab = a.explicitTab != null ? a.explicitTab : Number.MAX_SAFE_INTEGER;
      const bTab = b.explicitTab != null ? b.explicitTab : Number.MAX_SAFE_INTEGER;
      if (aTab !== bTab) return aTab - bTab;
      const aOrd = a.explicitOrder != null ? a.explicitOrder : Number.MAX_SAFE_INTEGER;
      const bOrd = b.explicitOrder != null ? b.explicitOrder : Number.MAX_SAFE_INTEGER;
      if (aOrd !== bOrd) return aOrd - bOrd;
      if (Math.abs(a.y - b.y) > 1e-3) return a.y - b.y;
      if (Math.abs(a.x - b.x) > 1e-3) return a.x - b.x;
      return a.index - b.index;
    });
    const focusedTarget = options && options.focusedTarget ? options.focusedTarget : null;
    const focusReason = options && options.focusReason ? String(options.focusReason) : null;
    const resolved = [];
    let orderCounter = 0;
    active.forEach(item => {
      const readingOrder = ++orderCounter;
      const tabIndex = (item.ariaDisabled || !item.tabEnabled)
        ? -1
        : (item.explicitTab != null ? item.explicitTab : readingOrder);
      const isFocused = Boolean(focusedTarget && (item.target === focusedTarget || item.id === focusedTarget.id || item.id === focusedTarget.name));
      resolved.push({
        id: item.id,
        childID: 0,
        parentId: null,
        target: item.target,
        role: item.role,
        tabIndex,
        readingOrder,
        ariaLabel: item.ariaLabel,
        ariaDescription: item.ariaDescription,
        valueText: item.valueText ?? null,
        defaultAction: item.defaultAction ?? null,
        selected: item.selected ?? null,
        checked: item.checked ?? null,
        pressed: item.pressed,
        expanded: item.expanded ?? null,
        readonly: item.readonly,
        selectable: item.selectable,
        focusChildId: item.focusChildId || 0,
        hooks: item.hooks || null,
        ariaLive: item.ariaLive,
        ariaHidden: false,
        ariaDisabled: item.ariaDisabled,
        shortcut: item.shortcut,
        x: item.x,
        y: item.y,
        width: item.width,
        height: item.height,
        focused: isFocused,
        focusReason: isFocused ? focusReason : null,
        keyboardFocusRing: Boolean(isFocused && focusReason === 'key'),
      });
      if (Array.isArray(item.accChildren) && item.accChildren.length > 0) {
        for (const child of item.accChildren) {
          const childReadingOrder = ++orderCounter;
          const childTab = (child.ariaDisabled || !child.focusable) ? -1 : 0;
          resolved.push({
            id: `${item.id}-child-${child.childID}`,
            childID: child.childID,
            parentId: item.id,
            target: item.target,
            role: child.role,
            tabIndex: childTab,
            readingOrder: childReadingOrder,
            ariaLabel: child.ariaLabel,
            ariaDescription: '',
            valueText: child.valueText ?? null,
            defaultAction: child.defaultAction ?? null,
            selected: child.selected ?? null,
            checked: child.checked ?? null,
            pressed: child.pressed,
            expanded: child.expanded ?? null,
            readonly: child.readonly,
            selectable: child.selectable,
            focusChildId: 0,
            hooks: child.hooks || null,
            ariaLive: 'off',
            ariaHidden: false,
            ariaDisabled: child.ariaDisabled,
            shortcut: '',
            x: child.x,
            y: child.y,
            width: child.width,
            height: child.height,
            focused: Boolean(isFocused && item.focusChildId === child.childID),
            focusReason: isFocused && item.focusChildId === child.childID ? focusReason : null,
            keyboardFocusRing: false,
          });
        }
      }
    });
    hidden.forEach(item => {
      resolved.push({
        id: item.id,
        childID: 0,
        parentId: null,
        target: item.target,
        role: item.role,
        tabIndex: -1,
        readingOrder: 0,
        ariaLabel: item.ariaLabel,
        ariaDescription: item.ariaDescription,
        valueText: null,
        defaultAction: null,
        selected: null,
        checked: null,
        pressed: false,
        expanded: null,
        readonly: false,
        selectable: false,
        focusChildId: 0,
        hooks: null,
        ariaLive: item.ariaLive,
        ariaHidden: true,
        ariaDisabled: item.ariaDisabled,
        shortcut: item.shortcut,
        x: item.x,
        y: item.y,
        width: item.width,
        height: item.height,
        focused: false,
        focusReason: null,
        keyboardFocusRing: false,
      });
    });
    return resolved;
  }

  function createFallbackOverlayDomElement(tagName = 'div') {
    const attrs = new Map();
    const dataset = {};
    const style = {};
    const listeners = new Map();
    return {
      tagName: String(tagName).toUpperCase(),
      tabIndex: -1,
      dataset,
      style,
      setAttribute(name, value) {
        const str = String(value);
        attrs.set(String(name), str);
        if (name === 'tabindex') this.tabIndex = Number(str);
        if (String(name).startsWith('data-')) {
          const camel = String(name).slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
          dataset[camel] = str;
        }
      },
      getAttribute(name) {
        return attrs.has(String(name)) ? attrs.get(String(name)) : null;
      },
      removeAttribute(name) {
        attrs.delete(String(name));
        if (String(name).startsWith('data-')) {
          const camel = String(name).slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
          delete dataset[camel];
        }
      },
      addEventListener(type, fn) {
        const list = listeners.get(type) || [];
        list.push(fn);
        listeners.set(type, list);
      },
      dispatchEvent(ev) {
        for (const fn of listeners.get(ev?.type) || []) fn(ev);
        return !ev?.defaultPrevented;
      },
      click() {
        if (attrs.get('aria-disabled') === 'true') return;
        for (const fn of listeners.get('click') || []) fn({ type: 'click', target: this });
      },
      focus() {
        if (attrs.get('aria-disabled') === 'true' || this.tabIndex < 0) return;
        for (const fn of listeners.get('focus') || []) fn({ type: 'focus', target: this });
      },
    };
  }

  // Keep authored accessibility nodes stable across animation frames. Replacing the complete
  // overlay on every render drops browser focus and forces assistive technology to rediscover
  // otherwise unchanged controls. The target identity still retires a node when a placement's
  // runtime lifetime changes, matching the Stage event/semantic-layer boundary.
  const accessibilityOverlayTargets = new WeakMap();
  const accessibilityOverlayBoundNodes = new WeakMap();

  const CONTENT_ACCESSIBLE_ROLE_MAP = Object.freeze({
    auto: 'generic',
    button: 'button',
    check_box: 'checkbox',
    radio_button: 'radio',
    combo_box: 'combobox',
    list: 'list',
    list_item: 'listitem',
    menu: 'menu',
    menu_item: 'menuitem',
    slider: 'slider',
    progress_bar: 'progressbar',
    tab: 'tab',
    tab_panel: 'tabpanel',
    dialog: 'dialog',
    heading: 'heading',
    link: 'link',
    image: 'img',
    group: 'group',
    region: 'region',
    application: 'application',
    presentation: 'presentation',
  });

  function elementStageBounds(element) {
    const tx = Number(element?.transform?.x ?? 0);
    const ty = Number(element?.transform?.y ?? 0);
    const sx = Math.max(0.001, Math.abs(Number(element?.transform?.scale_x ?? 1)));
    const sy = Math.max(0.001, Math.abs(Number(element?.transform?.scale_y ?? 1)));
    const kind = element?.kind || {};
    let w = 64 * sx;
    let h = 64 * sy;
    if (kind.type === 'rectangle' || kind.type === 'ellipse' || kind.type === 'video' || kind.type === 'text_box' || kind.type === 'bitmap') {
      w = Number(kind.width ?? 0) * sx;
      h = Number(kind.height ?? 0) * sy;
    } else if (kind.type === 'text') {
      const textLen = Math.max(1, Array.from(String(kind.text || '')).length);
      const fontSize = Number(kind.font_size ?? 16);
      w = textLen * (fontSize * 0.6) * sx;
      h = (fontSize * 1.2) * sy;
    } else if (kind.type === 'path' || kind.type === 'gradient_path') {
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (const cmd of (Array.isArray(kind.commands) ? kind.commands : [])) {
        if (Number.isFinite(Number(cmd?.x)) && Number.isFinite(Number(cmd?.y))) {
          minX = Math.min(minX, Number(cmd.x));
          minY = Math.min(minY, Number(cmd.y));
          maxX = Math.max(maxX, Number(cmd.x));
          maxY = Math.max(maxY, Number(cmd.y));
        }
      }
      if (Number.isFinite(minX) && Number.isFinite(maxX)) {
        w = Math.max(1, maxX - minX) * sx;
        h = Math.max(1, maxY - minY) * sy;
      } else {
        w = 24 * sx;
        h = 24 * sy;
      }
    }
    return [tx, ty, Math.max(1, w), Math.max(1, h)];
  }

  function safeGet(obj, key) {
    if (!obj || typeof obj !== 'object') return undefined;
    try { return obj[key]; } catch (_) { return undefined; }
  }

  function collectProjectAccessibilityEntries(project, frame = 0, options = {}) {
    if (!project || typeof project !== 'object' || !Array.isArray(project.symbols)) return [];
    const targetId = (options && options.symbolId) || project.root || 'root';
    const symbolsById = new Map(project.symbols.map(s => [s.id, s]));
    const symbol = symbolsById.get(targetId) || project.symbols[0];
    if (!symbol || !symbol.timeline || !Array.isArray(symbol.timeline.layers)) return [];

    const frameNum = Math.max(0, Number(frame) || 0);
    const resolveTarget = options && typeof options.resolveTarget === 'function' ? options.resolveTarget : null;
    const entries = [];
    const layers = symbol.timeline.layers.slice().reverse();

    for (const layer of layers) {
      if (!layer || layer.visible === false) continue;
      const layerRole = layer.properties?.role || 'normal';
      if (layerRole === 'guide' || layerRole === 'mask' || layerRole === 'folder') continue;
      const keyframes = Array.isArray(layer.keyframes) ? layer.keyframes : [];
      let kf = null;
      for (const candidate of keyframes) {
        const kfFrame = Number(candidate?.frame ?? 0);
        if (kfFrame <= frameNum && (!kf || kfFrame >= Number(kf.frame ?? 0))) {
          kf = candidate;
        }
      }
      if (!kf) continue;

      const extList = Array.isArray(kf.extended_accessibility) ? kf.extended_accessibility : [];
      const extMap = new Map(extList.map(e => [e.element_id, e]));
      const elements = Array.isArray(kf.elements) ? kf.elements : [];
      const elMap = new Map(elements.map(e => [e.id, e]));

      for (const element of elements) {
        if (!element || element.visible === false) continue;
        let runtimeTarget = null;
        if (resolveTarget) {
          try { runtimeTarget = resolveTarget(element); } catch (_) { runtimeTarget = null; }
        }
        if (runtimeTarget && safeGet(runtimeTarget, 'visible') === false) continue;

        const ext = extMap.get(element.id) || element.extended_accessibility || null;
        const hasNonDefaultExt = Boolean(ext && (
          (ext.role && ext.role !== 'auto') ||
          ext.live === 'polite' ||
          ext.live === 'assertive' ||
          Boolean(ext.silent) ||
          Boolean(ext.force_simple) ||
          Boolean(ext.no_auto_labeling) ||
          (typeof ext.shortcut === 'string' && ext.shortcut.trim() !== '') ||
          (Number(ext.reading_order) > 0) ||
          Boolean(ext.aria_labelled_by) ||
          Boolean(ext.aria_described_by) ||
          (typeof ext.focus_group === 'string' && ext.focus_group.trim() !== '')
        ));
        const kindType = element.kind?.type || '';
        const isInteractiveOrText = kindType === 'text' || kindType === 'text_box' || kindType === 'instance';
        const runtimeAcc = safeGet(runtimeTarget, 'accessibilityProperties') || null;
        const runtimeImpl = safeGet(runtimeTarget, 'accessibilityImplementation') || null;
        if (!element.accessibility && !hasNonDefaultExt && !isInteractiveOrText && !runtimeAcc && !runtimeImpl) continue;

        let role = 'img';
        if (runtimeAcc && typeof runtimeAcc.role === 'string' && runtimeAcc.role.trim()) {
          role = runtimeAcc.role.trim();
        } else if (ext && ext.role && ext.role !== 'auto') {
          role = CONTENT_ACCESSIBLE_ROLE_MAP[ext.role] || String(ext.role);
        } else if (element.accessibility && element.accessibility.role) {
          role = element.accessibility.role === 'button'
            ? 'button'
            : (element.accessibility.role === 'image' ? 'img' : String(element.accessibility.role));
        } else if (kindType === 'text') {
          role = 'note';
        } else if (kindType === 'text_box') {
          const fieldType = element.kind?.format?.fieldOptions?.field_type || element.kind?.format?.field?.type || element.kind?.format?.field_type;
          role = fieldType === 'input' ? 'textbox' : 'note';
        } else if (kindType === 'instance') {
          const symKind = symbolsById.get(element.kind?.symbol_id)?.kind || 'graphic';
          role = symKind === 'button' ? 'button' : (symKind === 'movie_clip' ? 'group' : 'img');
        }

        const autoLabel = !(ext && ext.no_auto_labeling) && !(runtimeAcc && runtimeAcc.noAutoLabeling);
        const explicitName = String((runtimeAcc && runtimeAcc.name) || (element.accessibility && element.accessibility.name) || '').trim();
        let ariaLabel = '';
        if (explicitName) {
          ariaLabel = explicitName;
        } else if (autoLabel) {
          const runtimeText = safeGet(runtimeTarget, 'text');
          const textVal = (kindType === 'text' || kindType === 'text_box') ? String(runtimeText ?? element.kind?.text ?? '').trim() : '';
          if (textVal) {
            ariaLabel = textVal;
          } else if (typeof element.name === 'string' && element.name.trim()) {
            ariaLabel = element.name.trim();
          } else if (kindType === 'instance') {
            ariaLabel = String(symbolsById.get(element.kind?.symbol_id)?.name || '').trim();
          }
        } else if (typeof element.name === 'string' && element.name.trim()) {
          ariaLabel = element.name.trim();
        }

        if (!ariaLabel && ext && ext.aria_labelled_by) {
          const labelEl = elMap.get(ext.aria_labelled_by);
          if (labelEl) {
            const lText = (labelEl.kind?.type === 'text' || labelEl.kind?.type === 'text_box') ? String(labelEl.kind?.text || '').trim() : '';
            ariaLabel = lText || String(labelEl.accessibility?.name || '').trim();
          }
        }

        let ariaDescription = String((runtimeAcc && runtimeAcc.description) || (element.accessibility && element.accessibility.description) || '').trim();
        if (!ariaDescription && ext && ext.aria_described_by) {
          const descEl = elMap.get(ext.aria_described_by);
          if (descEl) {
            const dText = (descEl.kind?.type === 'text' || descEl.kind?.type === 'text_box') ? String(descEl.kind?.text || '').trim() : '';
            ariaDescription = dText || String(descEl.accessibility?.description || '').trim();
          }
        }

        const liveRaw = String((runtimeAcc && runtimeAcc.live) || (ext && ext.live) || 'off').trim().toLowerCase();
        const ariaLive = (liveRaw === 'polite' || liveRaw === 'assertive') ? liveRaw : 'off';
        const shortcut = String((runtimeAcc && runtimeAcc.shortcut) || (ext && ext.shortcut) || '').trim();
        const silent = Boolean((runtimeAcc && runtimeAcc.silent) || (ext && ext.silent));
        const forceSimple = Boolean((runtimeAcc && runtimeAcc.forceSimple) || (ext && ext.force_simple));
        const rtEnabled = safeGet(runtimeTarget, 'enabled');
        const disabled = Boolean((element.accessibility && element.accessibility.disabled) || rtEnabled === false);
        const rtTabEnabled = safeGet(runtimeTarget, 'tabEnabled');
        const tabEnabled = rtTabEnabled !== undefined ? Boolean(rtTabEnabled) : undefined;
        const runtimeTab = Number(safeGet(runtimeTarget, 'tabIndex'));
        const accTab = Number(element.accessibility && element.accessibility.tab_order);
        const extOrder = Number(ext && ext.reading_order);
        const tabIndex = (Number.isInteger(runtimeTab) && runtimeTab > 0)
          ? runtimeTab
          : ((Number.isInteger(accTab) && accTab > 0)
            ? accTab
            : undefined);
        const [bx, by, bw, bh] = elementStageBounds(element);
        const rtx = Number(safeGet(runtimeTarget, 'x'));
        const rty = Number(safeGet(runtimeTarget, 'y'));
        const rtw = Number(safeGet(runtimeTarget, 'width'));
        const rth = Number(safeGet(runtimeTarget, 'height'));
        const x = Number.isFinite(rtx) ? rtx : bx;
        const y = Number.isFinite(rty) ? rty : by;
        const width = Number.isFinite(rtw) && rtw > 0 ? rtw : bw;
        const height = Number.isFinite(rth) && rth > 0 ? rth : bh;

        entries.push({
          id: String(element.id || `el-${entries.length + 1}`),
          name: String(element.name || ''),
          // Static authored elements may not have a runtime display proxy. Keep their project
          // object as the lifetime identity so repeated render passes preserve DOM focus while a
          // genuinely replaced element with the same authored ID still receives a fresh node.
          target: runtimeTarget || element,
          role,
          roleOverride: Boolean((runtimeAcc && runtimeAcc.role) || (ext && ext.role && ext.role !== 'auto')),
          ariaLabel: ariaLabel || String(element.id || `el-${entries.length + 1}`),
          autoTextLabel: !explicitName && autoLabel && (kindType === 'text' || kindType === 'text_box'),
          ariaDescription,
          ariaLive,
          shortcut,
          silent,
          forceSimple,
          noAutoLabeling: !autoLabel,
          disabled,
          tabEnabled,
          tabIndex,
          readingOrder: Number.isInteger(extOrder) && extOrder > 0 ? extOrder : undefined,
          x,
          y,
          width,
          height,
        });
      }
    }
    return entries;
  }

  function syncAccessibilityOverlay(overlayRoot, entries = [], options = {}) {
    const nodes = resolveAccessibilityOverlayNodes(entries, options);
    const diagnostics = validateAccessibilityReadOrder(entries);
    if (!overlayRoot || typeof overlayRoot !== 'object') {
      return { nodes, diagnostics, elements: [] };
    }
    if (typeof overlayRoot.setAttribute === 'function') {
      overlayRoot.setAttribute('role', 'region');
      overlayRoot.setAttribute('aria-label', (options && options.stageLabel) ? String(options.stageLabel) : 'Stage accessibility overlay');
      overlayRoot.setAttribute('data-focus-policy', 'keyboard-distinct');
    }
    const doc = overlayRoot.ownerDocument || (typeof document !== 'undefined' ? document : null);
    const createNode = () => (doc && typeof doc.createElement === 'function') ? doc.createElement('div') : createFallbackOverlayDomElement('div');
    const previous = Array.from(overlayRoot.children || []);
    const previousById = new Map();
    for (const el of previous) {
      const id = typeof el?.getAttribute === 'function'
        ? el.getAttribute('data-neoflash-id')
        : el?.dataset?.neoflashId;
      if (!id) continue;
      if (!previousById.has(id)) previousById.set(id, []);
      previousById.get(id).push(el);
    }
    const hostElementById = new Map();
    const elements = nodes.map(node => {
      const candidates = previousById.get(node.id) || [];
      const candidateIndex = candidates.findIndex(el => accessibilityOverlayTargets.get(el) === node.target);
      const el = candidateIndex >= 0 ? candidates.splice(candidateIndex, 1)[0] : createNode();
      accessibilityOverlayTargets.set(el, node.target);
      if (!accessibilityOverlayBoundNodes.has(el) && typeof el.addEventListener === 'function') {
        const triggerDefault = ev => {
          const cur = accessibilityOverlayBoundNodes.get(el);
          if (!cur || cur.ariaDisabled || !cur.hooks) return;
          if (cur.selectable) {
            const selOp = ev?.shiftKey ? 0x04 : ((ev?.ctrlKey || ev?.metaKey) ? (cur.selected === true ? 0x10 : 0x08) : 0x02);
            cur.hooks.onSelect?.(selOp);
          }
          cur.hooks.onDefaultAction?.();
        };
        el.addEventListener('click', ev => triggerDefault(ev));
        el.addEventListener('keydown', ev => {
          if (ev?.key === 'Enter' || ev?.key === ' ' || ev?.key === 'Spacebar') {
            ev.preventDefault?.();
            triggerDefault(ev);
          }
        });
        el.addEventListener('focus', () => {
          const cur = accessibilityOverlayBoundNodes.get(el);
          if (!cur || cur.ariaDisabled || !cur.hooks) return;
          cur.hooks.onSelect?.(0x01);
        });
      }
      accessibilityOverlayBoundNodes.set(el, node);
      el.id = `nf-a11y-${node.id}`;
      el.tabIndex = node.tabIndex;
      if (!node.childID) hostElementById.set(node.id, { el, node });
      if (typeof el.setAttribute === 'function') {
        el.setAttribute('id', `nf-a11y-${node.id}`);
        el.setAttribute('data-element-id', node.id);
        el.setAttribute('data-neoflash-id', node.id);
        el.setAttribute('data-reading-order', String(node.readingOrder));
        if (node.childID > 0) {
          el.setAttribute('data-child-id', String(node.childID));
        } else {
          el.removeAttribute?.('data-child-id');
        }
        el.setAttribute('role', node.role);
        el.setAttribute('tabindex', String(node.tabIndex));
        el.setAttribute('aria-label', node.ariaLabel);
        el.setAttribute('aria-hidden', node.ariaHidden ? 'true' : 'false');
        if (node.ariaDescription) {
          el.setAttribute('aria-description', node.ariaDescription);
        } else {
          el.removeAttribute?.('aria-description');
        }
        if (node.valueText !== null && node.valueText !== undefined && String(node.valueText).length > 0) {
          el.setAttribute('aria-valuetext', String(node.valueText));
        } else {
          el.removeAttribute?.('aria-valuetext');
        }
        if (typeof node.selected === 'boolean') {
          el.setAttribute('aria-selected', String(node.selected));
        } else {
          el.removeAttribute?.('aria-selected');
        }
        if (node.checked !== null && node.checked !== undefined && node.checked !== '') {
          el.setAttribute('aria-checked', String(node.checked));
        } else {
          el.removeAttribute?.('aria-checked');
        }
        if (node.pressed) {
          el.setAttribute('aria-pressed', 'true');
        } else {
          el.removeAttribute?.('aria-pressed');
        }
        if (typeof node.expanded === 'boolean') {
          el.setAttribute('aria-expanded', String(node.expanded));
        } else {
          el.removeAttribute?.('aria-expanded');
        }
        if (node.readonly) {
          el.setAttribute('aria-readonly', 'true');
        } else {
          el.removeAttribute?.('aria-readonly');
        }
        if (node.defaultAction) {
          el.setAttribute('data-default-action', String(node.defaultAction));
        } else {
          el.removeAttribute?.('data-default-action');
        }
        if (node.ariaLive && node.ariaLive !== 'off') {
          el.setAttribute('aria-live', node.ariaLive);
        } else {
          el.removeAttribute?.('aria-live');
        }
        if (node.ariaDisabled) {
          el.setAttribute('aria-disabled', 'true');
        } else {
          el.removeAttribute?.('aria-disabled');
        }
        if (node.shortcut) {
          el.setAttribute('aria-keyshortcuts', node.shortcut);
        } else {
          el.removeAttribute?.('aria-keyshortcuts');
        }
        if (node.focused && node.focusReason) {
          el.setAttribute('data-focus-reason', node.focusReason);
          el.setAttribute('data-keyboard-focus-ring', node.keyboardFocusRing ? 'true' : 'false');
        } else {
          el.removeAttribute?.('data-focus-reason');
          el.removeAttribute?.('data-keyboard-focus-ring');
        }
        if (!node.childID) el.removeAttribute?.('aria-activedescendant');
      }
      if (node.childID > 0 && node.parentId) {
        const parentEntry = hostElementById.get(node.parentId);
        if (parentEntry && parentEntry.node.focusChildId === node.childID) {
          parentEntry.el.setAttribute?.('aria-activedescendant', `nf-a11y-${node.id}`);
        }
      }
      if (el.style && typeof el.style === 'object') {
        el.style.position = 'absolute';
        el.style.left = `${node.x}px`;
        el.style.top = `${node.y}px`;
        el.style.width = `${node.width}px`;
        el.style.height = `${node.height}px`;
        el.style.pointerEvents = node.hooks ? 'auto' : 'none';
      }
      return el;
    });

    // Real DOM roots can reorder existing children without replacing them. Avoid any mutation
    // when the order is already correct so the active element remains attached across frames.
    if (typeof overlayRoot.insertBefore === 'function' && typeof overlayRoot.removeChild === 'function') {
      for (let index = 0; index < elements.length; index++) {
        const current = overlayRoot.children?.[index] || null;
        if (current !== elements[index]) overlayRoot.insertBefore(elements[index], current);
      }
      while ((overlayRoot.children?.length || 0) > elements.length) {
        overlayRoot.removeChild(overlayRoot.children[overlayRoot.children.length - 1]);
      }
    } else if (Array.isArray(overlayRoot.children)) {
      overlayRoot.children.splice(0, overlayRoot.children.length, ...elements);
    } else if (typeof overlayRoot.replaceChildren === 'function') {
      overlayRoot.replaceChildren(...elements);
    } else if (!('children' in overlayRoot)) {
      overlayRoot.children = elements;
    }
    return { nodes, diagnostics, elements };
  }
  const builtInItemsRecords = new WeakMap();
  class ContextMenuBuiltInItems {
    constructor() {
      if (new.target !== ContextMenuBuiltInItems) throw new TypeError('ContextMenuBuiltInItems is final and cannot be subclassed.');
      builtInItemsRecords.set(this, { forwardAndBack: true, loop: true, play: true, print: true, quality: true, rewind: true, save: true, zoom: true });
      return Object.seal(this);
    }
    get forwardAndBack() { return builtInItemsRecords.get(this).forwardAndBack; }
    set forwardAndBack(value) { builtInItemsRecords.get(this).forwardAndBack = Boolean(value); }
    get loop() { return builtInItemsRecords.get(this).loop; }
    set loop(value) { builtInItemsRecords.get(this).loop = Boolean(value); }
    get play() { return builtInItemsRecords.get(this).play; }
    set play(value) { builtInItemsRecords.get(this).play = Boolean(value); }
    get print() { return builtInItemsRecords.get(this).print; }
    set print(value) { builtInItemsRecords.get(this).print = Boolean(value); }
    get quality() { return builtInItemsRecords.get(this).quality; }
    set quality(value) { builtInItemsRecords.get(this).quality = Boolean(value); }
    get rewind() { return builtInItemsRecords.get(this).rewind; }
    set rewind(value) { builtInItemsRecords.get(this).rewind = Boolean(value); }
    get save() { return builtInItemsRecords.get(this).save; }
    set save(value) { builtInItemsRecords.get(this).save = Boolean(value); }
    get zoom() { return builtInItemsRecords.get(this).zoom; }
    set zoom(value) { builtInItemsRecords.get(this).zoom = Boolean(value); }
    clone() {
      const rec = builtInItemsRecords.get(this);
      const copy = new ContextMenuBuiltInItems();
      Object.assign(builtInItemsRecords.get(copy), rec);
      return copy;
    }
  }
  ContextMenuBuiltInItems.__nfIs = value => builtInItemsRecords.has(value);
  ContextMenuBuiltInItems.__nfInitialize = receiver => {
    if (!builtInItemsRecords.has(receiver)) throw new TypeError('ContextMenuBuiltInItems is final and cannot be subclassed.');
  };
  Object.defineProperty(ContextMenuBuiltInItems, Symbol.hasInstance, { value: ContextMenuBuiltInItems.__nfIs });
  const clipboardItemsRecords = new WeakMap();
  class ContextMenuClipboardItems {
    constructor() {
      if (new.target !== ContextMenuClipboardItems) throw new TypeError('ContextMenuClipboardItems is final and cannot be subclassed.');
      clipboardItemsRecords.set(this, { clear: false, copy: false, cut: false, paste: false, selectAll: true });
      return Object.seal(this);
    }
    get clear() { return clipboardItemsRecords.get(this).clear; }
    set clear(value) { clipboardItemsRecords.get(this).clear = Boolean(value); }
    get copy() { return clipboardItemsRecords.get(this).copy; }
    set copy(value) { clipboardItemsRecords.get(this).copy = Boolean(value); }
    get cut() { return clipboardItemsRecords.get(this).cut; }
    set cut(value) { clipboardItemsRecords.get(this).cut = Boolean(value); }
    get paste() { return clipboardItemsRecords.get(this).paste; }
    set paste(value) { clipboardItemsRecords.get(this).paste = Boolean(value); }
    get selectAll() { return clipboardItemsRecords.get(this).selectAll; }
    set selectAll(value) { clipboardItemsRecords.get(this).selectAll = Boolean(value); }
    clone() {
      const rec = clipboardItemsRecords.get(this);
      const copy = new ContextMenuClipboardItems();
      Object.assign(clipboardItemsRecords.get(copy), rec);
      return copy;
    }
  }
  ContextMenuClipboardItems.__nfIs = value => clipboardItemsRecords.has(value);
  ContextMenuClipboardItems.__nfInitialize = receiver => {
    if (!clipboardItemsRecords.has(receiver)) throw new TypeError('ContextMenuClipboardItems is final and cannot be subclassed.');
  };
  Object.defineProperty(ContextMenuClipboardItems, Symbol.hasInstance, { value: ContextMenuClipboardItems.__nfIs });
  const contextMenuItemRecords = new WeakMap();
  class ContextMenuItem extends (eventsAPI?.EventDispatcher || class {}) {
    constructor(caption, separatorBefore = false, enabled = true, visible = true) {
      if (caption == null) throw new TypeError('ContextMenuItem caption must be a String.');
      super();
      contextMenuItemRecords.set(this, { caption: String(caption), separatorBefore: Boolean(separatorBefore), enabled: Boolean(enabled), visible: Boolean(visible) });
    }
    get caption() { return contextMenuItemRecords.get(this).caption; }
    set caption(value) { if (value == null) throw new TypeError('ContextMenuItem.caption must be a String.'); contextMenuItemRecords.get(this).caption = String(value); }
    get separatorBefore() { return contextMenuItemRecords.get(this).separatorBefore; }
    set separatorBefore(value) { contextMenuItemRecords.get(this).separatorBefore = Boolean(value); }
    get enabled() { return contextMenuItemRecords.get(this).enabled; }
    set enabled(value) { contextMenuItemRecords.get(this).enabled = Boolean(value); }
    get visible() { return contextMenuItemRecords.get(this).visible; }
    set visible(value) { contextMenuItemRecords.get(this).visible = Boolean(value); }
    clone() {
      const rec = contextMenuItemRecords.get(this);
      return new ContextMenuItem(rec.caption, rec.separatorBefore, rec.enabled, rec.visible);
    }
  }
  ContextMenuItem.__nfIs = value => contextMenuItemRecords.has(value);
  ContextMenuItem.__nfInitialize = (receiver, args = []) => {
    if (!contextMenuItemRecords.has(receiver)) {
      eventsAPI?.EventDispatcher?.__nfInitialize?.(receiver, []);
      const [caption, separatorBefore = false, enabled = true, visible = true] = args;
      if (caption == null) throw new TypeError('ContextMenuItem caption must be a String.');
      contextMenuItemRecords.set(receiver, { caption: String(caption), separatorBefore: Boolean(separatorBefore), enabled: Boolean(enabled), visible: Boolean(visible) });
    }
  };
  Object.defineProperty(ContextMenuItem, Symbol.hasInstance, { value: ContextMenuItem.__nfIs });
  const contextMenuRecords = new WeakMap();
  const contextMenuItemOwners = new WeakMap();
  const stageContextMenuOpeners = new WeakMap();
  function makeStageRangeError(message, code = 2006) {
    const err = new RangeError(message);
    err.errorID = code;
    return err;
  }
  function isStageValidMenuItem(item) {
    if (!item || typeof item !== 'object') return false;
    return ContextMenuItem.__nfIs(item) || typeof item.caption === 'string' || typeof item.label === 'string';
  }
  class ContextMenu extends (eventsAPI?.EventDispatcher || class {}) {
    constructor() {
      super();
      contextMenuRecords.set(this, { builtInItems: new ContextMenuBuiltInItems(), clipboardItems: new ContextMenuClipboardItems(), clipboardMenu: false, customItems: [], link: null });
    }
    static get isSupported() { return true; }
    get builtInItems() { return contextMenuRecords.get(this).builtInItems; }
    set builtInItems(value) {
      if (!ContextMenuBuiltInItems.__nfIs(value)) throw new TypeError('ContextMenu.builtInItems requires a ContextMenuBuiltInItems instance.');
      contextMenuRecords.get(this).builtInItems = value;
    }
    get clipboardItems() { return contextMenuRecords.get(this).clipboardItems; }
    set clipboardItems(value) {
      if (!ContextMenuClipboardItems.__nfIs(value)) throw new TypeError('ContextMenu.clipboardItems requires a ContextMenuClipboardItems instance.');
      contextMenuRecords.get(this).clipboardItems = value;
    }
    get clipboardMenu() { return contextMenuRecords.get(this).clipboardMenu; }
    set clipboardMenu(value) { contextMenuRecords.get(this).clipboardMenu = Boolean(value); }
    get customItems() { return contextMenuRecords.get(this).customItems; }
    set customItems(value) {
      if (!Array.isArray(value)) throw new TypeError('ContextMenu.customItems requires an Array.');
      const rec = contextMenuRecords.get(this);
      for (const prev of rec.customItems) {
        if (contextMenuItemOwners.get(prev) === this) contextMenuItemOwners.delete(prev);
      }
      rec.customItems = value;
      for (const it of value) {
        if (it && typeof it === 'object') contextMenuItemOwners.set(it, this);
      }
    }
    get items() {
      return contextMenuRecords.get(this).customItems.slice();
    }
    set items(value) {
      if (!Array.isArray(value)) throw new TypeError('ContextMenu.items requires an Array.');
      for (const it of value) {
        if (!isStageValidMenuItem(it)) throw new TypeError('ContextMenu.items elements must be ContextMenuItem instances.');
      }
      this.removeAllItems();
      for (const it of value) this.addItemAt(it, this.numItems);
    }
    get numItems() {
      return contextMenuRecords.get(this).customItems.length;
    }
    addItem(item) {
      return this.addItemAt(item, this.numItems);
    }
    addItemAt(item, index) {
      if (!isStageValidMenuItem(item)) throw new TypeError('ContextMenu.addItemAt requires a ContextMenuItem.');
      const rec = contextMenuRecords.get(this);
      const raw = Number(index);
      const idx = raw | 0;
      if (!Number.isFinite(raw) || idx < 0 || idx > rec.customItems.length) {
        throw makeStageRangeError(`Error #2006: The supplied index ${idx} is out of bounds.`, 2006);
      }
      const prevOwner = contextMenuItemOwners.get(item);
      if (prevOwner && contextMenuRecords.has(prevOwner)) {
        const prevList = contextMenuRecords.get(prevOwner).customItems;
        const prevIdx = prevList.indexOf(item);
        if (prevIdx >= 0) prevList.splice(prevIdx, 1);
      }
      const clamped = Math.min(idx, rec.customItems.length);
      rec.customItems.splice(clamped, 0, item);
      contextMenuItemOwners.set(item, this);
      return item;
    }
    getItemAt(index) {
      const rec = contextMenuRecords.get(this);
      const raw = Number(index);
      const idx = raw | 0;
      if (!Number.isFinite(raw) || idx < 0 || idx >= rec.customItems.length) {
        throw makeStageRangeError(`Error #2006: The supplied index ${idx} is out of bounds.`, 2006);
      }
      return rec.customItems[idx];
    }
    getItemIndex(item) {
      return contextMenuRecords.get(this).customItems.indexOf(item);
    }
    containsItem(item) {
      if (!item || typeof item !== 'object') return false;
      return contextMenuRecords.get(this).customItems.includes(item);
    }
    removeItem(item) {
      const idx = this.getItemIndex(item);
      if (idx < 0) {
        const err = new Error('ArgumentError: Error #2025: The supplied MenuItem must be a child of the caller.');
        err.name = 'ArgumentError';
        err.errorID = 2025;
        throw err;
      }
      return this.removeItemAt(idx);
    }
    removeItemAt(index) {
      const rec = contextMenuRecords.get(this);
      const raw = Number(index);
      const idx = raw | 0;
      if (!Number.isFinite(raw) || idx < 0 || idx >= rec.customItems.length) {
        throw makeStageRangeError(`Error #2006: The supplied index ${idx} is out of bounds.`, 2006);
      }
      const [removed] = rec.customItems.splice(idx, 1);
      if (removed && contextMenuItemOwners.get(removed) === this) contextMenuItemOwners.delete(removed);
      return removed;
    }
    removeAllItems() {
      const rec = contextMenuRecords.get(this);
      for (const it of rec.customItems) {
        if (it && contextMenuItemOwners.get(it) === this) contextMenuItemOwners.delete(it);
      }
      rec.customItems.length = 0;
    }
    display(stageObj, stageX, stageY) {
      if (!stageObj || typeof stageObj !== 'object') {
        throw new TypeError('TypeError: Error #2007: Parameter stage must be non-null.');
      }
      const opener = stageContextMenuOpeners.get(stageObj);
      if (typeof opener === 'function') {
        const sx = Number(stageX) || 0;
        const sy = Number(stageY) || 0;
        opener({ clientX: sx, clientY: sy, stageX: sx, stageY: sy }, stageObj.focus || stageObj, this, stageObj);
      }
    }
    get link() { return contextMenuRecords.get(this).link; }
    set link(value) { contextMenuRecords.get(this).link = value ?? null; }
    hideBuiltInItems() {
      const b = contextMenuRecords.get(this).builtInItems;
      b.forwardAndBack = false; b.loop = false; b.play = false; b.print = false; b.quality = false; b.rewind = false; b.save = false; b.zoom = false;
    }
    clone() {
      const rec = contextMenuRecords.get(this);
      const copy = new ContextMenu();
      const copyRec = contextMenuRecords.get(copy);
      copyRec.builtInItems = rec.builtInItems.clone();
      copyRec.clipboardItems = rec.clipboardItems.clone();
      copyRec.clipboardMenu = rec.clipboardMenu;
      copyRec.customItems = rec.customItems.map(item => typeof item?.clone === 'function' ? item.clone() : item);
      for (const it of copyRec.customItems) {
        if (it && typeof it === 'object') contextMenuItemOwners.set(it, copy);
      }
      copyRec.link = rec.link;
      return copy;
    }
  }
  ContextMenu.__nfIs = value => contextMenuRecords.has(value);
  ContextMenu.__nfInitialize = receiver => {
    if (!contextMenuRecords.has(receiver)) {
      eventsAPI?.EventDispatcher?.__nfInitialize?.(receiver, []);
      contextMenuRecords.set(receiver, { builtInItems: new ContextMenuBuiltInItems(), clipboardItems: new ContextMenuClipboardItems(), clipboardMenu: false, customItems: [], link: null });
    }
  };
  Object.defineProperty(ContextMenu, Symbol.hasInstance, { value: ContextMenu.__nfIs });

  const stageCompRangeRecords = new WeakMap();
  class StageCompositionAttributeRange {
    constructor(relativeStart, relativeEnd, selected, converted) {
      if (new.target !== StageCompositionAttributeRange) {
        throw new TypeError('CompositionAttributeRange is final and cannot be subclassed.');
      }
      stageCompRangeRecords.set(this, {
        relativeStart: Number(relativeStart) | 0,
        relativeEnd: Number(relativeEnd) | 0,
        selected: Boolean(selected),
        converted: Boolean(converted),
      });
      return Object.seal(this);
    }
    get relativeStart() { return stageCompRangeRecords.get(this).relativeStart; }
    set relativeStart(v) { stageCompRangeRecords.get(this).relativeStart = Number(v) | 0; }
    get relativeEnd() { return stageCompRangeRecords.get(this).relativeEnd; }
    set relativeEnd(v) { stageCompRangeRecords.get(this).relativeEnd = Number(v) | 0; }
    get selected() { return stageCompRangeRecords.get(this).selected; }
    set selected(v) { stageCompRangeRecords.get(this).selected = Boolean(v); }
    get converted() { return stageCompRangeRecords.get(this).converted; }
    set converted(v) { stageCompRangeRecords.get(this).converted = Boolean(v); }
  }
  StageCompositionAttributeRange.__nfIs = v => stageCompRangeRecords.has(v);
  Object.defineProperty(StageCompositionAttributeRange, Symbol.hasInstance, { value: StageCompositionAttributeRange.__nfIs });

  const STAGE_IME_CLIENT_MEMBERS = [
    'compositionStartIndex',
    'compositionEndIndex',
    'selectionActiveIndex',
    'selectionAnchorIndex',
    'verticalTextLayout',
    'confirmComposition',
    'getTextBounds',
    'getTextInRange',
    'selectRange',
    'updateComposition',
  ];
  function isStageImeClient(value) {
    return Boolean(value && typeof value === 'object' && STAGE_IME_CLIENT_MEMBERS.every(m => m in value));
  }
  class StageScaleMode { constructor() { throw new TypeError('StageScaleMode supplies EXACT_FIT, NO_BORDER, NO_SCALE and SHOW_ALL constants.'); } }
  Object.assign(StageScaleMode, { EXACT_FIT: 'exactFit', NO_BORDER: 'noBorder', NO_SCALE: 'noScale', SHOW_ALL: 'showAll' }); Object.freeze(StageScaleMode);
  class StageAlign { constructor() { throw new TypeError('StageAlign supplies stage alignment constants.'); } }
  Object.assign(StageAlign, { BOTTOM: 'B', BOTTOM_LEFT: 'BL', BOTTOM_RIGHT: 'BR', LEFT: 'L', RIGHT: 'R', TOP: 'T', TOP_LEFT: 'TL', TOP_RIGHT: 'TR' }); Object.freeze(StageAlign);
  class StageDisplayState { constructor() { throw new TypeError('StageDisplayState supplies FULL_SCREEN, FULL_SCREEN_INTERACTIVE and NORMAL constants.'); } }
  Object.assign(StageDisplayState, { FULL_SCREEN: 'fullScreen', FULL_SCREEN_INTERACTIVE: 'fullScreenInteractive', NORMAL: 'normal' }); Object.freeze(StageDisplayState);
  class StageQuality { constructor() { throw new TypeError('StageQuality supplies stage rendering quality constants.'); } }
  Object.assign(StageQuality, { LOW: 'low', MEDIUM: 'medium', HIGH: 'high', BEST: 'best', HIGH_8X8: '8x8', HIGH_8X8_LINEAR: '8x8linear', HIGH_16X16: '16x16', HIGH_16X16_LINEAR: '16x16linear' }); Object.freeze(StageQuality);
  class KeyLocation { constructor() { throw new TypeError('KeyLocation supplies STANDARD, LEFT, RIGHT, NUM_PAD and D_PAD constants.'); } }
  Object.assign(KeyLocation, { STANDARD: 0, LEFT: 1, RIGHT: 2, NUM_PAD: 3, D_PAD: 4 }); Object.freeze(KeyLocation);
  class KeyboardType { constructor() { throw new TypeError('KeyboardType supplies ALPHANUMERIC, KEYPAD and NONE constants.'); } }
  Object.assign(KeyboardType, { ALPHANUMERIC: 'alphanumeric', KEYPAD: 'keypad', NONE: 'none' }); Object.freeze(KeyboardType);
  class MouseCursor { constructor() { throw new TypeError('MouseCursor supplies ARROW, AUTO, BUTTON, HAND and IBEAM constants.'); } }
  Object.assign(MouseCursor, { ARROW: 'arrow', AUTO: 'auto', BUTTON: 'button', HAND: 'hand', IBEAM: 'ibeam' }); Object.freeze(MouseCursor);
  const KEYBOARD_CONSTANTS = Object.freeze({ A:65,ALTERNATE:18,AUDIO:0x01000017,B:66,BACK:0x01000016,BACKQUOTE:192,
    BACKSLASH:220,BACKSPACE:8,BLUE:0x01000003,C:67,CAPS_LOCK:20,CHANNEL_DOWN:0x01000005,CHANNEL_UP:0x01000004,
    COMMA:188,COMMAND:15,CONTROL:17,D:68,DELETE:46,DOWN:40,DVR:0x01000019,E:69,END:35,ENTER:13,EQUAL:187,ESCAPE:27,
    EXIT:0x01000015,F:70,F1:112,F10:121,F11:122,F12:123,F13:124,F14:125,F15:126,F2:113,F3:114,F4:115,F5:116,F6:117,
    F7:118,F8:119,F9:120,FAST_FORWARD:0x0100000A,G:71,GREEN:0x01000001,GUIDE:0x01000014,H:72,HELP:0x0100001D,HOME:36,
    I:73,INFO:0x01000013,INPUT:0x0100001B,INSERT:45,J:74,K:75,L:76,LAST:0x01000011,LEFT:37,LEFTBRACKET:219,
    LIVE:0x01000010,M:77,MASTER_SHELL:0x0100001E,MENU:0x01000012,MINUS:189,N:78,NEXT:0x0100000E,NUMBER_0:48,
    NUMBER_1:49,NUMBER_2:50,NUMBER_3:51,NUMBER_4:52,NUMBER_5:53,NUMBER_6:54,NUMBER_7:55,NUMBER_8:56,NUMBER_9:57,
    NUMPAD:21,NUMPAD_0:96,NUMPAD_1:97,NUMPAD_2:98,NUMPAD_3:99,NUMPAD_4:100,NUMPAD_5:101,NUMPAD_6:102,NUMPAD_7:103,
    NUMPAD_8:104,NUMPAD_9:105,NUMPAD_ADD:107,NUMPAD_DECIMAL:110,NUMPAD_DIVIDE:111,NUMPAD_ENTER:108,
    NUMPAD_MULTIPLY:106,NUMPAD_SUBTRACT:109,O:79,P:80,PAGE_DOWN:34,PAGE_UP:33,PAUSE:0x01000008,PERIOD:190,
    PLAY:0x01000007,PLAY_PAUSE:0x01000020,PREVIOUS:0x0100000F,Q:81,QUOTE:222,R:82,RECORD:0x01000006,RED:0x01000000,
    REWIND:0x0100000B,RIGHT:39,RIGHTBRACKET:221,S:83,SEARCH:0x0100001F,SEMICOLON:186,SETUP:0x0100001C,SHIFT:16,
    SKIP_BACKWARD:0x0100000D,SKIP_FORWARD:0x0100000C,SLASH:191,SPACE:32,STOP:0x01000009,SUBTITLE:0x01000018,T:84,
    TAB:9,U:85,UP:38,V:86,VOD:0x0100001A,W:87,X:88,Y:89,YELLOW:0x01000002,Z:90,KEYNAME_BEGIN:'Begin',
    KEYNAME_BREAK:'Break',KEYNAME_CLEARDISPLAY:'ClrDsp',KEYNAME_CLEARLINE:'ClrLn',KEYNAME_DELETE:'Delete',
    KEYNAME_DELETECHAR:'DelChr',KEYNAME_DELETELINE:'DelLn',KEYNAME_DOWNARROW:'Down',KEYNAME_END:'End',
    KEYNAME_EXECUTE:'Exec',KEYNAME_F1:'F1',KEYNAME_F10:'F10',KEYNAME_F11:'F11',KEYNAME_F12:'F12',KEYNAME_F13:'F13',
    KEYNAME_F14:'F14',KEYNAME_F15:'F15',KEYNAME_F16:'F16',KEYNAME_F17:'F17',KEYNAME_F18:'F18',KEYNAME_F19:'F19',
    KEYNAME_F2:'F2',KEYNAME_F20:'F20',KEYNAME_F21:'F21',KEYNAME_F22:'F22',KEYNAME_F23:'F23',KEYNAME_F24:'F24',
    KEYNAME_F25:'F25',KEYNAME_F26:'F26',KEYNAME_F27:'F27',KEYNAME_F28:'F28',KEYNAME_F29:'F29',KEYNAME_F3:'F3',
    KEYNAME_F30:'F30',KEYNAME_F31:'F31',KEYNAME_F32:'F32',KEYNAME_F33:'F33',KEYNAME_F34:'F34',KEYNAME_F35:'F35',
    KEYNAME_F4:'F4',KEYNAME_F5:'F5',KEYNAME_F6:'F6',KEYNAME_F7:'F7',KEYNAME_F8:'F8',KEYNAME_F9:'F9',
    KEYNAME_FIND:'Find',KEYNAME_HELP:'Help',KEYNAME_HOME:'Home',KEYNAME_INSERT:'Insert',KEYNAME_INSERTCHAR:'InsChr',
    KEYNAME_INSERTLINE:'InsLn',KEYNAME_LEFTARROW:'Left',KEYNAME_MENU:'Menu',KEYNAME_MODESWITCH:'ModeSw',
    KEYNAME_NEXT:'Next',KEYNAME_PAGEDOWN:'PgDn',KEYNAME_PAGEUP:'PgUp',KEYNAME_PAUSE:'Pause',
    KEYNAME_PLAYPAUSE:'PlayPause',KEYNAME_PREV:'Prev',KEYNAME_PRINT:'Print',KEYNAME_PRINTSCREEN:'PrntScrn',
    KEYNAME_REDO:'Redo',KEYNAME_RESET:'Reset',KEYNAME_RIGHTARROW:'Right',KEYNAME_SCROLLLOCK:'ScrlLck',
    KEYNAME_SELECT:'Select',KEYNAME_STOP:'Stop',KEYNAME_SYSREQ:'SysReq',KEYNAME_SYSTEM:'Sys',KEYNAME_UNDO:'Undo',
    KEYNAME_UPARROW:'Up',KEYNAME_USER:'User',STRING_BEGIN:'\uf72a',STRING_BREAK:'\uf732',STRING_CLEARDISPLAY:'\uf73a',
    STRING_CLEARLINE:'\uf739',STRING_DELETE:'\uf728',STRING_DELETECHAR:'\uf73e',STRING_DELETELINE:'\uf73c',
    STRING_DOWNARROW:'\uf701',STRING_END:'\uf72b',STRING_EXECUTE:'\uf742',STRING_F1:'\uf704',STRING_F10:'\uf70d',
    STRING_F11:'\uf70e',STRING_F12:'\uf70f',STRING_F13:'\uf710',STRING_F14:'\uf711',STRING_F15:'\uf712',
    STRING_F16:'\uf713',STRING_F17:'\uf714',STRING_F18:'\uf715',STRING_F19:'\uf716',STRING_F2:'\uf705',
    STRING_F20:'\uf717',STRING_F21:'\uf718',STRING_F22:'\uf719',STRING_F23:'\uf71a',STRING_F24:'\uf71b',
    STRING_F25:'\uf71c',STRING_F26:'\uf71d',STRING_F27:'\uf71e',STRING_F28:'\uf71f',STRING_F29:'\uf720',
    STRING_F3:'\uf706',STRING_F30:'\uf721',STRING_F31:'\uf722',STRING_F32:'\uf723',STRING_F33:'\uf724',
    STRING_F34:'\uf725',STRING_F35:'\uf726',STRING_F4:'\uf707',STRING_F5:'\uf708',STRING_F6:'\uf709',
    STRING_F7:'\uf70a',STRING_F8:'\uf70b',STRING_F9:'\uf70c',STRING_FIND:'\uf745',STRING_HELP:'\uf746',
    STRING_HOME:'\uf729',STRING_INSERT:'\uf727',STRING_INSERTCHAR:'\uf73d',STRING_INSERTLINE:'\uf73b',
    STRING_LEFTARROW:'\uf702',STRING_MENU:'\uf735',STRING_MODESWITCH:'\uf747',STRING_NEXT:'\uf740',
    STRING_PAGEDOWN:'\uf72d',STRING_PAGEUP:'\uf72c',STRING_PAUSE:'\uf730',STRING_PREV:'\uf73f',STRING_PRINT:'\uf738',
    STRING_PRINTSCREEN:'\uf72e',STRING_REDO:'\uf744',STRING_RESET:'\uf733',STRING_RIGHTARROW:'\uf703',
    STRING_SCROLLLOCK:'\uf72f',STRING_SELECT:'\uf741',STRING_STOP:'\uf734',STRING_SYSREQ:'\uf731',
    STRING_SYSTEM:'\uf737',STRING_UNDO:'\uf743',STRING_UPARROW:'\uf700',STRING_USER:'\uf736' });
  const CHAR_CODE_STRINGS = Object.freeze(['Up','Down','Left','Right','F1','F2','F3','F4','F5','F6','F7','F8','F9',
    'F10','F11','F12','F13','F14','F15','F16','F17','F18','F19','F20','F21','F22','F23','F24','F25','F26','F27','F28',
    'F29','F30','F31','F32','F33','F34','F35','Insert','Delete','Home','Begin','End','PgUp','PgDn','PrntScrn',
    'ScrlLck','Pause','SysReq','Break','Reset','Stop','Menu','User','Sys','Print','ClrLn','ClrDsp','InsLn','DelLn',
    'InsChr','DelChr','Prev','Next','Select','Exec','Undo','Redo','Find','Help','ModeSw']);
  function createKeyboard(getState = () => ({})) {
    const charCodeStrings = CHAR_CODE_STRINGS.slice();
    class Keyboard { constructor() { throw new TypeError('Keyboard supplies static key constants and state.'); } }
    Object.assign(Keyboard, KEYBOARD_CONSTANTS);
    Object.defineProperties(Keyboard, {
      CharCodeStrings: { enumerable: true, get: () => charCodeStrings },
      capsLock: { enumerable: true, get: () => Boolean(getState().capsLock) },
      numLock: { enumerable: true, get: () => Boolean(getState().numLock) },
      hasVirtualKeyboard: { enumerable: true, get: () => Boolean(getState().hasVirtualKeyboard) },
      physicalKeyboardType: { enumerable: true, get: () => getState().physicalKeyboardType || KeyboardType.ALPHANUMERIC },
      isAccessible: { enumerable: true, value: () => Boolean(getState().accessible) },
    });
    return Object.freeze(Keyboard);
  }
  const VALID_CURSORS = new Set(['auto', 'arrow', 'button', 'hand', 'ibeam']);
  const COLOR_CORRECTIONS = new Set(['default', 'on', 'off']);
  const COLOR_CORRECTION_SUPPORTS = new Set(['unsupported', 'defaultOn', 'defaultOff']);
  const FOCUS_DIRECTIONS = new Set(['none', 'top', 'bottom']);
  const resetMouseToken = Symbol('resetMouseCursors');
  const cursorCssToken = Symbol('mouseCursorCss');
  function createMouse(state = { cursor: 'auto', visible: true, supportsCursor: true, supportsNativeCursor: false }, onChange = null) {
    const customCursors = new Map();
    let animTimer = null;
    let animFrameIndex = 0;
    function stopCursorAnim() {
      if (animTimer !== null) {
        clearInterval(animTimer);
        animTimer = null;
      }
      animFrameIndex = 0;
    }
    function startCursorAnimIfNeeded() {
      stopCursorAnim();
      const entry = customCursors.get(state.cursor);
      if (!entry || entry.framesCss.length <= 1 || !(entry.frameRate > 0)) return;
      const intervalMs = Math.max(16, Math.round(1000 / Math.min(120, entry.frameRate)));
      animTimer = setInterval(() => {
        const cur = customCursors.get(state.cursor);
        if (!cur || cur.framesCss.length <= 1) { stopCursorAnim(); return; }
        animFrameIndex = (animFrameIndex + 1) % cur.framesCss.length;
        onChange?.();
      }, intervalMs);
      if (animTimer && typeof animTimer.unref === 'function') animTimer.unref();
    }
    function resolveCustomCss() {
      const entry = customCursors.get(state.cursor);
      if (!entry || !entry.framesCss.length) return null;
      return entry.framesCss[animFrameIndex % entry.framesCss.length];
    }
    class Mouse { constructor() { throw new TypeError('Mouse supplies static cursor methods and properties.'); } }
    Object.defineProperties(Mouse, {
      cursor: { enumerable: true, get: () => state.cursor,
        set(value) {
          const next = String(value);
          if (!VALID_CURSORS.has(next) && !customCursors.has(next)) throw argumentError(`Mouse.cursor must be a registered cursor or one of auto, arrow, button, hand or ibeam; got '${next}'.`);
          if (state.cursor !== next) {
            state.cursor = next;
            startCursorAnimIfNeeded();
            onChange?.();
          }
        } },
      supportsCursor: { enumerable: true, get: () => Boolean(state.supportsCursor) },
      supportsNativeCursor: { enumerable: true, get: () => Boolean(state.supportsNativeCursor) },
      hide: { enumerable: true, value() { if (state.visible) { state.visible = false; onChange?.(); } } },
      show: { enumerable: true, value() { if (!state.visible) { state.visible = true; onChange?.(); } } },
      registerCursor: { enumerable: true, value(name, cursor) {
        if (name == null || cursor == null) throw new TypeError('Error #2007: Parameter name and cursor must be non-null.');
        if (!MouseCursorData.__nfIs(cursor)) throw new TypeError('Mouse.registerCursor requires a MouseCursorData instance.');
        const frames = cursor.data;
        if (!frames || frames.length === 0) throw argumentError('MouseCursorData.data must contain at least one BitmapData frame.');
        const key = String(name);
        const hs = cursor.hotSpot;
        const framesCss = Array.from(frames).map(bmp => encodeCursorFrameCss(bmp, hs));
        customCursors.set(key, {
          cursor,
          hotSpot: { x: Number(hs?.x) || 0, y: Number(hs?.y) || 0 },
          frameRate: Number(cursor.frameRate) || 0,
          framesCss,
        });
        if (state.cursor === key) {
          startCursorAnimIfNeeded();
          onChange?.();
        }
      } },
      unregisterCursor: { enumerable: true, value(name) {
        if (name == null) throw new TypeError('Error #2007: Parameter name must be non-null.');
        const key = String(name);
        customCursors.delete(key);
        if (state.cursor === key) {
          stopCursorAnim();
          state.cursor = 'auto';
          onChange?.();
        }
      } },
      [cursorCssToken]: { value: () => resolveCustomCss() },
      [resetMouseToken]: { value() { stopCursorAnim(); customCursors.clear(); } },
    });
    return Object.freeze(Mouse);
  }
  const SCALE_MODES = new Set(['showAll', 'exactFit', 'noBorder', 'noScale']);
  const ALIGN_MODES = Object.freeze({ '': '', T: 'T', B: 'B', L: 'L', R: 'R', TL: 'TL', TR: 'TR', BL: 'BL', BR: 'BR', LT: 'TL', RT: 'TR', LB: 'BL', RB: 'BR' });
  const QUALITY_MODES = Object.freeze({ low: 'LOW', medium: 'MEDIUM', high: 'HIGH', best: 'BEST', '8x8': '8X8', '8x8linear': '8X8LINEAR', '16x16': '16X16', '16x16linear': '16X16LINEAR' });
  const DISPLAY_STATES = new Set(['normal', 'fullScreen', 'fullScreenInteractive']);
  function normalizeAlign(value) {
    const key = String(value ?? '').toUpperCase();
    return Object.hasOwn(ALIGN_MODES, key) ? ALIGN_MODES[key] : '';
  }
  function computeStageLayout(docWidth, docHeight, viewportWidth, viewportHeight, settings = {}) {
    const vw = Math.max(1, Number(viewportWidth) || docWidth || 1);
    const vh = Math.max(1, Number(viewportHeight) || docHeight || 1);
    const w0 = Math.max(1, Number(docWidth) || 1);
    const h0 = Math.max(1, Number(docHeight) || 1);
    const mode = SCALE_MODES.has(settings.scaleMode) ? settings.scaleMode : 'showAll';
    const align = normalizeAlign(settings.align);
    const fsRect = settings.fullscreen && settings.fullScreenSourceRect && settings.fullScreenSourceRect.width > 0 && settings.fullScreenSourceRect.height > 0
      ? settings.fullScreenSourceRect
      : null;
    if (fsRect) {
      const s = Math.min(vw / fsRect.width, vh / fsRect.height);
      return {
        scale: s,
        scaleY: s,
        stageWidth: w0 * s,
        stageHeight: h0 * s,
        left: (vw - fsRect.width * s) / 2 - fsRect.x * s,
        top: (vh - fsRect.height * s) / 2 - fsRect.y * s,
        originX: 0,
        originY: 0,
        spaceWidth: vw,
        spaceHeight: vh,
        logicalWidth: w0,
        logicalHeight: h0,
      };
    }
    if (mode === 'noScale') {
      const originX = align.includes('L') ? 0 : align.includes('R') ? vw - w0 : (vw - w0) / 2;
      const originY = align.includes('T') ? 0 : align.includes('B') ? vh - h0 : (vh - h0) / 2;
      return {
        scale: 1,
        scaleY: 1,
        stageWidth: vw,
        stageHeight: vh,
        left: 0,
        top: 0,
        originX,
        originY,
        spaceWidth: vw,
        spaceHeight: vh,
        logicalWidth: vw,
        logicalHeight: vh,
      };
    }
    let sx, sy;
    if (mode === 'exactFit') {
      sx = vw / w0;
      sy = vh / h0;
    } else if (mode === 'noBorder') {
      sx = sy = Math.max(vw / w0, vh / h0);
    } else {
      sx = sy = Math.min(vw / w0, vh / h0);
    }
    const stageWidth = w0 * sx;
    const stageHeight = h0 * sy;
    const left = align.includes('L') ? 0 : align.includes('R') ? vw - stageWidth : (vw - stageWidth) / 2;
    const top = align.includes('T') ? 0 : align.includes('B') ? vh - stageHeight : (vh - stageHeight) / 2;
    return {
      scale: sx,
      scaleY: sy,
      stageWidth,
      stageHeight,
      left,
      top,
      originX: 0,
      originY: 0,
      spaceWidth: vw,
      spaceHeight: vh,
      logicalWidth: w0,
      logicalHeight: h0,
    };
  }
  function stageRgbValue(value) {
    const number = Number(value);
    if (!Number.isFinite(number)) throw new TypeError('Stage.color must be a finite integer.');
    return (Math.trunc(number) >>> 0) & 0xffffff;
  }
  function screenMetric(scr, key, fallback) {
    const value = Number(scr?.[key]);
    return Number.isFinite(value) && value > 0 ? (Math.trunc(value) >>> 0) : (Math.trunc(fallback) >>> 0);
  }
  const DefaultKeyboard = createKeyboard();
  const DefaultMouse = createMouse();
  function create(options) {
    if (!eventsAPI || !options?.events) throw new Error('Stage requires the NeoFlash event hub.');
    const { Event, MouseEvent, KeyboardEvent, FocusEvent, setStagePoint } = eventsAPI;
    const hub = options.events;
    const alive = options.alive || (() => true);
    let width = size(options.width, 'stageWidth'), height = size(options.height, 'stageHeight');
    let frameRate = rate(options.frameRate ?? 24), mouseX = 0, mouseY = 0, focus = null, focusReason = null;
    const initialScaleMode = SCALE_MODES.has(options.scaleMode) ? options.scaleMode : 'showAll';
    const initialAlign = normalizeAlign(options.align);
    let scaleMode = initialScaleMode, align = initialAlign, quality = 'HIGH', displayState = 'normal', colorCorrection = 'default';
    let fullScreenSourceRect = null, mouseLock = false, optInPointerLock = false, optInPointerLockRequested = false;
    let stageColor = stageRgbValue(options.color ?? 0xffffff), colorAssigned = false;
    let mouseChildren = true, tabChildren = true, doubleClickEnabled = false, showDefaultContextMenu = true, stageFocusRect = true, invalidated = false;
    const initial = { width, height, frameRate, color: stageColor, scaleMode: initialScaleMode, align: initialAlign };
    const fallbackChildren = Array.isArray(options.initialChildren) ? options.initialChildren.slice() : [];
    const keyboardState = { capsLock: Boolean(options.capsLock), numLock: Boolean(options.numLock),
      hasVirtualKeyboard: Boolean(options.hasVirtualKeyboard), physicalKeyboardType: options.physicalKeyboardType || KeyboardType.ALPHANUMERIC,
      accessible: Boolean(options.keyboardAccessible) };
    const nativeCursorSupported = options.supportsNativeCursor !== undefined
      ? Boolean(options.supportsNativeCursor)
      : Boolean(options.canvas?.style || options.viewport?.style);
    const mouseStateRecord = { cursor: 'auto', visible: true, supportsCursor: options.supportsCursor ?? true, supportsNativeCursor: nativeCursorSupported };
    const Keyboard = createKeyboard(() => keyboardState);
    let Mouse = null;
    function applyDomMouseCursor() {
      const style = options.canvas?.style;
      if (!style) return;
      if (!mouseStateRecord.visible) { style.cursor = 'none'; return; }
      const customCss = Mouse?.[cursorCssToken]?.();
      if (customCss) { style.cursor = customCss; return; }
      const c = mouseStateRecord.cursor;
      style.cursor = c === 'arrow' ? 'default' : c === 'button' ? 'pointer' : c === 'hand' ? 'grab' : c === 'ibeam' ? 'text' : 'auto';
    }
    Mouse = createMouse(mouseStateRecord, () => {
      applyDomMouseCursor();
      options.onMouseChanged?.();
    });
    const wmodeLower = String(options.wmode || options.acceleration || '').toLowerCase();
    const wmodeGPU = Boolean(options.wmodeGPU ?? (wmodeLower === 'gpu' || wmodeLower === 'gpu_web_gl'));
    function applyWmodeGpuSurface() {
      for (const el of [options.canvas, options.viewport]) {
        if (!el) continue;
        if (el.dataset) el.dataset.wmodeGpu = wmodeGPU ? 'true' : 'false';
        if (wmodeGPU && el.style) {
          el.style.willChange = 'transform';
          if (!el.style.transform) el.style.transform = 'translateZ(0)';
        }
      }
    }
    function stage2dContext() {
      if (options.context) return options.context;
      try { return options.canvas?.getContext?.('2d') || null; } catch (_) { return null; }
    }
    function applyQualitySurface(nextQuality) {
      const ctx = stage2dContext();
      if (ctx) {
        ctx.imageSmoothingEnabled = nextQuality !== 'LOW';
        if ('imageSmoothingQuality' in ctx) {
          ctx.imageSmoothingQuality = (nextQuality === 'LOW' || nextQuality === 'MEDIUM')
            ? 'low'
            : (nextQuality === 'HIGH' ? 'medium' : 'high');
        }
      }
      if (options.canvas?.dataset) options.canvas.dataset.stageQuality = nextQuality;
      if (options.canvas?.style) options.canvas.style.imageRendering = nextQuality === 'LOW' ? 'pixelated' : 'auto';
    }
    function detectColorCorrectionSupport() {
      if (COLOR_CORRECTION_SUPPORTS.has(options.colorCorrectionSupport)) return options.colorCorrectionSupport;
      const ctx = stage2dContext();
      const cs = ctx?.getContextAttributes?.()?.colorSpace;
      if (typeof cs === 'string' && cs.length > 0) return 'defaultOn';
      const mm = options.matchMedia || global.matchMedia;
      if (typeof mm === 'function') {
        try {
          if (mm('(color-gamut: p3)')?.matches || mm('(color-gamut: srgb)')?.matches) return 'defaultOn';
        } catch (_) {}
      }
      return 'unsupported';
    }
    function isColorCorrectionActive() {
      const sup = detectColorCorrectionSupport();
      if (sup === 'unsupported') return false;
      if (colorCorrection === 'on') return true;
      if (colorCorrection === 'off') return false;
      return sup === 'defaultOn';
    }
    function applyColorCorrectionSurface() {
      const active = isColorCorrectionActive();
      for (const el of [options.canvas, options.viewport]) {
        if (!el) continue;
        if (el.dataset) {
          el.dataset.colorCorrection = colorCorrection;
          el.dataset.colorCorrectionActive = active ? 'true' : 'false';
        }
        if (el.style) {
          el.style.colorInterpolation = active ? 'sRGB' : 'linearRGB';
        }
      }
      return active;
    }
    function pointerLockTarget() {
      return options.pointerLockTarget || options.canvas || options.viewport || null;
    }
    function clearMouseLockState() {
      if (!mouseLock) return;
      mouseLock = false;
      if (options.canvas?.dataset) options.canvas.dataset.mouseLock = 'false';
      options.onMouseLockChanged?.(false);
    }
    function requestStagePointerLock() {
      const target = pointerLockTarget();
      if (typeof options.requestPointerLock === 'function') {
        try {
          const res = options.requestPointerLock(target);
          if (res === false) { clearMouseLockState(); return; }
          if (res && typeof res.then === 'function') res.catch(() => clearMouseLockState());
        } catch (_) { clearMouseLockState(); return; }
      } else if (typeof target?.requestPointerLock === 'function') {
        try {
          const res = target.requestPointerLock();
          if (res === false) { clearMouseLockState(); return; }
          Promise.resolve(res).catch(() => clearMouseLockState());
        } catch (_) { clearMouseLockState(); return; }
      }
      if (options.canvas?.dataset) options.canvas.dataset.mouseLock = 'true';
    }
    function exitStagePointerLock() {
      const d = doc();
      const target = pointerLockTarget();
      if (typeof options.exitPointerLock === 'function') {
        options.exitPointerLock();
      } else if (d && typeof d.exitPointerLock === 'function' && (!d.pointerLockElement || d.pointerLockElement === target)) {
        try { d.exitPointerLock(); } catch (_) {}
      }
      if (options.canvas?.dataset) options.canvas.dataset.mouseLock = 'false';
    }
    applyWmodeGpuSurface();
    applyQualitySurface(quality);
    applyColorCorrectionSurface();
    let attached = false, over = null,focusVersion=0,focusDepth=0,nativeFocus=false;
    let captured=null,pressedTarget=null,clickTarget=null,hadPress=false,lastDeltaX=0,lastDeltaY=0;
    const deliveredPointers=new WeakMap();
    const cleanups = [];
    const stage = new Stage(stageToken);
    const doc = () => options.document || options.canvas?.ownerDocument || options.viewport?.ownerDocument || global.document || null;
    function buildFullScreenEvent(type, cancelable, fullScreen, interactive) {
      const FS = options.FullScreenEvent;
      if (FS) return new FS(type, false, Boolean(cancelable), Boolean(fullScreen), Boolean(interactive));
      const evt = Object.assign(new Event(type, false, Boolean(cancelable)), { fullScreen: Boolean(fullScreen), interactive: Boolean(interactive) });
      evt.clone = () => buildFullScreenEvent(evt.type, evt.cancelable, evt.fullScreen, evt.interactive);
      evt.toString = () => `[FullScreenEvent type="${evt.type}" bubbles=${evt.bubbles} cancelable=${evt.cancelable} eventPhase=${evt.eventPhase} fullScreen=${evt.fullScreen} interactive=${evt.interactive}]`;
      return evt;
    }
    function emitFullScreen(fullScreen, interactive) {
      const evt = buildFullScreenEvent(Event.FULLSCREEN || 'fullScreen', false, fullScreen, interactive);
      hub.dispatch(stage, evt);
      if (fullScreen && interactive) {
        const accepted = buildFullScreenEvent('fullScreenInteractiveAccepted', true, true, true);
        hub.dispatch(stage, accepted);
      }
    }
    const defaultStageContainer = {
      addChild(child) {
        if (!child || typeof child !== 'object') throw new TypeError('Stage.addChild requires a DisplayObject.');
        if (child === stage) throw argumentError('Stage cannot be added as a child of itself.');
        const existing = fallbackChildren.indexOf(child);
        if (existing >= 0) fallbackChildren.splice(existing, 1);
        fallbackChildren.push(child);
        return child;
      },
      addChildAt(child, index) {
        if (!child || typeof child !== 'object') throw new TypeError('Stage.addChildAt requires a DisplayObject.');
        if (child === stage) throw argumentError('Stage cannot be added as a child of itself.');
        const idx = Number(index);
        const max = fallbackChildren.includes(child) ? fallbackChildren.length - 1 : fallbackChildren.length;
        if (!Number.isInteger(idx) || idx < 0 || idx > max) throw new RangeError('Stage child index is out of bounds.');
        const existing = fallbackChildren.indexOf(child);
        if (existing >= 0) fallbackChildren.splice(existing, 1);
        fallbackChildren.splice(idx, 0, child);
        return child;
      },
      removeChild(child) {
        const idx = fallbackChildren.indexOf(child);
        if (idx < 0) throw argumentError('DisplayObject is not a child of Stage.');
        fallbackChildren.splice(idx, 1);
        return child;
      },
      removeChildAt(index) {
        const idx = Number(index);
        if (!Number.isInteger(idx) || idx < 0 || idx >= fallbackChildren.length) throw new RangeError('Stage child index is out of bounds.');
        return fallbackChildren.splice(idx, 1)[0];
      },
      getChildAt(index) {
        const idx = Number(index);
        if (!Number.isInteger(idx) || idx < 0 || idx >= fallbackChildren.length) throw new RangeError('Stage child index is out of bounds.');
        return fallbackChildren[idx];
      },
      getChildByName(name) {
        const wanted = String(name);
        return fallbackChildren.find(item => item?.name === wanted) || null;
      },
      getChildIndex(child) {
        const idx = fallbackChildren.indexOf(child);
        if (idx < 0) throw argumentError('DisplayObject is not a child of Stage.');
        return idx;
      },
      setChildIndex(child, index) {
        const current = fallbackChildren.indexOf(child);
        if (current < 0) throw argumentError('DisplayObject is not a child of Stage.');
        const idx = Number(index);
        if (!Number.isInteger(idx) || idx < 0 || idx >= fallbackChildren.length) throw new RangeError('Stage child index is out of bounds.');
        fallbackChildren.splice(current, 1);
        fallbackChildren.splice(idx, 0, child);
      },
      swapChildren(a, b) {
        const ia = fallbackChildren.indexOf(a), ib = fallbackChildren.indexOf(b);
        if (ia < 0 || ib < 0) throw argumentError('DisplayObject is not a child of Stage.');
        fallbackChildren[ia] = b; fallbackChildren[ib] = a;
      },
      swapChildrenAt(ia, ib) {
        const a = Number(ia), b = Number(ib);
        if (!Number.isInteger(a) || !Number.isInteger(b) || a < 0 || b < 0 || a >= fallbackChildren.length || b >= fallbackChildren.length) {
          throw new RangeError('Stage child index is out of bounds.');
        }
        const tmp = fallbackChildren[a]; fallbackChildren[a] = fallbackChildren[b]; fallbackChildren[b] = tmp;
      },
      contains(child) {
        if (!child || typeof child !== 'object') throw new TypeError('Stage.contains requires a DisplayObject.');
        if (child === stage) return true;
        let cur = child;
        for (let depth = 0; cur && depth < 65; depth++) {
          if (cur === stage || fallbackChildren.includes(cur)) return true;
          cur = options.parentOf ? options.parentOf(cur) : cur.parent;
        }
        return false;
      },
    };
    const container = () => options.stageContainer || defaultStageContainer;
    function docFullscreenElement() {
      const d = doc();
      return d?.fullscreenElement || d?.webkitFullscreenElement || null;
    }
    function requestViewportFullscreen() {
      const vp = options.viewport;
      if (vp?.requestFullscreen) return vp.requestFullscreen();
      if (vp?.webkitRequestFullscreen) return vp.webkitRequestFullscreen();
      return undefined;
    }
    function exitDocFullscreen() {
      const d = doc();
      if (d?.exitFullscreen) return d.exitFullscreen();
      if (d?.webkitExitFullscreen) return d.webkitExitFullscreen();
      return undefined;
    }
    function allowsFs(interactive) {
      const explicit = interactive ? (options.allowsFullScreenInteractive ?? options.allowsFullScreen) : options.allowsFullScreen;
      if (explicit !== undefined) return Boolean(explicit);
      const d = doc();
      const hasRequest = Boolean(options.requestFullscreen || options.viewport?.requestFullscreen || options.viewport?.webkitRequestFullscreen);
      const enabledFlag = d ? (d.fullscreenEnabled ?? d.webkitFullscreenEnabled) !== false : Boolean(options.requestFullscreen);
      return Boolean(hasRequest && enabledFlag);
    }
    Object.defineProperties(stage, {
      stageWidth: { enumerable: true, get: () => (displayState !== 'normal' && fullScreenSourceRect && fullScreenSourceRect.width > 0) ? fullScreenSourceRect.width : width },
      stageHeight: { enumerable: true, get: () => (displayState !== 'normal' && fullScreenSourceRect && fullScreenSourceRect.height > 0) ? fullScreenSourceRect.height : height },
      width: { enumerable: true, get: () => options.getStageContentSize ? Number(options.getStageContentSize().width) || 0 : 0,
        set() { throw illegalOperationError(); } },
      height: { enumerable: true, get: () => options.getStageContentSize ? Number(options.getStageContentSize().height) || 0 : 0,
        set() { throw illegalOperationError(); } },
      frameRate: { enumerable: true, get: () => options.getFrameRate ? options.getFrameRate() : frameRate,
        set(value) { const next = rate(value); if (options.setFrameRate) options.setFrameRate(next); frameRate = next; } },
      scaleMode: { enumerable: true, get: () => scaleMode,
        set(value) {
          const next = String(value);
          if (!SCALE_MODES.has(next)) throw argumentError(`Stage.scaleMode must be showAll, exactFit, noBorder or noScale; got '${next}'.`);
          if (scaleMode !== next) { scaleMode = next; options.onScaleModeChanged?.(scaleMode); }
        } },
      align: { enumerable: true, get: () => align,
        set(value) {
          const key = String(value).toUpperCase();
          if (!Object.hasOwn(ALIGN_MODES, key)) throw argumentError(`Stage.align is invalid: '${value}'.`);
          const next = ALIGN_MODES[key];
          if (align !== next) { align = next; options.onAlignChanged?.(align); }
        } },
      quality: { enumerable: true, get: () => quality,
        set(value) {
          const key = String(value).toLowerCase();
          if (!Object.hasOwn(QUALITY_MODES, key)) throw argumentError(`Stage.quality is invalid: '${value}'.`);
          const next = QUALITY_MODES[key];
          if (quality !== next) {
            quality = next;
            applyQualitySurface(quality);
            options.onQualityChanged?.(quality);
          }
        } },
      displayState: { enumerable: true, get: () => displayState,
        set(value) {
          const next = String(value);
          if (!DISPLAY_STATES.has(next)) throw argumentError(`Stage.displayState must be normal, fullScreen or fullScreenInteractive; got '${next}'.`);
          if (next === 'normal') {
            if (displayState !== 'normal') {
              const wasInteractive = displayState === 'fullScreenInteractive';
              const wasLocked = mouseLock;
              displayState = 'normal'; mouseLock = false;
              if (wasLocked) exitStagePointerLock();
              if (options.exitFullscreen) options.exitFullscreen();
              else if (docFullscreenElement()) exitDocFullscreen();
              options.onDisplayStateChanged?.(displayState);
              emitFullScreen(false, wasInteractive);
            }
            return;
          }
          if (next !== 'fullScreenInteractive' && mouseLock) {
            mouseLock = false;
            exitStagePointerLock();
          }
          if (!allowsFs(next === 'fullScreenInteractive')) {
            throw securityError('Stage.displayState fullscreen requires browser Fullscreen API permission and an active user gesture.');
          }
          if (options.requestFullscreen) {
            if (options.requestFullscreen(next) === false) throw securityError('Stage.displayState fullscreen request was denied.');
          } else {
            const activation = global.navigator?.userActivation;
            if (activation && !activation.isActive) throw securityError('Stage.displayState fullscreen requires an active user gesture.');
            Promise.resolve(requestViewportFullscreen()).catch(() => {});
          }
          const changed = displayState !== next;
          displayState = next;
          options.onDisplayStateChanged?.(displayState);
          if (changed) emitFullScreen(true, next === 'fullScreenInteractive');
        } },
      color: { enumerable: true, get: () => stageColor,
        set(value) { const next = stageRgbValue(value); stageColor = next; colorAssigned = true; options.onColorChanged?.(next); } },
      colorCorrection: { enumerable: true, get: () => colorCorrection,
        set(value) {
          const next = String(value);
          if (!COLOR_CORRECTIONS.has(next)) throw argumentError(`Stage.colorCorrection must be 'default', 'on' or 'off'; got '${next}'.`);
          if (colorCorrection !== next) {
            colorCorrection = next;
            const active = applyColorCorrectionSurface();
            options.onColorCorrectionChanged?.(active, colorCorrection);
          }
        } },
      colorCorrectionSupport: { enumerable: true, get: () => detectColorCorrectionSupport() },
      fullScreenSourceRect: { enumerable: true,
        get: () => fullScreenSourceRect ? makeRectangle(fullScreenSourceRect.x, fullScreenSourceRect.y, fullScreenSourceRect.width, fullScreenSourceRect.height) : null,
        set(value) {
          if (value === null) { fullScreenSourceRect = null; options.onFullScreenSourceRectChanged?.(null); return; }
          if (!isRectangle(value)) throw new TypeError('Stage.fullScreenSourceRect requires a Rectangle or null.');
          fullScreenSourceRect = { x: Number(value.x), y: Number(value.y), width: Number(value.width), height: Number(value.height) };
          options.onFullScreenSourceRectChanged?.(fullScreenSourceRect);
        } },
      mouseLock: { enumerable: true, get: () => mouseLock,
        set(value) {
          if (typeof value !== 'boolean') throw new TypeError('Stage.mouseLock requires a Boolean.');
          if (value && displayState !== 'fullScreenInteractive') throw illegalOperationError('Stage.mouseLock requires StageDisplayState.FULL_SCREEN_INTERACTIVE.');
          if (mouseLock !== value) {
            mouseLock = value;
            if (value) requestStagePointerLock();
            else exitStagePointerLock();
            options.onMouseLockChanged?.(mouseLock);
          }
        } },
      wmodeGPU: { enumerable: true, get: () => wmodeGPU },
      softKeyboardRect: { enumerable: true, get: () => options.getSoftKeyboardRect ? options.getSoftKeyboardRect() : makeRectangle(0, 0, 0, 0) },
      stage3Ds: { enumerable: true, configurable: true, get: () => options.getStage3Ds ? options.getStage3Ds() : cursorVectorFactory([]) },
      stageVideos: { enumerable: true, configurable: true, get: () => {
        const store = Stage.prototype[Symbol.for('neoflash.mediaEvents.stageHooks')];
        const fn = store?.byStage?.get(stage) || store?.fallback;
        if (fn) return fn(stage);
        return options.getStageVideos ? options.getStageVideos() : cursorVectorFactory([]);
      } },
      browserZoomFactor: { enumerable: true, get: () => {
        const zoom = Number(options.browserZoomFactor ?? global.devicePixelRatio ?? 1);
        return Number.isFinite(zoom) && zoom > 0 ? zoom : 1;
      } },
      textSnapshot: { enumerable: true, get() { throw illegalOperationError(); }, set() { throw illegalOperationError(); } },
      fullScreenWidth: { enumerable: true, get: () => screenMetric(options.screen || global.screen, 'width', width) },
      fullScreenHeight: { enumerable: true, get: () => screenMetric(options.screen || global.screen, 'height', height) },
      allowsFullScreen: { enumerable: true, get: () => allowsFs(false) },
      allowsFullScreenInteractive: { enumerable: true, get: () => allowsFs(true) },
      contentsScaleFactor: { enumerable: true, get: () => {
        const dpr = Number(options.contentsScaleFactor ?? options.devicePixelRatio ?? global.devicePixelRatio ?? 1);
        return Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
      } },
      numChildren: { enumerable: true, get: () => options.numChildren ? Math.max(0, Number(options.numChildren()) | 0) : (fallbackChildren.length || 1) },
      mouseChildren: { enumerable: true, get: () => mouseChildren,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.mouseChildren requires a Boolean.'); mouseChildren = value; } },
      tabChildren: { enumerable: true, get: () => tabChildren,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.tabChildren requires a Boolean.'); const changed = tabChildren !== value; tabChildren = value; if (changed) hub.dispatch(stage, new Event(Event.TAB_CHILDREN_CHANGE || 'tabChildrenChange', true, false)); } },
      doubleClickEnabled: { enumerable: true, get: () => doubleClickEnabled,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.doubleClickEnabled requires a Boolean.'); doubleClickEnabled = value; } },
      mouseEnabled: { enumerable: true, get: () => true,
        set() { throw illegalOperationError(); } },
      tabEnabled: { enumerable: true, get: () => false,
        set() { throw illegalOperationError(); } },
      tabIndex: { enumerable: true, get: () => -1,
        set() { throw illegalOperationError(); } },
      focusRect: { enumerable: true, get: () => null,
        set() { throw illegalOperationError(); } },
      contextMenu: { enumerable: true, get: () => null,
        set() { throw illegalOperationError(); } },
      accessibilityImplementation: { enumerable: true, get: () => null,
        set() { throw illegalOperationError(); } },
      accessibilityProperties: { enumerable: true, get: () => null,
        set() { throw illegalOperationError(); } },
      showDefaultContextMenu: { enumerable: true, get: () => showDefaultContextMenu,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.showDefaultContextMenu requires a Boolean.'); showDefaultContextMenu = value; } },
      stageFocusRect: { enumerable: true, get: () => stageFocusRect,
        set(value) { if (typeof value !== 'boolean') throw new TypeError('Stage.stageFocusRect requires a Boolean.'); stageFocusRect = value; } },
      mouseX: { enumerable: true, get: () => mouseX }, mouseY: { enumerable: true, get: () => mouseY },
      focus: { enumerable: true, get() { if (focus && !alive(focus)) changeFocus(null); return (focus && options.isObjectInaccessible?.(focus)) ? null : focus; },
        set(value) {changeFocus(value, 'programmatic');} },
      stage: { enumerable: true, get: () => stage }, parent: { enumerable: true, get: () => null },
      root: { enumerable: true, get: () => stage },
      assignFocus: { value(objectToFocus, direction) {
        const dir = String(direction);
        if (!FOCUS_DIRECTIONS.has(dir)) throw argumentError(`Stage.assignFocus direction must be 'none', 'top' or 'bottom'; got '${dir}'.`);
        changeFocus(objectToFocus, 'programmatic', {}, dir);
      } },
      isFocusInaccessible: { value: () => {
        if (focus && !alive(focus)) changeFocus(null);
        return Boolean(focus && options.isObjectInaccessible?.(focus));
      } },
      getObjectsUnderPoint: { value: pt => {
        if (!isPoint(pt)) throw new TypeError('Stage.getObjectsUnderPoint requires a Point.');
        return container().getObjectsUnderPoint ? container().getObjectsUnderPoint(pt) : [];
      } },
      areInaccessibleObjectsUnderPoint: { value: pt => {
        if (!isPoint(pt)) throw new TypeError('Stage.areInaccessibleObjectsUnderPoint requires a Point.');
        return container().areInaccessibleObjectsUnderPoint ? Boolean(container().areInaccessibleObjectsUnderPoint(pt)) : false;
      } },
      addChild: { value: child => container().addChild(child) },
      addChildAt: { value: (child, index) => container().addChildAt(child, index) },
      removeChild: { value: child => container().removeChild(child) },
      removeChildAt: { value: index => container().removeChildAt(index) },
      getChildAt: { value: index => container().getChildAt(index) },
      getChildByName: { value: name => container().getChildByName(name) },
      getChildIndex: { value: child => container().getChildIndex(child) },
      setChildIndex: { value: (child, index) => container().setChildIndex(child, index) },
      swapChildren: { value: (a, b) => container().swapChildren(a, b) },
      swapChildrenAt: { value: (a, b) => container().swapChildrenAt(a, b) },
      contains: { value: child => container().contains(child) },
      invalidate: { value() { if (!invalidated) { invalidated = true; options.onInvalidate?.(); } } },
      toString: { value: () => '[NeoFlash Stage]' },
    });
    const stageMethods = hub.methods(stage);
    Object.assign(stage, {
      ...stageMethods,
      addEventListener(type, listener, capture = false, priority = 0, weak = false) {
        stageMethods.addEventListener(type, listener, capture, priority, weak);
        if (String(type) === 'stageVideoAvailability') {
          const store = Stage.prototype[Symbol.for('neoflash.mediaEvents.stageHooks')];
          const fn = store?.availByStage?.get(stage) || store?.availFallback;
          if (typeof fn === 'function') fn(stage);
        }
      },
    });
    // Stage is a sealed native surface; unsupported scale/fullscreen properties must
    // not appear to work merely because JavaScript accepts arbitrary new fields.
    Object.freeze(stage);
    function changeFocus(value,reason='programmatic',domEvent={},direction='none') {
      if(value!==null&&(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('Stage.focus requires a display object or null.');
      if(value!==null&&!alive(value))throw new Error('Stage.focus requires an active display object or null.');
      if(value!==null&&options.canFocus&&!options.canFocus(value)){
        if(reason==='mouse')value=null;
        else throw new Error('This display object is not currently focusable.');
      }
      if(value===focus){if(value!==null)focusReason=reason;return true;}
      if(focusDepth>=64)throw new RangeError('Recursive focus changes exceeded 64 transitions.');
      focusDepth++;
      try{
        const previous=focus;
        if((reason==='mouse'||reason==='key')&&previous&&alive(previous)){
          const event=new FocusEvent(reason==='mouse'?FocusEvent.MOUSE_FOCUS_CHANGE:FocusEvent.KEY_FOCUS_CHANGE,true,true,value,domEvent.shiftKey,reason==='key'?9:0);
          if(!hub.dispatch(previous,event)){domEvent.preventDefault?.();return false;}
          if(focus!==previous)return false;
        }
        focus=value;focusReason=value?reason:null;nativeFocus=false;const version=++focusVersion;
        if(previous&&alive(previous))hub.dispatch(previous,new FocusEvent(FocusEvent.FOCUS_OUT,true,false,value));
        if(version!==focusVersion)return true;
        const handled=options.onFocusChanged?.(value,previous)===true;
        if(version!==focusVersion)return true;
        nativeFocus=handled;
        if(!handled)syncStandaloneImeClientInput(value);
        if(value&&!handled&&!standaloneImeInput)options.canvas?.focus?.({preventScroll:true});
        if(value&&alive(value))hub.dispatch(value,new FocusEvent(FocusEvent.FOCUS_IN,true,false,previous,false,0,direction));
        return true;
      }finally{focusDepth--;}
    }
    function size(value, name) {
      value = Number(value);
      if (!Number.isFinite(value) || value <= 0) throw new TypeError(`${name} must be positive and finite.`);
      return value;
    }
    function rate(value) {
      value = Number(value);
      if (!Number.isFinite(value) || value <= 0 || value > 1000) throw new RangeError('Stage.frameRate must be in (0, 1000].');
      return value;
    }
    function enabled() { return !options.isInputEnabled || options.isInputEnabled(); }
    function targetAt(domEvent, explicitTarget) {
      if (!mouseChildren) return stage;
      const supplied=explicitTarget&&explicitTarget!==stage&&options.resolveInput?options.resolveInput(explicitTarget):explicitTarget;
      const target = explicitTarget ? supplied||stage : options.hitTest?.(domEvent)||stage;
      return target === stage || alive(target) ? target : stage;
    }
    function updatePoint(domEvent) {
      const prevX = mouseX, prevY = mouseY;
      const point = options.coordinates?.(domEvent);
      if (point && point.length === 2 && point.every(Number.isFinite)) {
        [mouseX, mouseY] = point;
        lastDeltaX = mouseX - prevX;
        lastDeltaY = mouseY - prevY;
      } else if ((mouseLock || optInPointerLock) && (Number.isFinite(domEvent.movementX) || Number.isFinite(domEvent.movementY))) {
        const dx = Number(domEvent.movementX) || 0;
        const dy = Number(domEvent.movementY) || 0;
        mouseX = Math.max(0, Math.min(width, mouseX + dx));
        mouseY = Math.max(0, Math.min(height, mouseY + dy));
        lastDeltaX = dx;
        lastDeltaY = dy;
      } else {
        lastDeltaX = 0;
        lastDeltaY = 0;
      }
    }
    function mouse(type, target, domEvent, related = null) {
      const point = target === stage ? [mouseX, mouseY] : options.localPoint?.(target, mouseX, mouseY) || [mouseX, mouseY];
      const event = new MouseEvent(type, ![MouseEvent.ROLL_OVER,MouseEvent.ROLL_OUT].includes(type), type === MouseEvent.CONTEXT_MENU ? false : true, point[0], point[1], related,
        Boolean(domEvent.ctrlKey || domEvent.metaKey), domEvent.altKey, domEvent.shiftKey,
        Boolean((domEvent.buttons || 0) & 1), type === MouseEvent.MOUSE_WHEEL ? -Math.sign(domEvent.deltaY || 0) : 0,
        domEvent.metaKey, domEvent.ctrlKey, domEvent.detail || 0);
      if (Number.isFinite(domEvent.movementX)) event.movementX = Number(domEvent.movementX);
      else if ((mouseLock || optInPointerLock) && type === MouseEvent.MOUSE_MOVE) event.movementX = lastDeltaX;
      if (Number.isFinite(domEvent.movementY)) event.movementY = Number(domEvent.movementY);
      else if ((mouseLock || optInPointerLock) && type === MouseEvent.MOUSE_MOVE) event.movementY = lastDeltaY;
      if (related && options.isObjectInaccessible?.(related)) {
        event.relatedObject = null;
        event.isRelatedObjectInaccessible = true;
      }
      return setStagePoint(event, mouseX, mouseY);
    }
    function deliver(target, event, domEvent) {
      const result = hub.dispatch(target, event);
      if (!result && domEvent.cancelable !== false) domEvent.preventDefault?.();
      return result;
    }
    function releaseCapture(){const id=captured;captured=null;if(id!==null&&options.canvas?.hasPointerCapture?.(id))options.canvas.releasePointerCapture(id);}
    function cancelPointer(){pressedTarget=null;clickTarget=null;hadPress=false;releaseCapture();over=null;return options.cancelPointer?.();}
    function chain(target){const result=[];while(target&&target!==stage&&alive(target)&&result.length<65){result.push(target);target=options.parentOf?options.parentOf(target):target.parent;}return result;}
    function hover(target,domEvent){
      if(target===over)return;const previous=over;over=target;
      if(previous&&previous!==stage&&alive(previous))deliver(previous,mouse(MouseEvent.MOUSE_OUT,previous,domEvent,target),domEvent);
      const old=chain(previous),next=chain(target);
      for(const item of old)if(!next.includes(item)&&alive(item))deliver(item,mouse(MouseEvent.ROLL_OUT,item,domEvent,target),domEvent);
      for(const item of [...next].reverse())if(!old.includes(item)&&alive(item))deliver(item,mouse(MouseEvent.ROLL_OVER,item,domEvent,previous),domEvent);
      if(target!==stage&&alive(target))deliver(target,mouse(MouseEvent.MOUSE_OVER,target,domEvent,previous),domEvent);
    }
    function handlePointer(type, domEvent = {}, explicitTarget = null, physicalPick = false) {
      if(['pointercancel','lostpointercapture','cancel'].includes(type)){if((type!=='lostpointercapture'||captured!==null)&&cancelPointer())options.afterInput?.();return true;}
      if(!enabled()||domEvent.pointerType==='touch')return true;
      if(domEvent.isPrimary===false&&/^(?:pointerdown|pointerup|pointermove)$/.test(type))return true;
      if (type === 'mouseLeave') {
        hover(stage, domEvent);
        hub.dispatch(stage, new Event(Event.MOUSE_LEAVE || 'mouseLeave', false, false));
        options.afterInput?.();
        return true;
      }
      const isPointerLeave = type === 'pointerleave';
      if((type==='pointerdown'||type==='mousedown')&&domEvent.button===1)type='middleMouseDown';
      else if((type==='pointerdown'||type==='mousedown')&&domEvent.button===2)type='rightMouseDown';
      else if((type==='pointerup'||type==='mouseup')&&domEvent.button===1)type='middleMouseUp';
      else if((type==='pointerup'||type==='mouseup')&&domEvent.button===2)type='rightMouseUp';
      else if(type==='auxclick'&&domEvent.button===1)type='middleClick';
      else if(type==='auxclick'&&domEvent.button===2)type='rightClick';
      const mapped={pointerdown:'mouseDown',pointerup:'mouseUp',pointermove:'mouseMove',pointerleave:'mouseMove',mousedown:'mouseDown',mouseup:'mouseUp',mousemove:'mouseMove',dblclick:'doubleClick',wheel:'mouseWheel'};
      type=mapped[type]||type;
      if(!['mouseDown','mouseUp','mouseMove','mouseWheel','click','doubleClick','middleMouseDown','middleMouseUp','middleClick','rightMouseDown','rightMouseUp','rightClick','contextMenu','releaseOutside'].includes(type))throw new Error(`Unsupported native mouse event '${type}'.`);
      // Native editor events are forwarded directly, then bubble to the outer
      // pointer listener. One physical Event must enter the host only once.
      if(domEvent&&typeof domEvent==='object'&&typeof domEvent.type==='string'){let seen=deliveredPointers.get(domEvent);if(seen?.has(type))return true;if(!seen){seen=new Set();deliveredPointers.set(domEvent,seen);}seen.add(type);}
      updatePoint(domEvent);options.beforePointer?.(type,domEvent);
      const target=isPointerLeave?stage:targetAt(domEvent,explicitTarget),primary=domEvent.button===undefined||domEvent.button===0;
      if(type==='doubleClick'&&target?.doubleClickEnabled===false)return true;
      if(type==='click'&&(!explicitTarget||physicalPick)&&hadPress&&clickTarget!==target)return false;
      if(type===MouseEvent.MOUSE_DOWN&&primary){
        pressedTarget=target;clickTarget=null;hadPress=true;
        if(domEvent.target===options.canvas&&Number.isInteger(domEvent.pointerId)&&options.canvas?.setPointerCapture){options.canvas.setPointerCapture(domEvent.pointerId);captured=domEvent.pointerId;}
        const changed=changeFocus(target===stage?null:target,'mouse',domEvent);
        if(changed&&nativeFocus&&domEvent.target===options.canvas)domEvent.preventDefault?.();
      }
      if(type===MouseEvent.MOUSE_MOVE||type===MouseEvent.MOUSE_DOWN)hover(target,domEvent);
      if(target!==stage&&!alive(target)){over=null;options.afterInput?.();return true;}
      if(type===MouseEvent.MOUSE_UP&&primary){
        const oldPressed=pressedTarget;
        clickTarget=(oldPressed===target||Boolean(hadPress&&target&&target!==stage&&target.trackAsMenu===true))?target:null;pressedTarget=null;releaseCapture();
        if(oldPressed&&oldPressed!==target&&oldPressed!==stage&&alive(oldPressed)){
          deliver(oldPressed,mouse(MouseEvent.RELEASE_OUTSIDE||'releaseOutside',oldPressed,domEvent,target),domEvent);
        }
      }
      const result=deliver(target,mouse(type,target,domEvent),domEvent);
      if (isPointerLeave) hub.dispatch(stage, new Event(Event.MOUSE_LEAVE || 'mouseLeave', false, false));
      if(target===stage||alive(target))options.onLegacyInput?.(type,target,domEvent);
      options.onPointerTarget?.(target,type);options.afterInput?.();return result;
    }
    const activeTouches = new Map();
    let imeComposing = false, imeText = '';
    function buildTouchEvent(type, target, point, touchId, isPrimary, domEvent, touchItem = {}, related = null) {
      const local = target === stage ? point : (options.localPoint?.(target, point[0], point[1]) || point);
      const bubbles = !['touchRollOver', 'touchRollOut'].includes(type);
      const sizeX = Number.isFinite(touchItem.radiusX) ? Number(touchItem.radiusX) * 2 : (Number.isFinite(touchItem.width) ? Number(touchItem.width) : 1);
      const sizeY = Number.isFinite(touchItem.radiusY) ? Number(touchItem.radiusY) * 2 : (Number.isFinite(touchItem.height) ? Number(touchItem.height) : 1);
      const pressure = Number.isFinite(touchItem.force) ? Number(touchItem.force) : (Number.isFinite(touchItem.pressure) ? Number(touchItem.pressure) : 1);
      const ctrl = Boolean(domEvent.ctrlKey || domEvent.metaKey);
      const alt = Boolean(domEvent.altKey);
      const shift = Boolean(domEvent.shiftKey);
      const cmd = Boolean(domEvent.metaKey);
      const control = Boolean(domEvent.ctrlKey);
      const TE = options.TouchEvent;
      if (typeof TE === 'function') {
        const evt = new TE(type, bubbles, false, touchId, Boolean(isPrimary), local[0], local[1], sizeX, sizeY, pressure, related, ctrl, alt, shift, cmd, control);
        return setStagePoint(evt, point[0], point[1]);
      }
      const evt = new Event(type, bubbles, false);
      Object.assign(evt, {
        touchPointID: Number(touchId) | 0,
        isPrimaryTouchPoint: Boolean(isPrimary),
        localX: Number(local[0]) || 0,
        localY: Number(local[1]) || 0,
        stageX: Number(point[0]) || 0,
        stageY: Number(point[1]) || 0,
        sizeX,
        sizeY,
        pressure,
        relatedObject: related,
        ctrlKey: ctrl,
        altKey: alt,
        shiftKey: shift,
        commandKey: cmd,
        controlKey: control,
        updateAfterEvent() { stage.invalidate(); },
      });
      evt.clone = () => buildTouchEvent(evt.type, target, point, touchId, isPrimary, domEvent, touchItem, related);
      return evt;
    }
    function handleTouch(type, domEvent = {}, explicitTarget = null) {
      if (!enabled()) return true;
      const mapped = {
        touchstart: 'touchBegin',
        touchmove: 'touchMove',
        touchend: 'touchEnd',
        touchcancel: 'touchEnd',
        pointerdown: 'touchBegin',
        pointermove: 'touchMove',
        pointerup: 'touchEnd',
        pointercancel: 'touchEnd',
        touchBegin: 'touchBegin',
        touchMove: 'touchMove',
        touchEnd: 'touchEnd',
      }[type];
      if (!mapped) throw new Error(`Unsupported touch event '${type}'.`);
      const list = Array.from(domEvent.changedTouches || domEvent.touches || [domEvent]);
      let allAllowed = true;
      for (const item of list) {
        const touchId = Number.isInteger(item.identifier) ? item.identifier : (Number.isInteger(item.pointerId) ? item.pointerId : (Number.isInteger(item.touchPointID) ? item.touchPointID : 0));
        const coords = options.coordinates?.(item) || options.coordinates?.(domEvent) || [
          Number.isFinite(item.stageX) ? Number(item.stageX) : (Number.isFinite(item.clientX) ? Number(item.clientX) : mouseX),
          Number.isFinite(item.stageY) ? Number(item.stageY) : (Number.isFinite(item.clientY) ? Number(item.clientY) : mouseY),
        ];
        const pt = [Number(coords[0]) || 0, Number(coords[1]) || 0];
        const target = targetAt(item?.target ? item : domEvent, explicitTarget);
        if (mapped === 'touchBegin') {
          const isPrimary = activeTouches.size === 0 || item.isPrimary === true || item.isPrimaryTouchPoint === true;
          activeTouches.set(touchId, { target, startTarget: target, stageX: pt[0], stageY: pt[1], isPrimary });
          if (isPrimary) { [mouseX, mouseY] = pt; }
          if (target !== stage && alive(target)) {
            for (const anc of [...chain(target)].reverse()) {
              if (alive(anc)) deliver(anc, buildTouchEvent('touchRollOver', anc, pt, touchId, isPrimary, domEvent, item, null), domEvent);
            }
            deliver(target, buildTouchEvent('touchOver', target, pt, touchId, isPrimary, domEvent, item, null), domEvent);
          }
          if (!deliver(target, buildTouchEvent('touchBegin', target, pt, touchId, isPrimary, domEvent, item, null), domEvent)) allAllowed = false;
        } else if (mapped === 'touchMove') {
          const prev = activeTouches.get(touchId);
          const isPrimary = prev ? prev.isPrimary : (touchId === 0 || item.isPrimary === true);
          if (isPrimary) { [mouseX, mouseY] = pt; }
          if (prev && prev.target !== target) {
            const oldTarget = prev.target;
            prev.target = target;
            if (oldTarget && oldTarget !== stage && alive(oldTarget)) {
              deliver(oldTarget, buildTouchEvent('touchOut', oldTarget, pt, touchId, isPrimary, domEvent, item, target), domEvent);
            }
            const oldChain = chain(oldTarget), nextChain = chain(target);
            for (const anc of oldChain) if (!nextChain.includes(anc) && alive(anc)) deliver(anc, buildTouchEvent('touchRollOut', anc, pt, touchId, isPrimary, domEvent, item, target), domEvent);
            for (const anc of [...nextChain].reverse()) if (!oldChain.includes(anc) && alive(anc)) deliver(anc, buildTouchEvent('touchRollOver', anc, pt, touchId, isPrimary, domEvent, item, oldTarget), domEvent);
            if (target !== stage && alive(target)) {
              deliver(target, buildTouchEvent('touchOver', target, pt, touchId, isPrimary, domEvent, item, oldTarget), domEvent);
            }
          } else if (prev) {
            prev.stageX = pt[0]; prev.stageY = pt[1];
          }
          if (!deliver(target, buildTouchEvent('touchMove', target, pt, touchId, isPrimary, domEvent, item, null), domEvent)) allAllowed = false;
        } else if (mapped === 'touchEnd') {
          const prev = activeTouches.get(touchId);
          activeTouches.delete(touchId);
          const isPrimary = prev ? prev.isPrimary : (touchId === 0 || item.isPrimary === true);
          if (isPrimary) { [mouseX, mouseY] = pt; }
          if (!deliver(target, buildTouchEvent('touchEnd', target, pt, touchId, isPrimary, domEvent, item, null), domEvent)) allAllowed = false;
          if (prev && prev.startTarget === target && type !== 'touchcancel' && type !== 'pointercancel') {
            deliver(target, buildTouchEvent('touchTap', target, pt, touchId, isPrimary, domEvent, item, null), domEvent);
          }
          if (target !== stage && alive(target)) {
            deliver(target, buildTouchEvent('touchOut', target, pt, touchId, isPrimary, domEvent, item, null), domEvent);
            for (const anc of chain(target)) if (alive(anc)) deliver(anc, buildTouchEvent('touchRollOut', anc, pt, touchId, isPrimary, domEvent, item, null), domEvent);
          }
        }
      }
      options.afterInput?.();
      return allAllowed;
    }
    const AS3_IME_REGISTRY = Symbol.for('neoflash.as3.imeRegistry.v1');
    const AS2_IME_DOC_HOOKS = Symbol.for('neoflash.as2.imeDocHooks.v1');
    let standaloneImeInput = null;
    let standaloneImeCleanups = [];
    let standaloneImeClient = null;
    let standaloneImeBaseStart = 0;
    let activeStageContextMenuEl = null;
    let stageContextClipboardBuffer = '';
    let stageAssignedContextMenu = null;

    function getAs3StageImeEntry() {
      let reg = global[AS3_IME_REGISTRY];
      if (!reg) {
        reg = { byDisplay: new WeakMap(), byStage: new WeakMap(), latest: null };
        Object.defineProperty(global, AS3_IME_REGISTRY, { value: reg, configurable: true });
      }
      let entry = reg.byStage.get(stage);
      if (!entry && reg.latest && (!reg.latest.fte3d || reg.latest.fte3d.getStage?.() === stage)) {
        entry = reg.latest;
        reg.byStage.set(stage, entry);
      }
      if (!entry) {
        entry = { netSys: null, fte3d: null, stage: null };
        reg.byStage.set(stage, entry);
      }
      return entry;
    }

    function teardownStandaloneImeInput() {
      for (const fn of standaloneImeCleanups.splice(0)) {
        try { fn(); } catch (_) {}
      }
      if (standaloneImeInput) {
        try { standaloneImeInput.remove?.(); } catch (_) {}
        standaloneImeInput = null;
      }
      standaloneImeClient = null;
    }

    function syncStandaloneImeInputAttrs(targetObj, clientObj) {
      if (!standaloneImeInput) return;
      const entry = getAs3StageImeEntry();
      const imeOn = entry?.netSys ? Boolean(entry.netSys.isEnabled()) : true;
      const mode = entry?.netSys ? String(entry.netSys.getConversionMode() || 'ALPHANUMERIC_HALF') : 'ALPHANUMERIC_HALF';
      const editable = imeOn && !(clientObj && clientObj.readOnly === true);
      standaloneImeInput.readOnly = !editable;
      standaloneImeInput.setAttribute?.('data-ime-enabled', String(editable));
      standaloneImeInput.setAttribute?.('data-ime-mode', mode);
      if (clientObj && isStageImeClient(clientObj) && standaloneImeInput.style) {
        standaloneImeInput.style.writingMode = clientObj.verticalTextLayout ? 'vertical-rl' : 'horizontal-tb';
        try {
          const a = Number(clientObj.selectionAnchorIndex ?? 0) | 0;
          const b = Number(clientObj.selectionActiveIndex ?? a) | 0;
          const lo = Math.max(0, Math.min(a, b));
          const hi = Math.max(lo, Math.max(a, b));
          const rect = clientObj.getTextBounds(lo, hi);
          if (rect) {
            standaloneImeInput.style.left = `${(Number(rect.x) || 0) + (Number(targetObj?.x) || 0)}px`;
            standaloneImeInput.style.top = `${(Number(rect.y) || 0) + (Number(targetObj?.y) || 0)}px`;
            standaloneImeInput.style.width = `${Math.max(1, Number(rect.width) || 2)}px`;
            standaloneImeInput.style.height = `${Math.max(1, Number(rect.height) || 16)}px`;
          }
        } catch (_) {}
      }
    }

    function syncStandaloneImeClientInput(nextTarget) {
      const entry = getAs3StageImeEntry();
      if (entry?.fte3d && entry.fte3d.getStage?.() === stage) {
        teardownStandaloneImeInput();
        return;
      }
      teardownStandaloneImeInput();
      if (!nextTarget || typeof nextTarget !== 'object' || options.ownsTextInput?.(nextTarget)) return;
      if (!isStageImeClient(nextTarget)) return;
      const d = doc();
      const container = options.viewport || options.canvas?.parentElement || d?.body || null;
      if (!d || typeof d.createElement !== 'function') return;
      const input = d.createElement('input');
      input.type = 'text';
      input.setAttribute?.('data-neoflash-ime-client', 'true');
      input.setAttribute?.('aria-hidden', 'true');
      if (input.style) {
        Object.assign(input.style, {
          position: 'absolute',
          left: '0px',
          top: '0px',
          width: '2px',
          height: '16px',
          opacity: '0',
          pointerEvents: 'none',
        });
      }
      const bind = (t, fn) => {
        if (typeof input.addEventListener !== 'function') return;
        input.addEventListener(t, fn);
        standaloneImeCleanups.push(() => input.removeEventListener?.(t, fn));
      };
      bind('compositionstart', ev => handleComposition('compositionstart', ev, nextTarget));
      bind('compositionupdate', ev => handleComposition('compositionupdate', ev, nextTarget));
      bind('compositionend', ev => handleComposition('compositionend', ev, nextTarget));
      bind('select', () => {
        if (!imeComposing && isStageImeClient(nextTarget) && typeof input.selectionStart === 'number' && typeof input.selectionEnd === 'number') {
          nextTarget.selectRange(input.selectionStart, input.selectionEnd);
        }
      });
      if (container && typeof container.appendChild === 'function') container.appendChild(input);
      standaloneImeInput = input;
      standaloneImeClient = nextTarget;
      syncStandaloneImeInputAttrs(nextTarget, nextTarget);
      try { input.focus?.({ preventScroll: true }); } catch (_) {}
    }

    const stageImeBridgeHook = {
      getStage: () => stage,
      syncState: () => syncStandaloneImeInputAttrs(focus, standaloneImeClient || (isStageImeClient(focus) ? focus : null)),
      setComposition(txt, s = 0, e = String(txt ?? '').length) {
        const tgt = focus || stage;
        const cl = standaloneImeClient || (isStageImeClient(tgt) ? tgt : null);
        if (!imeComposing) {
          imeComposing = true;
          standaloneImeBaseStart = cl ? Math.max(0, Math.min(cl.selectionAnchorIndex | 0, cl.selectionActiveIndex | 0)) : 0;
        }
        imeText = String(txt ?? '');
        if (standaloneImeInput) standaloneImeInput.value = imeText;
        if (cl && isStageImeClient(cl)) {
          const ranges = imeText.length > 0 ? [new StageCompositionAttributeRange(s, e > s ? e : imeText.length, true, false)] : [];
          cl.updateComposition(imeText, ranges, standaloneImeBaseStart, standaloneImeBaseStart + imeText.length);
          syncStandaloneImeInputAttrs(tgt, cl);
        }
      },
      selectionChanged(s, e) {
        const tgt = focus || stage;
        const cl = standaloneImeClient || (isStageImeClient(tgt) ? tgt : null);
        if (!cl || !isStageImeClient(cl)) return;
        if (imeComposing && imeText.length > 0) {
          const lo = Math.max(0, Math.min(imeText.length, Math.min(s, e)));
          const hi = Math.max(lo, Math.min(imeText.length, Math.max(s, e)));
          const ranges = [];
          if (lo > 0) ranges.push(new StageCompositionAttributeRange(0, lo, false, false));
          ranges.push(new StageCompositionAttributeRange(lo, hi > lo ? hi : imeText.length, true, false));
          if (hi > lo && hi < imeText.length) ranges.push(new StageCompositionAttributeRange(hi, imeText.length, false, false));
          cl.updateComposition(imeText, ranges, standaloneImeBaseStart, standaloneImeBaseStart + imeText.length);
        } else {
          cl.selectRange(s | 0, e | 0);
        }
        syncStandaloneImeInputAttrs(tgt, cl);
      },
      abandon() {
        const tgt = focus || stage;
        const cl = standaloneImeClient || (isStageImeClient(tgt) ? tgt : null);
        const wasComp = imeComposing;
        imeComposing = false;
        imeText = '';
        if (standaloneImeInput) standaloneImeInput.value = '';
        if (wasComp && cl && isStageImeClient(cl)) {
          cl.updateComposition('', [], standaloneImeBaseStart, standaloneImeBaseStart);
          syncStandaloneImeInputAttrs(tgt, cl);
        }
      },
      confirmForDoConversion(txt) {
        const tgt = focus || stage;
        const cl = standaloneImeClient || (isStageImeClient(tgt) ? tgt : null);
        imeComposing = false;
        imeText = '';
        if (standaloneImeInput) standaloneImeInput.value = '';
        if (cl && isStageImeClient(cl) && !cl.readOnly) {
          cl.confirmComposition(String(txt ?? ''), false);
          syncStandaloneImeInputAttrs(tgt, cl);
        }
        return { client: cl || null, target: tgt };
      },
    };
    getAs3StageImeEntry().stage = stageImeBridgeHook;

    function closeContextMenu() {
      if (activeStageContextMenuEl) {
        try { activeStageContextMenuEl.remove?.(); } catch (_) {}
        activeStageContextMenuEl = null;
      }
    }

    function getActiveContextMenu() {
      return activeStageContextMenuEl;
    }

    function createStageMenuDomNode(tagName) {
      const d = doc();
      if (d && typeof d.createElement === 'function') return d.createElement(tagName);
      return createFallbackOverlayDomElement(tagName);
    }

    function openContextMenu(rawEvent = {}, targetOverride = null, explicitMenu = null, explicitOwner = null) {
      closeContextMenu();
      const mouseTarget = targetOverride || (options.hitTest ? options.hitTest(rawEvent) : null) || focus || stage;
      let owner = explicitOwner || null;
      let menu = explicitMenu || null;
      if (!menu) {
        let cur = mouseTarget;
        const seen = new Set();
        while (cur && typeof cur === 'object' && !seen.has(cur)) {
          seen.add(cur);
          if (cur.contextMenu && typeof cur.contextMenu === 'object') {
            owner = cur;
            menu = cur.contextMenu;
            break;
          }
          cur = options.parentOf ? options.parentOf(cur) : cur.parent;
        }
        if (!menu && (stage.contextMenu || stageAssignedContextMenu)) {
          owner = stage;
          menu = stage.contextMenu || stageAssignedContextMenu;
        }
      }
      if (!menu) return null;
      rawEvent?.preventDefault?.();

      const CtxEventCtor = options.ContextMenuEvent || eventsAPI.ContextMenuEvent || null;
      const makeCtxEvent = (type) => {
        if (CtxEventCtor) return new CtxEventCtor(type, false, false, mouseTarget, owner);
        const ev = new Event(type, false, false);
        ev.mouseTarget = mouseTarget;
        ev.contextMenuOwner = owner;
        return ev;
      };
      if (typeof menu.dispatchEvent === 'function') {
        menu.dispatchEvent(makeCtxEvent('menuSelect'));
      }

      const root = createStageMenuDomNode('div');
      root.className = 'nf-as3-context-menu';
      root.setAttribute?.('role', 'menu');
      if (root.style) {
        Object.assign(root.style, {
          position: 'absolute',
          left: `${Number(rawEvent?.clientX ?? rawEvent?.stageX ?? 0)}px`,
          top: `${Number(rawEvent?.clientY ?? rawEvent?.stageY ?? 0)}px`,
          zIndex: '10000',
          display: 'flex',
          flexDirection: 'column',
        });
      }
      const childrenList = [];
      const appendNode = child => {
        childrenList.push(child);
        if (typeof root.appendChild === 'function') root.appendChild(child);
        else if (Array.isArray(root.children)) root.children.push(child);
        else root.children = childrenList;
      };
      if (typeof root.querySelectorAll !== 'function') {
        root.querySelectorAll = sel => {
          if (sel === '[role="menuitem"]') return childrenList.filter(c => c.getAttribute?.('role') === 'menuitem');
          if (sel === '[role="separator"]') return childrenList.filter(c => c.getAttribute?.('role') === 'separator');
          return childrenList.slice();
        };
      }
      const addSep = () => {
        const sep = createStageMenuDomNode('div');
        sep.setAttribute?.('role', 'separator');
        appendNode(sep);
      };
      const addBtn = (caption, isEnabled, kind, extra, onClick) => {
        const btn = createStageMenuDomNode('button');
        btn.type = 'button';
        btn.textContent = String(caption);
        btn.disabled = !isEnabled;
        btn.setAttribute?.('role', 'menuitem');
        btn.setAttribute?.('data-kind', kind);
        btn.setAttribute?.('aria-disabled', String(!isEnabled));
        for (const [k, v] of Object.entries(extra || {})) btn.setAttribute?.(k, String(v));
        btn.addEventListener?.('click', () => {
          if (btn.disabled) return;
          try { onClick(); } finally { closeContextMenu(); }
        });
        appendNode(btn);
        return btn;
      };

      let customCount = 0;
      for (const item of (Array.isArray(menu.customItems) ? menu.customItems : [])) {
        if (!item || item.visible === false || !(item.caption ?? item.label)) continue;
        if (item.separatorBefore) addSep();
        customCount++;
        addBtn(item.caption ?? item.label, item.enabled !== false, 'custom', {}, () => {
          if (typeof item.dispatchEvent === 'function') {
            item.dispatchEvent(makeCtxEvent('menuItemSelect'));
          }
          options.afterInput?.();
        });
      }

      const clip = menu.clipboardItems;
      const clipSpecs = [
        ['cut', 'Cut', Event.CUT || 'cut'],
        ['copy', 'Copy', Event.COPY || 'copy'],
        ['paste', 'Paste', Event.PASTE || 'paste'],
        ['clear', 'Clear', Event.CLEAR || 'clear'],
        ['selectAll', 'Select All', Event.SELECT_ALL || 'selectAll'],
      ];
      const showClip = clip && (menu.clipboardMenu || clip.cut || clip.copy || clip.paste || clip.clear);
      if (showClip) {
        if (customCount > 0) addSep();
        for (const [key, label, evType] of clipSpecs) {
          if (!menu.clipboardMenu && !clip[key]) continue;
          addBtn(label, Boolean(clip[key]), 'clipboard', { 'data-action': key }, () => {
            const eventTarget = focus || mouseTarget || owner || stage;
            const clipEv = new Event(evType, true, true);
            let allowed = true;
            if (eventTarget && typeof hub.dispatch === 'function') {
              try { allowed = hub.dispatch(eventTarget, clipEv) !== false && !clipEv.defaultPrevented; } catch (_) {
                if (typeof eventTarget.dispatchEvent === 'function') {
                  allowed = eventTarget.dispatchEvent(clipEv) !== false && !clipEv.defaultPrevented;
                }
              }
            } else if (eventTarget && typeof eventTarget.dispatchEvent === 'function') {
              allowed = eventTarget.dispatchEvent(clipEv) !== false && !clipEv.defaultPrevented;
            }
            if (allowed) {
              const txtObj = eventTarget && typeof eventTarget.text === 'string'
                ? eventTarget
                : (mouseTarget && typeof mouseTarget.text === 'string' ? mouseTarget : null);
              if (txtObj) {
                const t = txtObj.text;
                const b = Math.max(0, Math.min(t.length, Number(txtObj.selectionBeginIndex ?? 0) | 0));
                const e = Math.max(b, Math.min(t.length, Number(txtObj.selectionEndIndex ?? t.length) | 0));
                if (key === 'selectAll') {
                  if (typeof txtObj.setSelection === 'function') txtObj.setSelection(0, t.length);
                  else { txtObj.selectionBeginIndex = 0; txtObj.selectionEndIndex = t.length; }
                } else if (key === 'copy') {
                  stageContextClipboardBuffer = b < e ? t.slice(b, e) : t;
                } else if (key === 'cut') {
                  stageContextClipboardBuffer = b < e ? t.slice(b, e) : t;
                  txtObj.text = b < e ? t.slice(0, b) + t.slice(e) : '';
                } else if (key === 'paste') {
                  txtObj.text = t.slice(0, b) + stageContextClipboardBuffer + t.slice(e);
                } else if (key === 'clear') {
                  txtObj.text = b < e ? t.slice(0, b) + t.slice(e) : '';
                }
              }
            }
            options.afterInput?.();
          });
        }
      }

      const container = options.menuContainer || options.viewport || options.canvas?.parentElement || doc()?.body || null;
      if (container && typeof container.appendChild === 'function') {
        container.appendChild(root);
      } else if (container && Array.isArray(container.children)) {
        container.children.push(root);
      }
      activeStageContextMenuEl = root;
      return root;
    }
    stageContextMenuOpeners.set(stage, openContextMenu);

    function handleComposition(type, domEvent = {}, explicitTarget = null) {
      if (!enabled()) return true;
      if (domEvent?.target && options.ownsTextInput?.(domEvent.target)) {
        imeComposing = type !== 'compositionend';
        return true;
      }
      const entry = getAs3StageImeEntry();
      if (entry?.fte3d?.handleComposition && entry.fte3d.getStage?.() === stage) {
        const phaseMap = {
          compositionstart: 'start',
          imeStartComposition: 'start',
          compositionupdate: 'update',
          imeComposition: 'update',
          compositionend: 'end',
        };
        const mapped = phaseMap[type];
        if (!mapped) throw new Error(`Unsupported IME composition event '${type}'.`);
        return entry.fte3d.handleComposition(mapped, domEvent, explicitTarget || stage.focus || stage);
      }
      const as2Reg = global[AS2_IME_DOC_HOOKS];
      if (as2Reg) {
        const d = doc();
        const phase = (type === 'compositionstart' || type === 'imeStartComposition') ? 'start' : (type === 'compositionend' ? 'end' : 'update');
        const set = d && typeof d === 'object' ? as2Reg.byDoc?.get(d) : null;
        if (set) for (const fn of set) fn(phase, domEvent);
        for (const fn of as2Reg.globalHooks || []) fn(phase, domEvent);
      }
      if (entry?.netSys && !entry.netSys.isEnabled()) {
        domEvent?.preventDefault?.();
        return false;
      }
      const text = typeof domEvent.data === 'string' ? domEvent.data : (typeof domEvent.text === 'string' ? domEvent.text : '');
      const target = explicitTarget || stage.focus || stage;
      const IMEEventCtor = options.IMEEvent || eventsAPI.IMEEvent || eventsAPI.TextEvent || Event;
      const makeImeEv = (evType, clientObj) => {
        const ev = new IMEEventCtor(evType, false, true, text, clientObj ?? null);
        if (ev.text === undefined) ev.text = text;
        if (ev.imeClient === undefined) ev.imeClient = clientObj ?? null;
        return ev;
      };
      if (type === 'compositionstart' || type === 'imeStartComposition') {
        const initClient = isStageImeClient(target) ? target : null;
        if (initClient && initClient.readOnly === true) {
          domEvent?.preventDefault?.();
          return false;
        }
        const startEvt = makeImeEv('imeStartComposition', initClient);
        const res = deliver(target, startEvt, domEvent);
        if (!res || startEvt.defaultPrevented) {
          domEvent?.preventDefault?.();
          options.afterInput?.();
          return false;
        }
        imeComposing = true;
        imeText = text;
        standaloneImeClient = isStageImeClient(startEvt.imeClient) ? startEvt.imeClient : initClient;
        if (standaloneImeClient) {
          const a = Number(standaloneImeClient.selectionAnchorIndex ?? 0) | 0;
          const b = Number(standaloneImeClient.selectionActiveIndex ?? a) | 0;
          standaloneImeBaseStart = Math.max(0, Math.min(a, b));
          try { standaloneImeClient.getTextInRange(0, standaloneImeBaseStart); } catch (_) {}
          syncStandaloneImeInputAttrs(target, standaloneImeClient);
          if (text.length > 0) {
            const ranges = [new StageCompositionAttributeRange(0, text.length, true, false)];
            standaloneImeClient.updateComposition(text, ranges, standaloneImeBaseStart, standaloneImeBaseStart + text.length);
          }
        }
        entry?.netSys?.notifyDomComposition?.('start', text, standaloneImeClient);
        options.afterInput?.();
        return res;
      }
      if (type === 'compositionupdate' || type === 'compositionend' || type === 'imeComposition') {
        imeText = text;
        const client = standaloneImeClient || (isStageImeClient(target) ? target : null);
        if (type === 'compositionend') {
          imeComposing = false;
          if (client && !client.readOnly) {
            client.confirmComposition(text, false);
            syncStandaloneImeInputAttrs(target, client);
          }
          entry?.netSys?.notifyDomComposition?.('end', text, client);
        } else {
          if (!imeComposing) {
            imeComposing = true;
            standaloneImeBaseStart = client ? Math.max(0, Math.min(client.selectionAnchorIndex | 0, client.selectionActiveIndex | 0)) : 0;
          }
          if (client && !client.readOnly) {
            const ranges = text.length > 0 ? [new StageCompositionAttributeRange(0, text.length, true, false)] : [];
            client.updateComposition(text, ranges, standaloneImeBaseStart, standaloneImeBaseStart + text.length);
            syncStandaloneImeInputAttrs(target, client);
          }
          entry?.netSys?.notifyDomComposition?.('update', text, client);
        }
        const imeEvt = makeImeEv('imeComposition', client);
        const res = deliver(target, imeEvt, domEvent);
        if (type === 'compositionend' && res && text.length > 0 && !options.ownsTextInput?.(domEvent.target)) {
          const textEvt = new (eventsAPI.TextEvent || Event)(eventsAPI.TextEvent?.TEXT_INPUT || 'textInput', true, true, text);
          deliver(target, textEvt, domEvent);
        }
        options.afterInput?.();
        return res;
      }
      throw new Error(`Unsupported IME composition event '${type}'.`);
    }
    function handleKey(type, domEvent = {}) {
      if (imeComposing && domEvent?.key === 'Escape') imeComposing = false;
      if (!enabled() || domEvent.isComposing || imeComposing) return true;
      const nativeTarget = domEvent.target;
      if (nativeTarget && nativeTarget !== options.canvas && !options.ownsTextInput?.(nativeTarget) &&
        (nativeTarget.isContentEditable || /^(?:INPUT|TEXTAREA|SELECT)$/.test(nativeTarget.tagName || ''))) return true;
      if (type === 'keydown') type = KeyboardEvent.KEY_DOWN;
      if (type === 'keyup') type = KeyboardEvent.KEY_UP;
      if (![KeyboardEvent.KEY_DOWN, KeyboardEvent.KEY_UP].includes(type)) throw new Error(`Unsupported native keyboard event '${type}'.`);
      if (typeof domEvent.getModifierState === 'function') {
        keyboardState.capsLock = Boolean(domEvent.getModifierState('CapsLock'));
        keyboardState.numLock = Boolean(domEvent.getModifierState('NumLock'));
      }
      const target = stage.focus || stage;
      const text = typeof domEvent.key === 'string' ? domEvent.key : '';
      const charCode = [...text].length === 1 ? text.codePointAt(0) : 0;
      const event = new KeyboardEvent(type, true, true, charCode, keyCode(domEvent), domEvent.location || 0,
        Boolean(domEvent.ctrlKey || domEvent.metaKey), domEvent.altKey, domEvent.shiftKey, domEvent.ctrlKey, domEvent.metaKey);
      const result = deliver(target, event, domEvent);
      if (target === stage || alive(target)) options.onLegacyInput?.(type, target, domEvent);
      if (domEvent.cancelable !== false && options.hasLegacyKeyListeners?.() && [32, 37, 38, 39, 40].includes(event.keyCode)) {
        domEvent.preventDefault?.();
      }
      options.afterInput?.();
      return result;
    }
    function keyCode(event) {
      if (Number.isInteger(event.keyCode) && event.keyCode > 0) return event.keyCode;
      const code = event.code || '';
      if (/^Key[A-Z]$/.test(code)) return code.charCodeAt(3);
      if (/^Digit[0-9]$/.test(code)) return code.charCodeAt(5);
      if (/^Numpad[0-9]$/.test(code)) return 96 + Number(code.slice(6));
      if (/^F(?:[1-9]|1[0-5])$/.test(code)) return 111 + Number(code.slice(1));
      return ({ ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Enter: 13, Space: 32,
        Escape: 27, Tab: 9, Backspace: 8, Delete: 46, Insert: 45, Home: 36, End: 35, PageUp: 33, PageDown: 34,
        CapsLock: 20, ShiftLeft: 16, ShiftRight: 16, ControlLeft: 17, ControlRight: 17, AltLeft: 18, AltRight: 18,
        MetaLeft: 15, MetaRight: 15, NumpadMultiply: 106, NumpadAdd: 107, NumpadEnter: 108, NumpadSubtract: 109,
        NumpadDecimal: 110, NumpadDivide: 111, Semicolon: 186, Equal: 187, Comma: 188, Minus: 189, Period: 190,
        Slash: 191, Backquote: 192, BracketLeft: 219, Backslash: 220, BracketRight: 221, Quote: 222 })[code] || 0;
    }
    function resize(nextWidth, nextHeight) {
      nextWidth = size(nextWidth, 'stageWidth'); nextHeight = size(nextHeight, 'stageHeight');
      if (nextWidth === width && nextHeight === height) return false;
      width = nextWidth; height = nextHeight;
      hub.dispatch(stage, new Event(Event.RESIZE)); options.afterInput?.();
      return true;
    }
    function listen(target, type, fn, settings) {
      if (!target?.addEventListener) return;
      const guarded = event => {
        try { fn(event); } catch (error) { if (options.onError) options.onError(error); else throw error; }
      };
      target.addEventListener(type, guarded, settings);
      cleanups.push(() => target.removeEventListener(type, guarded, settings));
    }
    function attach() {
      if (attached) return;
      attached = true;
      for (const type of ['pointerdown','pointerup','pointermove','pointercancel','lostpointercapture','pointerleave']) listen(options.canvas,type,event=>handlePointer(type,event));
      for (const type of ['touchstart','touchmove','touchend','touchcancel']) listen(options.canvas, type, event => handleTouch(type, event), { passive: false });
      const outside=options.pointerTarget||global;
      for(const type of ['pointermove','pointerup','pointercancel'])listen(outside,type,event=>{if(event.target!==options.canvas&&(captured!==null||pressedTarget!==null||options.hasDrag?.()))handlePointer(type,event);});
      listen(outside,'blur',()=>{if(cancelPointer())options.afterInput?.();});
      listen(options.canvas, 'wheel', event => handlePointer('wheel', event), { passive: false });
      listen(options.canvas, 'contextmenu', event => {
        if (!showDefaultContextMenu) event.preventDefault?.();
        if (!options.canvas?.__nfHasAs3ContextMenu) {
          openContextMenu(event);
        }
      });
      if (doc()) {
        const onFsChange = () => {
          const fsEl = docFullscreenElement();
          if (!fsEl && displayState !== 'normal') {
            const wasInteractive = displayState === 'fullScreenInteractive';
            const wasLocked = mouseLock;
            displayState = 'normal'; mouseLock = false;
            if (wasLocked) exitStagePointerLock();
            options.onDisplayStateChanged?.(displayState);
            emitFullScreen(false, wasInteractive);
          } else if (fsEl && (!options.viewport || fsEl === options.viewport) && displayState === 'normal') {
            displayState = 'fullScreen';
            options.onDisplayStateChanged?.(displayState);
            emitFullScreen(true, false);
          }
        };
        const onPointerLockChange = () => {
          const lockTarget = pointerLockTarget();
          const lockedEl = doc()?.pointerLockElement || null;
          if (!lockedEl && (mouseLock || optInPointerLock)) {
            mouseLock = false;
            optInPointerLock = false;
            if (options.canvas?.dataset) options.canvas.dataset.mouseLock = 'false';
            options.onMouseLockChanged?.(false);
          } else if (lockedEl && lockedEl === lockTarget && (displayState === 'fullScreenInteractive' || optInPointerLockRequested) && !mouseLock) {
            if (displayState === 'fullScreenInteractive') mouseLock = true;
            else optInPointerLock = true;
            optInPointerLockRequested = false;
            if (options.canvas?.dataset) options.canvas.dataset.mouseLock = 'true';
            options.onMouseLockChanged?.(true);
          }
        };
        listen(doc(), 'fullscreenchange', onFsChange);
        listen(doc(), 'webkitfullscreenchange', onFsChange);
        listen(doc(), 'pointerlockchange', onPointerLockChange);
        listen(doc(), 'pointerlockerror', () => { optInPointerLockRequested = false; optInPointerLock = false; clearMouseLockState(); });
      }
      // Existing runtime click owns authored Click dispatch; opt in only for standalone use.
      if (options.attachClick) listen(options.canvas, 'click', event => handlePointer('click', event));
      const deliveredKeyEvents = new WeakSet();
      const keyTarget = options.keyboardTarget || options.canvas;
      for (const type of ['keydown', 'keyup']) {
        listen(keyTarget, type, event => {
          if (event && typeof event === 'object') deliveredKeyEvents.add(event);
          handleKey(type, event);
        });
        if (options.fallbackWindowKeys && global !== keyTarget && typeof global?.addEventListener === 'function') {
          listen(global, type, event => {
            if (event && typeof event === 'object' && deliveredKeyEvents.has(event)) return;
            const d = doc();
            const active = d?.activeElement || null;
            if (active && active !== d?.body && active !== d?.documentElement && active !== options.viewport && active !== options.canvas) return;
            if (event && typeof event === 'object') deliveredKeyEvents.add(event);
            handleKey(type, event);
          });
        }
      }
      for (const type of ['compositionstart', 'compositionupdate', 'compositionend']) listen(options.keyboardTarget || options.canvas, type, event => handleComposition(type, event));
      if (options.getSize) listen(options.resizeTarget || global, 'resize', () => {
        const dimensions = options.getSize(); resize(dimensions[0], dimensions[1]);
      });
    }
    function resolveBroadcastTargets(type, targets) {
      if (typeof targets !== 'function') return [stage, ...targets];
      if (typeof hub.targetsWith === 'function') {
        const active = hub.targetsWith(type);
        if (active) {
          if (active.length === 0) return [];
          const nonStage = active.filter(t => t !== stage);
          if (nonStage.length <= 1) return active.includes(stage) ? [stage, ...nonStage] : nonStage;
          const ordered = [stage, ...targets()];
          const seen = new Set(ordered);
          for (const item of nonStage) if (!seen.has(item)) ordered.push(item);
          return ordered;
        }
      }
      return [stage, ...targets()];
    }
    function broadcastPhase(type, targets = []) {
      const list = resolveBroadcastTargets(type, targets);
      if (list.length) hub.broadcast(type, list);
    }
    function flushRender(targets = []) {
      if (!invalidated) return false;
      invalidated = false;
      const list = resolveBroadcastTargets(Event.RENDER || 'render', targets);
      if (list.length) hub.broadcast(Event.RENDER || 'render', list);
      return true;
    }
    function resetState() {
      const wasLocked = mouseLock || optInPointerLock;
      scaleMode = initial.scaleMode; align = initial.align; quality = 'HIGH'; displayState = 'normal'; colorCorrection = 'default';
      fullScreenSourceRect = null; mouseLock = false; optInPointerLock = false; optInPointerLockRequested = false;
      if (wasLocked) exitStagePointerLock();
      applyQualitySurface(quality);
      applyColorCorrectionSurface();
      stageColor = initial.color; colorAssigned = false;
      mouseChildren = true; tabChildren = true; doubleClickEnabled = false; showDefaultContextMenu = true; stageFocusRect = true; invalidated = false;
      keyboardState.capsLock = Boolean(options.capsLock); keyboardState.numLock = Boolean(options.numLock);
      activeTouches.clear(); imeComposing = false; imeText = '';
      closeContextMenu();
      teardownStandaloneImeInput();
      Mouse[resetMouseToken]?.();
      mouseStateRecord.cursor = 'auto'; mouseStateRecord.visible = true;
      fallbackChildren.splice(0, fallbackChildren.length, ...(Array.isArray(options.initialChildren) ? options.initialChildren : []));
      applyDomMouseCursor();
      options.onMouseChanged?.();
    }
    function dispose() {
      cancelPointer();
      closeContextMenu();
      teardownStandaloneImeInput();
      const entry = getAs3StageImeEntry();
      if (entry && entry.stage === stageImeBridgeHook) entry.stage = null;
      stageContextMenuOpeners.delete(stage);
      for (const cleanup of cleanups.splice(0)) cleanup();
      attached = false; hub.release(stage); focus = null; focusReason = null; nativeFocus=false; over = null; resetState();
    }
    function reset() {
      cancelPointer();
      closeContextMenu();
      teardownStandaloneImeInput();
      focus = null; focusReason = null; nativeFocus=false; focusVersion++; over = null; mouseX = 0; mouseY = 0;
      width = initial.width; height = initial.height;
      if (options.setFrameRate) options.setFrameRate(initial.frameRate);
      frameRate = initial.frameRate;
      resetState();
      hub.release(stage);
    }
    return Object.freeze({ stage, Keyboard, Mouse, mouseState: () => ({ cursor: mouseStateRecord.cursor, visible: mouseStateRecord.visible }),
      cursorCss: () => Mouse[cursorCssToken]?.() || null,
      colorCorrectionActive: () => isColorCorrectionActive(),
      focusReason: () => (focus && alive(focus) ? focusReason : null),
      fullScreenSourceRect: () => (fullScreenSourceRect ? { ...fullScreenSourceRect } : null),
      colorAssigned: () => colorAssigned,
      requestOptInPointerLock: () => {
        const target = pointerLockTarget();
        if (!target || (typeof target.requestPointerLock !== 'function' && typeof options.requestPointerLock !== 'function')) return false;
        optInPointerLockRequested = true;
        requestStagePointerLock();
        const d = doc();
        if (d?.pointerLockElement === target) { optInPointerLock = true; optInPointerLockRequested = false; }
        return true;
      },
      exitOptInPointerLock: () => {
        optInPointerLockRequested = false;
        optInPointerLock = false;
        exitStagePointerLock();
      },
      isPointerLocked: () => Boolean(mouseLock || optInPointerLock),
      broadcastPhase, flushRender, handlePointer, handleTouch, handleComposition, handleKey, cancelPointer, changeFocus, resize, attach, dispose, reset,
      openContextMenu, closeContextMenu, getActiveContextMenu, imeInputElement: () => standaloneImeInput,
      resolveAccessibilityOverlayNodes: (entries = [], extraOptions = {}) => resolveAccessibilityOverlayNodes(
        entries,
        {
          focusedTarget: focus && alive(focus) ? focus : null,
          focusReason: focus && alive(focus) ? focusReason : null,
          ...extraOptions,
        },
      ),
      validateAccessibilityReadOrder: (entries = []) => validateAccessibilityReadOrder(entries),
      collectProjectAccessibilityEntries: (project, frame = 0, extraOptions = {}) => collectProjectAccessibilityEntries(project, frame, extraOptions),
      syncAccessibilityOverlay: (overlayRoot, entries = [], extraOptions = {}) => syncAccessibilityOverlay(
        overlayRoot,
        entries,
        {
          focusedTarget: focus && alive(focus) ? focus : null,
          focusReason: focus && alive(focus) ? focusReason : null,
          ...extraOptions,
        },
      ),
      tick(targets = [], onConstructed = null) {
        if (typeof targets === 'function') {
          let cached = null;
          const lazy = () => (cached ??= targets());
          for (const type of [Event.ENTER_FRAME || 'enterFrame', Event.FRAME_CONSTRUCTED || 'frameConstructed']) {
            const list = resolveBroadcastTargets(type, lazy);
            if (list.length) hub.broadcast(type, list);
          }
          onConstructed?.();
          const exitList = resolveBroadcastTargets(Event.EXIT_FRAME || 'exitFrame', lazy);
          if (exitList.length) hub.broadcast(Event.EXIT_FRAME || 'exitFrame', exitList);
          flushRender(lazy);
          return;
        }
        const list = [stage, ...targets];
        hub.broadcast(Event.ENTER_FRAME || 'enterFrame', list);
        hub.broadcast(Event.FRAME_CONSTRUCTED || 'frameConstructed', list);
        onConstructed?.();
        hub.broadcast(Event.EXIT_FRAME || 'exitFrame', list);
        flushRender(targets);
      },
    });
  }
  const api = Object.freeze({ create, computeStageLayout, Stage, StageScaleMode, StageAlign, StageDisplayState, StageQuality,
    Keyboard: DefaultKeyboard, KeyLocation, KeyboardType, Mouse: DefaultMouse, MouseCursor, MouseCursorData,
    AccessibilityProperties, AccessibilityImplementation, ContextMenuBuiltInItems, ContextMenuClipboardItems, ContextMenu, ContextMenuItem,
    CompositionAttributeRange: StageCompositionAttributeRange, msaaRoleToAria, decodeMsaaState,
    createKeyboard, createMouse, setStageAdapters,
    collectProjectAccessibilityEntries, resolveAccessibilityOverlayNodes, validateAccessibilityReadOrder, syncAccessibilityOverlay });
  global.NeoFlashStage = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
