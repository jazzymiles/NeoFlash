/* Semantic DOM text layer, DOM forms & inputs, <neoflash-stage> Web Component,
 * and responsive stage layout / breakpoint engine for HTML5 publications. */
(function (global) {
  'use strict';

  const VALID_SCALE_MODES = new Set(['showAll', 'noBorder', 'exactFit', 'noScale', 'fit']);
  const VALID_CONTROL_KINDS = new Set(['input', 'textarea', 'select', 'button', 'checkbox', 'radio', 'form']);

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
          fontFamily: JSON.stringify(fontFamily) + ', sans-serif',
          fontSize: `${fontSize}px`,
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
            const node = fmt.url ? this.doc.createElement('a') : this.doc.createElement('span');
            node.textContent = slice;
            if (fmt.bold) node.style.fontWeight = 'bold';
            if (fmt.italic) node.style.fontStyle = 'italic';
            if (fmt.underline) node.style.textDecoration = 'underline';
            if (fmt.size) node.style.fontSize = `${fmt.size}px`;
            if (fmt.font) node.style.fontFamily = JSON.stringify(fmt.font) + ', sans-serif';
            if (fmt.url) {
              node.setAttribute('href', fmt.url);
              if (fmt.target) node.setAttribute('target', fmt.target);
              node.style.pointerEvents = 'auto';
              node.addEventListener('click', ev => {
                if (this.onLink) {
                  ev.preventDefault?.();
                  this.onLink(fmt.url, fmt.target || '', item.owner_ref);
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
        const root = typeof this.attachShadow === 'function' ? this.attachShadow({ mode: 'open' }) : this;
        this.shadowRootNode = root;
      }

      connectedCallback() {
        if (this._mounted) return;
        this._mounted = true;
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
              canvas.width = Math.max(1, Math.round(layout.logicalWidth));
              canvas.height = Math.max(1, Math.round(layout.logicalHeight));
            },
          });
          this.responsiveController.refresh(this._stageWidth, this._stageHeight);
        }
      }

      disconnectedCallback() {
        this.responsiveController?.dispose();
        this.domText?.clear();
        this.domForms?.clear();
        this._mounted = false;
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

  const NeoFlashStageElement = registerStageComponent();

  const api = Object.freeze({
    create,
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
