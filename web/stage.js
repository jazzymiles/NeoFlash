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
  function setStageAdapters(adapters = {}) {
    if (typeof adapters.cursorVector === 'function') cursorVectorFactory = adapters.cursorVector;
    if (typeof adapters.makePoint === 'function') cursorPointFactory = adapters.makePoint;
    if (typeof adapters.isBitmapData === 'function') isBitmapData = adapters.isBitmapData;
    if (typeof adapters.isPoint === 'function') isPoint = adapters.isPoint;
    if (typeof adapters.isRectangle === 'function') isRectangle = adapters.isRectangle;
    if (typeof adapters.makeRectangle === 'function') makeRectangle = adapters.makeRectangle;
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

  function extractAccessibilityEntry(raw, index = 0) {
    const target = raw && typeof raw === 'object' && raw.target ? raw.target : raw;
    const acc = (raw && raw.accessibilityProperties) || (target && target.accessibilityProperties) || null;
    const id = String((raw && raw.id) ?? (target && (target.id ?? target.name)) ?? `node-${index + 1}`);
    const roleRaw = (raw && raw.role) ?? (acc && acc.role) ?? (target && target.accessibilityRole) ?? '';
    const role = String(roleRaw || '').trim() || 'group';
    const ariaLabelRaw = (raw && raw.ariaLabel) || (acc && acc.name) || (target && target.accessibilityName) || (raw && raw.name) || (target && target.name) || '';
    const ariaLabel = String(ariaLabelRaw || '').trim() || id;
    const ariaDescRaw = (raw && (raw.ariaDescription ?? raw.description)) ?? (acc && acc.description) ?? (target && target.accessibilityDescription) ?? '';
    const ariaDescription = String(ariaDescRaw || '').trim();
    const liveRaw = String((raw && (raw.ariaLive ?? raw.live)) ?? (acc && acc.live) ?? (target && target.accessibilityLive) ?? 'off').trim().toLowerCase();
    const ariaLive = (liveRaw === 'polite' || liveRaw === 'assertive') ? liveRaw : 'off';
    const shortcut = String((raw && raw.shortcut) ?? (acc && acc.shortcut) ?? (target && target.accessibilityShortcut) ?? '').trim();
    const silent = Boolean((raw && (raw.silent ?? raw.ariaHidden)) ?? (acc && acc.silent) ?? false);
    const disabled = Boolean((raw && (raw.disabled ?? raw.ariaDisabled)) ?? (target && target.enabled === false) ?? false);
    const tabEnabled = raw && raw.tabEnabled !== undefined ? Boolean(raw.tabEnabled) : (target && target.tabEnabled !== undefined ? Boolean(target.tabEnabled) : true);
    const rawTab = Number((raw && raw.tabIndex) ?? (target && target.tabIndex));
    const explicitTab = Number.isInteger(rawTab) && rawTab > 0 ? rawTab : null;
    const rawOrder = Number((raw && (raw.readingOrder ?? raw.tabOrder)) ?? explicitTab);
    const explicitOrder = Number.isInteger(rawOrder) && rawOrder > 0 ? rawOrder : null;
    const x = Number.isFinite(Number(raw && raw.x)) ? Number(raw.x) : (Number.isFinite(Number(target && target.x)) ? Number(target.x) : 0);
    const y = Number.isFinite(Number(raw && raw.y)) ? Number(raw.y) : (Number.isFinite(Number(target && target.y)) ? Number(target.y) : 0);
    const width = Number.isFinite(Number(raw && raw.width)) ? Math.max(0, Number(raw.width)) : (Number.isFinite(Number(target && target.width)) ? Math.max(0, Number(target.width)) : 0);
    const height = Number.isFinite(Number(raw && raw.height)) ? Math.max(0, Number(raw.height)) : (Number.isFinite(Number(target && target.height)) ? Math.max(0, Number(target.height)) : 0);
    return {
      index,
      id,
      target,
      role,
      ariaLabel,
      ariaDescription,
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
    active.forEach((item, idx) => {
      const readingOrder = idx + 1;
      const tabIndex = (item.ariaDisabled || !item.tabEnabled)
        ? -1
        : (item.explicitTab != null ? item.explicitTab : readingOrder);
      const isFocused = Boolean(focusedTarget && (item.target === focusedTarget || item.id === focusedTarget.id || item.id === focusedTarget.name));
      resolved.push({
        id: item.id,
        target: item.target,
        role: item.role,
        tabIndex,
        readingOrder,
        ariaLabel: item.ariaLabel,
        ariaDescription: item.ariaDescription,
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
    });
    hidden.forEach(item => {
      resolved.push({
        id: item.id,
        target: item.target,
        role: item.role,
        tabIndex: -1,
        readingOrder: 0,
        ariaLabel: item.ariaLabel,
        ariaDescription: item.ariaDescription,
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
    };
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
    }
    const doc = overlayRoot.ownerDocument || (typeof document !== 'undefined' ? document : null);
    const createNode = () => (doc && typeof doc.createElement === 'function') ? doc.createElement('div') : createFallbackOverlayDomElement('div');
    const elements = nodes.map(node => {
      const el = createNode();
      el.tabIndex = node.tabIndex;
      if (typeof el.setAttribute === 'function') {
        el.setAttribute('data-neoflash-id', node.id);
        el.setAttribute('data-reading-order', String(node.readingOrder));
        el.setAttribute('role', node.role);
        el.setAttribute('tabindex', String(node.tabIndex));
        el.setAttribute('aria-label', node.ariaLabel);
        el.setAttribute('aria-hidden', node.ariaHidden ? 'true' : 'false');
        if (node.ariaDescription) {
          el.setAttribute('aria-description', node.ariaDescription);
        } else {
          el.removeAttribute?.('aria-description');
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
      }
      if (el.style && typeof el.style === 'object') {
        el.style.left = `${node.x}px`;
        el.style.top = `${node.y}px`;
        el.style.width = `${node.width}px`;
        el.style.height = `${node.height}px`;
      }
      return el;
    });
    if (typeof overlayRoot.replaceChildren === 'function') {
      overlayRoot.replaceChildren(...elements);
    } else if (Array.isArray(overlayRoot.children)) {
      overlayRoot.children.splice(0, overlayRoot.children.length, ...elements);
    } else {
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
      contextMenuRecords.get(this).customItems = value;
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
  const FOCUS_DIRECTIONS = new Set(['none', 'top', 'bottom']);
  const resetMouseToken = Symbol('resetMouseCursors');
  function createMouse(state = { cursor: 'auto', visible: true, supportsCursor: true }, onChange = null) {
    const customCursors = new Map();
    class Mouse { constructor() { throw new TypeError('Mouse supplies static cursor methods and properties.'); } }
    Object.defineProperties(Mouse, {
      cursor: { enumerable: true, get: () => state.cursor,
        set(value) {
          const next = String(value);
          if (!VALID_CURSORS.has(next) && !customCursors.has(next)) throw argumentError(`Mouse.cursor must be a registered cursor or one of auto, arrow, button, hand or ibeam; got '${next}'.`);
          if (state.cursor !== next) { state.cursor = next; onChange?.(); }
        } },
      supportsCursor: { enumerable: true, get: () => Boolean(state.supportsCursor) },
      supportsNativeCursor: { enumerable: true, get: () => false },
      hide: { enumerable: true, value() { if (state.visible) { state.visible = false; onChange?.(); } } },
      show: { enumerable: true, value() { if (!state.visible) { state.visible = true; onChange?.(); } } },
      registerCursor: { enumerable: true, value(name, cursor) {
        if (name == null || cursor == null) throw new TypeError('Error #2007: Parameter name and cursor must be non-null.');
        if (!MouseCursorData.__nfIs(cursor)) throw new TypeError('Mouse.registerCursor requires a MouseCursorData instance.');
        const frames = cursor.data;
        if (!frames || frames.length === 0) throw argumentError('MouseCursorData.data must contain at least one BitmapData frame.');
        customCursors.set(String(name), cursor);
      } },
      unregisterCursor: { enumerable: true, value(name) {
        if (name == null) throw new TypeError('Error #2007: Parameter name must be non-null.');
        const key = String(name);
        customCursors.delete(key);
        if (state.cursor === key) { state.cursor = 'auto'; onChange?.(); }
      } },
      [resetMouseToken]: { value() { customCursors.clear(); } },
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
    let fullScreenSourceRect = null, mouseLock = false;
    let stageColor = stageRgbValue(options.color ?? 0xffffff), colorAssigned = false;
    let mouseChildren = true, tabChildren = true, doubleClickEnabled = false, showDefaultContextMenu = true, stageFocusRect = true, invalidated = false;
    const initial = { width, height, frameRate, color: stageColor, scaleMode: initialScaleMode, align: initialAlign };
    const fallbackChildren = Array.isArray(options.initialChildren) ? options.initialChildren.slice() : [];
    const keyboardState = { capsLock: Boolean(options.capsLock), numLock: Boolean(options.numLock),
      hasVirtualKeyboard: Boolean(options.hasVirtualKeyboard), physicalKeyboardType: options.physicalKeyboardType || KeyboardType.ALPHANUMERIC,
      accessible: Boolean(options.keyboardAccessible) };
    const mouseStateRecord = { cursor: 'auto', visible: true, supportsCursor: options.supportsCursor ?? true };
    const Keyboard = createKeyboard(() => keyboardState);
    const Mouse = createMouse(mouseStateRecord, () => options.onMouseChanged?.());
    let attached = false, over = null,focusVersion=0,focusDepth=0,nativeFocus=false;
    let captured=null,pressedTarget=null,clickTarget=null,hadPress=false;
    const deliveredPointers=new WeakMap();
    const cleanups = [];
    const stage = new Stage(stageToken);
    const doc = () => options.document || global.document || null;
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
          if (quality !== next) { quality = next; options.onQualityChanged?.(quality); }
        } },
      displayState: { enumerable: true, get: () => displayState,
        set(value) {
          const next = String(value);
          if (!DISPLAY_STATES.has(next)) throw argumentError(`Stage.displayState must be normal, fullScreen or fullScreenInteractive; got '${next}'.`);
          if (next === 'normal') {
            if (displayState !== 'normal') {
              const wasInteractive = displayState === 'fullScreenInteractive';
              displayState = 'normal'; mouseLock = false;
              if (options.exitFullscreen) options.exitFullscreen();
              else if (docFullscreenElement()) exitDocFullscreen();
              options.onDisplayStateChanged?.(displayState);
              emitFullScreen(false, wasInteractive);
            }
            return;
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
          colorCorrection = next;
        } },
      colorCorrectionSupport: { enumerable: true, get: () => 'unsupported' },
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
          mouseLock = value;
        } },
      wmodeGPU: { enumerable: true, get: () => false },
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
      focus: { enumerable: true, get() { if (focus && !alive(focus)) changeFocus(null); return focus; },
        set(value) {changeFocus(value, 'programmatic');} },
      stage: { enumerable: true, get: () => stage }, parent: { enumerable: true, get: () => null },
      root: { enumerable: true, get: () => stage },
      assignFocus: { value(objectToFocus, direction) {
        const dir = String(direction);
        if (!FOCUS_DIRECTIONS.has(dir)) throw argumentError(`Stage.assignFocus direction must be 'none', 'top' or 'bottom'; got '${dir}'.`);
        changeFocus(objectToFocus, 'programmatic', {}, dir);
      } },
      isFocusInaccessible: { value: () => false },
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
        if(value&&!handled)options.canvas?.focus?.({preventScroll:true});
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
      const point = options.coordinates?.(domEvent);
      if (point && point.length === 2 && point.every(Number.isFinite)) [mouseX, mouseY] = point;
    }
    function mouse(type, target, domEvent, related = null) {
      const point = target === stage ? [mouseX, mouseY] : options.localPoint?.(target, mouseX, mouseY) || [mouseX, mouseY];
      const event = new MouseEvent(type, ![MouseEvent.ROLL_OVER,MouseEvent.ROLL_OUT].includes(type), type === MouseEvent.CONTEXT_MENU ? false : true, point[0], point[1], related,
        Boolean(domEvent.ctrlKey || domEvent.metaKey), domEvent.altKey, domEvent.shiftKey,
        Boolean((domEvent.buttons || 0) & 1), type === MouseEvent.MOUSE_WHEEL ? -Math.sign(domEvent.deltaY || 0) : 0,
        domEvent.metaKey, domEvent.ctrlKey, domEvent.detail || 0);
      if (Number.isFinite(domEvent.movementX)) event.movementX = Number(domEvent.movementX);
      if (Number.isFinite(domEvent.movementY)) event.movementY = Number(domEvent.movementY);
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
        clickTarget=oldPressed===target?target:null;pressedTarget=null;releaseCapture();
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
    function handleComposition(type, domEvent = {}, explicitTarget = null) {
      if (!enabled()) return true;
      const text = typeof domEvent.data === 'string' ? domEvent.data : (typeof domEvent.text === 'string' ? domEvent.text : '');
      const target = explicitTarget || stage.focus || stage;
      if (type === 'compositionstart' || type === 'imeStartComposition') {
        imeComposing = true;
        imeText = text;
        const startEvt = new (eventsAPI.TextEvent || Event)('imeStartComposition', false, true, text);
        const res = deliver(target, startEvt, domEvent);
        options.afterInput?.();
        return res;
      }
      if (type === 'compositionupdate' || type === 'compositionend' || type === 'imeComposition') {
        imeText = text;
        if (type === 'compositionend') imeComposing = false;
        const IME = options.IMEEvent || eventsAPI.TextEvent || Event;
        const imeEvt = new IME('imeComposition', false, true, text);
        if (imeEvt.text === undefined) imeEvt.text = text;
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
      listen(options.canvas, 'contextmenu', event => { if (!showDefaultContextMenu) event.preventDefault?.(); });
      if (doc()) {
        const onFsChange = () => {
          const fsEl = docFullscreenElement();
          if (!fsEl && displayState !== 'normal') {
            const wasInteractive = displayState === 'fullScreenInteractive';
            displayState = 'normal'; mouseLock = false;
            options.onDisplayStateChanged?.(displayState);
            emitFullScreen(false, wasInteractive);
          } else if (fsEl && (!options.viewport || fsEl === options.viewport) && displayState === 'normal') {
            displayState = 'fullScreen';
            options.onDisplayStateChanged?.(displayState);
            emitFullScreen(true, false);
          }
        };
        listen(doc(), 'fullscreenchange', onFsChange);
        listen(doc(), 'webkitfullscreenchange', onFsChange);
      }
      // Existing runtime click owns authored Click dispatch; opt in only for standalone use.
      if (options.attachClick) listen(options.canvas, 'click', event => handlePointer('click', event));
      for (const type of ['keydown', 'keyup']) listen(options.keyboardTarget || options.canvas, type, event => handleKey(type, event));
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
        }
      }
      return [stage, ...targets()];
    }
    function flushRender(targets = []) {
      if (!invalidated) return false;
      invalidated = false;
      const list = resolveBroadcastTargets(Event.RENDER || 'render', targets);
      if (list.length) hub.broadcast(Event.RENDER || 'render', list);
      return true;
    }
    function resetState() {
      scaleMode = initial.scaleMode; align = initial.align; quality = 'HIGH'; displayState = 'normal'; colorCorrection = 'default';
      fullScreenSourceRect = null; mouseLock = false;
      stageColor = initial.color; colorAssigned = false;
      mouseChildren = true; tabChildren = true; doubleClickEnabled = false; showDefaultContextMenu = true; stageFocusRect = true; invalidated = false;
      keyboardState.capsLock = Boolean(options.capsLock); keyboardState.numLock = Boolean(options.numLock);
      activeTouches.clear(); imeComposing = false; imeText = '';
      Mouse[resetMouseToken]?.();
      mouseStateRecord.cursor = 'auto'; mouseStateRecord.visible = true;
      fallbackChildren.splice(0, fallbackChildren.length, ...(Array.isArray(options.initialChildren) ? options.initialChildren : []));
      options.onMouseChanged?.();
    }
    function dispose() { cancelPointer();for (const cleanup of cleanups.splice(0)) cleanup(); attached = false; hub.release(stage); focus = null; focusReason = null; nativeFocus=false; over = null; resetState(); }
    function reset() {
      cancelPointer();
      focus = null; focusReason = null; nativeFocus=false; focusVersion++; over = null; mouseX = 0; mouseY = 0;
      width = initial.width; height = initial.height;
      if (options.setFrameRate) options.setFrameRate(initial.frameRate);
      frameRate = initial.frameRate;
      resetState();
      hub.release(stage);
    }
    return Object.freeze({ stage, Keyboard, Mouse, mouseState: () => ({ cursor: mouseStateRecord.cursor, visible: mouseStateRecord.visible }),
      focusReason: () => (focus && alive(focus) ? focusReason : null),
      fullScreenSourceRect: () => (fullScreenSourceRect ? { ...fullScreenSourceRect } : null),
      colorAssigned: () => colorAssigned, flushRender, handlePointer, handleTouch, handleComposition, handleKey, cancelPointer, changeFocus, resize, attach, dispose, reset,
      resolveAccessibilityOverlayNodes: (entries = [], extraOptions = {}) => resolveAccessibilityOverlayNodes(
        entries,
        {
          focusedTarget: focus && alive(focus) ? focus : null,
          focusReason: focus && alive(focus) ? focusReason : null,
          ...extraOptions,
        },
      ),
      validateAccessibilityReadOrder: (entries = []) => validateAccessibilityReadOrder(entries),
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
    AccessibilityProperties, ContextMenuBuiltInItems, ContextMenuClipboardItems, ContextMenu, ContextMenuItem,
    createKeyboard, createMouse, setStageAdapters,
    resolveAccessibilityOverlayNodes, validateAccessibilityReadOrder, syncAccessibilityOverlay });
  global.NeoFlashStage = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
