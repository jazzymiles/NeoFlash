/* Semantic DOM text layer, DOM forms & inputs, <neoflash-stage> Web Component,
 * and responsive stage layout / breakpoint engine for HTML5 publications. */
(function (global) {
  'use strict';

  const VALID_SCALE_MODES = new Set(['showAll', 'noBorder', 'exactFit', 'noScale', 'fit']);
  const VALID_CONTROL_KINDS = new Set(['input', 'textarea', 'select', 'button', 'checkbox', 'radio', 'form']);

  function sanitizeDomLinkHref(rawUrl) {
    if (typeof rawUrl !== 'string') return null;
    if (/[\u0000-\u0008\u000b\u000e-\u001f\u007f-\u009f]/.test(rawUrl)) return null;
    const trimmed = rawUrl.trim();
    if (!trimmed || trimmed.length > 8192 || /[\u0000-\u001f\u007f-\u009f]/.test(trimmed)) {
      return null;
    }
    if (trimmed.startsWith('#')) return trimmed;
    if (trimmed.startsWith('//') || trimmed.startsWith('\\')) return null;
    const firstDelim = trimmed.search(/[/?#]/);
    const beforeDelim = firstDelim === -1 ? trimmed : trimmed.slice(0, firstDelim);
    if (!beforeDelim.includes(':')) {
      if (trimmed.includes('\\')) return null;
      return trimmed;
    }
    if (/^mailto:/i.test(trimmed)) {
      if (!trimmed.slice(7).trim()) return null;
      try {
        const parsed = new URL(trimmed);
        return parsed.protocol === 'mailto:' ? trimmed : null;
      } catch (_err) {
        return null;
      }
    }
    if (/^https?:\/\//i.test(trimmed)) {
      try {
        const parsed = new URL(trimmed);
        if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null;
        if (parsed.username || parsed.password) return null;
        return trimmed;
      } catch (_err) {
        return null;
      }
    }
    return null;
  }

  function sanitizeDomLinkTarget(rawTarget) {
    if (typeof rawTarget !== 'string') return '';
    const trimmed = rawTarget.trim();
    if (!trimmed || trimmed.length > 256 || /[\s\u0000-\u001f\u007f-\u009f<>]/.test(trimmed)) {
      return '';
    }
    const lower = trimmed.toLowerCase();
    if (['_self', '_blank', '_parent', '_top'].includes(lower)) return lower;
    if (trimmed.startsWith('_')) return '';
    return trimmed;
  }

  function computeResponsiveLayout(stageWidth, stageHeight, viewportWidth, viewportHeight, options = {}) {
    if (stageWidth && typeof stageWidth === 'object') {
      const cfg = stageWidth;
      const vw = Math.max(1, Number(cfg.containerWidth ?? cfg.viewportWidth ?? cfg.stageWidth) || 800);
      const vh = Math.max(1, Number(cfg.containerHeight ?? cfg.viewportHeight ?? cfg.stageHeight) || 600);
      const bp = evaluateBreakpoint(vw, vh, cfg.breakpoints || []);
      const sw = Math.max(1, Number(bp?.stageWidth ?? cfg.stageWidth) || 800);
      const sh = Math.max(1, Number(bp?.stageHeight ?? cfg.stageHeight) || 600);
      const res = computeResponsiveLayout(sw, sh, vw, vh, {
        ...cfg,
        scaleMode: bp?.scaleMode || cfg.scaleMode || 'showAll',
        align: bp?.align ?? cfg.align ?? '',
      });
      res.breakpoint = bp ? bp.name || 'matched' : null;
      res.stageWidth = sw;
      res.stageHeight = sh;
      return res;
    }
    const sw = Math.max(1, Number(stageWidth) || 800);
    const sh = Math.max(1, Number(stageHeight) || 600);
    const vw = Math.max(1, Number(viewportWidth) || sw);
    const vh = Math.max(1, Number(viewportHeight) || sh);
    const gutter = Number.isFinite(options.gutter) ? Math.max(0, options.gutter) : 0;
    const availW = Math.max(1, vw - 2 * gutter);
    const availH = Math.max(1, vh - 2 * gutter);
    const mode = VALID_SCALE_MODES.has(options.scaleMode) ? options.scaleMode : 'showAll';
    const align = String(options.align || '').toUpperCase();

    let scaleX = 1;
    let scaleY = 1;
    if (mode === 'exactFit') {
      scaleX = availW / sw;
      scaleY = availH / sh;
    } else if (mode === 'noBorder') {
      scaleX = scaleY = Math.max(availW / sw, availH / sh);
    } else if (mode === 'noScale') {
      scaleX = scaleY = 1;
    } else {
      // showAll / fit
      scaleX = scaleY = Math.min(availW / sw, availH / sh);
    }

    const renderedW = sw * scaleX;
    const renderedH = sh * scaleY;

    let left = gutter + (availW - renderedW) / 2;
    let top = gutter + (availH - renderedH) / 2;
    if (align.includes('L')) left = gutter;
    else if (align.includes('R')) left = vw - gutter - renderedW;
    if (align.includes('T')) top = gutter;
    else if (align.includes('B')) top = vh - gutter - renderedH;

    return {
      scale: scaleX,
      scaleX,
      scaleY,
      stageWidth: renderedW,
      stageHeight: renderedH,
      logicalWidth: sw,
      logicalHeight: sh,
      left,
      top,
      offsetX: left,
      offsetY: top,
      spaceWidth: Math.max(vw, renderedW + 2 * gutter),
      spaceHeight: Math.max(vh, renderedH + 2 * gutter),
      scaleMode: mode,
      align,
    };
  }

  function evaluateBreakpoint(viewportWidth, viewportHeight, breakpoints = []) {
    if (!Array.isArray(breakpoints) || !breakpoints.length) return null;
    const vw = Math.max(1, Number(viewportWidth) || 1);
    const vh = Math.max(1, Number(viewportHeight) || 1);
    const aspect = vw / vh;
    for (const bp of breakpoints) {
      if (!bp || typeof bp !== 'object') continue;
      if (Number.isFinite(bp.minWidth) && vw < bp.minWidth) continue;
      if (Number.isFinite(bp.maxWidth) && vw > bp.maxWidth) continue;
      if (Number.isFinite(bp.minHeight) && vh < bp.minHeight) continue;
      if (Number.isFinite(bp.maxHeight) && vh > bp.maxHeight) continue;
      if (Number.isFinite(bp.minAspectRatio) && aspect < bp.minAspectRatio) continue;
      if (Number.isFinite(bp.maxAspectRatio) && aspect > bp.maxAspectRatio) continue;
      return bp;
    }
    return null;
  }

  class ResponsiveController {
    constructor({
      viewport = null,
      stageWidth = 800,
      stageHeight = 600,
      scaleMode = 'showAll',
      align = '',
      breakpoints = [],
      gutter = 0,
      onChange = null,
    } = {}) {
      this.viewport = viewport;
      this.baseWidth = stageWidth;
      this.baseHeight = stageHeight;
      this.scaleMode = scaleMode;
      this.align = align;
      this.breakpoints = Array.isArray(breakpoints) ? breakpoints.slice() : [];
      this.gutter = gutter;
      this.onChange = onChange;
      this.activeBreakpoint = null;
      this.currentLayout = null;
      this.observer = null;
      if (viewport && typeof global.ResizeObserver === 'function') {
        this.observer = new global.ResizeObserver(() => this.refresh());
        this.observer.observe(viewport);
      }
    }

    setBreakpoints(breakpoints) {
      this.breakpoints = Array.isArray(breakpoints) ? breakpoints.slice() : [];
      return this.refresh();
    }

    setScaleMode(scaleMode, align = this.align) {
      if (VALID_SCALE_MODES.has(scaleMode)) this.scaleMode = scaleMode;
      if (typeof align === 'string') this.align = align;
      return this.refresh();
    }

    compute(viewportWidth, viewportHeight) {
      const vw = viewportWidth ?? this.viewport?.clientWidth ?? this.baseWidth;
      const vh = viewportHeight ?? this.viewport?.clientHeight ?? this.baseHeight;
      const bp = evaluateBreakpoint(vw, vh, this.breakpoints);
      const prevBp = this.activeBreakpoint;
      this.activeBreakpoint = bp;
      const sw = bp?.stageWidth || this.baseWidth;
      const sh = bp?.stageHeight || this.baseHeight;
      const mode = bp?.scaleMode || this.scaleMode;
      const align = bp?.align ?? this.align;
      const next = computeResponsiveLayout(sw, sh, vw, vh, {
        scaleMode: mode,
        align,
        gutter: this.gutter,
      });
      next.breakpoint = bp ? bp.name || 'matched' : null;
      next.breakpointChanged = (prevBp?.name || null) !== (bp?.name || null);
      this.currentLayout = next;
      return next;
    }

    refresh(viewportWidth, viewportHeight) {
      const layout = this.compute(viewportWidth, viewportHeight);
      this.onChange?.(layout, this.activeBreakpoint);
      return layout;
    }

    dispose() {
      this.observer?.disconnect?.();
      this.observer = null;
    }
  }

  class SemanticDomTextLayer {
    constructor(container, optionsOrDoc = {}) {
      const opts =
        optionsOrDoc && typeof optionsOrDoc.createElement === 'function'
          ? { document: optionsOrDoc }
          : optionsOrDoc || {};
      this.container = container;
      this.doc = opts.document || global.document || container?.ownerDocument || null;
      this.onLink = opts.onLink || null;
      this.onNavigateUrl = typeof opts.onNavigateUrl === 'function' ? opts.onNavigateUrl : null;
      this.nodes = new Map();
      if (this.container) {
        this.container.setAttribute('data-neoflash-dom-text', 'true');
        Object.assign(this.container.style, {
          position: 'absolute',
          inset: '0',
          pointerEvents: 'none',
          overflow: 'hidden',
        });
      }
    }

    sync(scene = [], _activeEditingRefs = null, scale = { x: 1, y: 1 }) {
      return this.update(scene, scale);
    }

    update(scene = [], scale = { x: 1, y: 1 }) {
      if (!this.container || !this.doc) return 0;
      const scaleX = typeof scale === 'object' ? Number(scale.x) || 1 : Number(scale) || 1;
      const scaleY = typeof scale === 'object' ? Number(scale.y) || scaleX : scaleX;
      const seen = new Set();
      let count = 0;

      for (let i = 0; i < scene.length; i++) {
        const item = scene[i];
        if (!item || (item.alpha !== undefined && !(item.alpha > 0))) continue;
        const kind = item.kind;
        const field =
          kind?.type === 'text_field' ? kind.field : item.text_field ? item.text_field : null;
        if (!field && (!kind || kind.type !== 'text')) continue;
        if (field && field.display_as_password) continue;
        const rawText = field ? String(field.text ?? '') : String(kind.text ?? '');
        if (!rawText.trim()) continue;

        const key = item.owner_ref
          ? JSON.stringify(item.owner_ref)
          : item.instance_path
            ? JSON.stringify(item.instance_path)
            : `${item.element_id || 'text'}:${i}`;
        seen.add(key);

        let el = this.nodes.get(key);
        if (!el) {
          el = this.doc.createElement('div');
          el.className = 'neoflash-dom-text-node';
          el.setAttribute('data-neoflash-text-key', key);
          this.container.appendChild(el);
          this.nodes.set(key, el);
        }
        if (field) {
          el.setAttribute('aria-live', 'polite');
        }

        const [a, b, c, d, tx, ty] = item.transform || [1, 0, 0, 1, 0, 0];
        const selectable = field ? Boolean(field.selectable || field.field_type === 'input') : true;
        const fontFamily = field ? field.format?.font || 'sans-serif' : kind.font_family || 'sans-serif';
        const fontSize = field ? Number(field.format?.size) || 12 : Number(kind.font_size) || 12;
        const fontWeight = field?.format?.bold ? '700' : '400';
        const fontStyle = field?.format?.italic ? 'italic' : 'normal';
        const resolvedCssFamily = global.NeoFlashText?.cssFontFamily
          ? global.NeoFlashText.cssFontFamily(fontFamily)
          : (['serif', 'sans-serif', 'monospace', 'cursive', 'fantasy', 'system-ui', 'ui-serif', 'ui-sans-serif', 'ui-monospace', 'ui-rounded', 'emoji', 'math', 'fangsong'].includes(String(fontFamily).trim().toLowerCase())
              ? String(fontFamily).trim().toLowerCase()
              : `${JSON.stringify(fontFamily)}, sans-serif`);
        const colorCss = field
          ? `#${((field.format?.color ?? 0) & 0xffffff).toString(16).padStart(6, '0')}`
          : Array.isArray(kind.color)
            ? `rgba(${kind.color[0]},${kind.color[1]},${kind.color[2]},${(kind.color[3] ?? 255) / 255})`
            : '#000000';

        Object.assign(el.style, {
          position: 'absolute',
          left: '0px',
          top: '0px',
          transformOrigin: '0 0',
          transform: `matrix(${a * scaleX},${b * scaleY},${c * scaleX},${d * scaleY},${tx * scaleX},${ty * scaleY})`,
          fontFamily: resolvedCssFamily,
          fontSize: `${fontSize}px`,
          fontWeight,
          fontStyle,
          lineHeight: '1.2',
          color: 'transparent',
          userSelect: selectable ? 'text' : 'none',
          webkitUserSelect: selectable ? 'text' : 'none',
          pointerEvents: selectable ? 'auto' : 'none',
          whiteSpace: field && field.word_wrap ? 'pre-wrap' : 'pre',
          margin: '0',
          padding: '0',
        });
        el.setAttribute('data-text-color', colorCss);
        if (field?.box) {
          el.style.width = `${field.box.width}px`;
          el.style.height = `${field.box.height}px`;
        } else {
          el.style.width = 'auto';
          el.style.height = 'auto';
        }

        const runs =
          Array.isArray(field?.format_runs) && field.format_runs.length
            ? field.format_runs
            : field?.format?.url
              ? [{ start: 0, end: rawText.length, format: field.format }]
              : null;
        if (runs) {
          while (el.children?.length) el.removeChild(el.children[0]);
          el.textContent = '';
          for (const run of runs) {
            const slice = rawText.slice(run.start, run.end);
            if (!slice) continue;
            const fmt = run.format || field.format || {};
            const safeHref = fmt.url ? sanitizeDomLinkHref(String(fmt.url)) : null;
            const node = safeHref ? this.doc.createElement('a') : this.doc.createElement('span');
            node.textContent = slice;
            if (fmt.bold) node.style.fontWeight = 'bold';
            if (fmt.italic) node.style.fontStyle = 'italic';
            if (fmt.underline) node.style.textDecoration = 'underline';
            if (fmt.size) node.style.fontSize = `${fmt.size}px`;
            if (fmt.font) node.style.fontFamily = JSON.stringify(fmt.font) + ', sans-serif';
            if (safeHref) {
              const safeTarget = sanitizeDomLinkTarget(fmt.target);
              node.setAttribute('href', safeHref);
              node.href = safeHref;
              if (safeTarget) {
                node.setAttribute('target', safeTarget);
                node.target = safeTarget;
              }
              node.setAttribute('rel', 'noopener noreferrer');
              node.rel = 'noopener noreferrer';
              node.style.pointerEvents = 'auto';
              node.addEventListener('click', ev => {
                if (this.onNavigateUrl) {
                  ev.preventDefault?.();
                  ev.stopPropagation?.();
                  this.onNavigateUrl(safeHref, safeTarget || '_blank', item.owner_ref);
                } else if (this.onLink) {
                  ev.preventDefault?.();
                  this.onLink(safeHref, safeTarget || '', item.owner_ref);
                }
              });
            }
            el.appendChild(node);
          }
        } else if (el.textContent !== rawText) {
          el.textContent = rawText;
        }
        count++;
      }

      for (const [key, el] of this.nodes) {
        if (!seen.has(key)) {
          el.remove();
          this.nodes.delete(key);
        }
      }
      return count;
    }

    clear() {
      for (const el of this.nodes.values()) el.remove();
      this.nodes.clear();
    }
  }

  class DomFormsHost {
    constructor(container, { document: doc = global.document, onEvent = null } = {}) {
      this.container = container;
      this.doc = doc || container?.ownerDocument || null;
      this.onEvent = onEvent;
      this.controls = new Map();
      this.forms = new Map();
      if (this.container) {
        this.container.setAttribute('data-neoflash-dom-forms', 'true');
        Object.assign(this.container.style, {
          position: 'absolute',
          inset: '0',
          pointerEvents: 'none',
        });
      }
    }

    createForm({ id, x = 0, y = 0, width = 300, height = 200, controls = [], onSubmit = null } = {}) {
      if (!this.doc) throw new Error('DOM forms require a Document.');
      const formId = String(id || `form-${this.forms.size + 1}`);
      let record = this.forms.get(formId);
      if (record) record.element.remove();
      const formEl = this.doc.createElement('form');
      formEl.setAttribute('data-neoflash-form', formId);
      Object.assign(formEl.style, {
        position: 'absolute',
        left: `${x}px`,
        top: `${y}px`,
        width: `${width}px`,
        height: `${height}px`,
        pointerEvents: 'auto',
        margin: '0',
        padding: '0',
      });
      formEl.addEventListener('submit', event => {
        event.preventDefault?.();
        const values = this.getFormValues(formId);
        onSubmit?.(values, event);
        this.onEvent?.({ type: 'submit', formId, values, originalEvent: event });
      });
      this.container?.appendChild(formEl);
      record = { id: formId, element: formEl, x, y, width, height, controlIds: [], onSubmit };
      this.forms.set(formId, record);
      for (const ctrl of controls) {
        this.createControl({ ...ctrl, formId });
      }
      return record;
    }

    createControl(spec = {}) {
      if (!this.doc) throw new Error('DOM controls require a Document.');
      const rawKind = spec.kind || spec.type || 'input';
      const kind = VALID_CONTROL_KINDS.has(rawKind) ? rawKind : 'input';
      const id = Array.isArray(spec.id)
        ? JSON.stringify(spec.id)
        : String(spec.id || spec.name || `ctrl-${this.controls.size + 1}`);
      if (this.controls.has(id)) {
        this.controls.get(id).element.remove();
      }
      let el;
      if (kind === 'textarea') {
        el = this.doc.createElement('textarea');
        el.value = String(spec.value ?? '');
      } else if (kind === 'select') {
        el = this.doc.createElement('select');
        for (const opt of spec.options || []) {
          const optEl = this.doc.createElement('option');
          if (typeof opt === 'object' && opt !== null) {
            optEl.value = String(opt.value ?? opt.label ?? '');
            optEl.textContent = String(opt.label ?? opt.value ?? '');
            if (opt.selected) optEl.selected = true;
          } else {
            optEl.value = String(opt);
            optEl.textContent = String(opt);
          }
          el.appendChild(optEl);
        }
        if (spec.value !== undefined) el.value = String(spec.value);
      } else if (kind === 'button') {
        el = this.doc.createElement('button');
        el.type = spec.buttonType || 'button';
        el.textContent = String(spec.label ?? spec.value ?? 'Button');
      } else {
        el = this.doc.createElement('input');
        el.type = kind === 'checkbox' || kind === 'radio' ? kind : spec.type || 'text';
        if (kind === 'checkbox' || kind === 'radio') {
          el.checked = Boolean(spec.checked ?? spec.value);
          el.value = String(spec.value ?? 'on');
        } else {
          el.value = String(spec.value ?? '');
        }
      }

      el.name = String(spec.name || id);
      el.setAttribute('data-neoflash-control', id);
      if (spec.placeholder) el.placeholder = String(spec.placeholder);
      if (spec.disabled) el.disabled = true;
      if (spec.required) el.required = true;
      if (spec.ariaLabel) el.setAttribute('aria-label', String(spec.ariaLabel));

      const x = Number(spec.x) || 0;
      const y = Number(spec.y) || 0;
      const width = Number(spec.width) || 160;
      const height = Number(spec.height) || 28;
      Object.assign(el.style, {
        position: 'absolute',
        left: `${x}px`,
        top: `${y}px`,
        width: `${width}px`,
        height: `${height}px`,
        pointerEvents: 'auto',
        boxSizing: 'border-box',
      });

      const record = {
        id,
        path: Array.isArray(spec.id) ? spec.id : [id],
        name: el.name,
        kind,
        element: el,
        control: el,
        formId: spec.formId || null,
        ownerRef: spec.ownerRef || null,
        x,
        y,
        width,
        height,
        onChange: spec.onChange || null,
        onInput: spec.onInput || null,
        onClick: spec.onClick || null,
      };

      const readValue = () => (kind === 'checkbox' || kind === 'radio' ? Boolean(el.checked) : el.value);
      const notify = (type, val, ev) => {
        if (!this.onEvent) return;
        const payload = { type, controlId: id, name: record.name, value: val, checked: Boolean(el.checked), originalEvent: ev };
        if (this.onEvent.length >= 2) this.onEvent(record.path, type, payload);
        else this.onEvent(payload);
      };

      el.addEventListener('input', ev => {
        const val = readValue();
        record.onInput?.(val, ev);
        notify('input', val, ev);
      });
      el.addEventListener('change', ev => {
        const val = readValue();
        record.onChange?.(val, ev);
        notify('change', val, ev);
      });
      el.addEventListener('click', ev => {
        record.onClick?.(ev);
        notify('click', readValue(), ev);
      });
      el.addEventListener('keydown', ev => {
        if (ev.key === 'Enter' && kind !== 'textarea') {
          ev.preventDefault?.();
          notify('submit', readValue(), ev);
        }
      });

      const parentForm = spec.formId ? this.forms.get(String(spec.formId)) : null;
      if (parentForm) {
        parentForm.element.appendChild(el);
        parentForm.controlIds.push(id);
      } else {
        this.container?.appendChild(el);
      }
      this.controls.set(id, record);
      return record;
    }

    mountControl(idOrSpec, spec = {}) {
      if (Array.isArray(idOrSpec) || typeof idOrSpec === 'string') {
        return this.createControl({ ...spec, id: idOrSpec });
      }
      return this.createControl(idOrSpec || {});
    }

    getControlValue(controlId) {
      const key = Array.isArray(controlId) ? JSON.stringify(controlId) : String(controlId);
      const rec = this.controls.get(key);
      return rec ? rec.element.value : undefined;
    }

    setControlValue(controlId, value) {
      const key = Array.isArray(controlId) ? JSON.stringify(controlId) : String(controlId);
      return this.setValue(key, value);
    }

    getControlChecked(controlId) {
      const key = Array.isArray(controlId) ? JSON.stringify(controlId) : String(controlId);
      const rec = this.controls.get(key);
      return rec ? Boolean(rec.element.checked) : false;
    }

    destroy() {
      this.clear();
    }

    getValue(controlId) {
      const rec = this.controls.get(String(controlId));
      if (!rec) return undefined;
      if (rec.kind === 'checkbox' || rec.kind === 'radio') return Boolean(rec.element.checked);
      return rec.element.value;
    }

    setValue(controlId, value) {
      const rec = this.controls.get(String(controlId));
      if (!rec) return false;
      if (rec.kind === 'checkbox' || rec.kind === 'radio') rec.element.checked = Boolean(value);
      else rec.element.value = String(value ?? '');
      return true;
    }

    getFormValues(formId) {
      const form = this.forms.get(String(formId));
      const out = Object.create(null);
      const ids = form ? form.controlIds : [...this.controls.keys()];
      for (const cid of ids) {
        const rec = this.controls.get(cid);
        if (!rec) continue;
        out[rec.name] = this.getValue(cid);
      }
      return out;
    }

    setFormValues(formId, values = {}) {
      if (!values || typeof values !== 'object') return false;
      const form = this.forms.get(String(formId));
      const ids = form ? form.controlIds : [...this.controls.keys()];
      for (const cid of ids) {
        const rec = this.controls.get(cid);
        if (rec && Object.hasOwn(values, rec.name)) {
          this.setValue(cid, values[rec.name]);
        }
      }
      return true;
    }

    syncScale(scale = { x: 1, y: 1 }, objectsByRef = null) {
      const scaleX = typeof scale === 'object' ? Number(scale.x) || 1 : Number(scale) || 1;
      const scaleY = typeof scale === 'object' ? Number(scale.y) || scaleX : scaleX;
      for (const rec of this.controls.values()) {
        let x = rec.x;
        let y = rec.y;
        let visible = true;
        if (rec.ownerRef && objectsByRef) {
          const node = objectsByRef.get(JSON.stringify(rec.ownerRef));
          if (node) {
            visible = node.effective_visible !== false && node.on_stage !== false;
            if (Array.isArray(node.world)) {
              x = node.world[4] + rec.x;
              y = node.world[5] + rec.y;
            }
          }
        }
        rec.element.style.display = visible ? '' : 'none';
        if (!rec.formId) {
          rec.element.style.left = `${x * scaleX}px`;
          rec.element.style.top = `${y * scaleY}px`;
          rec.element.style.width = `${rec.width * scaleX}px`;
          rec.element.style.height = `${rec.height * scaleY}px`;
        }
      }
      for (const form of this.forms.values()) {
        form.element.style.left = `${form.x * scaleX}px`;
        form.element.style.top = `${form.y * scaleY}px`;
        form.element.style.width = `${form.width * scaleX}px`;
        form.element.style.height = `${form.height * scaleY}px`;
      }
    }

    clear() {
      for (const rec of this.controls.values()) rec.element.remove();
      for (const form of this.forms.values()) form.element.remove();
      this.controls.clear();
      this.forms.clear();
    }
  }

  function createStageComponentClass(BaseElement) {
    return class NeoFlashStageElement extends BaseElement {
      static get observedAttributes() {
        return ['src', 'width', 'height', 'scale-mode', 'align', 'autoplay', 'loop', 'responsive'];
      }

      constructor() {
        super();
        this._playing = !this.hasAttribute?.('autoplay') || this.getAttribute?.('autoplay') !== 'false';
        this._loop = !this.hasAttribute?.('loop') || this.getAttribute?.('loop') !== 'false';
        this._scaleMode = this.getAttribute?.('scale-mode') || 'showAll';
        this._align = this.getAttribute?.('align') || '';
        this._stageWidth = Number(this.getAttribute?.('width')) || 800;
        this._stageHeight = Number(this.getAttribute?.('height')) || 600;
        this._breakpoints = [];
        this._player = null;
        this._mounted = false;
        this._disposed = false;
        this._mountCount = 0;
        this._disposeCount = 0;
        this._onDispose = null;
        const isDomHost = this.getAttribute?.('data-render-mode') === 'dom';
        this._isDomHost = isDomHost;
        const root = !isDomHost && typeof this.attachShadow === 'function'
          ? this.attachShadow({ mode: 'open' })
          : this;
        this.shadowRootNode = root;
      }

      get mountCount() {
        return this._mountCount;
      }

      get disposeCount() {
        return this._disposeCount;
      }

      get isMounted() {
        return this._mounted && !this._disposed;
      }

      connectedCallback() {
        if (this._mounted) return;
        this._mounted = true;
        this._disposed = false;
        this._mountCount += 1;
        if (this.getAttribute?.('data-render-mode') === 'dom' || this._isDomHost) {
          this._isDomHost = true;
          this.shadowRootNode = this;
          const doc = this.ownerDocument || global.document;
          if (doc && !this.sceneElement) {
            const existing = this.querySelector?.('[data-nf-scene="root"]') || null;
            const scene = existing || doc.createElement('div');
            scene.className = 'nf-dom-scene';
            scene.setAttribute('data-nf-scene', 'root');
            if (!existing) this.appendChild(scene);
            this.sceneElement = scene;
          }
          return;
        }
        const doc = this.ownerDocument || global.document;
        if (doc && this.shadowRootNode) {
          const viewport = doc.createElement('div');
          viewport.className = 'neoflash-wc-viewport';
          Object.assign(viewport.style, {
            position: 'relative',
            width: '100%',
            height: '100%',
            overflow: 'hidden',
          });
          const surface = doc.createElement('div');
          surface.className = 'neoflash-wc-surface';
          Object.assign(surface.style, { position: 'absolute' });
          const canvas = doc.createElement('canvas');
          canvas.className = 'neoflash-wc-canvas';
          const textLayerEl = doc.createElement('div');
          const formsLayerEl = doc.createElement('div');
          surface.appendChild(canvas);
          surface.appendChild(textLayerEl);
          surface.appendChild(formsLayerEl);
          viewport.appendChild(surface);
          this.shadowRootNode.appendChild(viewport);
          this.viewportElement = viewport;
          this.surfaceElement = surface;
          this.canvasElement = canvas;
          this.domText = new SemanticDomTextLayer(textLayerEl, { document: doc });
          this.domForms = new DomFormsHost(formsLayerEl, { document: doc });
          this.responsiveController = new ResponsiveController({
            viewport,
            stageWidth: this._stageWidth,
            stageHeight: this._stageHeight,
            scaleMode: this._scaleMode,
            align: this._align,
            breakpoints: this._breakpoints,
            onChange: layout => {
              Object.assign(surface.style, {
                left: `${layout.left}px`,
                top: `${layout.top}px`,
                width: `${layout.stageWidth}px`,
                height: `${layout.stageHeight}px`,
              });
              const dpr = Math.min(4, Math.max(1, Number(global.devicePixelRatio) || 1));
              canvas.width = Math.max(1, Math.round(layout.logicalWidth * dpr));
              canvas.height = Math.max(1, Math.round(layout.logicalHeight * dpr));
              if (canvas.style) {
                canvas.style.width = `${layout.stageWidth}px`;
                canvas.style.height = `${layout.stageHeight}px`;
              }
              canvas.getContext?.('2d')?.setTransform?.(dpr, 0, 0, dpr, 0, 0);
            },
          });
          this.responsiveController.refresh(this._stageWidth, this._stageHeight);
        }
      }

      dispose() {
        if (!this._mounted || this._disposed) return false;
        this._disposed = true;
        this._mounted = false;
        this._disposeCount += 1;
        const onDispose = this._onDispose;
        this._onDispose = null;
        if (typeof onDispose === 'function') {
          try { onDispose(); } catch (_) {}
        }
        this.responsiveController?.dispose();
        this.domText?.clear();
        this.domForms?.clear();
        if (this.sceneElement) {
          this.sceneElement.replaceChildren?.();
          this.sceneElement.remove?.();
          this.sceneElement = null;
        }
        if (this.viewportElement) {
          this.viewportElement.remove?.();
          this.viewportElement = null;
          this.surfaceElement = null;
          this.canvasElement = null;
        }
        return true;
      }

      disconnectedCallback() {
        this.dispose();
      }

      attributeChangedCallback(name, _oldValue, newValue) {
        if (name === 'width') this._stageWidth = Math.max(1, Number(newValue) || 800);
        else if (name === 'height') this._stageHeight = Math.max(1, Number(newValue) || 600);
        else if (name === 'scale-mode') this._scaleMode = newValue || 'showAll';
        else if (name === 'align') this._align = newValue || '';
        else if (name === 'autoplay') this._playing = newValue !== 'false';
        else if (name === 'loop') this._loop = newValue !== 'false';
        if (this.responsiveController) {
          this.responsiveController.baseWidth = this._stageWidth;
          this.responsiveController.baseHeight = this._stageHeight;
          this.responsiveController.setScaleMode(this._scaleMode, this._align);
        }
      }

      get playing() {
        return this._playing;
      }
      play() {
        this._playing = true;
        this._player?.resume?.();
      }
      pause() {
        this._playing = false;
        this._player?.pause?.();
      }
      restart() {
        this._player?.restart?.();
      }
      setBreakpoints(breakpoints) {
        this._breakpoints = Array.isArray(breakpoints) ? breakpoints.slice() : [];
        return this.responsiveController?.setBreakpoints(this._breakpoints) || null;
      }
      resize(width, height) {
        return this.responsiveController?.refresh(width, height) || null;
      }
    };
  }

  function registerStageComponent(tagName = 'neoflash-stage', customElementsRegistry = global.customElements) {
    const Base = typeof global.HTMLElement === 'function' ? global.HTMLElement : class {};
    const ElementClass = createStageComponentClass(Base);
    if (customElementsRegistry && typeof customElementsRegistry.define === 'function' && !customElementsRegistry.get?.(tagName)) {
      try {
        customElementsRegistry.define(tagName, ElementClass);
      } catch (_) {}
    }
    return ElementClass;
  }

  function create({
    surface = null,
    viewport = null,
    project = null,
    document: doc = global.document,
    onLink = null,
    onFormEvent = null,
    onBreakpoint = null,
  } = {}) {
    let textContainer = surface?.querySelector?.('[data-neoflash-dom-text]') || null;
    let formsContainer = surface?.querySelector?.('[data-neoflash-dom-forms]') || null;
    if (surface && doc) {
      if (!textContainer) {
        textContainer = doc.createElement('div');
        surface.appendChild(textContainer);
      }
      if (!formsContainer) {
        formsContainer = doc.createElement('div');
        surface.appendChild(formsContainer);
      }
    }
    const textLayer = new SemanticDomTextLayer(textContainer, { document: doc, onLink });
    const formsHost = new DomFormsHost(formsContainer, { document: doc, onEvent: onFormEvent });
    const responsive = new ResponsiveController({
      viewport,
      stageWidth: project?.width || 800,
      stageHeight: project?.height || 600,
      scaleMode: project?.scaleMode || 'showAll',
      align: project?.align || '',
      breakpoints: project?.breakpoints || [],
      onChange: (layout, bp) => {
        if (layout.breakpointChanged) onBreakpoint?.(bp, layout);
      },
    });

    function update(snapshot, scale = { x: 1, y: 1 }) {
      const draws = snapshot?.draws || (Array.isArray(snapshot) ? snapshot : []);
      const objects = snapshot?.objects
        ? new Map(snapshot.objects.map(n => [JSON.stringify(n.object_ref), n]))
        : null;
      const textCount = textLayer.update(draws, scale);
      formsHost.syncScale(scale, objects);
      return { textCount, controlCount: formsHost.controls.size };
    }

    function reset() {
      textLayer.clear();
      formsHost.clear();
    }

    function dispose() {
      reset();
      responsive.dispose();
    }

    return Object.freeze({
      textLayer,
      formsHost,
      responsive,
      update,
      createForm: spec => formsHost.createForm(spec),
      createControl: spec => formsHost.createControl(spec),
      getFormValues: id => formsHost.getFormValues(id),
      setFormValues: (id, vals) => formsHost.setFormValues(id, vals),
      getValue: id => formsHost.getValue(id),
      setValue: (id, val) => formsHost.setValue(id, val),
      setBreakpoints: bps => responsive.setBreakpoints(bps),
      setScaleMode: (mode, align) => responsive.setScaleMode(mode, align),
      computeLayout: (vw, vh) => responsive.refresh(vw, vh),
      reset,
      dispose,
    });
  }

  const SVG_NS = 'http://www.w3.org/2000/svg';
  const CSS_BLEND_MODES = Object.freeze({
    normal: 'normal',
    layer: 'normal',
    multiply: 'multiply',
    screen: 'screen',
    overlay: 'overlay',
    darken: 'darken',
    lighten: 'lighten',
    hardlight: 'hard-light',
    'hard-light': 'hard-light',
    difference: 'difference',
  });
  const COMPOSITE_TO_CSS_BLEND = Object.freeze({
    'source-over': 'normal',
    multiply: 'multiply',
    screen: 'screen',
    overlay: 'overlay',
    darken: 'darken',
    lighten: 'lighten',
    'hard-light': 'hard-light',
    difference: 'difference',
  });

  function matMultiply(a, b) {
    return [
      a[0] * b[0] + a[2] * b[1],
      a[1] * b[0] + a[3] * b[1],
      a[0] * b[2] + a[2] * b[3],
      a[1] * b[2] + a[3] * b[3],
      a[0] * b[4] + a[2] * b[5] + a[4],
      a[1] * b[4] + a[3] * b[5] + a[5],
    ];
  }

  function matInverse(m) {
    if (!Array.isArray(m) || m.length < 6) return null;
    const det = m[0] * m[3] - m[1] * m[2];
    if (!Number.isFinite(det) || Math.abs(det) < 1e-9) return null;
    const inv = [
      m[3] / det,
      -m[1] / det,
      -m[2] / det,
      m[0] / det,
      (m[2] * m[5] - m[3] * m[4]) / det,
      (m[1] * m[4] - m[0] * m[5]) / det,
    ];
    return inv.every(Number.isFinite) ? inv : null;
  }

  function matPoint(m, x, y) {
    return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
  }

  function cssMatrix(m) {
    if (!Array.isArray(m) || m.length < 6 || !m.every(Number.isFinite)) return 'none';
    if (m[0] === 1 && m[1] === 0 && m[2] === 0 && m[3] === 1 && m[4] === 0 && m[5] === 0) return 'none';
    return `matrix(${m[0]}, ${m[1]}, ${m[2]}, ${m[3]}, ${m[4]}, ${m[5]})`;
  }

  function xmlAttrEscape(value) {
    return String(value ?? '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function svgPathData(commands = []) {
    const parts = [];
    for (const op of commands) {
      if (!op) continue;
      if (op.op === 'move_to') parts.push(`M${op.x} ${op.y}`);
      else if (op.op === 'line_to') parts.push(`L${op.x} ${op.y}`);
      else if (op.op === 'quadratic_to') parts.push(`Q${op.cx} ${op.cy} ${op.x} ${op.y}`);
      else if (op.op === 'cubic_to') parts.push(`C${op.cx1} ${op.cy1} ${op.cx2} ${op.cy2} ${op.x} ${op.y}`);
      else if (op.op === 'close') parts.push('Z');
    }
    return parts.join(' ');
  }

  function svgGradientDef(id, paint) {
    const radius = 819.2;
    const spread = paint.spread === 'reflect' || paint.spread_method === 'reflect'
      ? 'reflect'
      : (paint.spread === 'repeat' || paint.spread_method === 'repeat' ? 'repeat' : 'pad');
    const m = Array.isArray(paint.matrix) && paint.matrix.length === 6 ? paint.matrix : [1, 0, 0, 1, 0, 0];
    const gt = `matrix(${m[0]} ${m[1]} ${m[2]} ${m[3]} ${m[4]} ${m[5]})`;
    const stops = (paint.stops || []).map(s => {
      const c = s.color || [0, 0, 0, 255];
      const offset = Math.max(0, Math.min(1, Number(s.ratio || 0) / 255));
      const alpha = Math.max(0, Math.min(1, Number(c[3] ?? 255) / 255));
      return `<stop offset="${offset}" stop-color="rgb(${c[0] | 0},${c[1] | 0},${c[2] | 0})" stop-opacity="${alpha}"/>`;
    }).join('');
    if (paint.kind === 'radial') {
      const fx = (Number(paint.focal_point_ratio) || 0) * radius;
      return `<radialGradient id="${xmlAttrEscape(id)}" gradientUnits="userSpaceOnUse" cx="0" cy="0" r="${radius}" fx="${fx}" fy="0" spreadMethod="${spread}" gradientTransform="${gt}">${stops}</radialGradient>`;
    }
    return `<linearGradient id="${xmlAttrEscape(id)}" gradientUnits="userSpaceOnUse" x1="${-radius}" y1="0" x2="${radius}" y2="0" spreadMethod="${spread}" gradientTransform="${gt}">${stops}</linearGradient>`;
  }

  function computeTextDiff(before, after, caretAfter) {
    if (Number.isInteger(caretAfter) && caretAfter >= 0 && caretAfter <= after.length) {
      let beforeEnd = before.length;
      let afterEnd = after.length;
      while (beforeEnd > 0 && afterEnd > caretAfter && before[beforeEnd - 1] === after[afterEnd - 1]) {
        beforeEnd--;
        afterEnd--;
      }
      if (afterEnd < after.length && /[\uDC00-\uDFFF]/.test(after[afterEnd])) {
        beforeEnd++;
        afterEnd++;
      }
      let begin = 0;
      while (begin < beforeEnd && begin < afterEnd && before[begin] === after[begin]) begin++;
      if (begin > 0 && begin < afterEnd && /[\uDC00-\uDFFF]/.test(after[begin])) begin--;
      return { begin, end: beforeEnd, text: after.slice(begin, afterEnd) };
    }
    let begin = 0;
    while (begin < before.length && begin < after.length && before[begin] === after[begin]) begin++;
    if (begin > 0 && begin < before.length && /[\uDC00-\uDFFF]/.test(before[begin])) begin--;
    let beforeEnd = before.length;
    let afterEnd = after.length;
    while (beforeEnd > begin && afterEnd > begin && before[beforeEnd - 1] === after[afterEnd - 1]) {
      beforeEnd--;
      afterEnd--;
    }
    if (beforeEnd < before.length && /[\uDC00-\uDFFF]/.test(before[beforeEnd])) {
      beforeEnd++;
      afterEnd++;
    }
    return { begin, end: beforeEnd, text: after.slice(begin, afterEnd) };
  }

  function displayedFieldText(field) {
    const raw = String(field?.text ?? '');
    return field?.multiline ? raw : raw.replace(/\n/g, ' ');
  }

  function rgbIntToCss(rgbInt, fallback = '#000000') {
    if (!Number.isInteger(rgbInt) || rgbInt < 0) return fallback;
    const r = (rgbInt >>> 16) & 255;
    const g = (rgbInt >>> 8) & 255;
    const b = rgbInt & 255;
    return `rgb(${r}, ${g}, ${b})`;
  }

  function containsNode(parent, node) {
    for (let current = node; current; current = current.parentNode) {
      if (current === parent) return true;
    }
    return false;
  }

  function syncChildrenOrder(parent, desired, preserveFocus = fn => fn()) {
    const desiredList = desired.filter(Boolean);
    const desiredSet = new Set(desiredList);
    for (let cur = parent.firstElementChild; cur;) {
      const next = cur.nextElementSibling;
      if (!desiredSet.has(cur)) cur.remove();
      cur = next;
    }
    const doc = parent.ownerDocument;
    const active = doc?.activeElement || null;
    const selStart = active && typeof active.selectionStart === 'number' ? active.selectionStart : null;
    const selEnd = active && typeof active.selectionEnd === 'number' ? active.selectionEnd : null;
    const selDirection = active?.selectionDirection;
    const retainedFocus = active && desiredList.some(child => containsNode(child, active));
    const reorder = () => {
      let cursor = parent.firstElementChild;
      for (const child of desiredList) {
        if (child === cursor) {
          cursor = cursor.nextElementSibling;
        } else {
          parent.insertBefore(child, cursor);
        }
      }
      if (retainedFocus && doc?.activeElement !== active && !active.disabled) {
        try {
          active.focus({ preventScroll: true });
          if (selStart !== null && selEnd !== null && typeof active.setSelectionRange === 'function') {
            active.setSelectionRange(selStart, selEnd, selDirection);
          }
        } catch (_) {}
      }
    };
    if (retainedFocus) preserveFocus(reorder); else reorder();
  }

  function defaultViewportBreakpointName(vw) {
    if (vw <= 600) return 'mobile';
    if (vw <= 1024) return 'tablet';
    return 'desktop';
  }

  function createDomStageHost(options = {}) {
    const {
      document: doc = global.document,
      surface = doc?.getElementById?.('stage-surface') || null,
      viewport = doc?.getElementById?.('viewport') || null,
      canvas = doc?.getElementById?.('stage') || null,
      project = null,
      config = {},
      player = null,
      getDisplay = () => null,
      getStageHost = () => null,
      getTextHost = () => null,
      getBitmapHost = () => null,
      getVideoHost = () => null,
      getEventHub = () => null,
      getEvents = () => global.NeoFlashEvents || null,
      getAs2ObjectHost = () => null,
      getAs2TextHost = () => null,
      getAudioHost = () => null,
      proxyFor = () => null,
      proxyForRef = () => null,
      referenceOf = () => null,
      onActivateObject = null,
      onTextLink = null,
      onAsfunction = null,
      onNavigateUrl = null,
      afterInput = null,
      onError = null,
      paintCanvasIsland = null,
    } = options;

    const a11yApi = typeof module !== 'undefined' && module.exports
      ? require('./accessibility.js')
      : global.NeoFlashAccessibility;

    if (!doc || !surface) {
      return Object.freeze({
        isActive: () => false,
        evaluateViewport: () => null,
        render: () => {},
        sync: () => {},
        owns: () => false,
        canFocus: () => true,
        focusChanged: () => true,
        reset: () => {},
        dispose: () => {},
      });
    }

    let stageElement = doc.getElementById?.('stage-dom-root')
      || surface.querySelector?.('neoflash-stage[data-render-mode="dom"]')
      || null;
    if (!stageElement) {
      stageElement = doc.createElement('neoflash-stage');
      stageElement.id = 'stage-dom-root';
      stageElement.setAttribute('data-render-mode', 'dom');
      stageElement.setAttribute('width', String(project?.width || 800));
      stageElement.setAttribute('height', String(project?.height || 600));
      surface.insertBefore(stageElement, surface.firstElementChild?.nextSibling || null);
    }
    if (typeof stageElement.connectedCallback === 'function' && !stageElement.isMounted) {
      stageElement.connectedCallback();
    }
    let sceneElement = stageElement.sceneElement || stageElement.querySelector?.('[data-nf-scene="root"]') || null;
    if (!sceneElement) {
      sceneElement = doc.createElement('div');
      sceneElement.className = 'nf-dom-scene';
      sceneElement.setAttribute('data-nf-scene', 'root');
      stageElement.appendChild(sceneElement);
      stageElement.sceneElement = sceneElement;
    }

    const authoredClickPlacements = new Set();
    if (Array.isArray(project?.symbols)) {
      for (const sym of project.symbols) {
        for (const layer of sym?.timeline?.layers || []) {
          for (const kf of layer?.keyframes || []) {
            for (const el of kf?.elements || []) {
              if (Array.isArray(el?.scripts) && el.scripts.length > 0) {
                if (el.id) authoredClickPlacements.add(String(el.id));
                if (el.name) authoredClickPlacements.add(String(el.name));
              }
            }
          }
        }
      }
    }

    const configuredBreakpoints = Array.isArray(config?.breakpoints) && config.breakpoints.length
      ? config.breakpoints.map(bp => ({
          name: bp.name,
          minWidth: bp.min_width ?? bp.minWidth,
          maxWidth: bp.max_width ?? bp.maxWidth,
          minHeight: bp.min_height ?? bp.minHeight,
          maxHeight: bp.max_height ?? bp.maxHeight,
          stageWidth: bp.stage_width ?? bp.stageWidth,
          stageHeight: bp.stage_height ?? bp.stageHeight,
          scaleMode: bp.scale_mode ?? bp.scaleMode,
          align: bp.stage_align ?? bp.align,
        }))
      : (Array.isArray(project?.breakpoints) ? project.breakpoints.slice() : []);

    let activeBreakpointName = null;
    let disposed = false;
    let idSeq = 1;
    let scratchEncodeCanvas = null;

    const containerRecords = new Map();
    const leafRecords = new Map();
    const islandRecords = new Map();
    const inputRecordsByRef = new Map();
    const bitmapUrlCache = new Map();
    const ownedInputControls = new WeakSet();
    const ownedFocusControls = new WeakSet();
    const suppressedAccessibility = new Map();
    const cleanups = [];
    let reconcilingFocus = false;
    let pendingNativeFocus = null;
    let rendering = false;
    let queuedFrameState = null;
    let inputUpdatePending = false;

    function notifyAfterInput() {
      if (rendering) inputUpdatePending = true;
      else afterInput?.();
    }

    function preserveNativeFocus(fn) {
      const previous = reconcilingFocus;
      reconcilingFocus = true;
      try { return fn(); } finally { reconcilingFocus = previous; }
    }

    function clearControlFocus(ownerKey) {
      const sh = getStageHost();
      const focused = sh?.stage?.focus;
      if (focused && referenceOf(focused)?.key === ownerKey) sh.stage.focus = null;
    }

    function restoreSuppressedAccessibility() {
      for (const [element, state] of suppressedAccessibility) {
        for (const [attribute, value] of Object.entries(state)) {
          if (value === null) element.removeAttribute(attribute);
          else element.setAttribute(attribute, value);
        }
      }
      suppressedAccessibility.clear();
    }

    function suppressDescendantAccessibility(owners) {
      const visit = (element, suppressed) => {
        if (suppressed) {
          suppressedAccessibility.set(element, {
            'aria-hidden': element.getAttribute('aria-hidden'),
            tabindex: element.getAttribute('tabindex'),
          });
          element.setAttribute('aria-hidden', 'true');
          if (/^(?:A|BUTTON|INPUT|TEXTAREA|SELECT)$/.test(element.tagName || '') || element.hasAttribute('tabindex')) {
            element.setAttribute('tabindex', '-1');
          }
          if (doc.activeElement === element) element.blur?.();
        }
        const owner = owners.get(element);
        const hideChildren = Boolean(owner)
          || element.getAttribute('aria-hidden') === 'true'
          || element.getAttribute('data-force-simple') === 'true';
        for (const child of Array.from(element.children || [])) {
          visit(child, suppressed || (hideChildren && child !== owner?.control));
        }
      };
      visit(sceneElement, false);
    }

    function guardedCall(fn) {
      try {
        return fn();
      } catch (err) {
        if (typeof onError === 'function') onError(err);
        else throw err;
        return undefined;
      }
    }

    function evaluateViewport(vw, vh, defaults = {}) {
      const matched = configuredBreakpoints.length
        ? evaluateBreakpoint(vw, vh, configuredBreakpoints)
        : null;
      const bpName = matched?.name || defaultViewportBreakpointName(vw);
      if (bpName !== activeBreakpointName) {
        activeBreakpointName = bpName;
        stageElement.setAttribute('data-breakpoint', bpName);
        if (doc.body) doc.body.setAttribute('data-breakpoint', bpName);
      }
      if (stageElement.style?.setProperty) {
        stageElement.style.setProperty('--neoflash-viewport-width', `${vw}px`);
        stageElement.style.setProperty('--neoflash-viewport-height', `${vh}px`);
      }
      const mapScale = sm => {
        if (!sm) return null;
        if (sm === 'no_scale' || sm === 'noScale') return 'noScale';
        if (sm === 'exact_fit' || sm === 'exactFit') return 'exactFit';
        if (sm === 'fill_crop' || sm === 'noBorder') return 'noBorder';
        if (sm === 'fit_keep_ratio' || sm === 'showAll') return 'showAll';
        return sm;
      };
      const alignMap = {
        center: '', top: 'T', bottom: 'B', left: 'L', right: 'R',
        top_left: 'TL', top_right: 'TR', bottom_left: 'BL', bottom_right: 'BR',
      };
      const mappedAlign = matched?.align !== undefined && matched?.align !== null
        ? (Object.hasOwn(alignMap, matched.align) ? alignMap[matched.align] : String(matched.align))
        : defaults.align;
      return {
        breakpoint: bpName,
        matched,
        docW: Number(matched?.stageWidth) > 0 ? Number(matched.stageWidth) : defaults.docW,
        docH: Number(matched?.stageHeight) > 0 ? Number(matched.stageHeight) : defaults.docH,
        scaleMode: mapScale(matched?.scaleMode) || defaults.scaleMode,
        align: mappedAlign,
      };
    }

    function hasExplicitClientCoords(domEvent) {
      if (!domEvent || !Number.isFinite(domEvent.clientX) || !Number.isFinite(domEvent.clientY)) return false;
      // A keyboard/programmatic `click()` has the browser sentinel coordinates 0,0/detail 0 and
      // should activate the control centre. Pointer events legitimately report 0,0 at the top-left
      // of a fullscreen stage, so never reinterpret those coordinates as missing.
      return domEvent.type !== 'click'
        || domEvent.clientX !== 0
        || domEvent.clientY !== 0
        || domEvent.detail !== 0;
    }

    function syntheticPointerFromDom(domEvent, element) {
      if (hasExplicitClientCoords(domEvent)) {
        return domEvent;
      }
      const rect = element?.getBoundingClientRect?.() || canvas?.getBoundingClientRect?.() || { left: 0, top: 0, width: 2, height: 2 };
      return {
        type: domEvent?.type || 'click',
        clientX: rect.left + rect.width / 2,
        clientY: rect.top + rect.height / 2,
        button: domEvent?.button ?? 0,
        detail: domEvent?.detail || 1,
        pointerType: 'mouse',
        isPrimary: true,
        ctrlKey: Boolean(domEvent?.ctrlKey),
        altKey: Boolean(domEvent?.altKey),
        shiftKey: Boolean(domEvent?.shiftKey),
        metaKey: Boolean(domEvent?.metaKey),
      };
    }

    function unlockMediaFromGesture() {
      for (const host of [getAudioHost(), getVideoHost()]) {
        if (host && host.status?.().state === 'awaiting_gesture') {
          host.resumeFromGesture?.().catch(() => {});
        }
      }
    }

    function stageCoordsFromClient(clientX, clientY) {
      if (!Number.isFinite(clientX) || !Number.isFinite(clientY)) return null;
      const refEl = canvas || stageElement;
      const r = refEl?.getBoundingClientRect?.();
      if (!r || !(r.width > 0 && r.height > 0)) return null;
      const lw = Number(stageElement.dataset.logicalWidth) || project?.width || 800;
      const lh = Number(stageElement.dataset.logicalHeight) || project?.height || 600;
      const ox = Number(stageElement.dataset.originX) || 0;
      const oy = Number(stageElement.dataset.originY) || 0;
      return {
        sx: (clientX - r.left) * lw / r.width - ox,
        sy: (clientY - r.top) * lh / r.height - oy,
      };
    }

    function isNativeInteractiveControlTarget(t) {
      if (!t || t === sceneElement || t === stageElement) return false;
      const tag = (t.tagName || '').toUpperCase();
      if (tag === 'BUTTON' || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'A' || tag === 'SELECT') return true;
      return Boolean(t.closest?.('button, input, textarea, a, select'));
    }

    function onSceneBackgroundPointer(type, event) {
      if (disposed) return;
      if (type === 'pointerdown') unlockMediaFromGesture();
      if (isNativeInteractiveControlTarget(event.target)) return;
      guardedCall(() => {
        const sh = getStageHost();
        const disp = getDisplay();
        if (!sh) return;
        const ptEvent = syntheticPointerFromDom(event, sceneElement);
        const pt = stageCoordsFromClient(ptEvent.clientX, ptEvent.clientY);
        const picked = pt && disp?.pickDetails ? disp.pickDetails(pt.sx, pt.sy) : null;
        sh.handlePointer(type, ptEvent, picked?.target || sh.stage, Boolean(picked?.target));
      });
    }

    function onSceneBackgroundClick(event) {
      if (disposed) return;
      if (isNativeInteractiveControlTarget(event.target)) return;
      guardedCall(() => {
        unlockMediaFromGesture();
        const sh = getStageHost();
        const disp = getDisplay();
        if (!sh) return;
        const ptEvent = syntheticPointerFromDom(event, sceneElement);
        const pt = stageCoordsFromClient(ptEvent.clientX, ptEvent.clientY);
        const picked = pt && disp?.pickDetails ? disp.pickDetails(pt.sx, pt.sy) : null;
        const target = picked?.target || null;
        const authored = picked?.owner || target;
        if (sh.handlePointer('click', ptEvent, target || sh.stage, Boolean(target)) === false) return;
        if (target && authored && disp?.alive?.(target) && disp?.alive?.(authored) && typeof onActivateObject === 'function') {
          onActivateObject(target, authored, ptEvent);
        }
      });
    }

    function onScenePointerDown(event) {
      onSceneBackgroundPointer('pointerdown', event);
    }
    function onScenePointerMove(event) {
      onSceneBackgroundPointer('pointermove', event);
    }
    function onScenePointerUp(event) {
      onSceneBackgroundPointer('pointerup', event);
    }
    function onStagePointerLeave(event) {
      if (disposed) return;
      guardedCall(() => {
        const sh = getStageHost();
        if (!sh) return;
        sh.handlePointer('pointerleave', event);
        sh.handlePointer('mouseLeave', event);
      });
    }

    const hostWin = doc?.defaultView || global;
    function onWindowPageHide(ev) {
      if (!ev?.persisted) dispose();
    }
    function onWindowUnload() {
      dispose();
    }
    sceneElement.addEventListener('pointerdown', onScenePointerDown);
    sceneElement.addEventListener('pointermove', onScenePointerMove);
    sceneElement.addEventListener('pointerup', onScenePointerUp);
    sceneElement.addEventListener('click', onSceneBackgroundClick);
    stageElement.addEventListener?.('pointerleave', onStagePointerLeave);
    hostWin?.addEventListener?.('pagehide', onWindowPageHide);
    hostWin?.addEventListener?.('unload', onWindowUnload);
    cleanups.push(() => {
      sceneElement.removeEventListener('pointerdown', onScenePointerDown);
      sceneElement.removeEventListener('pointermove', onScenePointerMove);
      sceneElement.removeEventListener('pointerup', onScenePointerUp);
      sceneElement.removeEventListener('click', onSceneBackgroundClick);
      stageElement.removeEventListener?.('pointerleave', onStagePointerLeave);
      hostWin?.removeEventListener?.('pagehide', onWindowPageHide);
      hostWin?.removeEventListener?.('unload', onWindowUnload);
    });

    function bitmapDataUrlFor(kind) {
      const bitmapHost = getBitmapHost();
      if (!bitmapHost?.cache?.paint) return null;
      const pixelKey = JSON.stringify(kind.pixel_ref || kind.id || '');
      const c = kind.color_transform || {};
      const colorKey = Array.isArray(c.multiply) ? JSON.stringify([c.multiply, c.add]) : '';
      const cacheKey = `${pixelKey}:${kind.revision ?? 0}:${colorKey}`;
      const cached = bitmapUrlCache.get(cacheKey);
      if (cached) return cached;
      let url = null;
      bitmapHost.cache.paint(kind, record => {
        if (!record || record.width <= 0 || record.height <= 0) return;
        if (record.canvas && typeof record.canvas.toDataURL === 'function') {
          url = record.canvas.toDataURL('image/png');
          return;
        }
        if (!scratchEncodeCanvas && typeof doc.createElement === 'function') {
          scratchEncodeCanvas = doc.createElement('canvas');
        }
        if (scratchEncodeCanvas && typeof scratchEncodeCanvas.getContext === 'function') {
          scratchEncodeCanvas.width = record.width;
          scratchEncodeCanvas.height = record.height;
          const ctx = scratchEncodeCanvas.getContext('2d');
          if (ctx) {
            if (record.canvas) {
              ctx.clearRect(0, 0, record.width, record.height);
              ctx.drawImage(record.canvas, 0, 0);
            } else if (record.bytes) {
              const img = ctx.createImageData(record.width, record.height);
              img.data.set(record.bytes);
              ctx.putImageData(img, 0, 0);
            }
            if (typeof scratchEncodeCanvas.toDataURL === 'function') {
              url = scratchEncodeCanvas.toDataURL('image/png');
            }
          }
        }
      });
      if (url) {
        if (bitmapUrlCache.size > 256) {
          const firstKey = bitmapUrlCache.keys().next().value;
          bitmapUrlCache.delete(firstKey);
        }
        bitmapUrlCache.set(cacheKey, url);
      }
      return url;
    }

    function isSimpleSvgClip(clipIds, clipsById, alphaMaskClipIds) {
      if (!clipIds?.length) return true;
      for (const id of clipIds) {
        const clip = clipsById.get(id);
        if (!clip || clip.invert || (clip.alpha_components !== null && clip.alpha_components !== undefined) || alphaMaskClipIds?.has(id)) {
          return false;
        }
        if (
          !Array.isArray(clip.components)
          || clip.components.some(c =>
            c.clip_ids?.length
            || c.fill_gradient
            || c.fill_bitmap
            || c.resource_bitmap
            || c.luminance
            || (typeof c.alpha === 'number' && c.alpha > 0 && c.alpha < 1)
          )
        ) {
          return false;
        }
      }
      return true;
    }

    function directFilterGroupReason(fg) {
      if (!fg) return 'filter_group';
      if (fg.filters?.length) return 'filters';
      if (fg.alpha_masks?.length) return 'alpha_mask';
      if (fg.opaque_background !== null && fg.opaque_background !== undefined) return 'opaque_background';
      if (fg.composite_operation && !Object.hasOwn(COMPOSITE_TO_CSS_BLEND, fg.composite_operation)) {
        return `blend:${fg.composite_operation}`;
      }
      return null;
    }

    function islandReasonForDraw(item, filterGroupsById, clipsById, alphaMaskClipIds, rootGroupReasons, fgAncestryFn) {
      if (item.filter_id !== null && item.filter_id !== undefined) {
        const chain = fgAncestryFn ? fgAncestryFn(item.filter_id) : [];
        if (!chain.length) return 'filter_group';
        for (const fg of chain) {
          const direct = directFilterGroupReason(fg);
          if (direct) return direct;
        }
        const rootReason = rootGroupReasons?.get(chain[0].id);
        if (rootReason) return rootReason;
      }
      const kind = item.kind;
      if (kind?.fill_bitmap || kind?.stroke_bitmap) return 'bitmap_fill';
      if (item.clip_ids?.length) {
        const isSvgKind = ['rectangle', 'ellipse', 'path', 'gradient_path'].includes(kind?.type);
        if (!isSvgKind || !isSimpleSvgClip(item.clip_ids, clipsById, alphaMaskClipIds)) return 'mask';
      }
      return null;
    }

    function ensureButtonRecord(containerRec, ownerKey) {
      if (containerRec.buttonRecord) return containerRec.buttonRecord;
      const btn = doc.createElement('button');
      btn.type = 'button';
      btn.className = 'nf-dom-button';
      btn.setAttribute('data-nf-id', `btn:${ownerKey}`);
      btn.setAttribute('data-nf-kind', 'button');
      const resolveProxy = () => {
        try {
          const parsed = JSON.parse(ownerKey);
          return Array.isArray(parsed) ? proxyFor(parsed) : proxyForRef(parsed);
        } catch (_) {
          return null;
        }
      };
      const hasExplicitCoords = hasExplicitClientCoords;
      const onPointer = (type, ev) => guardedCall(() => {
        if (disposed || btn.disabled) return;
        if (type === 'pointerdown') unlockMediaFromGesture();
        const sh = getStageHost();
        const disp = getDisplay();
        const target = resolveProxy();
        if (!sh || !target || (disp && !disp.alive(target))) return;
        const syn = syntheticPointerFromDom(ev, btn);
        if (type !== 'pointerleave' && disp?.resolveInput && hasExplicitCoords(ev)) {
          const pt = stageCoordsFromClient(ev.clientX, ev.clientY);
          if (pt) {
            const hit = disp.resolveInput(target, undefined, pt.sx, pt.sy);
            if (!hit) {
              const fallback = disp.pickDetails?.(pt.sx, pt.sy);
              sh.handlePointer(type, syn, fallback?.target || sh.stage, Boolean(fallback?.target));
              return;
            }
            sh.handlePointer(type, syn, hit, false);
            return;
          }
        }
        const resolved = disp?.resolveInput ? disp.resolveInput(target) : target;
        if (!resolved && type !== 'pointerleave') return;
        sh.handlePointer(type, syn, resolved || sh.stage, false);
      });
      let hadPointerPress = false;
      const onEnter = ev => onPointer('pointermove', ev);
      const onMove = ev => onPointer('pointermove', ev);
      const onDown = ev => {
        hadPointerPress = true;
        onPointer('pointerdown', ev);
      };
      const onUp = ev => onPointer('pointerup', ev);
      const onLeave = ev => {
        hadPointerPress = false;
        onPointer('pointerleave', ev);
      };
      const onClick = ev => {
        ev.stopPropagation();
        const fromPointer = hadPointerPress;
        hadPointerPress = false;
        if (disposed || btn.disabled) return;
        guardedCall(() => {
          unlockMediaFromGesture();
          const sh = getStageHost();
          const disp = getDisplay();
          const target = resolveProxy();
          if (!target || (disp && !disp.alive(target))) return;
          const syn = syntheticPointerFromDom(ev, btn);
          let resolved = null;
          let activationOwner = target;
          if (disp?.resolveInput && hasExplicitCoords(ev)) {
            const pt = stageCoordsFromClient(ev.clientX, ev.clientY);
            if (pt) {
              const hit = disp.resolveInput(target, undefined, pt.sx, pt.sy);
              if (!hit) {
                const fallback = disp.pickDetails?.(pt.sx, pt.sy);
                const fbTarget = fallback?.target || null;
                const fbOwner = fallback?.owner || fbTarget;
                if (sh && sh.handlePointer('click', syn, fbTarget || sh.stage, Boolean(fbTarget)) === false) return;
                if (fbTarget && fbOwner && disp.alive(fbTarget) && disp.alive(fbOwner) && typeof onActivateObject === 'function') {
                  onActivateObject(fbTarget, fbOwner, syn);
                }
                return;
              }
              resolved = hit;
            }
          }
          if (!resolved) {
            if (disp?.descriptor) {
              const desc = disp.descriptor(target);
              if (desc?.owner === 'button' && desc.button_hit_authoritative && !desc.button_hits?.length) return;
            }
            resolved = disp?.resolveInput ? disp.resolveInput(target) : target;
          }
          if (!resolved) return;
          if (!fromPointer && disp?.pointer) {
            disp.pointer('mouseDown', resolved, syn);
            disp.pointer('mouseUp', resolved, syn);
          }
          if (sh && (!disp || disp.alive(resolved))) {
            sh.stage.focus = resolved;
            if (sh.handlePointer('click', syn, resolved, false) === false) return;
          }
          if ((!disp || disp.alive(resolved)) && typeof onActivateObject === 'function') {
            onActivateObject(resolved, activationOwner, syn);
          } else {
            notifyAfterInput();
          }
        });
      };
      const onFocus = () => guardedCall(() => {
        if (disposed || record.syncingFocus || reconcilingFocus) return;
        const sh = getStageHost(), disp = getDisplay(), target = resolveProxy();
        if (!sh || !target || (disp && !disp.alive(target))) return;
        if (sh.stage.focus !== target) {
          sh.stage.focus = target;
          notifyAfterInput();
        }
      });
      const onBlur = ev => guardedCall(() => {
        if (disposed || record.syncingFocus || reconcilingFocus) return;
        if (ev?.relatedTarget && ownedFocusControls.has(ev.relatedTarget)) return;
        const sh = getStageHost(), target = resolveProxy();
        if (sh && target && sh.stage.focus === target) {
          sh.stage.focus = null;
          notifyAfterInput();
        }
      });
      ownedFocusControls.add(btn);
      btn.addEventListener('focus', onFocus);
      btn.addEventListener('blur', onBlur);
      btn.addEventListener('pointerenter', onEnter);
      btn.addEventListener('pointermove', onMove);
      btn.addEventListener('pointerdown', onDown);
      btn.addEventListener('pointerup', onUp);
      btn.addEventListener('pointerleave', onLeave);
      btn.addEventListener('click', onClick);
      const record = {
        element: btn,
        syncingFocus: false,
        cleanup() {
          ownedFocusControls.delete(btn);
          btn.removeEventListener('focus', onFocus);
          btn.removeEventListener('blur', onBlur);
          btn.removeEventListener('pointerenter', onEnter);
          btn.removeEventListener('pointermove', onMove);
          btn.removeEventListener('pointerdown', onDown);
          btn.removeEventListener('pointerup', onUp);
          btn.removeEventListener('pointerleave', onLeave);
          btn.removeEventListener('click', onClick);
          btn.remove();
          clearControlFocus(ownerKey);
        },
      };
      containerRec.buttonRecord = record;
      return record;
    }

    function ensureInputRecord(leafKey, ownerKey, multiline) {
      let rec = leafRecords.get(leafKey);
      const wantTag = multiline ? 'TEXTAREA' : 'INPUT';
      if (rec && (rec.kind !== 'input' || rec.element.tagName.toUpperCase() !== wantTag)) {
        rec.cleanup?.();
        leafRecords.delete(leafKey);
        inputRecordsByRef.delete(ownerKey);
        rec = null;
      }
      if (rec) return rec;

      const el = doc.createElement(multiline ? 'textarea' : 'input');
      if (!multiline) el.type = 'text';
      el.className = 'nf-dom-input';
      el.setAttribute('data-nf-id', `input:${ownerKey}`);
      el.setAttribute('data-nf-kind', 'input');
      el.setAttribute('autocomplete', 'off');
      el.setAttribute('autocapitalize', 'off');
      el.setAttribute('spellcheck', 'false');
      ownedInputControls.add(el);
      ownedFocusControls.add(el);

      const resolveReceiver = () => {
        try {
          const parsed = JSON.parse(ownerKey);
          return Array.isArray(parsed) ? proxyFor(parsed) : proxyForRef(parsed);
        } catch (_) {
          return null;
        }
      };

      const onFocus = () => guardedCall(() => {
        if (disposed || rec.syncingFocus || reconcilingFocus) return;
        const sh = getStageHost();
        const disp = getDisplay();
        const receiver = resolveReceiver();
        if (!sh || !receiver || (disp && !disp.alive(receiver))) return;
        if (sh.stage.focus !== receiver) {
          const prev = sh.stage.focus;
          sh.stage.focus = receiver;
          receiver.onSetFocus?.(prev);
          getAs2TextHost()?.textEvent?.(ownerKey, 'onSetFocus', [prev]);
          notifyAfterInput();
        }
      });

      const onBlur = ev => guardedCall(() => {
        if (disposed || rec.syncingFocus || reconcilingFocus) return;
        const sh = getStageHost();
        const receiver = resolveReceiver();
        if (!sh || !receiver) return;
        const related = ev?.relatedTarget;
        if (related && ownedFocusControls.has(related)) return;
        if (sh.stage.focus === receiver) {
          sh.stage.focus = null;
          receiver.onKillFocus?.(null);
          getAs2TextHost()?.textEvent?.(ownerKey, 'onKillFocus', [null]);
          notifyAfterInput();
        }
      });

      const onKeyDown = ev => guardedCall(() => {
        if (!multiline && ev.key === 'Enter') {
          ev.preventDefault();
        }
      });

      const onInput = () => guardedCall(() => {
        if (disposed || rec.syncingValue) return;
        const textHost = getTextHost();
        const disp = getDisplay();
        const receiver = resolveReceiver();
        if (!textHost || !player?.object_text_input || !receiver || (disp && !disp.alive(receiver))) return;
        const base = textHost.info(ownerKey);
        const before = displayedFieldText(base);
        const rawValue = String(el.value ?? '');
        const normalized = rawValue.replace(/\r\n?|\n/g, '\n');
        const normalizedCaret = Number.isInteger(el.selectionEnd)
          ? rawValue.slice(0, el.selectionEnd).replace(/\r\n?|\n/g, '\n').length
          : undefined;
        const edit = computeTextDiff(before, normalized, normalizedCaret);
        const insert = base.multiline ? edit.text : edit.text.replace(/\n/g, '');
        if (!edit.text.length && edit.begin === edit.end) return;

        const evApi = getEvents();
        const hub = getEventHub();
        if (insert.length > 0 && evApi?.TextEvent && hub) {
          const textEvt = new evApi.TextEvent(evApi.TextEvent.TEXT_INPUT || 'textInput', true, true, insert);
          if (!hub.dispatch(receiver, textEvt)) {
            const current = textHost.info(ownerKey);
            rec.syncingValue = true;
            try { el.value = displayedFieldText(current); } finally { rec.syncingValue = false; }
            notifyAfterInput();
            return;
          }
        }
        const post = textHost.info(ownerKey);
        const postEdit = post.revision === base.revision
          ? { begin: edit.begin, end: edit.end, insert }
          : (() => {
              const d = computeTextDiff(displayedFieldText(post), normalized, normalizedCaret);
              return { begin: d.begin, end: d.end, insert: post.multiline ? d.text : d.text.replace(/\n/g, '') };
            })();
        const committed = JSON.parse(player.object_text_input(ownerKey, JSON.stringify({
          expected_revision: post.revision,
          begin: postEdit.begin,
          end: postEdit.end,
          text: postEdit.insert,
        })));
        const nextText = displayedFieldText(committed);
        if (el.value !== nextText) {
          rec.syncingValue = true;
          try {
            el.value = nextText;
            if (doc.activeElement === el && typeof el.setSelectionRange === 'function' && Number.isInteger(committed.caret_index)) {
              el.setSelectionRange(committed.caret_index, committed.caret_index);
            }
          } finally {
            rec.syncingValue = false;
          }
        }
        if (committed.text !== base.text) {
          if (evApi?.Event && hub) {
            hub.dispatch(receiver, new evApi.Event(evApi.Event.CHANGE || 'change', true, false));
          }
          getAs2ObjectHost()?.textChanged?.(receiver);
          receiver.onChanged?.(receiver);
          getAs2TextHost()?.textEvent?.(ownerKey, 'onChanged', [receiver]);
        }
        notifyAfterInput();
      });

      const syncSelection = () => guardedCall(() => {
        if (disposed || rec.syncingValue) return;
        const textHost = getTextHost();
        const disp = getDisplay();
        const receiver = resolveReceiver();
        if (!textHost?.info || !textHost?.update || !receiver || (disp && !disp.alive(receiver))) return;
        const field = textHost.info(ownerKey);
        if (!field) return;
        const begin = el.selectionStart ?? 0;
        const end = el.selectionEnd ?? begin;
        const backward = el.selectionDirection === 'backward';
        if (
          begin !== field.selection_begin ||
          end !== field.selection_end ||
          (backward ? begin : end) !== field.caret
        ) {
          textHost.update(ownerKey, {
            selection: { begin: backward ? end : begin, end: backward ? begin : end },
          });
        }
      });

      const stopClick = ev => ev.stopPropagation();
      el.addEventListener('focus', onFocus);
      el.addEventListener('blur', onBlur);
      el.addEventListener('keydown', onKeyDown);
      el.addEventListener('keyup', syncSelection);
      el.addEventListener('mouseup', syncSelection);
      el.addEventListener('select', syncSelection);
      el.addEventListener('input', onInput);
      el.addEventListener('click', stopClick);

      rec = {
        kind: 'input',
        ownerKey,
        element: el,
        syncingValue: false,
        syncingFocus: false,
        cleanup() {
          ownedInputControls.delete(el);
          ownedFocusControls.delete(el);
          el.removeEventListener('focus', onFocus);
          el.removeEventListener('blur', onBlur);
          el.removeEventListener('keydown', onKeyDown);
          el.removeEventListener('keyup', syncSelection);
          el.removeEventListener('mouseup', syncSelection);
          el.removeEventListener('select', syncSelection);
          el.removeEventListener('input', onInput);
          el.removeEventListener('click', stopClick);
          inputRecordsByRef.delete(ownerKey);
          el.remove();
          clearControlFocus(ownerKey);
        },
      };
      leafRecords.set(leafKey, rec);
      inputRecordsByRef.set(ownerKey, rec);
      return rec;
    }

    function updateInputElement(rec, field, localTransform, alpha, semanticMeta, nodeInput) {
      const el = rec.element;
      if (el.tagName.toUpperCase() === 'INPUT') {
        const wantType = field.display_as_password ? 'password' : 'text';
        if (el.type !== wantType) el.type = wantType;
      }
      const box = field.box || { width: 160, height: Math.max(22, (field.format?.size || 14) * 1.5) };
      const fmt = field.format || {};
      const fontSize = Number(fmt.size) || 14;
      const fontFamily = global.NeoFlashText?.cssFontFamily
        ? global.NeoFlashText.cssFontFamily(fmt.font)
        : (fmt.font ? `${JSON.stringify(fmt.font)}, sans-serif` : 'sans-serif');
      const vp = field.as2_viewport || {};
      const hasBg = Boolean(vp.background ?? field.background);
      const bgRgb = vp.backgroundColor ?? field.background_color ?? 0xffffff;
      const hasBorder = Boolean(vp.border ?? field.border);
      const borderRgb = vp.borderColor ?? field.border_color ?? 0x000000;
      const enabled = nodeInput?.mouse_enabled !== false;

      Object.assign(el.style, {
        transform: cssMatrix(localTransform),
        width: `${Math.max(4, box.width)}px`,
        height: `${Math.max(4, box.height)}px`,
        fontFamily,
        fontSize: `${fontSize}px`,
        fontWeight: fmt.bold ? 'bold' : 'normal',
        fontStyle: fmt.italic ? 'italic' : 'normal',
        textDecoration: fmt.underline ? 'underline' : 'none',
        textAlign: fmt.align || 'left',
        color: rgbIntToCss(fmt.color ?? 0, '#000000'),
        background: hasBg ? rgbIntToCss(bgRgb, '#ffffff') : 'transparent',
        border: hasBorder ? `1px solid ${rgbIntToCss(borderRgb, '#000000')}` : 'none',
        padding: '2px 4px',
        opacity: String(Math.max(0, Math.min(1, alpha * ((field.alpha ?? 255) / 255)))),
        resize: 'none',
        whiteSpace: field.multiline ? (field.word_wrap ? 'pre-wrap' : 'pre') : 'nowrap',
        overflow: field.multiline ? 'auto' : 'hidden',
      });
      el.readOnly = !enabled;
      const nextDisplayed = displayedFieldText(field);
      if (el.value !== nextDisplayed) {
        rec.syncingValue = true;
        try { el.value = nextDisplayed; } finally { rec.syncingValue = false; }
      }
      a11yApi?.applyDomAccessibility?.(el, semanticMeta, {
        autoLabel: semanticMeta?.name || field.variable || '',
        isInteractive: true,
        tabEnabled: nodeInput?.tab_enabled,
        explicitTabIndex: nodeInput?.tab_index,
        disabled: !enabled,
      });
    }

    function renderTextRunsInto(container, field, ownerKey) {
      const fullText = String(field.text ?? '');
      const runs = Array.isArray(field.format_runs) && field.format_runs.length
        ? field.format_runs
        : [{ begin: 0, end: fullText.length, format: field.format || {} }];
      container.replaceChildren();
      let cursor = 0;
      const appendSegment = (segmentText, fmt) => {
        if (!segmentText) return;
        const rawUrl = typeof fmt?.url === 'string' ? fmt.url : '';
        const trimmedUrl = rawUrl.trim();
        const hasControlChars = /[\u0000-\u001f\u007f-\u009f]/.test(trimmedUrl)
          || /[\u0000-\u0008\u000b\u000e-\u001f\u007f-\u009f]/.test(rawUrl);
        const isEventLink = !hasControlChars && /^event:/i.test(trimmedUrl);
        const isAsfunctionLink = !hasControlChars && /^asfunction:/i.test(trimmedUrl);
        const safeHref = (isEventLink || isAsfunctionLink)
          ? '#'
          : sanitizeDomLinkHref(rawUrl);
        const node = safeHref ? doc.createElement('a') : doc.createElement('span');
        node.textContent = segmentText;
        if (fmt?.bold) node.style.fontWeight = 'bold';
        if (fmt?.italic) node.style.fontStyle = 'italic';
        if (fmt?.underline || safeHref) node.style.textDecoration = 'underline';
        if (Number.isInteger(fmt?.color)) node.style.color = rgbIntToCss(fmt.color);
        if (Number(fmt?.size) > 0) node.style.fontSize = `${fmt.size}px`;
        if (fmt?.font) {
          node.style.fontFamily = global.NeoFlashText?.cssFontFamily
            ? global.NeoFlashText.cssFontFamily(fmt.font)
            : `${JSON.stringify(fmt.font)}, sans-serif`;
        }
        if (safeHref) {
          node.style.pointerEvents = 'auto';
          node.style.cursor = 'pointer';
          if (isEventLink) {
            node.setAttribute?.('href', '#');
            node.href = '#';
            node.addEventListener('click', ev => {
              ev.preventDefault?.();
              ev.stopPropagation?.();
              if (disposed || !containsNode(sceneElement, node)) return;
              guardedCall(() => onTextLink?.(ownerKey, trimmedUrl.slice(6)));
            });
          } else if (isAsfunctionLink) {
            node.setAttribute?.('href', '#');
            node.href = '#';
            node.addEventListener('click', ev => {
              ev.preventDefault?.();
              ev.stopPropagation?.();
              if (disposed || !containsNode(sceneElement, node)) return;
              guardedCall(() => onAsfunction?.(ownerKey, trimmedUrl.slice(11)));
            });
          } else {
            const safeTarget = sanitizeDomLinkTarget(fmt?.target) || '_blank';
            node.setAttribute?.('href', safeHref);
            node.href = safeHref;
            node.setAttribute?.('target', safeTarget);
            node.target = safeTarget;
            node.setAttribute?.('rel', 'noopener noreferrer');
            node.rel = 'noopener noreferrer';
            if (typeof onNavigateUrl === 'function') {
              node.addEventListener('click', ev => {
                ev.preventDefault?.();
                ev.stopPropagation?.();
                if (disposed || !containsNode(sceneElement, node)) return;
                guardedCall(() => onNavigateUrl(safeHref, safeTarget));
              });
            }
          }
        }
        container.appendChild(node);
      };
      for (const run of runs) {
        const b = Math.max(cursor, Math.min(fullText.length, Number(run.begin) || 0));
        const e = Math.max(b, Math.min(fullText.length, Number(run.end) || 0));
        if (b > cursor) appendSegment(fullText.slice(cursor, b), field.format || {});
        if (e > b) appendSegment(fullText.slice(b, e), { ...(field.format || {}), ...(run.format || {}) });
        cursor = e;
      }
      if (cursor < fullText.length) {
        appendSegment(fullText.slice(cursor), field.format || {});
      }
    }

    function updateTextElement(rec, item, localTransform, insideInteractiveButton, semanticMeta) {
      const el = rec.element;
      const kind = item.kind;
      if (kind.type === 'text') {
        const rgbaCol = Array.isArray(kind.color) ? kind.color : [0, 0, 0, 255];
        const fontSize = Number(kind.font_size) || 14;
        const fontFamily = global.NeoFlashText?.cssFontFamily
          ? global.NeoFlashText.cssFontFamily(kind.font_family)
          : `${JSON.stringify(kind.font_family || 'sans-serif')}, sans-serif`;
        const sig = JSON.stringify([kind.text, fontSize, fontFamily, rgbaCol, item.alpha, insideInteractiveButton]);
        Object.assign(el.style, {
          transform: cssMatrix(localTransform),
          fontFamily,
          fontSize: `${fontSize}px`,
          fontWeight: kind.bold ? 'bold' : 'normal',
          fontStyle: kind.italic ? 'italic' : 'normal',
          lineHeight: '1.2',
          whiteSpace: 'pre',
          color: `rgba(${rgbaCol[0] | 0}, ${rgbaCol[1] | 0}, ${rgbaCol[2] | 0}, ${(rgbaCol[3] ?? 255) / 255})`,
          background: 'transparent',
          border: 'none',
          opacity: String(Math.max(0, Math.min(1, item.alpha))),
          userSelect: insideInteractiveButton ? 'none' : 'text',
          webkitUserSelect: insideInteractiveButton ? 'none' : 'text',
          pointerEvents: insideInteractiveButton ? 'none' : 'auto',
          cursor: insideInteractiveButton ? 'inherit' : 'text',
        });
        if (rec.signature !== sig) {
          rec.signature = sig;
          el.textContent = String(kind.text ?? '');
        }
        a11yApi?.applyDomAccessibility?.(el, semanticMeta, { autoLabel: kind.text || '', isInteractive: false });
        return;
      }

      const field = kind.field;
      const fmt = field.format || {};
      const fontSize = Number(fmt.size) || 14;
      const fontFamily = global.NeoFlashText?.cssFontFamily
        ? global.NeoFlashText.cssFontFamily(fmt.font)
        : `${JSON.stringify(fmt.font || 'sans-serif')}, sans-serif`;
      const vp = field.as2_viewport || {};
      const hasBg = Boolean(vp.background ?? field.background);
      const bgRgb = vp.backgroundColor ?? field.background_color ?? 0xffffff;
      const hasBorder = Boolean(vp.border ?? field.border);
      const borderRgb = vp.borderColor ?? field.border_color ?? 0x000000;
      const selectable = !insideInteractiveButton && field.selectable !== false;
      const leading = Number(fmt.leading) || 0;
      const box = field.box;
      Object.assign(el.style, {
        transform: cssMatrix(localTransform),
        width: box ? `${Math.max(1, box.width)}px` : 'auto',
        height: box ? `${Math.max(1, box.height)}px` : 'auto',
        overflow: box ? 'hidden' : 'visible',
        fontFamily,
        fontSize: `${fontSize}px`,
        fontWeight: fmt.bold ? 'bold' : 'normal',
        fontStyle: fmt.italic ? 'italic' : 'normal',
        textDecoration: fmt.underline ? 'underline' : 'none',
        textAlign: fmt.align || 'left',
        lineHeight: `${Math.max(1, fontSize * 1.2 + leading)}px`,
        paddingLeft: fmt.leftMargin ? `${fmt.leftMargin}px` : '2px',
        paddingRight: fmt.rightMargin ? `${fmt.rightMargin}px` : '2px',
        paddingTop: '2px',
        textIndent: fmt.indent ? `${fmt.indent}px` : '0px',
        letterSpacing: fmt.letterSpacing ? `${fmt.letterSpacing}px` : 'normal',
        whiteSpace: field.word_wrap ? 'pre-wrap' : 'pre',
        color: rgbIntToCss(fmt.color ?? 0, '#000000'),
        background: hasBg ? rgbIntToCss(bgRgb, '#ffffff') : 'transparent',
        border: hasBorder ? `1px solid ${rgbIntToCss(borderRgb, '#000000')}` : 'none',
        opacity: String(Math.max(0, Math.min(1, item.alpha * ((field.alpha ?? 255) / 255)))),
        userSelect: selectable ? 'text' : 'none',
        webkitUserSelect: selectable ? 'text' : 'none',
        pointerEvents: selectable ? 'auto' : 'none',
        cursor: selectable ? 'text' : 'inherit',
      });
      const sig = JSON.stringify([field.revision, field.text, fmt, field.format_runs]);
      if (rec.signature !== sig) {
        rec.signature = sig;
        renderTextRunsInto(el, field, rec.ownerKey);
      }
      a11yApi?.applyDomAccessibility?.(el, semanticMeta, { autoLabel: field.text || '', isInteractive: false });
    }

    function updateSvgElement(rec, item, localTransform, clipsById) {
      const svg = rec.element;
      const kind = item.kind;
      svg.style.overflow = 'visible';
      svg.style.transform = cssMatrix(localTransform);
      svg.style.opacity = String(Math.max(0, Math.min(1, item.alpha)));

      const itemInv = item.clip_ids?.length ? matInverse(item.transform) : null;
      const clipSig = item.clip_ids?.length
        ? item.clip_ids.map(cid => clipsById.get(cid))
        : null;
      const sig = JSON.stringify([kind, clipSig, item.clip_ids?.length ? item.transform : null]);
      if (rec.signature === sig) return;
      rec.signature = sig;

      const defs = [];
      let fillAttr = 'none';
      let fillOpacityAttr = '';
      if (kind.fill_gradient) {
        const gid = `nf-grad-${rec.uid}-f`;
        defs.push(svgGradientDef(gid, kind.fill_gradient));
        fillAttr = `url(#${gid})`;
      } else if (Array.isArray(kind.fill) && (kind.fill[3] ?? 0) > 0) {
        fillAttr = `rgb(${kind.fill[0] | 0},${kind.fill[1] | 0},${kind.fill[2] | 0})`;
        if (kind.fill[3] < 255) fillOpacityAttr = ` fill-opacity="${kind.fill[3] / 255}"`;
      }

      let strokeAttrs = ' stroke="none"';
      const sw = Number(kind.stroke_width) || 0;
      if (sw > 0) {
        if (kind.stroke_gradient) {
          const gid = `nf-grad-${rec.uid}-s`;
          defs.push(svgGradientDef(gid, kind.stroke_gradient));
          strokeAttrs = ` stroke="url(#${gid})" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round"`;
        } else if (Array.isArray(kind.stroke) && (kind.stroke[3] ?? 0) > 0) {
          const sa = kind.stroke[3] < 255 ? ` stroke-opacity="${kind.stroke[3] / 255}"` : '';
          strokeAttrs = ` stroke="rgb(${kind.stroke[0] | 0},${kind.stroke[1] | 0},${kind.stroke[2] | 0})"${sa} stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round"`;
        }
      }

      let clipWrapStart = '';
      let clipWrapEnd = '';
      if (item.clip_ids?.length && itemInv) {
        for (let ci = 0; ci < item.clip_ids.length; ci++) {
          const clip = clipsById.get(item.clip_ids[ci]);
          if (!clip) continue;
          const cid = `nf-clip-${rec.uid}-${ci}`;
          const compPaths = (clip.components || []).map(comp => {
            if (typeof comp.alpha === 'number' && comp.alpha <= 0) return '';
            const relM = matMultiply(itemInv, comp.matrix || [1, 0, 0, 1, 0, 0]);
            const d = svgPathData(comp.commands || []);
            const rule = comp.fill_rule === 'evenodd' ? 'evenodd' : 'nonzero';
            return `<path d="${xmlAttrEscape(d)}" clip-rule="${rule}" transform="matrix(${relM[0]} ${relM[1]} ${relM[2]} ${relM[3]} ${relM[4]} ${relM[5]})"/>`;
          }).join('');
          defs.push(`<clipPath id="${cid}" clipPathUnits="userSpaceOnUse">${compPaths}</clipPath>`);
          clipWrapStart += `<g clip-path="url(#${cid})">`;
          clipWrapEnd = `</g>${clipWrapEnd}`;
        }
      }

      let body = '';
      if (kind.type === 'rectangle') {
        body = `<rect x="0" y="0" width="${Math.max(0, kind.width)}" height="${Math.max(0, kind.height)}" fill="${fillAttr}"${fillOpacityAttr}${strokeAttrs}/>`;
      } else if (kind.type === 'ellipse') {
        const rx = Math.max(0, kind.width / 2);
        const ry = Math.max(0, kind.height / 2);
        body = `<ellipse cx="${rx}" cy="${ry}" rx="${rx}" ry="${ry}" fill="${fillAttr}"${fillOpacityAttr}${strokeAttrs}/>`;
      } else {
        const d = svgPathData(kind.commands || []);
        const rule = kind.fill_rule === 'evenodd' ? ' fill-rule="evenodd"' : '';
        body = `<path d="${xmlAttrEscape(d)}" fill="${fillAttr}"${fillOpacityAttr}${rule}${strokeAttrs}/>`;
      }
      const defsMarkup = defs.length ? `<defs>${defs.join('')}</defs>` : '';
      svg.setAttribute('width', '1');
      svg.setAttribute('height', '1');
      svg.setAttribute('viewBox', '0 0 1 1');
      svg.innerHTML = `${defsMarkup}${clipWrapStart}${body}${clipWrapEnd}`;
    }

    function computeOwnerLocalBounds(ownerNode, ownerDraws, semBoundsByKey, ownerKey, drawsByOwnerAndAncestors, objectsByKey) {
      const invOwner = matInverse(ownerNode.world || [1, 0, 0, 1, 0, 0]);
      let box = null;
      const include = b => {
        if (!b || !Number.isFinite(b.left) || !Number.isFinite(b.top) || !Number.isFinite(b.right) || !Number.isFinite(b.bottom)) return;
        if (b.right <= b.left || b.bottom <= b.top) return;
        box = box
          ? {
              left: Math.min(box.left, b.left),
              top: Math.min(box.top, b.top),
              right: Math.max(box.right, b.right),
              bottom: Math.max(box.bottom, b.bottom),
            }
          : { left: b.left, top: b.top, right: b.right, bottom: b.bottom };
      };
      const textHost = getTextHost();
      if (ownerNode.owner === 'button' && (ownerNode.button_hit_authoritative || ownerNode.button_hits?.length)) {
        if (!ownerNode.button_hits?.length) return null;
        for (const item of ownerNode.button_hits) {
          const enrichedKind = item.kind?.type === 'text_field' && textHost
            ? { ...item.kind, field: { ...item.kind.field, alpha: 255 }, text_bounds: textHost.bounds({ ...item.kind.field, alpha: 255 }, true) }
            : item.kind;
          const b = a11yApi?.itemBounds?.(
            { ...item, alpha: 1, transform: item.transform || [1, 0, 0, 1, 0, 0], kind: enrichedKind },
            (line, tk) => line.length * (Number(tk.font_size) || 14) * 0.6,
          );
          include(b);
        }
        return box;
      }
      if (ownerNode.input?.hit_area && invOwner) {
        const hitAreaKey = JSON.stringify(ownerNode.input.hit_area);
        const hitDraws = drawsByOwnerAndAncestors?.get(hitAreaKey);
        if (hitDraws?.length) {
          for (const item of hitDraws) {
            const relM = matMultiply(invOwner, item.transform || [1, 0, 0, 1, 0, 0]);
            const enrichedKind = item.kind?.type === 'text_field' && textHost
              ? { ...item.kind, field: { ...item.kind.field, alpha: 255 }, text_bounds: textHost.bounds({ ...item.kind.field, alpha: 255 }, true) }
              : item.kind;
            const b = a11yApi?.itemBounds?.(
              { ...item, alpha: 1, transform: relM, kind: enrichedKind },
              (line, tk) => line.length * (Number(tk.font_size) || 14) * 0.6,
            );
            include(b);
          }
          if (box) return box;
        }
        const disp = getDisplay();
        try {
          const parsed = JSON.parse(ownerKey);
          const ownerProxy = Array.isArray(parsed) ? proxyFor(parsed) : proxyForRef(parsed);
          const hitProxy = proxyForRef(ownerNode.input.hit_area);
          if (ownerProxy && hitProxy && disp?.alive?.(ownerProxy) && disp?.alive?.(hitProxy) && typeof hitProxy.getBounds === 'function') {
            const rect = hitProxy.getBounds(ownerProxy);
            if (rect && rect.width > 0 && rect.height > 0) {
              include({ left: rect.x, top: rect.y, right: rect.x + rect.width, bottom: rect.y + rect.height });
              return box;
            }
          }
        } catch (_) {}
        const hitNode = objectsByKey?.get(hitAreaKey);
        if (hitNode && !hitDraws?.length) return null;
      }
      if (invOwner && ownerDraws?.length) {
        for (const item of ownerDraws) {
          const relM = matMultiply(invOwner, item.transform || [1, 0, 0, 1, 0, 0]);
          const enrichedKind = item.kind?.type === 'text_field' && textHost
            ? { ...item.kind, text_bounds: textHost.bounds(item.kind.field, true) }
            : item.kind;
          const b = a11yApi?.itemBounds?.(
            { ...item, alpha: 1, transform: relM, kind: enrichedKind },
            (line, tk) => line.length * (Number(tk.font_size) || 14) * 0.6,
          );
          include(b);
        }
      }
      if (!box) {
        const disp = getDisplay();
        try {
          const parsed = JSON.parse(ownerKey);
          const proxy = Array.isArray(parsed) ? proxyFor(parsed) : proxyForRef(parsed);
          if (proxy && disp?.alive?.(proxy) && typeof proxy.getBounds === 'function') {
            const rect = proxy.getBounds(proxy);
            if (rect && rect.width > 0 && rect.height > 0) {
              include({ left: rect.x, top: rect.y, right: rect.x + rect.width, bottom: rect.y + rect.height });
            }
          }
        } catch (_) {}
      }
      if (!box && invOwner && semBoundsByKey?.has(ownerKey)) {
        const wb = semBoundsByKey.get(ownerKey);
        const pts = [[wb.left, wb.top], [wb.right, wb.top], [wb.right, wb.bottom], [wb.left, wb.bottom]].map(p => matPoint(invOwner, p[0], p[1]));
        const xs = pts.map(p => p[0]);
        const ys = pts.map(p => p[1]);
        include({ left: Math.min(...xs), top: Math.min(...ys), right: Math.max(...xs), bottom: Math.max(...ys) });
      }
      return box;
    }

    function isInteractiveButtonNode(node, nodeKey, semByRef) {
      if (!node || !node.parent_ref) return false;
      if (node.visible === false || node.effective_visible === false) return false;
      if (node.input?.mouse_enabled === false) return false;
      const sem = semByRef.get(nodeKey);
      if (sem?.accessibility?.role === 'button') return true;
      if (node.owner === 'button') return true;
      if (node.owner !== 'movie_clip' && node.owner !== 'sprite') return false;
      if (node.input?.button_mode || node.input?.hit_area) return true;
      if (node.name && authoredClickPlacements.has(node.name)) return true;
      if (Array.isArray(node.source_path) && node.source_path.some(seg => authoredClickPlacements.has(String(seg)))) return true;
      const disp = getDisplay();
      try {
        const proxy = node.object_ref ? proxyForRef(node.object_ref) : proxyFor(node.path || []);
        if (!proxy || (disp && !disp.alive(proxy))) return false;
        if (disp?.handCursor?.(proxy)) return true;
        if (typeof proxy.hasEventListener === 'function') {
          for (const t of ['click', 'mouseDown', 'mouseUp', 'rollOver', 'rollOut']) {
            if (proxy.hasEventListener(t)) return true;
          }
        }
      } catch (_) {}
      return false;
    }

    function ensureContainerRecord(key, node, relMatrix) {
      let rec = containerRecords.get(key);
      if (!rec) {
        const el = doc.createElement('div');
        el.className = 'nf-dom-container';
        el.setAttribute('data-nf-id', key);
        rec = {
          key,
          element: el,
          clipWrapper: null,
          buttonRecord: null,
          cleanup() {
            rec.buttonRecord?.cleanup?.();
            rec.buttonRecord = null;
            el.remove();
          },
        };
        containerRecords.set(key, rec);
      }
      const el = rec.element;
      if (node.owner && el.getAttribute('data-nf-owner') !== node.owner) {
        el.setAttribute('data-nf-owner', node.owner);
      }
      if (node.name) {
        if (el.getAttribute('data-nf-name') !== node.name) el.setAttribute('data-nf-name', node.name);
      } else if (el.hasAttribute('data-nf-name')) {
        el.removeAttribute('data-nf-name');
      }
      const localM = relMatrix || node.content_local_matrix || node.local_matrix || [1, 0, 0, 1, 0, 0];
      el.style.transform = cssMatrix(localM);
      el.style.display = node.visible === false || node.effective_visible === false ? 'none' : 'block';
      const cssBlend = node.blend_mode ? CSS_BLEND_MODES[String(node.blend_mode).toLowerCase()] : null;
      el.style.mixBlendMode = cssBlend && cssBlend !== 'normal' ? cssBlend : '';

      if (node.scroll_rect && node.scroll_rect.width > 0 && node.scroll_rect.height > 0) {
        if (!rec.clipWrapper) {
          const clip = doc.createElement('div');
          clip.className = 'nf-dom-scroll-rect';
          Object.assign(clip.style, {
            position: 'absolute',
            left: '0px',
            top: '0px',
            overflow: 'hidden',
            pointerEvents: 'none',
          });
          const inner = doc.createElement('div');
          inner.className = 'nf-dom-scroll-inner';
          Object.assign(inner.style, { position: 'absolute', left: '0px', top: '0px', transformOrigin: '0 0' });
          clip.appendChild(inner);
          rec.clipWrapper = clip;
          rec.clipInner = inner;
        }
        const sr = node.scroll_rect;
        rec.clipWrapper.style.width = `${sr.width}px`;
        rec.clipWrapper.style.height = `${sr.height}px`;
        rec.clipInner.style.transform = `translate(${-sr.x}px, ${-sr.y}px)`;
      } else if (rec.clipWrapper) {
        rec.clipWrapper.remove();
        rec.clipWrapper = null;
        rec.clipInner = null;
      }
      return rec;
    }

    function render(frameState) {
      if (disposed) return;
      // Native blur can fire inside remove()/insertBefore(). Its authored events
      // still run immediately, but their redraw must wait for the current tree.
      if (rendering) {
        queuedFrameState = frameState;
        return;
      }
      let nextFrameState = frameState;
      do {
        queuedFrameState = null;
        rendering = true;
        try { renderFrame(nextFrameState); } finally { rendering = false; }
        nextFrameState = queuedFrameState;
      } while (nextFrameState && !disposed);
      if (inputUpdatePending && !disposed) {
        inputUpdatePending = false;
        afterInput?.();
      }
    }

    function renderFrame(frameState) {
      restoreSuppressedAccessibility();
      const {
        snapshot,
        semantics = [],
        width = project?.width || 800,
        height = project?.height || 600,
        originX = 0,
        originY = 0,
        scale = { x: 1, y: 1 },
        stageColor = null,
        background = project?.background || [255, 255, 255, 255],
      } = frameState || {};

      const scaleX = Number(scale?.x) || 1;
      const scaleY = Number(scale?.y) || scaleX;
      const dpr = Math.min(4, Math.max(1, Number(global.devicePixelRatio) || 1));
      stageElement.dataset.logicalWidth = String(width);
      stageElement.dataset.logicalHeight = String(height);
      stageElement.dataset.originX = String(originX);
      stageElement.dataset.originY = String(originY);
      stageElement.dataset.dpr = String(dpr);
      if (stageElement.style?.setProperty) {
        stageElement.style.setProperty('--neoflash-scale-x', String(scaleX));
        stageElement.style.setProperty('--neoflash-scale-y', String(scaleY));
        stageElement.style.setProperty('--neoflash-dpr', String(dpr));
      }
      stageElement.style.backgroundColor = Number.isInteger(stageColor)
        ? rgbIntToCss(stageColor)
        : `rgba(${background[0] | 0}, ${background[1] | 0}, ${background[2] | 0}, ${(background[3] ?? 255) / 255})`;

      Object.assign(sceneElement.style, {
        width: `${width}px`,
        height: `${height}px`,
        transform: `matrix(${scaleX}, 0, 0, ${scaleY}, ${originX * scaleX}, ${originY * scaleY})`,
      });

      const draws = snapshot?.draws || [];
      const objectsList = snapshot?.objects || [];
      const objectsByKey = new Map();
      for (const n of objectsList) {
        const k = n.object_ref ? JSON.stringify(n.object_ref) : JSON.stringify(n.path || []);
        objectsByKey.set(k, n);
      }

      const semByRef = new Map();
      for (const sem of semantics) {
        const k = sem?.owner_ref ? JSON.stringify(sem.owner_ref) : JSON.stringify(sem?.instance_path || []);
        if (k) semByRef.set(k, sem);
      }

      const filterGroupsById = new Map((snapshot?.filter_groups || []).map(fg => [fg.id, fg]));
      const clipsById = new Map((snapshot?.clips || []).map(c => [c.id, c]));
      const alphaMaskClipIds = new Set();
      for (const fg of filterGroupsById.values()) {
        for (const am of fg.alpha_masks || []) {
          if (typeof am?.clip_id === 'string') alphaMaskClipIds.add(am.clip_id);
        }
      }

      const fgAncestry = id => {
        const chain = [];
        const seen = new Set();
        for (let cur = id ? filterGroupsById.get(id) : null; cur && !seen.has(cur.id); cur = cur.parent_id ? filterGroupsById.get(cur.parent_id) : null) {
          seen.add(cur.id);
          chain.unshift(cur);
        }
        return chain;
      };

      const rootGroupReasons = new Map();
      for (const fg of filterGroupsById.values()) {
        const reason = directFilterGroupReason(fg);
        if (!reason) continue;
        const root = fgAncestry(fg.id)[0] || fg;
        if (!rootGroupReasons.has(root.id)) rootGroupReasons.set(root.id, reason);
      }

      const seenContainers = new Set();
      const seenLeaves = new Set();
      const seenIslands = new Set();
      const desiredChildrenByContainer = new Map();
      const sceneDesired = [];
      const suppressionOwners = new Map();

      const interactiveOwnerKeys = new Set();
      for (const [k, n] of objectsByKey) {
        if (isInteractiveButtonNode(n, k, semByRef)) interactiveOwnerKeys.add(k);
      }

      const drawsByOwnerAndAncestors = new Map();
      for (const item of draws) {
        const ownKey = item.owner_ref ? JSON.stringify(item.owner_ref) : JSON.stringify(item.instance_path || []);
        for (let cur = objectsByKey.get(ownKey); cur; cur = cur.parent_ref ? objectsByKey.get(JSON.stringify(cur.parent_ref)) : null) {
          const ck = cur.object_ref ? JSON.stringify(cur.object_ref) : JSON.stringify(cur.path || []);
          let arr = drawsByOwnerAndAncestors.get(ck);
          if (!arr) drawsByOwnerAndAncestors.set(ck, arr = []);
          arr.push(item);
        }
      }

      const containerChainFor = ownerKey => {
        const chain = [];
        const visited = new Set();
        for (let cur = objectsByKey.get(ownerKey); cur; cur = cur.parent_ref ? objectsByKey.get(JSON.stringify(cur.parent_ref)) : null) {
          const ck = cur.object_ref ? JSON.stringify(cur.object_ref) : JSON.stringify(cur.path || []);
          if (visited.has(ck)) break;
          visited.add(ck);
          if (['movie_clip', 'sprite', 'button', 'loader'].includes(cur.owner)) {
            chain.unshift({ key: ck, node: cur });
          }
        }
        return chain;
      };

      const mountContainerChain = chain => {
        let parentChildren = sceneDesired;
        let currentWorld = [1, 0, 0, 1, 0, 0];
        let insideButton = false;
        for (const { key, node } of chain) {
          const nodeWorld = node.world || [1, 0, 0, 1, 0, 0];
          const invWorld = matInverse(nodeWorld);
          const invParentWorld = matInverse(currentWorld);
          if (!invWorld || !invParentWorld) {
            return { targetList: null, worldMatrix: currentWorld, insideButton };
          }
          const hasScrollRect = Boolean(node.scroll_rect && node.scroll_rect.width > 0 && node.scroll_rect.height > 0);
          const outerWorld = hasScrollRect
            ? matMultiply(nodeWorld, [1, 0, 0, 1, node.scroll_rect.x, node.scroll_rect.y])
            : nodeWorld;
          const relMatrix = matMultiply(invParentWorld, outerWorld);
          const rec = ensureContainerRecord(key, node, relMatrix);
          seenContainers.add(key);
          if (!parentChildren.includes(rec.element)) {
            parentChildren.push(rec.element);
          }
          let childList = desiredChildrenByContainer.get(key);
          if (!childList) desiredChildrenByContainer.set(key, childList = []);
          parentChildren = childList;
          currentWorld = nodeWorld;
          if (interactiveOwnerKeys.has(key)) insideButton = true;
        }
        return { targetList: parentChildren, worldMatrix: currentWorld, insideButton };
      };

      // Group contiguous island draws so whole filter_groups rasterize together accurately.
      const renderQueue = [];
      for (let i = 0; i < draws.length; i++) {
        const item = draws[i];
        const reason = islandReasonForDraw(item, filterGroupsById, clipsById, alphaMaskClipIds, rootGroupReasons, fgAncestry);
        if (!reason) {
          renderQueue.push({ type: 'dom', item, index: i });
          continue;
        }
        const rootFg = item.filter_id ? fgAncestry(item.filter_id)[0] : null;
        if (rootFg) {
          const groupDraws = [item];
          while (i + 1 < draws.length) {
            const nextFg = draws[i + 1].filter_id ? fgAncestry(draws[i + 1].filter_id)[0] : null;
            if (nextFg && nextFg.id === rootFg.id) {
              groupDraws.push(draws[++i]);
            } else {
              break;
            }
          }
          renderQueue.push({
            type: 'island',
            islandKey: `fg:${rootFg.id}`,
            reason,
            draws: groupDraws,
            rootFg,
          });
        } else {
          const ownKey = item.owner_ref ? JSON.stringify(item.owner_ref) : `idx:${i}`;
          renderQueue.push({
            type: 'island',
            islandKey: `draw:${ownKey}:${item.element_id || i}`,
            reason,
            draws: [item],
            rootFg: null,
          });
        }
      }

      for (const entry of renderQueue) {
        if (entry.type === 'island') {
          if (typeof paintCanvasIsland !== 'function') continue;
          const islandKey = entry.islandKey;
          let rec = islandRecords.get(islandKey);
          if (!rec) {
            const c = doc.createElement('canvas');
            c.className = 'nf-dom-canvas-island';
            c.setAttribute('data-nf-id', `island:${islandKey}`);
            c.setAttribute('data-nf-kind', 'canvas-island');
            rec = { element: c, cleanup() { c.remove(); } };
            islandRecords.set(islandKey, rec);
          }
          rec.element.setAttribute('data-nf-island-reason', entry.reason);
          const rootOp = entry.rootFg?.composite_operation || 'source-over';
          const islandCssBlend = rootOp === 'lighter'
            ? 'plus-lighter'
            : (Object.hasOwn(COMPOSITE_TO_CSS_BLEND, rootOp) ? COMPOSITE_TO_CSS_BLEND[rootOp] : null);
          const painted = paintCanvasIsland(rec.element, entry.draws, snapshot, {
            width,
            height,
            originX,
            originY,
            scale: { x: scaleX, y: scaleY },
            rootCssBlend: Boolean(islandCssBlend && islandCssBlend !== 'normal'),
          });
          if (painted && painted.width > 0 && painted.height > 0) {
            seenIslands.add(islandKey);
            Object.assign(rec.element.style, {
              transform: `translate(${painted.left}px, ${painted.top}px)`,
              width: `${painted.width}px`,
              height: `${painted.height}px`,
              mixBlendMode: islandCssBlend && islandCssBlend !== 'normal' ? islandCssBlend : '',
            });
            sceneDesired.push(rec.element);
          }
          continue;
        }

        const { item, index } = entry;
        if (!(item.alpha > 0)) continue;
        const ownerKey = item.owner_ref ? JSON.stringify(item.owner_ref) : JSON.stringify(item.instance_path || []);
        const ownerNode = objectsByKey.get(ownerKey) || null;
        const chain = containerChainFor(ownerKey);
        const mounted = mountContainerChain(chain);
        if (!mounted.targetList) continue;

        const invContainer = matInverse(mounted.worldMatrix) || [1, 0, 0, 1, 0, 0];
        const localM = matMultiply(invContainer, item.transform || [1, 0, 0, 1, 0, 0]);
        const kind = item.kind;
        const semMeta = semByRef.get(ownerKey)?.accessibility || null;
        const isContainerOwner = ownerNode && ['movie_clip', 'sprite', 'button', 'loader'].includes(ownerNode.owner);
        const leafBlend = !isContainerOwner && ownerNode?.blend_mode
          ? CSS_BLEND_MODES[String(ownerNode.blend_mode).toLowerCase()]
          : null;
        const leafMixBlend = leafBlend && leafBlend !== 'normal' ? leafBlend : '';

        if (kind.type === 'text_field' && kind.field?.field_type === 'input') {
          const leafKey = `input:${ownerKey}`;
          const rec = ensureInputRecord(leafKey, ownerKey, Boolean(kind.field.multiline));
          seenLeaves.add(leafKey);
          updateInputElement(rec, kind.field, localM, item.alpha, semMeta, ownerNode?.input);
          mounted.targetList.push(rec.element);
          continue;
        }

        if (kind.type === 'text' || kind.type === 'text_field') {
          const leafKey = `text:${ownerKey}:${item.element_id || index}`;
          let rec = leafRecords.get(leafKey);
          if (rec && rec.kind !== 'text') {
            rec.cleanup?.();
            leafRecords.delete(leafKey);
            rec = null;
          }
          if (!rec) {
            const el = doc.createElement('div');
            el.className = 'nf-dom-text';
            el.setAttribute('data-nf-id', leafKey);
            el.setAttribute('data-nf-kind', 'text');
            rec = { kind: 'text', ownerKey, element: el, signature: null, cleanup() { el.remove(); } };
            leafRecords.set(leafKey, rec);
          }
          seenLeaves.add(leafKey);
          updateTextElement(rec, item, localM, mounted.insideButton, semMeta);
          rec.element.style.mixBlendMode = leafMixBlend;
          mounted.targetList.push(rec.element);
          continue;
        }

        if (['rectangle', 'ellipse', 'path', 'gradient_path'].includes(kind.type)) {
          const leafKey = `svg:${ownerKey}:${item.element_id || index}`;
          let rec = leafRecords.get(leafKey);
          if (rec && rec.kind !== 'svg') {
            rec.cleanup?.();
            leafRecords.delete(leafKey);
            rec = null;
          }
          if (!rec) {
            const svg = doc.createElementNS
              ? doc.createElementNS(SVG_NS, 'svg')
              : doc.createElement('svg');
            svg.setAttribute('class', 'nf-dom-svg');
            svg.setAttribute('data-nf-id', leafKey);
            svg.setAttribute('data-nf-kind', 'svg');
            rec = { kind: 'svg', uid: idSeq++, element: svg, signature: null, cleanup() { svg.remove(); } };
            leafRecords.set(leafKey, rec);
          }
          seenLeaves.add(leafKey);
          updateSvgElement(rec, item, localM, clipsById);
          rec.element.style.mixBlendMode = leafMixBlend;
          mounted.targetList.push(rec.element);
          continue;
        }

        if (kind.type === 'bitmap_pixels') {
          const url = bitmapDataUrlFor(kind);
          if (!url) continue;
          const leafKey = `bmp:${ownerKey}:${item.element_id || index}`;
          let rec = leafRecords.get(leafKey);
          if (rec && rec.kind !== 'bitmap') {
            rec.cleanup?.();
            leafRecords.delete(leafKey);
            rec = null;
          }
          if (!rec) {
            const img = doc.createElement('img');
            img.className = 'nf-dom-bitmap';
            img.setAttribute('data-nf-id', leafKey);
            img.setAttribute('data-nf-kind', 'bitmap');
            img.decoding = 'sync';
            img.draggable = false;
            rec = { kind: 'bitmap', element: img, lastUrl: null, cleanup() { img.remove(); } };
            leafRecords.set(leafKey, rec);
          }
          seenLeaves.add(leafKey);
          const img = rec.element;
          img.alt = semMeta?.name || '';
          if (rec.lastUrl !== url) {
            rec.lastUrl = url;
            img.src = url;
          }
          Object.assign(img.style, {
            transform: cssMatrix(localM),
            width: `${kind.width}px`,
            height: `${kind.height}px`,
            opacity: String(Math.max(0, Math.min(1, item.alpha))),
            imageRendering: kind.smoothing === false ? 'pixelated' : 'auto',
            mixBlendMode: leafMixBlend,
          });
          a11yApi?.applyDomAccessibility?.(img, semMeta, { autoLabel: '', isInteractive: false });
          mounted.targetList.push(img);
          continue;
        }

        if (kind.type === 'video_frame') {
          const videoHost = getVideoHost();
          if (!videoHost) continue;
          const leafKey = `vid:${ownerKey}:${item.element_id || index}`;
          let rec = leafRecords.get(leafKey);
          if (!rec || rec.kind !== 'video') {
            rec?.cleanup?.();
            const vc = doc.createElement('canvas');
            vc.className = 'nf-dom-video';
            vc.setAttribute('data-nf-id', leafKey);
            vc.setAttribute('data-nf-kind', 'video');
            rec = { kind: 'video', element: vc, cleanup() { vc.remove(); } };
            leafRecords.set(leafKey, rec);
          }
          seenLeaves.add(leafKey);
          const vc = rec.element;
          const vw = Math.max(1, Math.round(kind.width || 1));
          const vh = Math.max(1, Math.round(kind.height || 1));
          if (vc.width !== vw || vc.height !== vh) {
            vc.width = vw;
            vc.height = vh;
          }
          const vctx = vc.getContext?.('2d');
          if (vctx) {
            vctx.clearRect(0, 0, vw, vh);
            videoHost.draw(vctx, kind, item.owner_ref);
          }
          Object.assign(vc.style, {
            transform: cssMatrix(localM),
            width: `${vw}px`,
            height: `${vh}px`,
            opacity: String(Math.max(0, Math.min(1, item.alpha))),
            mixBlendMode: leafMixBlend,
          });
          mounted.targetList.push(vc);
        }
      }

      // Ensure hit-only / empty-frame interactive buttons still mount in the DOM tree.
      for (const ownerKey of interactiveOwnerKeys) {
        if (!seenContainers.has(ownerKey)) {
          const chain = containerChainFor(ownerKey);
          if (chain.length) mountContainerChain(chain);
        }
      }

      // Attach/update interactive <button> overlays inside their owning containers.
      for (const [key, rec] of containerRecords) {
        if (!seenContainers.has(key)) continue;
        const node = objectsByKey.get(key);
        const childList = desiredChildrenByContainer.get(key) || [];
        if (interactiveOwnerKeys.has(key) && node) {
          const localBounds = computeOwnerLocalBounds(node, drawsByOwnerAndAncestors.get(key), null, key, drawsByOwnerAndAncestors, objectsByKey);
          if (localBounds && localBounds.right > localBounds.left && localBounds.bottom > localBounds.top) {
            const btnRec = ensureButtonRecord(rec, key);
            const btn = btnRec.element;
            Object.assign(btn.style, {
              left: `${localBounds.left}px`,
              top: `${localBounds.top}px`,
              width: `${localBounds.right - localBounds.left}px`,
              height: `${localBounds.bottom - localBounds.top}px`,
            });
            const autoLabel = (drawsByOwnerAndAncestors.get(key) || [])
              .map(d => d.kind?.type === 'text' ? d.kind.text : (d.kind?.type === 'text_field' ? d.kind.field?.text : ''))
              .filter(Boolean)
              .join(' ')
              .trim() || node.name || '';
            const semMeta = semByRef.get(key)?.accessibility || null;
            const disabled = Boolean(
              (node.owner === 'button' && node.input?.button_enabled === false)
              || (node.owner === 'movie_clip' && node.input?.movieclip_enabled === false)
              || semMeta?.disabled,
            );
            a11yApi?.applyDomAccessibility?.(btn, semMeta, {
              autoLabel,
              isInteractive: true,
              tabEnabled: node.input?.tab_enabled,
              explicitTabIndex: node.input?.tab_index,
              disabled,
            });
            childList.push(btn);
          } else if (rec.buttonRecord) {
            rec.buttonRecord.cleanup();
            rec.buttonRecord = null;
          }
        } else if (rec.buttonRecord) {
          rec.buttonRecord.cleanup();
          rec.buttonRecord = null;
        }
        const semMeta = !interactiveOwnerKeys.has(key) ? (semByRef.get(key)?.accessibility || null) : null;
        a11yApi?.applyDomAccessibility?.(rec.element, semMeta, semMeta ? {
            autoLabel: node?.name || '',
            isInteractive: false,
            tabEnabled: node?.input?.tab_enabled,
            explicitTabIndex: node?.input?.tab_index,
          } : {});
        const ownerMeta = semByRef.get(key)?.accessibility;
        if (ownerMeta?.silent || ownerMeta?.forceSimple || ownerMeta?.force_simple) {
          suppressionOwners.set(rec.element, { control: rec.buttonRecord?.element });
        }
        if (rec.clipWrapper && rec.clipInner) {
          syncChildrenOrder(rec.clipInner, childList, preserveNativeFocus);
          syncChildrenOrder(rec.element, [rec.clipWrapper], preserveNativeFocus);
        } else {
          syncChildrenOrder(rec.element, childList, preserveNativeFocus);
        }
      }

      // Prune stale records (zero DOM node or listener leaks!).
      for (const [k, rec] of leafRecords) {
        if (!seenLeaves.has(k)) {
          rec.cleanup?.();
          leafRecords.delete(k);
        }
      }
      for (const [k, rec] of islandRecords) {
        if (!seenIslands.has(k)) {
          rec.cleanup?.();
          islandRecords.delete(k);
        }
      }
      for (const [k, rec] of containerRecords) {
        if (!seenContainers.has(k)) {
          rec.cleanup?.();
          containerRecords.delete(k);
        }
      }

      syncChildrenOrder(sceneElement, sceneDesired, preserveNativeFocus);
      suppressDescendantAccessibility(suppressionOwners);
      if (pendingNativeFocus && getStageHost()?.stage?.focus === pendingNativeFocus && canFocus(pendingNativeFocus)) {
        if (focusNativeControl(pendingNativeFocus)) pendingNativeFocus = null;
      }
    }

    function sync(refKey) {
      if (disposed || !refKey) return;
      const rec = inputRecordsByRef.get(refKey);
      if (!rec) return;
      const textHost = getTextHost();
      if (!textHost) return;
      try {
        const field = textHost.info(refKey);
        const nextVal = displayedFieldText(field);
        if (rec.element.value !== nextVal) {
          rec.syncingValue = true;
          try { rec.element.value = nextVal; } finally { rec.syncingValue = false; }
        }
      } catch (_) {}
    }

    function owns(node) {
      return Boolean(node && ownedInputControls.has(node));
    }

    function canFocus(target) {
      const disp = getDisplay();
      const ref = referenceOf(target);
      if (!ref || !disp?.alive?.(target)) return true;
      const desc = disp.descriptor(target);
      // Native controls reflect the last render; script may enable and focus an
      // object in the same turn, so eligibility comes from its current runtime state.
      if (desc?.on_stage === false || desc?.effective_visible === false || desc?.visible === false
          || (desc?.owner === 'button' && desc.input?.button_enabled === false)
          || (desc?.owner === 'movie_clip' && desc.input?.movieclip_enabled === false)) return false;
      if (desc?.owner !== 'text') return true;
      return desc.text?.field_type === 'input' || desc.text?.selectable !== false;
    }

    function focusNativeControl(target) {
      const ref = referenceOf(target);
      const rec = ref ? (inputRecordsByRef.get(ref.key) || containerRecords.get(ref.key)?.buttonRecord) : null;
      if (!rec || rec.element.disabled || rec.element.getAttribute('aria-hidden') === 'true') return false;
      if (doc.activeElement !== rec.element) {
        rec.syncingFocus = true;
        try { rec.element.focus?.({ preventScroll: true }); } finally { rec.syncingFocus = false; }
      }
      return doc.activeElement === rec.element;
    }

    function focusChanged(next, previous) {
      if (disposed) return true;
      pendingNativeFocus = null;
      if (previous) {
        const prevRef = referenceOf(previous);
        const prevRec = prevRef ? (inputRecordsByRef.get(prevRef.key) || containerRecords.get(prevRef.key)?.buttonRecord) : null;
        if (prevRec && doc.activeElement === prevRec.element) {
          prevRec.syncingFocus = true;
          try { prevRec.element.blur?.(); } finally { prevRec.syncingFocus = false; }
        }
      }
      if (next) {
        if (!focusNativeControl(next)) pendingNativeFocus = next;
      }
      return true;
    }

    const Selection = Object.freeze({
      getFocus() {
        const sh = getStageHost();
        const disp = getDisplay();
        const focused = sh?.stage?.focus;
        if (!focused || focused === sh?.stage || !disp?.alive?.(focused)) return null;
        return disp.targetPath?.(focused) || null;
      },
      setFocus(target) {
        const sh = getStageHost();
        if (!sh) return false;
        if (target === null || target === undefined) {
          sh.stage.focus = null;
          return true;
        }
        if (!canFocus(target)) return false;
        sh.stage.focus = target;
        return sh.stage.focus === target;
      },
      getBeginIndex() {
        const el = doc.activeElement;
        return ownedInputControls.has(el) ? (el.selectionStart ?? -1) : -1;
      },
      getEndIndex() {
        const el = doc.activeElement;
        return ownedInputControls.has(el) ? (el.selectionEnd ?? -1) : -1;
      },
      getCaretIndex() {
        const el = doc.activeElement;
        return ownedInputControls.has(el) ? (el.selectionEnd ?? -1) : -1;
      },
      setSelection(begin, end) {
        const el = doc.activeElement;
        if (ownedInputControls.has(el) && typeof el.setSelectionRange === 'function') {
          el.setSelectionRange(Math.max(0, Number(begin) | 0), Math.max(0, Number(end) | 0));
        }
      },
    });

    function clearSceneRecords() {
      pendingNativeFocus = null;
      restoreSuppressedAccessibility();
      for (const rec of leafRecords.values()) rec.cleanup?.();
      for (const rec of islandRecords.values()) rec.cleanup?.();
      for (const rec of containerRecords.values()) rec.cleanup?.();
      leafRecords.clear();
      islandRecords.clear();
      containerRecords.clear();
      inputRecordsByRef.clear();
      bitmapUrlCache.clear();
      sceneElement.replaceChildren?.();
    }

    function reset() {
      if (disposed) return;
      clearSceneRecords();
    }

    function dispose() {
      if (disposed) return;
      disposed = true;
      clearSceneRecords();
      while (cleanups.length) {
        const fn = cleanups.pop();
        try { fn(); } catch (_) {}
      }
      stageElement.dispose?.();
    }

    stageElement._onDispose = () => {
      if (!disposed) {
        disposed = true;
        clearSceneRecords();
        while (cleanups.length) {
          const fn = cleanups.pop();
          try { fn(); } catch (_) {}
        }
      }
    };

    return Object.freeze({
      isActive: () => !disposed,
      stageElement,
      sceneElement,
      evaluateViewport,
      render,
      sync,
      owns,
      canFocus,
      focusChanged,
      Selection,
      reset,
      dispose,
    });
  }

  const NeoFlashStageElement = registerStageComponent();

  const api = Object.freeze({
    create,
    createDomStageHost,
    sanitizeDomLinkHref,
    computeResponsiveLayout,
    evaluateBreakpoint,
    ResponsiveController,
    SemanticDomTextLayer,
    DomFormsHost,
    NeoFlashStageElement,
    createStageComponentClass,
    registerStageComponent,
  });

  global.NeoFlashDomLayout = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
