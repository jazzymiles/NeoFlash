// ============================================================================
// NeoFlash UI Builder — core helpers
// Builds real Figma nodes (frames / components / text / vectors). No bitmaps.
// ============================================================================

const FONTS = {};
const ERRLOG = [];
function guard(label, fn) {
  try { fn(); } catch (e) { ERRLOG.push(label + ': ' + (e && e.message ? e.message : e) + '\n' + (e && e.stack ? String(e.stack).split('\n').slice(0, 4).join('\n') : '')); console.error(label, e); }
}
const pendingStyles = []; // [textNode, textStyle] applied async if sync assignment is not allowed

async function ensureFont(key, candidates) {
  for (const c of candidates) {
    try { await figma.loadFontAsync(c); FONTS[key] = c; return; } catch (e) { /* try next */ }
  }
  throw new Error('No font available for ' + key);
}

async function loadAllFonts() {
  await ensureFont('v1', [{ family: 'Inter', style: 'Regular' }]);
  await ensureFont('v1mono', [{ family: 'Roboto Mono', style: 'Regular' }, { family: 'Inter', style: 'Regular' }]);
  await ensureFont('i400', [{ family: 'Inter', style: 'Regular' }]);
  await ensureFont('i500', [{ family: 'Inter', style: 'Medium' }]);
  await ensureFont('i600', [{ family: 'Inter', style: 'Semi Bold' }]);
  await ensureFont('mono2', [{ family: 'Roboto Mono', style: 'Regular' }, { family: 'Inter', style: 'Regular' }]);
}

// ---------------------------------------------------------------- tokens ----
let CTX = null; // active token collection: { vars, hex, num }

function hexToRgba(hex) {
  let h = hex.replace('#', '');
  if (h.length === 3) h = h.split('').map(c => c + c).join('');
  const r = parseInt(h.slice(0, 2), 16) / 255, g = parseInt(h.slice(2, 4), 16) / 255, b = parseInt(h.slice(4, 6), 16) / 255;
  const a = h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1;
  return { r, g, b, a };
}

// defs: { 'color/x': [modeA, modeB?], 'space/1': [4] }  — strings starting with # are colors, numbers are FLOAT
function createTokens(collName, modeNames, defs) {
  const coll = figma.variables.createVariableCollection(collName);
  coll.renameMode(coll.modes[0].modeId, modeNames[0]);
  const modeIds = [coll.modes[0].modeId];
  for (let i = 1; i < modeNames.length; i++) modeIds.push(coll.addMode(modeNames[i]));
  const ctx = { coll, modeIds, vars: {}, hex: {}, num: {} };
  for (const name of Object.keys(defs)) {
    const vals = defs[name];
    const isColor = typeof vals[0] === 'string';
    const v = figma.variables.createVariable(name, coll, isColor ? 'COLOR' : 'FLOAT');
    v.scopes = isColor ? ['ALL_SCOPES'] : ['ALL_SCOPES'];
    modeIds.forEach((id, i) => {
      const val = vals[Math.min(i, vals.length - 1)];
      v.setValueForMode(id, isColor ? hexToRgba(val) : val);
    });
    ctx.vars[name] = v;
    if (isColor) ctx.hex[name] = vals[0]; else ctx.num[name] = vals[0];
  }
  return ctx;
}

function isToken(c) { return typeof c === 'string' && c[0] !== '#'; }

function solid(c, opacity) {
  let paint;
  if (isToken(c)) {
    if (!CTX || !CTX.vars[c]) throw new Error('Unknown color token: ' + c);
    const rgb = hexToRgba(CTX.hex[c]);
    paint = { type: 'SOLID', color: { r: rgb.r, g: rgb.g, b: rgb.b }, opacity: rgb.a };
    if (opacity != null) paint.opacity = opacity;
    paint = figma.variables.setBoundVariableForPaint(paint, 'color', CTX.vars[c]);
  } else {
    const rgb = hexToRgba(c);
    paint = { type: 'SOLID', color: { r: rgb.r, g: rgb.g, b: rgb.b }, opacity: opacity != null ? opacity : rgb.a };
  }
  return paint;
}

// number or numeric token name -> number, with optional variable binding
function num(node, field, v) {
  if (v == null) return;
  if (typeof v === 'string') {
    const variable = CTX && CTX.vars[v];
    if (!variable) throw new Error('Unknown number token: ' + v);
    node[field] = CTX.num[v];
    try { node.setBoundVariable(field, variable); } catch (e) { /* binding is best effort */ }
  } else {
    node[field] = v;
  }
}

// ----------------------------------------------------------- text styles ----
// specs: { 'NF1/Label': {font:'v1', size:11, lh:14, ls:0, upper:false} }
function createTextStyles(specs) {
  const out = {};
  for (const name of Object.keys(specs)) {
    const s = specs[name];
    const st = figma.createTextStyle();
    st.name = name;
    st.fontName = FONTS[s.font];
    st.fontSize = s.size;
    if (s.lh) st.lineHeight = { unit: 'PIXELS', value: s.lh };
    if (s.ls) st.letterSpacing = { unit: 'PERCENT', value: s.ls };
    if (s.upper) st.textCase = 'UPPER';
    out[name] = { style: st, spec: s };
  }
  return out;
}

// ------------------------------------------------------------- builders -----
function applyAuto(f, o) {
  const dir = o.dir;
  f.layoutMode = dir === 'h' ? 'HORIZONTAL' : dir === 'v' ? 'VERTICAL' : 'NONE';
  if (dir !== 'h' && dir !== 'v') return;
  const p = o.pad;
  let pt = 0, pr = 0, pb = 0, pl = 0;
  if (Array.isArray(p)) {
    if (p.length === 2) { pt = pb = p[0]; pr = pl = p[1]; } else { [pt, pr, pb, pl] = p; }
  } else if (p != null) { pt = pr = pb = pl = p; }
  num(f, 'paddingTop', pt); num(f, 'paddingRight', pr); num(f, 'paddingBottom', pb); num(f, 'paddingLeft', pl);
  num(f, 'itemSpacing', o.gap != null ? o.gap : 0);
  f.primaryAxisAlignItems = o.main || 'MIN';
  f.counterAxisAlignItems = o.cross || 'MIN';
  if (o.wrap) f.layoutWrap = 'WRAP';
}

function styleBox(f, o) {
  f.fills = o.fill ? [solid(o.fill, o.fillOp)] : [];
  if (o.stroke) {
    f.strokes = [solid(o.stroke)];
    f.strokeWeight = o.sw != null ? o.sw : 1;
    f.strokeAlign = o.sa || 'INSIDE';
    if (o.sides) {
      // NB: after the first per-side write f.strokeWeight becomes figma.mixed — keep the number in a local
      const sw0 = o.sw != null ? o.sw : 1;
      f.strokeTopWeight = o.sides.indexOf('t') >= 0 ? sw0 : 0;
      f.strokeRightWeight = o.sides.indexOf('r') >= 0 ? sw0 : 0;
      f.strokeBottomWeight = o.sides.indexOf('b') >= 0 ? sw0 : 0;
      f.strokeLeftWeight = o.sides.indexOf('l') >= 0 ? sw0 : 0;
    }
  } else { f.strokes = []; }
  if (o.r != null) {
    if (Array.isArray(o.r)) {
      f.topLeftRadius = o.r[0]; f.topRightRadius = o.r[1]; f.bottomRightRadius = o.r[2]; f.bottomLeftRadius = o.r[3];
    } else num(f, 'cornerRadius', o.r);
  }
  if (o.shadow) f.effects = [{ type: 'DROP_SHADOW', color: { r: 0, g: 0, b: 0, a: o.shadow.a != null ? o.shadow.a : 0.25 }, offset: { x: 0, y: o.shadow.y || 4 }, radius: o.shadow.blur || 12, spread: 0, visible: true, blendMode: 'NORMAL' }];
  f.clipsContent = !!o.clip;
}

function sizeBox(f, o) {
  const W = o.w, H = o.h;
  if (W != null || H != null) f.resize(W != null ? W : Math.max(f.width, 1), H != null ? H : Math.max(f.height, 1));
  if (o.dir === 'h' || o.dir === 'v') {
    const horiz = o.dir === 'h';
    f.primaryAxisSizingMode = (horiz ? W : H) != null ? 'FIXED' : 'AUTO';
    f.counterAxisSizingMode = (horiz ? H : W) != null ? 'FIXED' : 'AUTO';
  }
}

function mkFrame(name, o) {
  o = o || {};
  const f = o.component ? figma.createComponent() : figma.createFrame();
  f.name = name;
  applyAuto(f, o);
  styleBox(f, o);
  sizeBox(f, o);
  if (o.x != null) f.x = o.x;
  if (o.y != null) f.y = o.y;
  return f;
}
function mkComp(name, o) { o = Object.assign({}, o, { component: true }); return mkFrame(name, o); }

// add child to parent; opts: { x, y, fillW, fillH, grow, abs }
function put(parent, child) { return add(parent, child, {}); }

function add(parent, child, opts) {
  const px0 = child.x, py0 = child.y;
  parent.appendChild(child);
  opts = opts || {};
  const auto = parent.layoutMode && parent.layoutMode !== 'NONE';
  if (!auto) { child.x = px0; child.y = py0; }
  if (auto) {
    if (opts.abs) { child.layoutPositioning = 'ABSOLUTE'; }
    if (opts.fillW) child.layoutSizingHorizontal = 'FILL';
    if (opts.fillH) child.layoutSizingVertical = 'FILL';
    if (opts.grow) child.layoutGrow = 1;
  }
  if (opts.x != null) child.x = opts.x;
  if (opts.y != null) child.y = opts.y;
  return child;
}

function mkText(str, o) {
  o = o || {};
  const t = figma.createText();
  t.fontName = FONTS[o.font || 'v1'];
  t.fontSize = o.size || 11;
  t.characters = str;
  t.fills = [solid(o.color || '#000000')];
  if (o.lh) t.lineHeight = { unit: 'PIXELS', value: o.lh };
  if (o.ls) t.letterSpacing = { unit: 'PERCENT', value: o.ls };
  if (o.upper) t.textCase = 'UPPER';
  if (o.align) t.textAlignHorizontal = o.align;
  t.textAutoResize = 'WIDTH_AND_HEIGHT';
  if (o.w != null) { t.textAutoResize = 'HEIGHT'; t.resize(o.w, t.height); }
  if (o.name) t.name = o.name;
  if (o.style) {
    try { t.textStyleId = o.style.style.id; } catch (e) { pendingStyles.push([t, o.style.style]); }
  }
  if (o.x != null) t.x = o.x;
  if (o.y != null) t.y = o.y;
  return t;
}

async function flushStyles() {
  for (const [t, s] of pendingStyles) { try { await t.setTextStyleIdAsync(s.id); } catch (e) { /* ignore */ } }
  pendingStyles.length = 0;
}

function mkRect(name, o) {
  const r = figma.createRectangle();
  r.name = name;
  r.resize(o.w, o.h);
  r.fills = o.fill ? [solid(o.fill, o.fillOp)] : [];
  if (o.stroke) { r.strokes = [solid(o.stroke)]; r.strokeWeight = o.sw || 1; r.strokeAlign = o.sa || 'INSIDE'; }
  if (o.r != null) { if (Array.isArray(o.r)) { r.topLeftRadius = o.r[0]; r.topRightRadius = o.r[1]; r.bottomRightRadius = o.r[2]; r.bottomLeftRadius = o.r[3]; } else r.cornerRadius = o.r; }
  if (o.x != null) r.x = o.x;
  if (o.y != null) r.y = o.y;
  return r;
}

function mkEllipse(name, o) {
  const e = figma.createEllipse();
  e.name = name;
  e.resize(o.w, o.h != null ? o.h : o.w);
  e.fills = o.fill ? [solid(o.fill, o.fillOp)] : [];
  if (o.stroke) { e.strokes = [solid(o.stroke)]; e.strokeWeight = o.sw || 1; e.strokeAlign = o.sa || 'INSIDE'; }
  if (o.x != null) e.x = o.x;
  if (o.y != null) e.y = o.y;
  return e;
}

function mkLine(name, o) { // straight line as a thin rectangle -> stays crisp and token-bindable
  return mkRect(name, { w: o.w, h: o.h, fill: o.fill, x: o.x, y: o.y, fillOp: o.fillOp });
}

// ---------------------------------------------------------------- icons -----
// 16x16 viewBox, stroke icons by default; elements marked fill="#000" are solid.
const ICONS = {
  select: '<path d="M3.5 2.2 L3.5 12.6 L6.2 10.2 L8 14 L9.6 13.3 L7.8 9.6 L11.3 9.4 Z" fill="#000" stroke="none"/>',
  subselect: '<path d="M3.5 2.2 L3.5 12.6 L6.2 10.2 L8 14 L9.6 13.3 L7.8 9.6 L11.3 9.4 Z"/>',
  transform: '<rect x="4" y="4" width="8" height="8"/><rect x="2" y="2" width="3" height="3" fill="#000"/><rect x="11" y="2" width="3" height="3" fill="#000"/><rect x="2" y="11" width="3" height="3" fill="#000"/><rect x="11" y="11" width="3" height="3" fill="#000"/>',
  lasso: '<path d="M8 3 C12 3 14 5 13 8 C12 11 7 12 4.5 10.5 C2.5 9 3.5 5 8 3 Z"/><path d="M5 11 C5 13 6 14 7.5 14"/>',
  pen: '<path d="M8 2 L12 7 L8 14 L4 7 Z"/><circle cx="8" cy="7.2" r="1.1"/><path d="M8 8.3 L8 14"/>',
  text: '<path d="M3.5 3.5 H12.5 M8 3.5 V13 M6 13 H10"/>',
  line: '<path d="M3 13 L13 3"/>',
  rect: '<rect x="3" y="4" width="10" height="8"/>',
  oval: '<ellipse cx="8" cy="8" rx="5.5" ry="4.5"/>',
  pencil: '<path d="M3 13 L3.6 10.4 L10.8 3.2 L12.8 5.2 L5.6 12.4 Z M9.4 4.6 L11.4 6.6"/>',
  brush: '<path d="M12.5 3 C10 5 8 7 7 9"/><path d="M5.5 10 C4 10 3 11.2 3 13.2 C5 13.2 6.5 12.5 7 11 Z"/>',
  stamp: '<path d="M5 7 H11 V13 H5 Z M7 7 V4.5 H9 V7"/>',
  bucket: '<path d="M3.5 8 L8 3.5 L12.5 8 L8.5 12 Z"/><path d="M13 9.5 C13 11 14 11.5 14 12.3 C14 13 13 13.3 13 13.3"/>',
  eyedropper: '<path d="M11.5 2.5 L13.5 4.5 L11.8 6.2 L9.8 4.2 Z"/><path d="M9.8 5 L4 10.8 L3.5 13 L5.7 12.5 L11.2 6.7"/>',
  eraser: '<path d="M3 10 L8.5 4 L13 8 L9 13 H5 Z M6 7.5 L10.5 11.5"/>',
  gradient: '<rect x="3" y="3" width="10" height="10" fill="#000" stroke="none"/>',
  hand: '<path d="M5.5 8 V4.5 a1 1 0 0 1 2 0 V8 M7.5 7.5 V3.5 a1 1 0 0 1 2 0 V8 M9.5 8 V4.5 a1 1 0 0 1 2 0 V10 C11.5 12.5 10 14 8 14 C6 14 5 13 4 11 L3 9 a1 1 0 0 1 1.6-1.2 L5.5 9"/>',
  zoomin: '<circle cx="7" cy="7" r="4"/><path d="M10 10 L13.5 13.5 M5.5 7 H8.5 M7 5.5 V8.5"/>',
  zoomout: '<circle cx="7" cy="7" r="4"/><path d="M10 10 L13.5 13.5 M5.5 7 H8.5"/>',
  fit: '<path d="M3 6 V3 H6 M10 3 H13 V6 M13 10 V13 H10 M6 13 H3 V10"/>',
  lock: '<rect x="4" y="7.5" width="8" height="6"/><path d="M5.5 7.5 V5.5 a2.5 2.5 0 0 1 5 0 V7.5"/>',
  unlock: '<rect x="4" y="7.5" width="8" height="6"/><path d="M5.5 7.5 V5.5 a2.5 2.5 0 0 1 4.8 -1"/>',
  eye: '<path d="M1.5 8 C3.5 4.5 12.5 4.5 14.5 8 C12.5 11.5 3.5 11.5 1.5 8 Z"/><circle cx="8" cy="8" r="1.8" fill="#000"/>',
  eyeoff: '<path d="M1.5 8 C3.5 4.5 12.5 4.5 14.5 8 C12.5 11.5 3.5 11.5 1.5 8 Z"/><path d="M3 13 L13 3"/>',
  layers: '<path d="M8 2.5 L14 6 L8 9.5 L2 6 Z M2 9 L8 12.5 L14 9"/>',
  play: '<path d="M5 3 L13 8 L5 13 Z" fill="#000" stroke="none"/>',
  pause: '<rect x="4" y="3" width="3" height="10" fill="#000" stroke="none"/><rect x="9" y="3" width="3" height="10" fill="#000" stroke="none"/>',
  first: '<path d="M4 3 V13"/><path d="M12.5 3 L6.5 8 L12.5 13 Z" fill="#000" stroke="none"/>',
  last: '<path d="M12 3 V13"/><path d="M3.5 3 L9.5 8 L3.5 13 Z" fill="#000" stroke="none"/>',
  stepback: '<path d="M11 3 L5 8 L11 13 Z" fill="#000" stroke="none"/>',
  stepfwd: '<path d="M5 3 L11 8 L5 13 Z" fill="#000" stroke="none"/>',
  plus: '<path d="M8 3 V13 M3 8 H13"/>',
  minus: '<path d="M3 8 H13"/>',
  trash: '<path d="M3 4.5 H13 M6.5 4.5 V3 H9.5 V4.5 M4.5 4.5 L5 13.5 H11 L11.5 4.5 M7 7 V11 M9 7 V11"/>',
  tridown: '<path d="M4 6 H12 L8 11 Z" fill="#000" stroke="none"/>',
  triright: '<path d="M6 4 V12 L11 8 Z" fill="#000" stroke="none"/>',
  folder: '<path d="M2 4.5 V12.5 H14 V6 H8 L7 4.5 Z"/>',
  symbol: '<path d="M8 2.5 L13.5 8 L8 13.5 L2.5 8 Z"/><path d="M8 5.5 L10.5 8 L8 10.5 L5.5 8 Z"/>',
  grip: '<circle cx="8" cy="3.5" r="1" fill="#000" stroke="none"/><circle cx="8" cy="8" r="1" fill="#000" stroke="none"/><circle cx="8" cy="12.5" r="1" fill="#000" stroke="none"/>',
  external: '<path d="M9 3 H13 V7 M13 3 L7 9 M11 9.5 V12.5 H3.5 V5 H6.5"/>',
  check: '<path d="M3.5 8.5 L6.5 11.5 L12.5 4.5"/>',
  chevdown: '<path d="M4 6 L8 10 L12 6"/>',
  chevright: '<path d="M6 4 L10 8 L6 12"/>',
  chevup: '<path d="M4 10 L8 6 L12 10"/>',
  search: '<circle cx="7" cy="7" r="4"/><path d="M10 10 L13.5 13.5"/>',
  x: '<path d="M4 4 L12 12 M12 4 L4 12"/>',
  more: '<circle cx="3.5" cy="8" r="1" fill="#000" stroke="none"/><circle cx="8" cy="8" r="1" fill="#000" stroke="none"/><circle cx="12.5" cy="8" r="1" fill="#000" stroke="none"/>',
  onion: '<circle cx="6" cy="8" r="3.8"/><circle cx="10" cy="8" r="3.8"/>',
  code: '<path d="M6 4 L2.5 8 L6 12 M10 4 L13.5 8 L10 12"/>',
  terminal: '<rect x="2" y="3" width="12" height="10"/><path d="M5 7 L7 8.5 L5 10 M8.5 10.5 H11"/>',
  access: '<circle cx="8" cy="3.5" r="1.3" fill="#000" stroke="none"/><path d="M3 6 H13 M8 6 V10 M8 10 L5.5 14 M8 10 L10.5 14"/>',
  upload: '<path d="M8 11 V3 M5 6 L8 3 L11 6 M3 13 H13"/>',
  link: '<path d="M6.5 9.5 L9.5 6.5 M7 4.5 L8 3.5 a2.1 2.1 0 0 1 3 3 L10 7.5 M9 11.5 L8 12.5 a2.1 2.1 0 0 1 -3 -3 L6 8.5"/>',
  loop: '<path d="M3 7 V6 a2 2 0 0 1 2 -2 H12 M10 2 L12 4 L10 6 M13 9 V10 a2 2 0 0 1 -2 2 H4 M6 14 L4 12 L6 10"/>',
  film: '<rect x="2.5" y="3" width="11" height="10"/><path d="M5.5 3 V13 M10.5 3 V13 M2.5 6.5 H5.5 M2.5 9.5 H5.5 M10.5 6.5 H13.5 M10.5 9.5 H13.5"/>',
  save: '<path d="M3 3 H11.5 L13 4.5 V13 H3 Z M5.5 3 V6.5 H10.5 V3 M5.5 13 V9.5 H10.5 V13"/>',
  undo: '<path d="M5 6 H10 a3 3 0 0 1 0 6 H6 M7 3.5 L4.5 6 L7 8.5"/>',
  settings: '<circle cx="8" cy="8" r="2"/><path d="M8 2 V4 M8 12 V14 M2 8 H4 M12 8 H14 M3.8 3.8 L5.2 5.2 M10.8 10.8 L12.2 12.2 M3.8 12.2 L5.2 10.8 M10.8 5.2 L12.2 3.8"/>',
};

// icon library: ICON_COMPS[size][name] -> ComponentNode
let ICON_COMPS = {};
let ICON_PREFIX = 'Icon';

function recolor(node, color) {
  const list = node.findAll ? node.findAll(() => true) : [];
  list.forEach(n => {
    if ('fills' in n && n.fills !== figma.mixed && n.fills.length) n.fills = [solid(color)];
    if ('strokes' in n && n.strokes.length) n.strokes = [solid(color)];
  });
}

function buildIconComponent(name, size, parent, sw) {
  const svg = '<svg width="' + size + '" height="' + size + '" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="' + (sw || 1.3) + '" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg">' + ICONS[name] + '</svg>';
  const node = figma.createNodeFromSvg(svg);
  const comp = figma.createComponent();
  comp.name = ICON_PREFIX + '/' + size + '/' + name;
  comp.resize(size, size);
  comp.fills = [];
  comp.clipsContent = false;
  // move svg children into the component so the component itself is the icon
  const kids = node.children.slice();
  kids.forEach(k => comp.appendChild(k));
  node.remove();
  parent.appendChild(comp);
  return comp;
}

function buildIconSet(sizes, names, color, parent, sw) {
  ICON_COMPS = {};
  sizes.forEach(size => {
    ICON_COMPS[size] = {};
    names.forEach(n => {
      const c = buildIconComponent(n, size, parent, sw);
      recolor(c, color);
      ICON_COMPS[size][n] = c;
    });
  });
}

function icon(name, size, color) {
  const comp = ICON_COMPS[size] && ICON_COMPS[size][name];
  if (!comp) throw new Error('Missing icon ' + name + '@' + size);
  const inst = comp.createInstance();
  if (color) recolor(inst, color);
  return inst;
}

// replace the first nested icon instance inside `inst` by another icon
function setIcon(inst, name, size, color) {
  const nested = inst.findOne(n => n.type === 'INSTANCE' && n.name.indexOf(ICON_PREFIX + '/') === 0);
  if (!nested) throw new Error('No icon slot in ' + inst.name);
  nested.swapComponent(ICON_COMPS[size][name]);
  if (color) recolor(nested, color);
}

// ------------------------------------------------------- component sets -----
// variants: [{ props: {State:'Default'}, comp }]
function combine(name, comps, parent, propsList, width) {
  comps.forEach((c, i) => {
    c.name = Object.keys(propsList[i]).map(k => k + '=' + propsList[i][k]).join(', ');
    parent.appendChild(c);
  });
  const set = figma.combineAsVariants(comps, parent);
  set.name = name;
  set.layoutMode = 'HORIZONTAL';
  set.layoutWrap = 'WRAP';
  set.itemSpacing = 16;
  set.counterAxisSpacing = 16;
  set.paddingTop = set.paddingRight = set.paddingBottom = set.paddingLeft = 16;
  set.primaryAxisSizingMode = 'FIXED';
  set.counterAxisSizingMode = 'AUTO';
  set.resize(width || 520, 100);
  set.counterAxisSizingMode = 'AUTO';
  set.fills = [{ type: 'SOLID', color: { r: 1, g: 1, b: 1 }, opacity: 0.0 }];
  set.strokes = [{ type: 'SOLID', color: { r: 0.6, g: 0.4, b: 1 } }];
  set.strokeWeight = 1;
  set.dashPattern = [6, 4];
  set.cornerRadius = 6;
  return set;
}

// expose a text label as a component property on a set (best effort)
function labelProperty(set, propName, defaultValue) {
  try {
    const key = set.addComponentProperty(propName, 'TEXT', defaultValue);
    set.children.forEach(variant => {
      const t = variant.findOne(n => n.type === 'TEXT' && n.name === 'Label');
      if (t) t.componentPropertyReferences = { characters: key };
    });
    return key;
  } catch (e) { return null; }
}

// ------------------------------------------------------------- pages --------
async function newPage(name) {
  const p = figma.createPage();
  p.name = name;
  return p;
}
async function gotoPage(p) {
  if (figma.setCurrentPageAsync) await figma.setCurrentPageAsync(p); else figma.currentPage = p;
}

// labelled section on a components page (plain frame used as titled shelf)
function shelf(parent, title, x, y, w, opts) {
  opts = opts || {};
  const f = mkFrame('Shelf / ' + title, { dir: 'v', gap: 12, pad: 20, w: w, fill: opts.fill || '#FFFFFF', stroke: opts.stroke || '#E3E5EA', r: 8, x: x, y: y });
  const h = mkText(title, { font: 'i600', size: 13, color: opts.titleColor || '#222222' });
  add(f, h);
  parent.appendChild(f);
  f.x = x; f.y = y;
  return f;
}

function sectionTitle(parent, title, x, y) {
  const t = mkText(title, { font: 'i600', size: 28, color: '#111111' });
  parent.appendChild(t);
  t.x = x; t.y = y;
  return t;
}
// ============================================================================
// v1 — "as is" design system, reconstructed from the screenshots.
// Everything lives under the NF1 prefix: own variables, own text styles, own components.
// ============================================================================

const V1 = { c: {}, s: {}, set: {}, tokens: null };

function styled(styles, name, str, color, o) {
  const st = styles[name];
  if (!st) throw new Error('Unknown text style ' + name);
  return mkText(str, Object.assign({ font: st.spec.font, size: st.spec.size, lh: st.spec.lh, ls: st.spec.ls, upper: st.spec.upper, color: color || '#000000', style: st }, o || {}));
}
const T1 = (name, str, color, o) => styled(V1.s, name, str, color, o);

// pick a variant out of a component set by props, e.g. pick(set, {State:'Selected'})
function pick(set, props) {
  const want = Object.keys(props).map(k => k + '=' + props[k]);
  const comp = set.children.find(c => want.every(w => c.name.split(', ').indexOf(w) >= 0));
  if (!comp) throw new Error('No variant ' + want.join(',') + ' in ' + set.name);
  return comp;
}
function make(set, props) { return pick(set, props || {}).createInstance(); }

// set the text of an instance's "Label" (component property if present, else direct override)
function setLabel(inst, str, nodeName) {
  const keys = Object.keys(inst.componentProperties || {});
  const k = keys.find(x => x.indexOf('Label#') === 0);
  if (k && !nodeName) { inst.setProperties({ [k]: str }); return; }
  const t = inst.findOne(n => n.type === 'TEXT' && n.name === (nodeName || 'Label'));
  if (t) t.characters = str;
}

function v1BuildTokens() {
  V1.tokens = createTokens('NF1 · Tokens', ['Light'], {
    'color/bg/panel': ['#F0F1F4'],
    'color/bg/header': ['#EAECF1'],
    'color/bg/canvas': ['#C3C6D0'],
    'color/bg/stage': ['#131824'],
    'color/bg/field': ['#FFFFFF'],
    'color/bg/field-number': ['#E7EAF2'],
    'color/bg/selected': ['#D7E3F8'],
    'color/bg/row-selected': ['#DDE2EE'],
    'color/bg/dialog-header': ['#DDE1EA'],
    'color/bg/shaded': ['#E3E6EE'],
    'color/border/default': ['#B4BACA'],
    'color/border/strong': ['#6D7690'],
    'color/border/field': ['#8890A5'],
    'color/border/cell': ['#CBCFD9'],
    'color/accent/primary': ['#2B4A98'],
    'color/accent/selected-border': ['#7E9BD6'],
    'color/text/primary': ['#1F2533'],
    'color/text/muted': ['#666D80'],
    'color/text/disabled': ['#9CA2B0'],
    'color/text/on-dark': ['#FFFFFF'],
    'color/layer/green': ['#3DAA78'],
    'color/layer/red': ['#DA5A57'],
    'color/layer/blue': ['#4C8FE3'],
    'color/timeline/playhead': ['#D3303A'],
    'color/scrollbar': ['#5E6579'],
    'color/overlay': ['#00000073'],
    'radius/sm': [3],
    'radius/md': [4],
  });
  CTX = V1.tokens;
  V1.s = createTextStyles({
    'NF1/Label': { font: 'v1', size: 14, lh: 18 },
    'NF1/Label Small': { font: 'v1', size: 12, lh: 16 },
    'NF1/Caption': { font: 'v1', size: 12, lh: 14 },
    'NF1/Caps': { font: 'v1', size: 11, lh: 14, ls: 4, upper: true },
    'NF1/Dialog Title': { font: 'v1', size: 16, lh: 20 },
    'NF1/Dialog Body': { font: 'v1', size: 16, lh: 22 },
    'NF1/Code': { font: 'v1mono', size: 14, lh: 20 },
    'NF1/Ruler': { font: 'v1', size: 11, lh: 12 },
    'NF1/Output': { font: 'v1mono', size: 13, lh: 18 },
  });
}

// layout cursor for component shelves: two columns
function makeFlow(page, colW, gap, y0, opts) {
  const cols = [0, 0];
  const base = y0 != null ? y0 : 120;
  return {
    shelf(title, col, w) {
      const x = col * (colW + gap);
      const sh = shelf(page, title, x, cols[col] + base, w || colW, opts);
      return { sh, done() { cols[col] = sh.y + sh.height - base + gap; } };
    },
    bottom() { return Math.max(cols[0], cols[1]) + base; },
  };
}

function v1BuildComponents(page) {
  CTX = V1.tokens;
  const C = V1.c;

  const title = mkText('NF1 — Components (v1, as built)', { font: 'i600', size: 32, color: '#111111' });
  page.appendChild(title); title.x = 0; title.y = 0;
  const sub = mkText('Reconstruction of the existing NeoFlash interface. Own variables (NF1 · Tokens), own text styles (NF1/*) — independent of v2.', { font: 'i400', size: 14, color: '#666666' });
  page.appendChild(sub); sub.x = 0; sub.y = 52;

  // ---- icon set (kept on this page, own components)
  ICON_PREFIX = 'NF1 Icon';
  const iconShelf = shelf(page, 'Icons · 16px', 1440, 120, 560);
  iconShelf.layoutMode = 'VERTICAL';
  const iconGrid = mkFrame('Icon grid', { dir: 'h', gap: 12, wrap: true, w: 520 });
  iconGrid.layoutMode = 'HORIZONTAL'; iconGrid.layoutWrap = 'WRAP';
  add(iconShelf, iconGrid, { fillW: true });
  buildIconSet([16], Object.keys(ICONS), 'color/text/primary', iconGrid, 1.2);
  V1.iconNames = Object.keys(ICONS);

  const flow = makeFlow(page, 640, 40);

  // ---- Buttons
  let s = flow.shelf('Button · Text', 0);
  guard('v1 block 1', () => {
    const defs = [
      { st: 'Default', stroke: 'color/border/strong', text: 'color/text/primary', fill: 'color/bg/header' },
      { st: 'Accent', stroke: 'color/accent/primary', text: 'color/accent/primary', fill: 'color/bg/header' },
      { st: 'Pressed', stroke: 'color/accent/selected-border', text: 'color/text/primary', fill: 'color/bg/selected' },
      { st: 'Disabled', stroke: 'color/border/default', text: 'color/text/disabled', fill: 'color/bg/header' },
    ];
    const comps = defs.map(d => {
      const c = mkComp('b', { dir: 'h', main: 'CENTER', cross: 'CENTER', pad: [0, 14], h: 28, fill: d.fill, stroke: d.stroke, r: 'radius/sm' });
      add(c, T1('NF1/Label', 'Button', d.text, { name: 'Label' }));
      return c;
    });
    V1.set.button = combine('NF1/Button/Text', comps, s.sh, defs.map(d => ({ Style: d.st })));
    labelProperty(V1.set.button, 'Label', 'Button');
    s.done();
  });

  s = flow.shelf('Button · Icon / Tool', 0);
  guard('v1 block 2', () => {
    const defs = [{ st: 'Default', fill: 'color/bg/header', stroke: 'color/border/default' }, { st: 'Selected', fill: 'color/bg/selected', stroke: 'color/accent/selected-border' }, { st: 'Active', fill: 'color/bg/header', stroke: 'color/accent/primary' }];
    const comps = defs.map(d => {
      const c = mkComp('b', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 28, h: 28, fill: d.fill, stroke: d.stroke, r: 'radius/sm' });
      add(c, icon('select', 16));
      return c;
    });
    V1.set.tool = combine('NF1/Button/Icon', comps, s.sh, defs.map(d => ({ State: d.st })));

    const ov = mkComp('NF1/Button/Overflow', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 24, h: 26, fill: 'color/bg/header', stroke: 'color/border/strong', r: 'radius/sm' });
    add(ov, T1('NF1/Label', '…', 'color/text/primary', { name: 'Label' }));
    s.sh.appendChild(ov); V1.c.overflow = ov;
    s.done();
  });

  // ---- Checkbox
  s = flow.shelf('Checkbox', 0);
  guard('v1 block 3', () => {
    const comps = [false, true].map(on => {
      const c = mkComp('c', { dir: 'h', cross: 'CENTER', gap: 7 });
      const box = mkFrame('Box', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 18, h: 18, fill: 'color/bg/header', stroke: 'color/border/strong', r: 'radius/sm' });
      if (on) add(box, icon('check', 16, 'color/accent/primary'));
      add(c, box);
      add(c, T1('NF1/Label', 'Onion Skin', 'color/text/primary', { name: 'Label' }));
      return c;
    });
    V1.set.checkbox = combine('NF1/Checkbox', comps, s.sh, [{ Checked: 'False' }, { Checked: 'True' }]);
    labelProperty(V1.set.checkbox, 'Label', 'Checkbox');
    s.done();
  });

  // ---- Fields
  s = flow.shelf('Field · Text / Number / Select', 0);
  guard('v1 block 4', () => {
    const defs = [
      { t: 'Text', fill: 'color/bg/field', stroke: 'color/border/field', main: 'MIN', tc: 'color/text/primary' },
      { t: 'Placeholder', fill: 'color/bg/field', stroke: 'color/border/field', main: 'MIN', tc: 'color/text/muted' },
      { t: 'Number', fill: 'color/bg/field-number', stroke: 'color/border/strong', main: 'CENTER', tc: 'color/text/primary' },
      { t: 'Focus', fill: 'color/bg/field', stroke: 'color/accent/primary', main: 'MIN', tc: 'color/text/primary' },
    ];
    const comps = defs.map(d => {
      const c = mkComp('f', { dir: 'h', main: d.main, cross: 'CENTER', pad: [0, 8], w: 150, h: 26, fill: d.fill, stroke: d.stroke, r: 'radius/sm' });
      add(c, T1('NF1/Label', d.t === 'Number' ? '24' : 'Value', d.tc, { name: 'Label' }));
      return c;
    });
    V1.set.field = combine('NF1/Field', comps, s.sh, defs.map(d => ({ Type: d.t })));
    labelProperty(V1.set.field, 'Label', 'Value');

    const sel = mkComp('NF1/Select', { dir: 'h', main: 'SPACE_BETWEEN', cross: 'CENTER', pad: [0, 8, 0, 10], w: 220, h: 28, fill: 'color/bg/header', stroke: 'color/border/strong', r: 'radius/sm' });
    add(sel, T1('NF1/Label', 'Sort: Name', 'color/text/primary', { name: 'Label' }));
    add(sel, icon('tridown', 16));
    s.sh.appendChild(sel); V1.c.select = sel;
    s.done();
  });

  // ---- Slider
  s = flow.shelf('Slider', 0);
  guard('v1 block 5', () => {
    const sl = mkComp('NF1/Slider', { w: 100, h: 20 });
    add(sl, mkRect('Track', { w: 100, h: 4, fill: 'color/border/strong', r: 2, x: 0, y: 8 }));
    add(sl, mkRect('Value', { w: 50, h: 4, fill: 'color/accent/primary', r: 2, x: 0, y: 8 }));
    add(sl, mkRect('Thumb', { w: 14, h: 20, fill: 'color/bg/header', stroke: 'color/border/strong', r: 3, x: 43, y: 0 }));
    s.sh.appendChild(sl); V1.c.slider = sl;
    s.done();
  });

  // ---- Panel tab + header
  s = flow.shelf('Panel tab / header', 0);
  guard('v1 block 6', () => {
    const comps = [true, false].map(a => {
      const c = mkComp('t', { dir: 'h', main: 'CENTER', cross: 'CENTER', pad: [0, 12], h: 28, fill: a ? 'color/bg/panel' : null, stroke: a ? 'color/accent/primary' : null, r: 2 });
      add(c, T1('NF1/Label', 'Timeline', a ? 'color/accent/primary' : 'color/text/primary', { name: 'Label' }));
      return c;
    });
    V1.set.tab = combine('NF1/Panel Tab', comps, s.sh, [{ State: 'Active' }, { State: 'Inactive' }]);
    labelProperty(V1.set.tab, 'Label', 'Timeline');

    const hd = mkComp('NF1/Panel Header', { dir: 'h', cross: 'CENTER', gap: 6, pad: [0, 8, 0, 6], w: 360, h: 30, fill: 'color/bg/header', stroke: 'color/border/default', sides: 'b' });
    add(hd, icon('grip', 16, 'color/text/muted'));
    add(hd, make(V1.set.tab, { State: 'Active' }));
    const sp = mkFrame('Spacer', { w: 4, h: 4 });
    add(hd, sp, { grow: true });
    add(hd, V1.c.overflow.createInstance());
    s.sh.appendChild(hd); V1.c.panelHeader = hd;
    s.done();
  });

  // ---- Section header
  s = flow.shelf('Section header', 1);
  guard('v1 block 7', () => {
    const comps = [true, false].map(open => {
      const c = mkComp('s', { dir: 'h', cross: 'CENTER', gap: 6, w: 260, h: 28 });
      add(c, icon(open ? 'tridown' : 'triright', 16, 'color/text/primary'));
      add(c, T1('NF1/Label', 'Stage', 'color/text/primary', { name: 'Label' }));
      return c;
    });
    V1.set.section = combine('NF1/Section Header', comps, s.sh, [{ State: 'Expanded' }, { State: 'Collapsed' }]);
    labelProperty(V1.set.section, 'Label', 'Section');
    s.done();
  });

  // ---- Tree item
  s = flow.shelf('Library tree item', 1);
  guard('v1 block 8', () => {
    const defs = [
      { t: 'Folder', open: true, ic: 'folder', sel: true }, { t: 'Folder', open: true, ic: 'folder', sel: false },
      { t: 'Folder Collapsed', open: false, ic: null, sel: false },
      { t: 'Symbol', open: null, ic: 'symbol', sel: false },
    ];
    const comps = defs.map(d => {
      const c = mkComp('i', { dir: 'h', cross: 'CENTER', gap: 6, pad: [0, 8], w: 260, h: 28, fill: d.sel ? 'color/bg/selected' : null });
      if (d.open === true) add(c, icon('tridown', 16)); else if (d.open === false) add(c, icon('triright', 16));
      else { const sp = mkFrame('Indent', { w: 16, h: 16 }); add(c, sp); }
      if (d.ic) add(c, icon(d.ic, 16));
      add(c, T1('NF1/Label', d.t === 'Symbol' ? 'Orbit' : 'Library root', 'color/text/primary', { name: 'Label' }));
      return c;
    });
    V1.set.tree = combine('NF1/Tree Item', comps, s.sh, defs.map(d => ({ Type: d.t, Selected: d.sel ? 'True' : 'False' })));
    labelProperty(V1.set.tree, 'Label', 'Item');
    s.done();
  });

  // ---- Layer row
  s = flow.shelf('Timeline layer row', 1);
  guard('v1 block 9', () => {
    const comps = [false, true].map(sel => {
      const c = mkComp('l', { dir: 'h', cross: 'CENTER', gap: 6, pad: [0, 8, 0, 6], w: 212, h: 24, fill: sel ? 'color/bg/row-selected' : null, stroke: 'color/border/cell', sides: 'b' });
      add(c, icon('layers', 16, 'color/text/muted'));
      const nm = T1('NF1/Label Small', 'Layer name', 'color/text/primary', { name: 'Label' });
      add(c, nm, { grow: true });
      add(c, icon('lock', 16, 'color/text/muted'));
      add(c, icon('eye', 16, 'color/text/primary'));
      add(c, mkRect('Color', { w: 12, h: 12, fill: 'color/layer/green' }));
      return c;
    });
    V1.set.layer = combine('NF1/Layer Row', comps, s.sh, [{ Selected: 'False' }, { Selected: 'True' }]);
    labelProperty(V1.set.layer, 'Label', 'Layer');
    s.done();
  });

  // ---- Swatch / registration / scrollbar / timeline atoms
  s = flow.shelf('Swatch · Registration cell · Scrollbar', 1);
  guard('v1 block 10', () => {
    const sw = mkComp('NF1/Swatch', { w: 24, h: 24, fill: 'color/bg/header', stroke: 'color/border/strong', r: 2 });
    add(sw, mkRect('Color', { w: 18, h: 18, fill: 'color/accent/primary', x: 3, y: 3 }));
    s.sh.appendChild(sw); V1.c.swatch = sw;

    const comps = [true, false].map(on => {
      const c = mkComp('r', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 30, h: 30, fill: on ? 'color/bg/selected' : 'color/bg/header', stroke: on ? 'color/accent/selected-border' : 'color/border/strong', r: 'radius/sm' });
      add(c, on ? mkRect('Marker', { w: 10, h: 10, fill: 'color/accent/primary' }) : mkRect('Marker', { w: 9, h: 9, stroke: 'color/text/primary', sw: 1.5 }));
      return c;
    });
    V1.set.reg = combine('NF1/Registration Cell', comps, s.sh, [{ Selected: 'True' }, { Selected: 'False' }]);

    const sb = mkComp('NF1/Scrollbar/H', { w: 300, h: 8 });
    add(sb, mkRect('Track', { w: 300, h: 8, fill: 'color/scrollbar', r: 4 }));
    s.sh.appendChild(sb); V1.c.scrollbar = sb;
    s.done();
  });

  // ---- Menu item, caption, dialog header
  s = flow.shelf('Menu item · Dialog header', 1);
  guard('v1 block 11', () => {
    const mi = mkComp('NF1/Menu Item', { dir: 'h', cross: 'CENTER', pad: [0, 8], h: 28 });
    add(mi, T1('NF1/Label', 'File', 'color/text/primary', { name: 'Label' }));
    s.sh.appendChild(mi); V1.c.menuItem = mi;

    const defs = [{ st: 'Centered', fill: 'color/bg/dialog-header', main: 'CENTER' }, { st: 'Plain', fill: 'color/bg/field', main: 'MIN' }];
    const comps = defs.map(d => {
      const c = mkComp('d', { dir: 'h', main: d.main, cross: 'CENTER', pad: [0, 16], w: 480, h: 44, fill: d.fill, stroke: 'color/border/default', sides: 'b' });
      add(c, T1('NF1/Dialog Title', 'Dialog title', 'color/text/primary', { name: 'Label' }));
      return c;
    });
    V1.set.dialogHeader = combine('NF1/Dialog Header', comps, s.sh, defs.map(d => ({ Style: d.st })));
    labelProperty(V1.set.dialogHeader, 'Label', 'Title');
    s.done();
  });

  // ---- Code editor
  s = flow.shelf('Code editor (Actions)', 1);
  guard('v1 block 12', () => {
    const ed = mkComp('NF1/Code Editor', { dir: 'v', pad: [8, 10], w: 560, h: 220, fill: 'color/bg/field', stroke: 'color/accent/primary', sw: 2, r: 2 });
    const code = T1('NF1/Code', 'trace("NeoFlash: the stage is ready");', 'color/text/primary', { name: 'Label' });
    add(ed, code);
    const caret = mkRect('Caret', { w: 2, h: 20, fill: 'color/accent/primary' });
    add(ed, caret, { abs: true });
    caret.x = 10 + code.width; caret.y = 8;
    s.sh.appendChild(ed); V1.c.codeEditor = ed;
    s.done();
  });

  // ---- Timeline atoms (wide shelf)
  const FW = 12.9, FRAMES = 120;
  V1.FW = FW; V1.FRAMES = FRAMES;
  const wide = shelf(page, 'Timeline · Frame cell / Row / Ruler / Playhead', 0, flow.bottom() + 20, 1760);
  guard('v1 block 13', () => {
    const cells = [false, true].map(shaded => {
      const c = mkComp('c', { w: FW, h: 24, fill: shaded ? 'color/bg/shaded' : 'color/bg/field', stroke: 'color/border/cell', sides: 'r' });
      return c;
    });
    V1.set.cell = combine('NF1/Frame Cell', cells, wide, [{ Shaded: 'False' }, { Shaded: 'True' }]);

    const rows = [false, true].map(sel => {
      const r = mkComp('r', { w: FW * FRAMES, h: 24, fill: sel ? 'color/bg/row-selected' : 'color/bg/field' });
      for (let i = 0; i < FRAMES; i++) {
        const shaded = (i + 1) % 5 === 0;
        const inst = make(V1.set.cell, { Shaded: shaded ? 'True' : 'False' });
        inst.x = i * FW; inst.y = 0;
        put(r, inst);
      }
      put(r, mkRect('Row line', { w: FW * FRAMES, h: 1, fill: 'color/border/default', y: 23, x: 0 }));
      put(r, mkEllipse('Keyframe', { w: 7, fill: 'color/text/primary', x: 3, y: 8.5 }));
      put(r, mkRect('End marker', { w: 7, h: 9, stroke: 'color/text/primary', x: FW * FRAMES - 10, y: 7 }));
      return r;
    });
    V1.set.row = combine('NF1/Frame Row', rows, wide, [{ Selected: 'False' }, { Selected: 'True' }], 1700);

    const rl = mkComp('NF1/Ruler', { w: FW * FRAMES + 40, h: 30, fill: 'color/bg/header' });
    for (let f = 5; f <= FRAMES; f += 5) {
      const t = T1('NF1/Ruler', String(f), 'color/text/muted', { x: (f - 1) * FW - 3, y: 16 });
      put(rl, t);
    }
    [1, 2, 3, 4].forEach(sec => put(rl, T1('NF1/Ruler', sec + 's', 'color/text/muted', { x: (sec * 24 - 1) * FW - 2, y: 2 })));
    wide.appendChild(rl); V1.c.ruler = rl;

    const ph = mkComp('NF1/Playhead', { w: 12, h: 174 });
    add(ph, mkRect('Line', { w: 2, h: 174, fill: 'color/timeline/playhead', x: 5, y: 0 }));
    add(ph, mkRect('Head', { w: 12, h: 16, fill: 'color/timeline/playhead', r: [2, 2, 6, 6], x: 0, y: 0 }));
    wide.appendChild(ph); V1.c.playhead = ph;
  });

  V1.icons = ICON_COMPS;
  return page;
}
// ============================================================================
// v1 — screens assembled from NF1 components (2560 x 1440 css px, as in the screenshot @2x)
// ============================================================================

function v1Inst(set, props, label, w, h) {
  const i = make(set, props);
  if (label != null) setLabel(i, label);
  if (w != null) i.resize(w, h != null ? h : i.height);
  return i;
}
function v1Btn(label, style, w) { return v1Inst(V1.set.button, { Style: style || 'Default' }, label, w); }
function v1Tool(name, selected) {
  const i = make(V1.set.tool, { State: selected ? 'Selected' : 'Default' });
  setIcon(i, name, 16, selected ? 'color/accent/primary' : null);
  return i;
}
function v1Row(name, o) {
  return mkFrame(name, Object.assign({ dir: 'h', cross: 'CENTER', gap: 8 }, o || {}));
}
function v1Label(str, color, style) { return T1(style || 'NF1/Label', str, color || 'color/text/primary'); }

function v1Panel(name, x, y, w, h, o) {
  return mkFrame(name, Object.assign({ fill: 'color/bg/panel', stroke: 'color/border/default', w, h, x, y, clip: true }, o || {}));
}

function v1ActionTab(label, active) { return v1Inst(V1.set.tab, { State: active ? 'Active' : 'Inactive' }, label); }

// custom header with several tabs (bottom dock)
function v1TabsHeader(labels, activeIdx, w) {
  const hd = mkFrame('Panel Header (tabs)', { dir: 'h', cross: 'CENTER', gap: 6, pad: [0, 8, 0, 6], w: w, h: 30, fill: 'color/bg/header', stroke: 'color/border/default', sides: 'b' });
  add(hd, icon('grip', 16, 'color/text/muted'));
  labels.forEach((l, i) => add(hd, v1ActionTab(l, i === activeIdx)));
  add(hd, mkFrame('Spacer', { w: 4, h: 4 }), { grow: true });
  add(hd, V1.c.overflow.createInstance());
  return hd;
}
function v1HeaderFor(label, w) {
  const hd = V1.c.panelHeader.createInstance();
  hd.resize(w, 30);
  const t = hd.findOne(n => n.type === 'TEXT' && n.name === 'Label' && n.characters === 'Timeline');
  if (t) t.characters = label;
  return hd;
}

function v1LabeledRow(label, node, labelW, muted) {
  const r = v1Row('Row / ' + label, { gap: 8 });
  const l = T1('NF1/Label', label, muted ? 'color/text/disabled' : 'color/text/primary', { w: labelW || 90 });
  add(r, l);
  add(r, node);
  return r;
}

// ------------------------------------------------------------------ stage ---
function v1StageArt() {
  const K = 1.28; // screenshot px -> css px
  const st = mkFrame('Stage — "Ideas in motion" (user content, vector)', { w: 720, h: 452, fill: 'color/bg/stage', clip: true, shadow: { y: 2, blur: 8, a: 0.25 } });
  const A = (n) => Math.round(n * K * 10) / 10;
  const txt = (s, size, color, x, y, o) => put(st, mkText(s, Object.assign({ font: 'v1', size: size, color: color, x: A(x), y: A(y) }, o || {})));
  txt('NEOFLASH / MOTION STUDY 01', 8.5, '#6B7388', 38, 35, { ls: 4 });
  put(st, mkRect('Accent rule', { w: 31, h: 3, fill: '#F28A4B', x: A(38), y: A(64) }));
  txt('Ideas in motion.', 36, '#F2F4F8', 36, 83);
  txt('One symbol. Many possibilities.', 15, '#7F889D', 38, 130);
  txt('A familiar timeline. A modern canvas.', 15, '#7F889D', 38, 150);
  put(st, mkRect('CTA', { w: 165, h: 33, fill: '#1F2738', x: A(38), y: A(199) }));
  txt('BUILT TO BE INTERACTIVE', 8.5, '#A9B4CC', 49, 207, { ls: 3 });
  // big orbit
  const cx = A(439), cy = A(169);
  put(st, mkEllipse('Orbit ring outer', { w: 163, stroke: '#3A4258', sw: 1.2, x: cx - 81.5, y: cy - 81.5 }));
  put(st, mkEllipse('Orbit ring mid', { w: 100, stroke: '#2A3144', sw: 1, x: cx - 50, y: cy - 50 }));
  put(st, mkEllipse('Core', { w: 46, fill: '#5B8CFF', x: cx - 23, y: cy - 23 }));
  put(st, mkEllipse('Planet', { w: 17, fill: '#F28A4B', x: cx - 8.5, y: A(106) - 8.5 }));
  // small orbit
  const sx = A(324), sy = A(248);
  put(st, mkEllipse('Mini ring', { w: 49, stroke: '#2F374B', sw: 1, x: sx - 24.5, y: sy - 24.5 }));
  put(st, mkEllipse('Mini core', { w: 13, fill: '#4C6FE0', x: sx - 6.5, y: sy - 6.5 }));
  put(st, mkEllipse('Mini planet', { w: 5, fill: '#F28A4B', x: sx - 2.5, y: sy - 24 }));
  txt('CLICK THE ORBIT IN WEB PREVIEW', 8, '#5A6279', 363, 270, { ls: 2 });
  put(st, mkRect('Footer rule', { w: A(487), h: 1, fill: '#252C3D', x: A(38), y: A(299) }));
  txt('EDIT A SYMBOL  ·  SCRUB THE TIMELINE  ·  EXPORT HTML5', 8, '#5A6279', 38, 313, { ls: 2 });
  return st;
}

// ------------------------------------------------------------- main window --
function v1BuildMain() {
  const win = mkFrame('NF1 / Editor — Main window', { w: 2560, h: 1440, fill: 'color/bg/panel', clip: true });

  // menu bar
  const mb = mkFrame('Menu bar', { dir: 'h', cross: 'CENTER', gap: 4, pad: [0, 10], w: 2560, h: 28, fill: 'color/bg/panel', stroke: 'color/border/default', sides: 'b' });
  add(win, mb, { x: 0, y: 0 });
  ['NeoFlash', 'File', 'Edit', 'Modify', 'Insert', 'Control', 'Window'].forEach((m, i) => {
    const it = V1.c.menuItem.createInstance();
    it.findOne(n => n.type === 'TEXT').characters = m;
    add(mb, it);
    if (i === 0) add(mb, mkRect('Divider', { w: 1, h: 18, fill: 'color/border/default' }));
  });
  add(mb, mkFrame('Spacer', { w: 4, h: 4 }), { grow: true });
  add(mb, T1('NF1/Caps', 'Classic workspace', 'color/text/muted'));
  add(mb, mkFrame('gap', { w: 14, h: 4 }));
  const tm = V1.c.menuItem.createInstance();
  tm.findOne(n => n.type === 'TEXT').characters = 'Test Movie';
  add(mb, icon('external', 16));
  add(mb, tm);

  // ---------------- timeline panel (0,28 .. 297 high)
  const tp = v1Panel('Timeline panel', 0, 28, 2560, 297);
  add(win, tp, { x: 0, y: 28 });
  add(tp, v1HeaderFor('Timeline', 2560), { x: 0, y: 0 });

  const ctl = v1Row('Timeline controls', { gap: 8, h: 28 });
  add(tp, ctl, { x: 18, y: 34 });
  add(ctl, v1Btn('Scene 1', 'Accent'));
  add(ctl, v1Btn('24 fps'));
  add(ctl, v1Btn('5.00 s', 'Default', 76));
  add(ctl, v1Label('1 / 120 F · 0.00 s', 'color/text/primary', 'NF1/Label Small'));
  const zm = make(V1.set.tool, { State: 'Default' }); setIcon(zm, 'minus', 16); zm.resize(24, 24);
  add(ctl, zm);
  add(ctl, V1.c.slider.createInstance());
  add(ctl, v1Label('Zoom', 'color/text/primary', 'NF1/Label Small'));
  const zp = make(V1.set.tool, { State: 'Default' }); setIcon(zp, 'plus', 16); zp.resize(24, 24);
  add(ctl, zp);
  add(ctl, v1Btn('Fit'));

  // layers column
  const lh = mkFrame('Layers header', { dir: 'h', cross: 'CENTER', pad: [0, 8], w: 211, h: 30, fill: 'color/bg/header', stroke: 'color/border/default', sides: 'br' });
  add(tp, lh, { x: 0, y: 66 });
  add(lh, T1('NF1/Label Small', 'Layers', 'color/text/muted'), { });
  lh.itemSpacing = 0;
  lh.primaryAxisAlignItems = 'SPACE_BETWEEN';
  const hi = v1Row('Header icons', { gap: 6 });
  add(hi, icon('lock', 16, 'color/text/muted'));
  add(hi, icon('eye', 16, 'color/text/muted'));
  add(hi, mkRect('Outline', { w: 12, h: 12, stroke: 'color/text/muted', sw: 1.2 }));
  add(lh, hi);

  const layers = [['Orbit instances', 'color/layer/green', false], ['Accents', 'color/layer/red', false], ['Typography', 'color/layer/blue', true]];
  layers.forEach((l, i) => {
    const r = make(V1.set.layer, { Selected: l[2] ? 'True' : 'False' });
    setLabel(r, l[0]);
    r.findOne(n => n.name === 'Color').fills = [solid(l[1])];
    add(tp, r, { x: 0, y: 96 + i * 24 });
  });
  add(tp, mkFrame('Layers empty', { w: 211, h: 150, fill: 'color/bg/panel', stroke: 'color/border/default', sides: 'r', x: 0, y: 168 }), { x: 0, y: 168 });
  tp.children[tp.children.length - 1].resize(211, 78);

  // frames area
  const ruler = V1.c.ruler.createInstance();
  add(tp, ruler, { x: 211, y: 66 });
  for (let i = 0; i < 6; i++) {
    const r = make(V1.set.row, { Selected: i === 2 ? 'True' : 'False' });
    if (i > 2) {
      r.findOne(n => n.name === 'Keyframe').visible = false;
      r.findOne(n => n.name === 'End marker').visible = false;
    }
    add(tp, r, { x: 211, y: 96 + i * 24 });
  }
  const ph = V1.c.playhead.createInstance();
  add(tp, ph, { x: 211 + 6.5 - 6, y: 66 });
  const sb = V1.c.scrollbar.createInstance();
  sb.resize(2560 - 211 - 10, 8);
  add(tp, sb, { x: 211, y: 246 });

  // bottom bar
  const bb = v1Row('Timeline options', { gap: 8, h: 30 });
  add(tp, bb, { x: 12, y: 262 });
  add(bb, v1Tool('plus', false));
  add(bb, v1Tool('trash', false));
  add(bb, mkFrame('gap', { w: 4, h: 4 }));
  add(bb, v1Tool('first', false));
  add(bb, v1Tool('play', false));
  add(bb, v1Btn('Actions'));
  add(bb, mkFrame('gap', { w: 4, h: 4 }));
  add(bb, v1Inst(V1.set.checkbox, { Checked: 'False' }, 'Onion Skin'));
  add(bb, v1Inst(V1.set.checkbox, { Checked: 'False' }, 'Outlines'));
  add(bb, v1Label('Before', null, 'NF1/Label'));
  add(bb, v1Inst(V1.set.field, { Type: 'Focus' }, '3', 34));
  add(bb, v1Label('After'));
  add(bb, v1Inst(V1.set.field, { Type: 'Text' }, '3', 34));
  add(bb, v1Inst(V1.set.checkbox, { Checked: 'True' }, 'Anchor'));
  add(bb, V1.c.slider.createInstance());
  add(bb, v1Label('Opacity'));

  // splitter handle
  add(win, mkRect('Splitter grip', { w: 60, h: 4, fill: 'color/border/default', r: 2 }), { x: 1250, y: 326 });

  // ---------------- tools panel
  const TW = 136;
  const tools = v1Panel('Tools panel', 0, 330, TW, 1110);
  add(win, tools, { x: 0, y: 330 });
  add(tools, v1HeaderFor('Tools', TW), { x: 0, y: 0 });
  const tc = mkFrame('Tool groups', { dir: 'v', gap: 6, pad: [10, 0, 0, 14], w: TW, h: 880 });
  tc.primaryAxisSizingMode = 'AUTO';
  add(tools, tc, { x: 0, y: 30 });
  const group = (title, rows) => {
    add(tc, T1('NF1/Caption', title, 'color/text/muted'));
    rows.forEach(r => {
      const rr = v1Row('Tools row', { gap: 6 });
      r.forEach((nm, i) => add(rr, v1Tool(nm, nm === 'select')));
      add(tc, rr);
    });
  };
  group('Select', [['select', 'subselect'], ['transform', 'lasso']]);
  group('Draw', [['pen', 'text'], ['line', 'rect'], ['oval', 'pencil'], ['brush', 'stamp'], ['bucket', 'eyedropper'], ['eraser', 'gradient']]);
  group('View', [['hand', 'zoomin'], ['zoomout', 'fit']]);

  const fs = mkFrame('Fill / Stroke / Options', { dir: 'v', gap: 6, pad: [0, 0, 0, 14], w: TW });
  add(tools, fs, { x: 0, y: 1110 - 130 });
  const lbl = v1Row('Fill Stroke labels', { gap: 20 });
  add(lbl, T1('NF1/Caption', 'Fill', 'color/text/primary'));
  add(lbl, T1('NF1/Caption', 'Stroke', 'color/text/primary'));
  add(fs, lbl);
  const sw = v1Row('Swatches', { gap: 8 });
  const sFill = V1.c.swatch.createInstance(); sFill.findOne(n => n.name === 'Color').fills = [solid('#2D5DAE')];
  const sStroke = V1.c.swatch.createInstance(); sStroke.findOne(n => n.name === 'Color').fills = [solid('#141B2E')];
  add(sw, sFill); add(sw, sStroke);
  add(fs, sw);
  add(fs, v1Btn('Options', 'Default', 72));

  // ---------------- stage panel
  const SX = TW + 6, SY = 330, SW = 2281 - SX, SH = 1251 - 330;
  const stage = mkFrame('Stage panel', { w: SW, h: SH, fill: 'color/bg/canvas', clip: true });
  add(win, stage, { x: SX, y: SY });
  add(stage, T1('NF1/Label Small', 'Ideas in motion', 'color/text/primary'), { x: 10, y: 8 });
  const zr = v1Row('Stage zoom', { gap: 6 });
  add(zr, v1Btn('Fit stage'));
  add(zr, T1('NF1/Label Small', '75%', 'color/text/primary'));
  add(stage, zr, { x: SW - 142, y: 6 });
  add(stage, v1StageArt(), { x: Math.round((SW - 720) / 2), y: 236 });
  add(stage, T1('NF1/Label Small', 'Ready · F5 frame · F6 keyframe · F7 blank keyframe · ⌘□ Test Movie', 'color/text/primary'), { x: 8, y: SH - 24 });

  // ---------------- bottom dock
  const bd = v1Panel('Bottom dock', SX, 1254, SW, 186);
  add(win, bd, { x: SX, y: 1254 });
  add(bd, v1TabsHeader(['Actions', 'Accessibility', 'Output'], 2, SW), { x: 0, y: 0 });
  add(bd, T1('NF1/Output', 'NeoFlash development preview. Editor playback shows authored animation; Test Movie executes scripts in the browser.', 'color/text/primary'), { x: 10, y: 42 });

  // ---------------- right dock: library
  const RX = 2288, RW = 2560 - RX;
  const lib = v1Panel('Library panel', RX, 330, RW, 656);
  add(win, lib, { x: RX, y: 330 });
  add(lib, v1HeaderFor('Library', RW), { x: 0, y: 0 });
  const lc = mkFrame('Library body', { dir: 'v', gap: 10, pad: [12, 8, 0, 8], w: RW });
  add(lib, lc, { x: 0, y: 30 });
  const sr = v1Row('Search row', { gap: 8 });
  add(sr, v1Inst(V1.set.field, { Type: 'Placeholder' }, 'Search library', 198));
  add(sr, v1Btn('Clear', 'Disabled'));
  add(lc, sr);
  const so = v1Row('Sort row', { gap: 8 });
  const sel = V1.c.select.createInstance(); sel.resize(198, 28);
  add(so, sel);
  add(so, v1Btn('A–Z'));
  add(lc, so);
  const tree = mkFrame('Tree', { dir: 'v', gap: 0, w: RW - 16 });
  add(lc, tree);
  const ti = (type, sel, label, indent) => {
    const t = make(V1.set.tree, { Type: type, Selected: sel ? 'True' : 'False' });
    setLabel(t, label);
    if (indent) { const w = mkFrame('indent', { dir: 'h', pad: [0, 0, 0, indent], w: RW - 16 }); return [w, t]; }
    return [null, t];
  };
  const treeRow = (type, sel, label, indent) => {
    const [w, t] = ti(type, sel, label, indent);
    if (w) { add(w, t); add(tree, w, { fillW: true }); w.primaryAxisSizingMode = 'FIXED'; w.counterAxisSizingMode = 'AUTO'; }
    else add(tree, t, { fillW: true });
  };
  treeRow('Folder', true, 'Library root');
  treeRow('Symbol', false, 'Orbit', 24);
  treeRow('Symbol', false, 'Spark', 24);
  treeRow('Folder Collapsed', false, 'Resource files (0)');
  const lbtn = v1Row('Library actions', { gap: 8 });
  const li = make(V1.set.tool, { State: 'Default' }); setIcon(li, 'symbol', 16);
  add(lbtn, li); add(lbtn, v1Tool('plus', false)); add(lbtn, v1Btn('Import…')); add(lbtn, v1Btn('More'));
  add(lib, lbtn, { x: 8, y: 656 - 46 });

  // ---------------- right dock: properties
  const pp = v1Panel('Properties panel', RX, 998, RW, 442);
  add(win, pp, { x: RX, y: 998 });
  add(pp, v1HeaderFor('Properties', RW), { x: 0, y: 0 });
  const pc = mkFrame('Properties body', { dir: 'v', gap: 8, pad: [10, 8, 0, 8], w: RW });
  add(pp, pc, { x: 0, y: 30 });
  const tabs = v1Row('Selection / Tool', { gap: 10 });
  add(tabs, v1Btn('Selection', 'Accent'));
  add(tabs, v1ActionTab('Tool', false));
  add(pc, tabs);
  add(pc, T1('NF1/Label', 'Document', 'color/text/primary'));
  const sec = (label, open) => add(pc, v1Inst(V1.set.section, { State: open ? 'Expanded' : 'Collapsed' }, label));
  const indent = (node) => { const w = mkFrame('indent', { dir: 'h', pad: [0, 0, 0, 22] }); add(w, node); add(pc, w); };
  sec('Stage', true);
  indent(v1LabeledRow('Name', v1Inst(V1.set.field, { Type: 'Text' }, 'Ideas in motion', 160), 60));
  const wh = v1Row('W H', { gap: 8 });
  add(wh, T1('NF1/Label', 'W', 'color/text/primary', { w: 52 }));
  add(wh, v1Inst(V1.set.field, { Type: 'Text' }, '960', 94));
  add(wh, T1('NF1/Label', 'H', 'color/text/primary'));
  add(wh, v1Inst(V1.set.field, { Type: 'Text' }, '600', 94));
  indent(wh);
  const bg = v1Row('Background', { gap: 8 });
  const bsw = V1.c.swatch.createInstance(); bsw.findOne(n => n.name === 'Color').fills = [solid('#131824')];
  add(bg, T1('NF1/Label', 'Background', 'color/text/primary', { w: 84 }));
  add(bg, bsw);
  add(bg, T1('NF1/Label Small', '#131824', 'color/text/primary'));
  indent(bg);
  indent(v1LabeledRow('Alpha', v1Inst(V1.set.field, { Type: 'Number' }, '100 %', 94), 84));
  sec('Playback', true);
  indent(v1LabeledRow('FPS', v1Inst(V1.set.field, { Type: 'Number' }, '24', 94), 84));
  indent(v1LabeledRow('Frames', v1Inst(V1.set.field, { Type: 'Number' }, '120', 94), 84));
  sec('Publish', true);
  indent(T1('NF1/Label Small', 'HTML5 · Rust / WebAssembly', 'color/text/primary'));
  indent(v1Btn('Publish HTML5…', 'Default', 152));

  return win;
}

// ---------------------------------------------------------- Actions panel ---
function v1BuildActions() {
  const f = mkFrame('NF1 / Panel — Actions', { dir: 'v', w: 645, fill: 'color/bg/panel', stroke: 'color/border/default', clip: true });
  add(f, v1TabsHeaderSingle('Actions', 645), { fillW: true });
  const body = mkFrame('Body', { dir: 'v', gap: 10, pad: [10, 10, 10, 10], w: 645 });
  add(f, body, { fillW: true });
  const tb = v1Row('Script bar', { gap: 8 });
  const sel1 = V1.c.select.createInstance(); sel1.resize(480, 32); setLabel(sel1, 'Scene 1 · Frame 1 · script');
  const sel2 = V1.c.select.createInstance(); sel2.resize(100, 32); setLabel(sel2, 'Jav…');
  add(tb, sel1); add(tb, sel2); add(tb, V1.c.overflow.createInstance());
  add(body, tb);
  const ed = V1.c.codeEditor.createInstance();
  ed.resize(625, 400);
  add(body, ed);
  return f;
}
function v1TabsHeaderSingle(label, w) {
  const hd = V1.c.panelHeader.createInstance();
  hd.resize(w, 30);
  const t = hd.findOne(n => n.type === 'TEXT' && n.name === 'Label' && n.characters === 'Timeline');
  if (t) t.characters = label;
  return hd;
}

// ----------------------------------------------------------------- dialogs --
function v1DialogShell(name, w) {
  return mkFrame(name, { dir: 'v', w: w, fill: 'color/bg/field', stroke: 'color/border/strong', r: 6, clip: true, shadow: { y: 8, blur: 28, a: 0.35 } });
}

function v1BuildUnsaved() {
  const d = v1DialogShell('Dialog / Unsaved changes', 520);
  add(d, v1Inst(V1.set.dialogHeader, { Style: 'Centered' }, 'Unsaved changes', 520, 52), { fillW: true });
  const body = mkFrame('Body', { dir: 'v', gap: 14, pad: [18, 18, 20, 18], w: 520 });
  add(d, body, { fillW: true });
  add(body, T1('NF1/Dialog Body', 'Save changes to this project before continuing?', 'color/text/primary'));
  const br = v1Row('Buttons', { gap: 12 });
  ['Save', 'Discard', 'Cancel'].forEach(l => add(br, v1Btn(l, 'Default', null)));
  add(body, br);
  return d;
}

function v1BuildConvert() {
  const d = v1DialogShell('Dialog / Convert to Symbol', 552);
  add(d, v1Inst(V1.set.dialogHeader, { Style: 'Plain' }, 'Convert to Symbol', 552, 48), { fillW: true });
  const body = mkFrame('Body', { dir: 'v', gap: 10, pad: [12, 14, 14, 14], w: 552 });
  add(d, body, { fillW: true });
  const L = 112;
  add(body, v1LabeledRow('Name', v1Inst(V1.set.field, { Type: 'Text' }, 'Badge symbol', 380, 34), L));
  const ty = V1.c.select.createInstance(); ty.resize(380, 34); setLabel(ty, 'Movie Clip');
  add(body, v1LabeledRow('Type', ty, L));
  const fo = V1.c.select.createInstance(); fo.resize(262, 34); setLabel(fo, 'Library root');
  add(body, v1LabeledRow('Folder', fo, L));
  const grid = mkFrame('Registration grid', { dir: 'v', gap: 4 });
  for (let r = 0; r < 3; r++) {
    const row = v1Row('r' + r, { gap: 4 });
    for (let c = 0; c < 3; c++) add(row, v1Inst(V1.set.reg, { Selected: r === 0 && c === 0 ? 'True' : 'False' }));
    add(grid, row);
  }
  const rr = mkFrame('Row / Registration', { dir: 'h', gap: 8 });
  add(rr, T1('NF1/Label', 'Registration', 'color/text/primary', { w: L }));
  add(rr, grid);
  add(body, rr);
  add(body, T1('NF1/Label', 'Movie Clips have an independent timeline.', 'color/text/primary'));
  add(body, v1Inst(V1.set.section, { State: 'Expanded' }, 'ActionScript linkage'));
  const lk = mkFrame('Linkage', { dir: 'v', gap: 10, pad: [0, 0, 0, 14], w: 500 });
  const lkInner = mkFrame('Linkage inner', { dir: 'v', gap: 10, pad: [0, 0, 0, 14], stroke: 'color/border/default', sides: 'l' });
  add(lk, lkInner);
  add(lkInner, v1Inst(V1.set.checkbox, { Checked: 'False' }, 'Export for ActionScript'));
  add(lkInner, v1LabeledRow('Identifier', v1Inst(V1.set.field, { Type: 'Placeholder' }, 'Badge symbol', 262, 34), 96, true));
  add(lkInner, v1LabeledRow('Class', v1Inst(V1.set.field, { Type: 'Placeholder' }, '', 262, 34), 96, true));
  add(lkInner, T1('NF1/Label', 'Define the optional class and its base class in Project files.', 'color/text/primary'));
  add(body, lk);
  add(body, mkRect('Divider', { w: 524, h: 1, fill: 'color/border/default' }));
  const fr = mkFrame('Footer', { dir: 'h', main: 'MAX', gap: 12, w: 524, h: 40 });
  fr.counterAxisAlignItems = 'CENTER';
  add(fr, v1Btn('Cancel', 'Default', 108)); add(fr, v1Btn('Convert', 'Default', 108));
  add(body, fr, { fillW: true });
  return d;
}

function v1WithOverlay(main, dialog, name) {
  const c = main.clone();
  c.name = name;
  add(c, mkRect('Overlay', { w: 2560, h: 1440, fill: 'color/overlay' }), { x: 0, y: 0 });
  add(c, dialog, { x: Math.round((2560 - dialog.width) / 2), y: Math.round((1440 - dialog.height) / 2) });
  return c;
}

function v1BuildScreens(page) {
  CTX = V1.tokens;
  ICON_PREFIX = 'NF1 Icon'; ICON_COMPS = V1.icons;
  const head = mkText('NF1 — Interface (v1, as built)', { font: 'i600', size: 32, color: '#111111' });
  page.appendChild(head); head.x = 0; head.y = -90;
  const sub = mkText('Assembled only from NF1 components: no bitmaps, no pasted screenshots. Frames are 2560×1440 (the screenshot is @2x).', { font: 'i400', size: 14, color: '#666666' });
  page.appendChild(sub); sub.x = 0; sub.y = -40;

  const main = v1BuildMain();
  page.appendChild(main); main.x = 0; main.y = 0;
  V1.main = main;

  const act = v1BuildActions();
  page.appendChild(act); act.x = 2660; act.y = 0;

  const un = v1BuildUnsaved();
  const d1 = v1WithOverlay(main, un, 'NF1 / Editor — Unsaved changes dialog');
  page.appendChild(d1); d1.x = 0; d1.y = 1560;

  const cv = v1BuildConvert();
  const d2 = v1WithOverlay(main, cv, 'NF1 / Editor — Convert to Symbol dialog');
  page.appendChild(d2); d2.x = 2660; d2.y = 1560;
}
// ============================================================================
// UX/UI audit page — plain documentation frames (neutral colours, not part of v1 or v2 systems)
// ============================================================================

const AUDIT = [
  // n, severity, area, problem, decision(v2), marker css-coords on v1 window (or null)
  [1, 'High', 'Иерархия действий', '«Test Movie» — мелкая ссылка в углу, «Publish HTML5…» спрятан в самом низу панели Properties. Главного действия на экране нет.', 'В верхней панели: «Test Movie» (вторичная) и «Publish» (главная, акцентная). Название проекта и признак несохранённых изменений — по центру.', [2400, 14]],
  [2, 'High', 'Таймлайн: управление', 'Сцена, FPS и длительность — вверху, Play и Onion Skin — в нижней строке, зум — в третьей. Транспорт (play) прижат к кнопкам «добавить/удалить слой».', 'Одна панель управления над сеткой: ⏮ ▶ ⏭ + таймкод + сцена / FPS / длительность + зум справа.', [700, 52]],
  [3, 'Med', 'Колонка слоёв', 'Заголовки колонок — три иконки без подписей и тултипов; под тремя слоями ~60% пустого места; кнопки «+ / корзина» оторваны от списка.', 'Кнопки «добавить / удалить слой» — в шапке колонки слоёв; строки 32px, цветная точка слоя, lock/eye с тултипами; ghost-строка «Add layer».', [150, 109]],
  [4, 'High', 'Нижняя строка таймлайна', '«Onion Skin ☐ Outlines ☐ Before [3] After [3] ■ Anchor ──○── Opacity» — чекбоксы, поля и слайдер в одной линии без группировки; порядок «Anchor [слайдер] Opacity» читается двусмысленно.', 'Onion skin — одна кнопка-переключатель с поповером: Before / After (степперы), Outlines, Anchor, Opacity (слайдер с подписью и значением).', [1000, 305]],
  [5, 'High', 'Палитра инструментов', '20 иконок 28px подряд, без подписей и горячих клавиш; названия групп (Select / Draw / View) — мелкий серый текст; tooltip не предусмотрен.', 'Рейл 56px: группы разделены линиями, активный инструмент — акцентный, tooltip «Название + клавиша». Zoom/Hand — в плавающей панели над сценой.', [68, 520]],
  [6, 'Med', 'Типографика и контраст', 'Базовый текст 14px, вторичный 11–12px. Неактивный текст #9CA2B0 на #EAECF1 даёт контраст 2.16:1 (норма AA — 4.5:1); «muted» #666D80 — 4.58:1, на грани.', 'Шкала 11/12/13/14/16/18, минимум 12px для текста. Токены text/primary, secondary, tertiary, disabled — все ≥ 4.5:1 (кроме disabled, он исключён нормой).', null],
  [7, 'Med', 'Поверхности и рамки', 'Каждая кнопка, поле, вкладка и панель обведены рамкой 1px #6D7690. Панель, поле и кнопка выглядят одинаково — нет уровней глубины, много визуального шума.', 'Уровни поверхностей bg/app → panel → raised → field; рамки только как разделители; радиусы 6 / 8 / 12 из токенов.', null],
  [8, 'Med', 'Поля свойств', 'FPS / Frames / Alpha выглядят как кнопки (серо-синий фон), Name / W / H — белые поля. Нет единиц измерения, степперов, scrub по лейблу. Подписи не выровнены.', 'Единый Field: префикс-лейбл внутри (W, H, FPS), единицы в значении, состояния hover / focus / disabled. Размер — парой W × H со значком связи.', [2420, 1180]],
  [9, 'Med', 'Библиотека', 'Поиск + Clear + Sort + A–Z занимают две строки; дерево — 4 строки; остальные ~500px пустые. Нет пустого состояния.', 'Поиск со встроенным clear, сортировка — иконка-меню в шапке; футер с действиями; место под превью символа и empty state.', [2420, 520]],
  [10, 'Low', 'Нижний док', 'Actions / Accessibility / Output — отдельная панель 186px, постоянно открытая, при этом показывает одну строку текста.', 'Один док с вкладками: Timeline · Actions · Output · Accessibility. Output открывается по требованию.', [1200, 1330]],
  [11, 'High', 'Диалог «Unsaved changes»', 'Save / Discard / Cancel равнозначны: нет главной кнопки, деструктивное «Discard» не выделено; заголовок серый по центру, в другом диалоге — белый слева (два стиля одного компонента).', 'Единый Dialog: Save — primary, Discard — danger-ghost слева, Cancel — secondary. Подпись последствий, Esc / Enter.', null],
  [12, 'Med', 'Диалог «Convert to Symbol»', 'Восемь полей в одну колонку; ActionScript linkage раскрыт по умолчанию; disabled-поля (Identifier, Class) читаются как активные; Registration 3×3 без пояснения; Cancel и Convert одного веса.', 'Основное сверху (Name, Type — сегменты, Folder), linkage свёрнут как Advanced, Registration с подписью, Convert — primary.', null],
  [13, 'Low', 'Статус-строка', 'Горячие клавиши и подсказки в одной серой строке 12px; в конце — битый глиф «⌘□ Test Movie».', 'Статус-бар 24px: подсказки состояния слева, горячие клавиши — чипами (F5 / F6 / F7) справа.', [900, 1228]],
  [14, 'Med', 'Доступность и темы', 'Выбор показан цветом и рамкой 1px, фокус едва заметен, зоны клика 24–28px, тёмной темы нет.', 'Focus-ring 2px, hit-area ≥ 32px, две темы (Dark / Light) на переменных одной коллекции.', null],
];

function auditBuild(page, v1Main) {
  CTX = null;
  const INK = '#15171C', MUTED = '#5E6573', LINE = '#E1E4EA', BG = '#FFFFFF';
  const sevColor = { High: ['#FDE8E8', '#B42318'], Med: ['#FEF3E2', '#B54708'], Low: ['#E8F0FE', '#2B50C4'] };
  const T = (s, size, color, o) => mkText(s, Object.assign({ font: 'i400', size, color }, o || {}));

  const root = mkFrame('Audit / v1 → v2', { dir: 'v', gap: 40, pad: 64, w: 2560, fill: '#F6F7F9' });
  page.appendChild(root);

  add(root, T('Аудит интерфейса NeoFlash v1', 44, INK, { font: 'i600' }));
  add(root, T('Что мешает работе в текущем интерфейсе и как это решается в версии 2. Номера на скриншоте совпадают с номерами в таблице.', 16, MUTED, { w: 1400 }));

  // summary strip
  const sum = mkFrame('Summary', { dir: 'h', gap: 20 });
  add(root, sum);
  const stat = (n, label, col) => {
    const c = mkFrame('Stat', { dir: 'v', gap: 4, pad: [16, 24], fill: BG, stroke: LINE, r: 12, w: 260 });
    add(c, T(String(n), 36, col, { font: 'i600' }));
    add(c, T(label, 13, MUTED));
    add(sum, c);
  };
  const cnt = k => AUDIT.filter(a => a[1] === k).length;
  stat(AUDIT.length, 'находок всего', INK);
  stat(cnt('High'), 'критичных (High)', '#B42318');
  stat(cnt('Med'), 'средних (Med)', '#B54708');
  stat(cnt('Low'), 'низких (Low)', '#2B50C4');
  stat('2.16:1', 'контраст disabled-текста (норма 4.5:1)', '#B42318');
  stat('20', 'кнопок-инструментов без подписей и tooltip', INK);

  // body: annotated screenshot + table
  const body = mkFrame('Body', { dir: 'h', gap: 48, cross: 'MIN' });
  add(root, body);

  // annotated v1 (vector clone, scaled down)
  const left = mkFrame('Annotated v1', { dir: 'v', gap: 16 });
  add(body, left);
  add(left, T('v1 — карта проблем', 20, INK, { font: 'i600' }));
  const holder = mkFrame('v1 preview', { w: 1280, h: 720, fill: '#FFFFFF', stroke: LINE, r: 6, clip: true });
  add(left, holder);
  const clone = v1Main.clone();
  clone.name = 'v1 preview (clone, 50%)';
  holder.appendChild(clone);
  clone.rescale(0.5);
  clone.x = 0; clone.y = 0;
  AUDIT.forEach(a => {
    if (!a[5]) return;
    const m = mkFrame('Marker ' + a[0], { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 28, h: 28, fill: '#E5484D', stroke: '#FFFFFF', sw: 2, r: 14 });
    add(m, T(String(a[0]), 13, '#FFFFFF', { font: 'i600' }));
    add(holder, m, { x: a[5][0] / 2 - 14, y: a[5][1] / 2 - 14 });
  });
  add(left, T('Без маркера — проблема «сквозная» (типографика, рамки, диалоги, доступность).', 13, MUTED, { w: 1280 }));

  // findings table
  const right = mkFrame('Findings', { dir: 'v', gap: 10, w: 1100 });
  add(body, right);
  add(right, T('Находки и решения', 20, INK, { font: 'i600' }));
  AUDIT.forEach(a => {
    const row = mkFrame('Finding ' + a[0], { dir: 'h', gap: 14, pad: 14, w: 1100, fill: BG, stroke: LINE, r: 10 });
    add(right, row);
    const badge = mkFrame('Badge', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 28, h: 28, fill: '#E5484D', r: 14 });
    add(badge, T(String(a[0]), 13, '#FFFFFF', { font: 'i600' }));
    add(row, badge);
    const col = mkFrame('Text', { dir: 'v', gap: 6, w: 1010 });
    add(row, col, { grow: true });
    const head = mkFrame('Head', { dir: 'h', gap: 10, cross: 'CENTER' });
    add(col, head);
    add(head, T(a[2], 15, INK, { font: 'i600' }));
    const chip = mkFrame('Severity', { dir: 'h', pad: [2, 8], fill: sevColor[a[1]][0], r: 10 });
    add(chip, T(a[1], 11, sevColor[a[1]][1], { font: 'i600' }));
    add(head, chip);
    add(col, T('Проблема: ' + a[3], 13, MUTED, { w: 1000, lh: 19 }), {});
    add(col, T('v2: ' + a[4], 13, INK, { w: 1000, lh: 19 }), {});
  });

  // principles
  const pr = mkFrame('Principles', { dir: 'v', gap: 12, pad: 28, w: 2432, fill: BG, stroke: LINE, r: 12 });
  add(root, pr);
  add(pr, T('Принципы v2', 24, INK, { font: 'i600' }));
  const grid = mkFrame('Principles grid', { dir: 'h', gap: 24 });
  add(pr, grid);
  [
    ['Один главный путь', 'На экране всегда ясно главное действие: Publish, Play, Save. Остальное — вторичное и не конкурирует по весу.'],
    ['Группировка по задаче', 'Транспорт, onion skin, зум, слои — собраны в логические группы, а не разбросаны по строкам.'],
    ['Поверхности вместо рамок', 'Глубину задают уровни фона, а не рамка у каждого контрола.'],
    ['Системность', 'Свои переменные (цвет, отступ, радиус), текстовые стили и компоненты NF2; две темы на одной коллекции.'],
    ['Доступность', 'Контраст ≥ 4.5:1, focus-ring 2px, зоны клика ≥ 32px, подсказки с горячими клавишами.'],
    ['Изоляция от v1', 'Ни одна переменная, стиль или компонент v2 не ссылается на v1 — изменения в одной версии не затрагивают другую.'],
  ].forEach(p => {
    const c = mkFrame('Principle', { dir: 'v', gap: 6, w: 380 });
    add(c, T(p[0], 15, INK, { font: 'i600' }));
    add(c, T(p[1], 13, MUTED, { w: 380, lh: 19 }));
    add(grid, c);
  });
  return root;
}
// ============================================================================
// v2 — new design system "NF2": own variables (Dark/Light), own text styles, own components.
// Nothing here references NF1 (no shared variables, styles or components).
// ============================================================================

const V2 = { c: {}, set: {}, s: {}, tokens: null, icons: null };
const T2 = (name, str, color, o) => styled(V2.s, name, str, color, o);

function textProp(set, propName, nodeName, def) {
  try {
    const key = set.addComponentProperty(propName, 'TEXT', def);
    set.children.forEach(v => {
      const t = v.findOne(n => n.type === 'TEXT' && n.name === nodeName);
      if (t) t.componentPropertyReferences = Object.assign({}, t.componentPropertyReferences, { characters: key });
    });
    return key;
  } catch (e) { return null; }
}
function boolProp(set, propName, nodeName, def) {
  try {
    const key = set.addComponentProperty(propName, 'BOOLEAN', def);
    set.children.forEach(v => {
      const n = v.findOne(x => x.name === nodeName);
      if (n) n.componentPropertyReferences = Object.assign({}, n.componentPropertyReferences, { visible: key });
    });
    return key;
  } catch (e) { return null; }
}
function setProp(inst, propName, value) {
  const k = Object.keys(inst.componentProperties || {}).find(x => x.indexOf(propName + '#') === 0);
  if (k) { inst.setProperties({ [k]: value }); return true; }
  return false;
}

function v2BuildTokens() {
  V2.tokens = createTokens('NF2 · Tokens', ['Dark', 'Light'], {
    'color/bg/app': ['#0D1016', '#EEF0F5'],
    'color/bg/panel': ['#12151C', '#F6F7FA'],
    'color/bg/raised': ['#171B24', '#FFFFFF'],
    'color/bg/field': ['#0D1016', '#FFFFFF'],
    'color/bg/hover': ['#1E2330', '#E9ECF3'],
    'color/bg/selected': ['#1F2A48', '#E3EBFF'],
    'color/bg/canvas': ['#0A0C11', '#DADDE6'],
    'color/bg/stage': ['#131824', '#131824'],
    'color/border/subtle': ['#232938', '#E1E4EC'],
    'color/border/default': ['#2E3547', '#CFD4E0'],
    'color/border/strong': ['#444D63', '#A9B0C2'],
    'color/border/focus': ['#6C93FF', '#3C6BF0'],
    'color/text/primary': ['#E7EAF2', '#171B26'],
    'color/text/secondary': ['#9AA3B6', '#5B6477'],
    'color/text/tertiary': ['#8790A4', '#667086'],
    'color/text/disabled': ['#566073', '#A2A9B8'],
    'color/text/on-accent': ['#0B1020', '#FFFFFF'],
    'color/text/inverse': ['#0D1016', '#F6F7FA'],
    'color/accent/default': ['#6C93FF', '#3C6BF0'],
    'color/accent/hover': ['#86A5FF', '#2F59D6'],
    'color/accent/subtle': ['#1F2A48', '#E3EBFF'],
    'color/brand/orange': ['#FF9A5C', '#E8731F'],
    'color/danger/default': ['#FF6B6B', '#D92D2D'],
    'color/danger/subtle': ['#3A1D22', '#FDE8E8'],
    'color/success/default': ['#3DD68C', '#168A57'],
    'color/warning/default': ['#F5B94A', '#B26A00'],
    'color/warning/subtle': ['#38300F', '#FEF3D6'],
    'color/layer/green': ['#3DD68C', '#168A57'],
    'color/layer/red': ['#FF6B6B', '#D92D2D'],
    'color/layer/blue': ['#6C93FF', '#3C6BF0'],
    'color/timeline/cell': ['#12151C', '#FFFFFF'],
    'color/timeline/cell-alt': ['#151922', '#F6F7FA'],
    'color/timeline/line': ['#1C2130', '#E8EBF2'],
    'color/timeline/row-selected': ['#1A2236', '#E8EEFF'],
    'color/timeline/playhead': ['#FF5C6C', '#E5334A'],
    'color/syntax/function': ['#82AAFF', '#2F5BD8'],
    'color/syntax/string': ['#FFB482', '#B4561A'],
    'color/overlay': ['#05070BB3', '#10131A66'],
    'color/tooltip/bg': ['#E7EAF2', '#171B26'],
    'space/1': [4], 'space/2': [8], 'space/3': [12], 'space/4': [16], 'space/5': [24], 'space/6': [32],
    'radius/sm': [4], 'radius/md': [6], 'radius/lg': [8], 'radius/xl': [12], 'radius/full': [999],
  });
  CTX = V2.tokens;
  V2.s = createTextStyles({
    'NF2/Heading': { font: 'i600', size: 20, lh: 28 },
    'NF2/Title': { font: 'i600', size: 16, lh: 24 },
    'NF2/Body': { font: 'i400', size: 13, lh: 20 },
    'NF2/Body Strong': { font: 'i500', size: 13, lh: 20 },
    'NF2/Small': { font: 'i400', size: 12, lh: 16 },
    'NF2/Small Strong': { font: 'i500', size: 12, lh: 16 },
    'NF2/Overline': { font: 'i600', size: 11, lh: 16, ls: 6, upper: true },
    'NF2/Mono': { font: 'mono2', size: 12, lh: 20 },
    'NF2/Mono Small': { font: 'mono2', size: 11, lh: 14 },
    'NF2/Dialog Title': { font: 'i600', size: 18, lh: 26 },
    'NF2/Dialog Body': { font: 'i400', size: 14, lh: 22 },
  });
}

// ------------------------------------------------------------ component kit --
function v2Icon(name, size, color) { return icon(name, size, color); }
function v2IconWrap(name, size, color, nodeName) {
  const w = mkFrame(nodeName || 'Icon', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: size, h: size });
  add(w, icon(name, size, color));
  return w;
}

function v2Make(set, props, label) {
  const i = make(set, props);
  if (label != null && !setProp(i, 'Label', label)) {
    const t = i.findOne(n => n.type === 'TEXT' && n.name === 'Label');
    if (t) t.characters = label;
  }
  return i;
}

function v2BuildComponents(page) {
  CTX = V2.tokens;
  const C = V2.c;
  const title = mkText('NF2 — Design system (v2)', { font: 'i600', size: 32, color: '#111111' });
  page.appendChild(title); title.x = 0; title.y = 0;
  const sub = mkText('New, independent system: own variables (NF2 · Tokens, modes Dark / Light), own text styles (NF2/*), own icons and components. Nothing references NF1.', { font: 'i400', size: 14, color: '#666666' });
  page.appendChild(sub); sub.x = 0; sub.y = 52;

  // ----------------------------------------------------------- foundations --
  v2BuildFoundations(page);

  // ----------------------------------------------------------------- icons --
  ICON_PREFIX = 'NF2 Icon';
  const flowY0 = 2300;
  const SH2 = { fill: 'color/bg/app', stroke: 'color/border/subtle', titleColor: 'color/text/primary' };
  const iconShelf = shelf(page, 'Icons · 16 and 20 px', 1540, flowY0, 700, SH2);
  const iconGrid = mkFrame('Icon grid', { dir: 'h', gap: 10, wrap: true, w: 660 });
  iconGrid.layoutMode = 'HORIZONTAL'; iconGrid.layoutWrap = 'WRAP';
  add(iconShelf, iconGrid, { fillW: true });
  buildIconSet([16, 20], Object.keys(ICONS), 'color/text/secondary', iconGrid, 1.4);
  V2.icons = ICON_COMPS;

  const flow = makeFlow(page, 700, 40, flowY0, SH2);

  // -------------------------------------------------------------- buttons --
  let s = flow.shelf('Button', 0);
  guard('v2 block 1', () => {
    const types = {
      Primary: { fill: 'color/accent/default', text: 'color/text/on-accent', hover: 'color/accent/hover', stroke: null },
      Secondary: { fill: 'color/bg/raised', text: 'color/text/primary', hover: 'color/bg/hover', stroke: 'color/border/default' },
      Ghost: { fill: null, text: 'color/text/secondary', hover: 'color/bg/hover', stroke: null },
      Danger: { fill: 'color/danger/subtle', text: 'color/danger/default', hover: 'color/danger/subtle', stroke: null, hoverStroke: 'color/danger/default' },
    };
    const comps = [], props = [];
    Object.keys(types).forEach(tn => ['Default', 'Hover', 'Disabled'].forEach(st => {
      const d = types[tn];
      const dis = st === 'Disabled';
      const fill = st === 'Hover' ? d.hover : (dis && d.fill ? 'color/bg/hover' : d.fill);
      const stroke = st === 'Hover' && d.hoverStroke ? d.hoverStroke : (dis ? (d.stroke ? 'color/border/subtle' : null) : d.stroke);
      const tc = dis ? 'color/text/disabled' : (st === 'Hover' && tn === 'Ghost' ? 'color/text/primary' : d.text);
      const c = mkComp('b', { dir: 'h', main: 'CENTER', cross: 'CENTER', gap: 'space/2', pad: [0, 'space/3'], h: 32, fill: fill, stroke: stroke, r: 'radius/md' });
      add(c, v2IconWrap('plus', 16, tc, 'Icon'));
      add(c, T2('NF2/Body Strong', 'Button', tc, { name: 'Label' }));
      comps.push(c); props.push({ Type: tn, State: st });
    }));
    V2.set.button = combine('NF2/Button', comps, s.sh, props, 660);
    textProp(V2.set.button, 'Label', 'Label', 'Button');
    boolProp(V2.set.button, 'Icon', 'Icon', false);
    s.done();
  });

  s = flow.shelf('Icon button', 0);
  guard('v2 block 2', () => {
    const types = {
      Ghost: { fill: null, ic: 'color/text/secondary', hover: 'color/bg/hover', hic: 'color/text/primary', r: 'radius/md' },
      Selected: { fill: 'color/accent/subtle', ic: 'color/accent/default', hover: 'color/bg/selected', hic: 'color/accent/hover', r: 'radius/md' },
      Primary: { fill: 'color/accent/default', ic: 'color/text/on-accent', hover: 'color/accent/hover', hic: 'color/text/on-accent', r: 'radius/full' },
    };
    const comps = [], props = [];
    Object.keys(types).forEach(tn => ['SM', 'MD'].forEach(sz => ['Default', 'Hover'].forEach(st => {
      const d = types[tn];
      const box = sz === 'SM' ? 32 : 40, isz = sz === 'SM' ? 16 : 20;
      const c = mkComp('i', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: box, h: box, fill: st === 'Hover' ? d.hover : d.fill, r: d.r });
      add(c, icon('select', isz, st === 'Hover' ? d.hic : d.ic));
      comps.push(c); props.push({ Type: tn, Size: sz, State: st });
    })));
    V2.set.iconButton = combine('NF2/Icon Button', comps, s.sh, props, 660);
    s.done();
  });

  s = flow.shelf('Segmented control', 0);
  guard('v2 block 3', () => {
    const comps = [true, false].map(on => {
      const c = mkComp('s', { dir: 'h', main: 'CENTER', cross: 'CENTER', pad: [0, 'space/3'], h: 28, fill: on ? 'color/bg/raised' : null, stroke: on ? 'color/border/default' : null, r: 'radius/sm' });
      add(c, T2('NF2/Small Strong', 'Segment', on ? 'color/text/primary' : 'color/text/secondary', { name: 'Label' }));
      return c;
    });
    V2.set.segment = combine('NF2/Segment', comps, s.sh, [{ Selected: 'True' }, { Selected: 'False' }], 660);
    textProp(V2.set.segment, 'Label', 'Label', 'Segment');
    const track = mkComp('NF2/Segmented Control', { dir: 'h', gap: 2, pad: 2, fill: 'color/bg/field', stroke: 'color/border/subtle', r: 'radius/md' });
    add(track, make(V2.set.segment, { Selected: 'True' }));
    add(track, make(V2.set.segment, { Selected: 'False' }));
    s.sh.appendChild(track); C.segmented = track;
    s.done();
  });

  // ---------------------------------------------------------------- fields --
  s = flow.shelf('Field · Search · Select', 0);
  guard('v2 block 4', () => {
    const defs = [
      { t: 'Default', stroke: 'color/border/default', sw: 1, tc: 'color/text/primary' },
      { t: 'Hover', stroke: 'color/border/strong', sw: 1, tc: 'color/text/primary' },
      { t: 'Focus', stroke: 'color/border/focus', sw: 2, tc: 'color/text/primary' },
      { t: 'Disabled', stroke: 'color/border/subtle', sw: 1, tc: 'color/text/disabled' },
    ];
    const comps = defs.map(d => {
      const c = mkComp('f', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 10], w: 160, h: 32, fill: d.t === 'Disabled' ? 'color/bg/panel' : 'color/bg/field', stroke: d.stroke, sw: d.sw, r: 'radius/md' });
      add(c, T2('NF2/Small', 'W', 'color/text/tertiary', { name: 'Prefix' }));
      add(c, T2('NF2/Body', '960', d.tc, { name: 'Label' }), { grow: true });
      return c;
    });
    V2.set.field = combine('NF2/Field', comps, s.sh, defs.map(d => ({ State: d.t })), 660);
    textProp(V2.set.field, 'Label', 'Label', '960');
    textProp(V2.set.field, 'Prefix', 'Prefix', 'W');
    boolProp(V2.set.field, 'Show prefix', 'Prefix', true);

    const sdefs = [{ st: 'Empty', tc: 'color/text/tertiary', x: false }, { st: 'Filled', tc: 'color/text/primary', x: true }];
    const scomps = sdefs.map(d => {
      const c = mkComp('s', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 10], w: 240, h: 32, fill: 'color/bg/field', stroke: d.x ? 'color/border/focus' : 'color/border/default', sw: d.x ? 2 : 1, r: 'radius/md' });
      add(c, icon('search', 16, 'color/text/tertiary'));
      add(c, T2('NF2/Body', d.x ? 'orbit' : 'Search library', d.tc, { name: 'Label' }), { grow: true });
      if (d.x) add(c, icon('x', 16, 'color/text/secondary'));
      return c;
    });
    V2.set.search = combine('NF2/Search', scomps, s.sh, sdefs.map(d => ({ State: d.st })), 660);
    textProp(V2.set.search, 'Label', 'Label', 'Search');

    const sel = [{ st: 'Default', stroke: 'color/border/default', tc: 'color/text/primary' }, { st: 'Hover', stroke: 'color/border/strong', tc: 'color/text/primary' }, { st: 'Disabled', stroke: 'color/border/subtle', tc: 'color/text/disabled' }];
    const selc = sel.map(d => {
      const c = mkComp('s', { dir: 'h', main: 'SPACE_BETWEEN', cross: 'CENTER', pad: [0, 'space/2', 0, 'space/3'], w: 200, h: 32, fill: 'color/bg/raised', stroke: d.stroke, r: 'radius/md' });
      add(c, T2('NF2/Body', 'Movie Clip', d.tc, { name: 'Label' }));
      add(c, icon('chevdown', 16, d.tc));
      return c;
    });
    V2.set.select = combine('NF2/Select', selc, s.sh, sel.map(d => ({ State: d.st })), 660);
    textProp(V2.set.select, 'Label', 'Label', 'Option');
    s.done();
  });

  s = flow.shelf('Checkbox · Slider · Swatch · Badge', 0);
  guard('v2 block 5', () => {
    const comps = [false, true].map(on => {
      const c = mkComp('c', { dir: 'h', cross: 'CENTER', gap: 'space/2' });
      const box = mkFrame('Box', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 18, h: 18, fill: on ? 'color/accent/default' : 'color/bg/field', stroke: on ? null : 'color/border/strong', r: 5 });
      if (on) add(box, icon('check', 16, 'color/text/on-accent'));
      add(c, box);
      add(c, T2('NF2/Body', 'Label', 'color/text/primary', { name: 'Label' }));
      return c;
    });
    V2.set.checkbox = combine('NF2/Checkbox', comps, s.sh, [{ Checked: 'False' }, { Checked: 'True' }], 660);
    textProp(V2.set.checkbox, 'Label', 'Label', 'Checkbox');

    const sl = mkComp('NF2/Slider', { w: 140, h: 20 });
    add(sl, mkRect('Track', { w: 140, h: 4, fill: 'color/border/default', r: 2, x: 0, y: 8 }));
    add(sl, mkRect('Value', { w: 70, h: 4, fill: 'color/accent/default', r: 2, x: 0, y: 8 }));
    add(sl, mkEllipse('Thumb', { w: 14, fill: 'color/text/primary', stroke: 'color/bg/panel', sw: 2, x: 63, y: 3 }));
    s.sh.appendChild(sl); C.slider = sl;

    const sw = mkComp('NF2/Swatch', { w: 22, h: 22, stroke: 'color/border/default', r: 6, fill: 'color/bg/field', clip: true });
    add(sw, mkRect('Color', { w: 22, h: 22, fill: 'color/bg/stage' }));
    s.sh.appendChild(sw); C.swatch = sw;

    const rcs = [true, false].map(on => {
      const c = mkComp('r', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 28, h: 28, fill: on ? 'color/accent/subtle' : 'color/bg/field', stroke: on ? 'color/accent/default' : 'color/border/default', r: 'radius/md' });
      add(c, mkEllipse('Marker', { w: on ? 8 : 6, fill: on ? 'color/accent/default' : 'color/border/strong' }));
      return c;
    });
    V2.set.reg = combine('NF2/Registration Cell', rcs, s.sh, [{ Selected: 'True' }, { Selected: 'False' }], 660);

    const bt = ['Neutral', 'Accent', 'Warning'];
    const bcomps = bt.map(t => {
      const fill = t === 'Neutral' ? 'color/bg/hover' : t === 'Accent' ? 'color/accent/subtle' : 'color/warning/subtle';
      const tc = t === 'Neutral' ? 'color/text/secondary' : t === 'Accent' ? 'color/accent/default' : 'color/warning/default';
      const c = mkComp('b', { dir: 'h', cross: 'CENTER', gap: 6, pad: [2, 8], h: 22, fill: fill, r: 'radius/full' });
      add(c, mkEllipse('Dot', { w: 6, fill: tc }));
      add(c, T2('NF2/Small Strong', 'Badge', tc, { name: 'Label' }));
      return c;
    });
    V2.set.badge = combine('NF2/Badge', bcomps, s.sh, bt.map(t => ({ Type: t })), 660);
    textProp(V2.set.badge, 'Label', 'Label', 'Badge');
    s.done();
  });

  // ------------------------------------------------------------ navigation --
  s = flow.shelf('Dock tab · Section header · Panel header', 1);
  guard('v2 block 6', () => {
    const defs = [{ st: 'Active', tc: 'color/text/primary', ic: 'color/accent/default', fill: null, line: true }, { st: 'Inactive', tc: 'color/text/secondary', ic: 'color/text/tertiary', fill: null, line: false }, { st: 'Hover', tc: 'color/text/primary', ic: 'color/text/secondary', fill: 'color/bg/hover', line: false }];
    const comps = defs.map(d => {
      const c = mkComp('t', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 14], h: 40, fill: d.fill, stroke: d.line ? 'color/accent/default' : null, sw: 2, sides: 'b' });
      add(c, icon('film', 16, d.ic));
      add(c, T2('NF2/Body Strong', 'Timeline', d.tc, { name: 'Label' }));
      return c;
    });
    V2.set.tab = combine('NF2/Dock Tab', comps, s.sh, defs.map(d => ({ State: d.st })), 660);
    textProp(V2.set.tab, 'Label', 'Label', 'Tab');

    const secs = [true, false].map(open => {
      const c = mkComp('s', { dir: 'h', cross: 'CENTER', gap: 'space/1', w: 288, h: 32 });
      add(c, icon(open ? 'chevdown' : 'chevright', 16, 'color/text/tertiary'));
      add(c, T2('NF2/Overline', 'Section', 'color/text/secondary', { name: 'Label' }));
      return c;
    });
    V2.set.section = combine('NF2/Section Header', secs, s.sh, [{ State: 'Expanded' }, { State: 'Collapsed' }], 660);
    textProp(V2.set.section, 'Label', 'Label', 'Section');

    const ph = mkComp('NF2/Panel Header', { dir: 'h', cross: 'CENTER', pad: [0, 'space/2', 0, 'space/4'], w: 320, h: 40, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 'b' });
    add(ph, T2('NF2/Title', 'Panel', 'color/text/primary', { name: 'Label' }));
    add(ph, mkFrame('Spacer', { w: 4, h: 4 }), { grow: true });
    const more = make(V2.set.iconButton, { Type: 'Ghost', Size: 'SM', State: 'Default' });
    setIcon(more, 'more', 16, 'color/text/secondary');
    add(ph, more);
    s.sh.appendChild(ph); C.panelHeader = ph;
    s.done();
  });

  s = flow.shelf('Tree item · Layer row', 1);
  guard('v2 block 7', () => {
    const defs = [
      { t: 'Folder Open', sel: true, open: true, ic: 'folder' }, { t: 'Folder Open', sel: false, open: true, ic: 'folder' },
      { t: 'Folder Closed', sel: false, open: false, ic: 'folder' },
      { t: 'Symbol', sel: false, open: null, ic: 'symbol' }, { t: 'Symbol', sel: true, open: null, ic: 'symbol' },
    ];
    const comps = defs.map(d => {
      const c = mkComp('i', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 'space/2'], w: 288, h: 32, fill: d.sel ? 'color/bg/selected' : null, r: 'radius/md' });
      const cc = d.sel ? 'color/accent/default' : 'color/text/secondary';
      if (d.open === null) add(c, mkFrame('Indent', { w: 16, h: 16 })); else add(c, icon(d.open ? 'chevdown' : 'chevright', 16, 'color/text/tertiary'));
      add(c, icon(d.ic, 16, cc));
      add(c, T2('NF2/Body', 'Item', d.sel ? 'color/text/primary' : 'color/text/secondary', { name: 'Label' }), { grow: true });
      return c;
    });
    V2.set.tree = combine('NF2/Tree Item', comps, s.sh, defs.map(d => ({ Type: d.t, Selected: d.sel ? 'True' : 'False' })), 660);
    textProp(V2.set.tree, 'Label', 'Label', 'Item');

    const rows = [false, true].map(sel => {
      const c = mkComp('l', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 'space/2', 0, 'space/4'], w: 260, h: 32, fill: sel ? 'color/bg/selected' : null, stroke: 'color/border/subtle', sides: 'b' });
      add(c, mkEllipse('Dot', { w: 8, fill: 'color/layer/green' }));
      add(c, T2('NF2/Small Strong', 'Layer', sel ? 'color/text/primary' : 'color/text/secondary', { name: 'Label' }), { grow: true });
      add(c, icon('lock', 16, 'color/text/tertiary'));
      add(c, icon('eye', 16, 'color/text/secondary'));
      return c;
    });
    V2.set.layer = combine('NF2/Layer Row', rows, s.sh, [{ Selected: 'False' }, { Selected: 'True' }], 660);
    textProp(V2.set.layer, 'Label', 'Label', 'Layer');
    s.done();
  });

  // ------------------------------------------------- tooltip / kbd / dialog --
  s = flow.shelf('Kbd · Tooltip · Dialog header · Zoom control · Logo', 1);
  guard('v2 block 8', () => {
    const kbd = mkComp('NF2/Kbd', { dir: 'h', main: 'CENTER', cross: 'CENTER', pad: [0, 6], h: 20, fill: 'color/bg/raised', stroke: 'color/border/default', r: 'radius/sm' });
    add(kbd, T2('NF2/Mono Small', 'F6', 'color/text/secondary', { name: 'Label' }));
    s.sh.appendChild(kbd); C.kbd = kbd;

    const tip = mkComp('NF2/Tooltip', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [6, 10], fill: 'color/tooltip/bg', r: 'radius/md', shadow: { y: 4, blur: 12, a: 0.3 } });
    add(tip, T2('NF2/Small Strong', 'Selection', 'color/text/inverse', { name: 'Label' }));
    const key = mkFrame('Key', { dir: 'h', main: 'CENTER', cross: 'CENTER', pad: [0, 5], h: 18, fill: 'color/text/inverse', fillOp: 0.14, r: 4 });
    add(key, T2('NF2/Mono Small', 'V', 'color/text/inverse', { name: 'Key' }));
    add(tip, key);
    s.sh.appendChild(tip); C.tooltip = tip;

    const dh = mkComp('NF2/Dialog Header', { dir: 'h', cross: 'CENTER', pad: [0, 'space/3', 0, 24], w: 520, h: 60 });
    add(dh, T2('NF2/Dialog Title', 'Dialog title', 'color/text/primary', { name: 'Label' }), { grow: true });
    const cl = make(V2.set.iconButton, { Type: 'Ghost', Size: 'SM', State: 'Default' });
    setIcon(cl, 'x', 16, 'color/text/secondary');
    add(dh, cl);
    s.sh.appendChild(dh); C.dialogHeader = dh;

    const zc = mkComp('NF2/Zoom Control', { dir: 'h', cross: 'CENTER', gap: 2, pad: 4, fill: 'color/bg/raised', stroke: 'color/border/default', r: 'radius/lg', shadow: { y: 4, blur: 16, a: 0.3 } });
    const zb = (ic) => { const b = make(V2.set.iconButton, { Type: 'Ghost', Size: 'SM', State: 'Default' }); setIcon(b, ic, 16, 'color/text/secondary'); return b; };
    add(zc, zb('hand'));
    add(zc, mkRect('Divider', { w: 1, h: 18, fill: 'color/border/default' }));
    add(zc, zb('minus'));
    add(zc, T2('NF2/Small Strong', '75%', 'color/text/primary', { name: 'Label', w: 40, align: 'CENTER' }));
    add(zc, zb('plus'));
    add(zc, mkRect('Divider', { w: 1, h: 18, fill: 'color/border/default' }));
    add(zc, zb('fit'));
    s.sh.appendChild(zc); C.zoom = zc;

    const lg = mkComp('NF2/Logo', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 28, h: 28, fill: 'color/accent/default', r: 8 });
    const lw = mkFrame('Mark', { w: 18, h: 18 });
    add(lw, mkEllipse('Ring', { w: 18, stroke: 'color/text/on-accent', sw: 1.6, x: 0, y: 0 }));
    add(lw, mkEllipse('Core', { w: 6, fill: 'color/text/on-accent', x: 6, y: 6 }));
    add(lg, lw);
    s.sh.appendChild(lg); C.logo = lg;
    s.done();
  });

  s = flow.shelf('Code editor', 1);
  guard('v2 block 9', () => {
    const ed = mkComp('NF2/Code Editor', { dir: 'h', w: 640, h: 200, fill: 'color/bg/field', stroke: 'color/border/subtle', r: 'radius/lg', clip: true });
    const gut = mkFrame('Gutter', { dir: 'v', gap: 0, pad: [8, 12], fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 'r' });
    for (let i = 1; i <= 8; i++) add(gut, T2('NF2/Mono', String(i), i === 1 ? 'color/text/secondary' : 'color/text/disabled', { w: 24, align: 'RIGHT' }));
    add(ed, gut, { });
    gut.layoutSizingVertical = 'FILL';
    const code = mkFrame('Code', { dir: 'v', gap: 0, pad: [8, 0] });
    add(ed, code, { grow: true });
    code.layoutSizingVertical = 'FILL';
    const line = mkFrame('Line 1', { dir: 'h', cross: 'CENTER', pad: [0, 12], h: 20, fill: 'color/bg/hover' });
    add(code, line, { fillW: true });
    add(line, T2('NF2/Mono', 'trace', 'color/syntax/function', { name: 'Fn' }));
    add(line, T2('NF2/Mono', '(', 'color/text/secondary'));
    add(line, T2('NF2/Mono', '"NeoFlash: the stage is ready"', 'color/syntax/string', { name: 'Str' }));
    add(line, T2('NF2/Mono', ');', 'color/text/secondary'));
    s.sh.appendChild(ed); C.codeEditor = ed;
    s.done();
  });

  // --------------------------------------------------------------- timeline --
  const FW = 13, FRAMES = 120;
  V2.FW = FW; V2.FRAMES = FRAMES;
  const wide = shelf(page, 'Timeline · Frame cell / Row / Ruler / Playhead', 0, flow.bottom() + 20, 1760, SH2);
  guard('v2 block 10', () => {
    const cells = [false, true].map(alt => mkComp('c', { w: FW, h: 32, fill: alt ? 'color/timeline/cell-alt' : 'color/timeline/cell', stroke: 'color/timeline/line', sides: 'r' }));
    V2.set.cell = combine('NF2/Frame Cell', cells, wide, [{ Alt: 'False' }, { Alt: 'True' }], 1700);

    const rows = [false, true].map(sel => {
      const r = mkComp('r', { w: FW * FRAMES, h: 32, fill: sel ? 'color/timeline/row-selected' : 'color/timeline/cell' });
      for (let i = 0; i < FRAMES; i++) {
        const alt = Math.floor(i / 5) % 2 === 1;
        const inst = make(V2.set.cell, { Alt: alt ? 'True' : 'False' });
        inst.x = i * FW; inst.y = 0;
        put(r, inst);
      }
      put(r, mkRect('Row line', { w: FW * FRAMES, h: 1, fill: 'color/timeline/line', x: 0, y: 31 }));
      put(r, mkRect('Span', { w: FW * FRAMES - 6, h: 20, fill: 'color/accent/subtle', r: 5, x: 3, y: 6 }));
      put(r, mkRect('End', { w: 3, h: 20, fill: 'color/accent/default', r: 1.5, x: FW * FRAMES - 6, y: 6 }));
      put(r, mkEllipse('Keyframe', { w: 10, fill: 'color/accent/default', stroke: 'color/bg/panel', sw: 2, x: 4, y: 11 }));
      return r;
    });
    V2.set.row = combine('NF2/Frame Row', rows, wide, [{ Selected: 'False' }, { Selected: 'True' }], 1700);

    const rl = mkComp('NF2/Ruler', { w: FW * FRAMES + 40, h: 32, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 'b' });
    for (let f = 10; f <= FRAMES; f += 10) {
      if (f % 24 === 0) continue;
      put(rl, T2('NF2/Mono Small', String(f), 'color/text/tertiary', { x: f * FW - 8, y: 16 }));
    }
    [1, 2, 3, 4, 5].forEach(sec => put(rl, T2('NF2/Small Strong', sec + 's', 'color/text/secondary', { x: sec * 24 * FW - 6, y: 3 })));
    put(rl, mkRect('Tick 0', { w: 1, h: 6, fill: 'color/border/strong', x: 0, y: 26 }));
    wide.appendChild(rl); C.ruler = rl;

    const ph = mkComp('NF2/Playhead', { w: 24, h: 220 });
    add(ph, mkRect('Line', { w: 2, h: 220, fill: 'color/timeline/playhead', x: 11, y: 0 }));
    const head = mkFrame('Head', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 24, h: 18, fill: 'color/timeline/playhead', r: 5 });
    add(head, T2('NF2/Mono Small', '1', '#FFFFFF', { name: 'Label' }));
    add(ph, head, { x: 0, y: 0 });
    wide.appendChild(ph); C.playhead = ph;
  });
  return page;
}

// ------------------------------------------------------------- foundations --
function v2BuildFoundations(page) {
  const names = Object.keys(V2.tokens.hex);
  const groups = {};
  names.forEach(n => { const g = n.split('/').slice(0, 2).join('/'); (groups[g] = groups[g] || []).push(n); });

  const makeBoard = (modeIdx, modeName, x) => {
    const board = mkFrame('Colors / ' + modeName, { dir: 'v', gap: 20, pad: 24, w: 700, fill: 'color/bg/app', stroke: 'color/border/default', r: 12, x: x, y: 120 });
    page.appendChild(board);
    board.x = x; board.y = 120;
    try { board.setExplicitVariableModeForCollection(V2.tokens.coll, V2.tokens.modeIds[modeIdx]); }
    catch (e) { try { board.setExplicitVariableModeForCollection(V2.tokens.coll.id, V2.tokens.modeIds[modeIdx]); } catch (e2) { /* ignore */ } }
    add(board, T2('NF2/Heading', 'Color — ' + modeName, 'color/text/primary'));
    Object.keys(groups).forEach(g => {
      add(board, T2('NF2/Overline', g.replace('color/', ''), 'color/text/secondary'));
      const row = mkFrame('Row', { dir: 'h', gap: 12, wrap: true, w: 650 });
      row.layoutWrap = 'WRAP';
      add(board, row, { fillW: true });
      groups[g].forEach(n => {
        const cell = mkFrame('Token', { dir: 'v', gap: 6, w: 100 });
        add(row, cell);
        add(cell, mkRect('Swatch', { w: 100, h: 44, fill: n, stroke: 'color/border/default', r: 8 }));
        add(cell, T2('NF2/Mono Small', n.split('/').slice(2).join('/'), 'color/text/secondary', { w: 100 }));
      });
    });
    return board;
  };
  makeBoard(0, 'Dark', 0);
  makeBoard(1, 'Light', 740);

  // type + spacing + radius
  const typ = mkFrame('Type scale', { dir: 'v', gap: 14, pad: 24, w: 640, fill: 'color/bg/app', stroke: 'color/border/default', r: 12, x: 1480, y: 120 });
  page.appendChild(typ); typ.x = 1480; typ.y = 120;
  add(typ, T2('NF2/Heading', 'Typography — Inter', 'color/text/primary'));
  const samples = [
    ['NF2/Heading', 'Heading 20 / 28 · Semibold', 'Ideas in motion'],
    ['NF2/Title', 'Title 16 / 24 · Semibold', 'Convert to Symbol'],
    ['NF2/Body Strong', 'Body strong 13 / 20 · Medium', 'Test Movie'],
    ['NF2/Body', 'Body 13 / 20 · Regular', 'Save changes to this project before continuing?'],
    ['NF2/Small Strong', 'Small strong 12 / 16 · Medium', 'Typography'],
    ['NF2/Small', 'Small 12 / 16 · Regular', 'Ready · stage 960 × 600'],
    ['NF2/Overline', 'Overline 11 / 16 · Semibold, caps', 'Playback'],
    ['NF2/Mono', 'Mono 12 / 20 · Roboto Mono', 'trace("NeoFlash");'],
  ];
  samples.forEach(sm => {
    const r = mkFrame('Sample', { dir: 'v', gap: 2 });
    add(r, T2(sm[0], sm[2], 'color/text/primary'));
    add(r, T2('NF2/Small', sm[1], 'color/text/tertiary'));
    add(typ, r);
  });

  const sp = mkFrame('Spacing and radius', { dir: 'v', gap: 14, pad: 24, w: 640, fill: 'color/bg/app', stroke: 'color/border/default', r: 12, x: 1480, y: 840 });
  page.appendChild(sp); sp.x = 1480; sp.y = 840;
  add(sp, T2('NF2/Heading', 'Spacing and radius', 'color/text/primary'));
  const srow = mkFrame('Spacing', { dir: 'h', gap: 16, cross: 'MAX' });
  add(sp, srow);
  ['space/1', 'space/2', 'space/3', 'space/4', 'space/5', 'space/6'].forEach(n => {
    const c = mkFrame('Space', { dir: 'v', gap: 6, cross: 'CENTER' });
    const v = V2.tokens.num[n];
    add(c, mkRect('Bar', { w: v, h: v, fill: 'color/accent/default', r: 2 }));
    add(c, T2('NF2/Mono Small', n + ' · ' + v, 'color/text/secondary'));
    add(srow, c);
  });
  const rrow = mkFrame('Radius', { dir: 'h', gap: 16 });
  add(sp, rrow);
  ['radius/sm', 'radius/md', 'radius/lg', 'radius/xl'].forEach(n => {
    const c = mkFrame('Radius', { dir: 'v', gap: 6, cross: 'CENTER' });
    const v = V2.tokens.num[n];
    const box = mkFrame('Box', { w: 56, h: 56, fill: 'color/bg/raised', stroke: 'color/border/strong', r: n });
    add(c, box);
    add(c, T2('NF2/Mono Small', n + ' · ' + v, 'color/text/secondary'));
    add(rrow, c);
  });
}
// ============================================================================
// v2 — screens assembled only from NF2 components (1920 x 1080)
// ============================================================================

const W2 = 1920, H2 = 1080, TOP = 48, RAIL = 56, INSP = 320, DOCK = 284, STATUS = 24;
const BTN_ICON_COLOR = { Primary: 'color/text/on-accent', Secondary: 'color/text/primary', Ghost: 'color/text/secondary', Danger: 'color/danger/default' };

function setText(inst, propName, nodeName, value) {
  if (!setProp(inst, propName, value)) {
    const t = inst.findOne(n => n.type === 'TEXT' && n.name === nodeName);
    if (t) t.characters = value;
  }
}

function v2Btn(label, type, ic, state) {
  const b = make(V2.set.button, { Type: type, State: state || 'Default' });
  setText(b, 'Label', 'Label', label);
  if (ic) { setProp(b, 'Icon', true); setIcon(b, ic, 16, state === 'Disabled' ? 'color/text/disabled' : BTN_ICON_COLOR[type]); }
  return b;
}
function v2IconBtn(name, type, size, state) {
  const i = make(V2.set.iconButton, { Type: type || 'Ghost', Size: size || 'SM', State: state || 'Default' });
  const col = type === 'Primary' ? 'color/text/on-accent' : type === 'Selected' ? 'color/accent/default' : 'color/text/secondary';
  setIcon(i, name, size === 'MD' ? 20 : 16, col);
  return i;
}
function v2Field(prefix, value, w, state) {
  const f = make(V2.set.field, { State: state || 'Default' });
  setText(f, 'Label', 'Label', value);
  if (prefix == null) setProp(f, 'Show prefix', false); else setText(f, 'Prefix', 'Prefix', prefix);
  if (w) f.resize(w, 32);
  return f;
}
function v2Select(label, w, state) {
  const s = make(V2.set.select, { State: state || 'Default' });
  setText(s, 'Label', 'Label', label);
  if (w) s.resize(w, 32);
  return s;
}
function v2Tab(label, ic, active) {
  const t = make(V2.set.tab, { State: active ? 'Active' : 'Inactive' });
  setText(t, 'Label', 'Label', label);
  setIcon(t, ic, 16, active ? 'color/accent/default' : 'color/text/tertiary');
  return t;
}
function v2Section(label, open) { const s = make(V2.set.section, { State: open ? 'Expanded' : 'Collapsed' }); setText(s, 'Label', 'Label', label); return s; }
function v2Divider(h) { return mkRect('Divider', { w: 1, h: h || 20, fill: 'color/border/default' }); }
function v2Spacer() { return mkFrame('Spacer', { w: 4, h: 4 }); }
function v2Panel(name, x, y, w, h, o) {
  return mkFrame(name, Object.assign({ w, h, x, y, fill: 'color/bg/panel', stroke: 'color/border/subtle', clip: true }, o || {}));
}
function v2Kbd(label) { const k = V2.c.kbd.createInstance(); k.findOne(n => n.type === 'TEXT').characters = label; return k; }
function v2Label(str) { return T2('NF2/Small Strong', str, 'color/text/secondary'); }
function v2FieldGroup(label, node) {
  const g = mkFrame('Group / ' + label, { dir: 'v', gap: 'space/1' });
  add(g, v2Label(label));
  add(g, node, { fillW: true });
  return g;
}

function v2BuildEditor(opts) {
  opts = opts || {};
  const tab = opts.dockTab || 'timeline';
  const win = mkFrame('NF2 / Editor — ' + (opts.name || 'Dark'), { w: W2, h: H2, fill: 'color/bg/app', clip: true });

  // ------------------------------------------------------------- top bar --
  const tb = mkFrame('Top bar', { dir: 'h', cross: 'CENTER', gap: 'space/1', pad: [0, 'space/3'], w: W2, h: TOP, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 'b' });
  add(win, tb, { x: 0, y: 0 });
  add(tb, V2.c.logo.createInstance());
  add(tb, T2('NF2/Title', 'NeoFlash', 'color/text/primary'));
  add(tb, mkFrame('gap', { w: 8, h: 4 }));
  ['File', 'Edit', 'Modify', 'Insert', 'Control', 'Window'].forEach(m => add(tb, v2Btn(m, 'Ghost')));
  add(tb, v2Spacer(), { grow: true });
  add(tb, v2Select('Classic workspace', 176));
  add(tb, v2Btn('Test Movie', 'Secondary', 'play'));
  add(tb, v2Btn('Publish', 'Primary', 'upload'));
  const ttl = mkFrame('Project title', { dir: 'h', cross: 'CENTER', gap: 'space/2' });
  add(ttl, T2('NF2/Body Strong', 'Ideas in motion', 'color/text/primary'));
  const bd = make(V2.set.badge, { Type: 'Warning' }); setText(bd, 'Label', 'Label', 'Unsaved');
  add(ttl, bd);
  add(tb, ttl, { abs: true });
  ttl.x = Math.round((W2 - ttl.width) / 2); ttl.y = Math.round((TOP - ttl.height) / 2);

  // ---------------------------------------------------------------- rail --
  const RH = H2 - TOP - DOCK - STATUS;
  const rail = mkFrame('Tool rail', { dir: 'v', cross: 'CENTER', gap: 2, pad: [8, 8], w: RAIL, h: RH, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 'r' });
  add(win, rail, { x: 0, y: TOP });
  const groups = [['select', 'subselect', 'transform', 'lasso'], ['pen', 'text', 'line', 'rect', 'oval', 'pencil', 'brush', 'bucket', 'eyedropper', 'eraser']];
  groups.forEach((g, gi) => {
    g.forEach(n => add(rail, v2IconBtn(n, n === 'select' ? 'Selected' : 'Ghost', 'MD')));
    if (gi < groups.length - 1) { const d = mkRect('Divider', { w: 24, h: 1, fill: 'color/border/default' }); add(rail, d); }
  });
  add(rail, v2Spacer(), { grow: true });
  const sws = mkFrame('Fill / Stroke', { dir: 'v', gap: 6, cross: 'CENTER', pad: [0, 0, 6, 0] });
  const fillSw = V2.c.swatch.createInstance(); fillSw.findOne(n => n.name === 'Color').fills = [solid('#2D5DAE')];
  const strokeSw = V2.c.swatch.createInstance(); strokeSw.findOne(n => n.name === 'Color').fills = [solid('#141B2E')];
  add(sws, fillSw); add(sws, strokeSw);
  add(rail, sws);

  // -------------------------------------------------------------- canvas --
  const CW = W2 - RAIL - INSP;
  const canvas = mkFrame('Canvas', { w: CW, h: RH, fill: 'color/bg/canvas', clip: true });
  add(win, canvas, { x: RAIL, y: TOP });
  const crumb = mkFrame('Breadcrumb', { dir: 'h', cross: 'CENTER', gap: 'space/1', pad: ['space/2', 'space/3'], fill: 'color/bg/raised', stroke: 'color/border/subtle', r: 'radius/md' });
  add(crumb, T2('NF2/Small', 'Scene 1', 'color/text/secondary'));
  add(crumb, icon('chevright', 16, 'color/text/tertiary'));
  add(crumb, T2('NF2/Small Strong', 'Ideas in motion', 'color/text/primary'));
  add(canvas, crumb, { x: 16, y: 16 });
  add(canvas, v1StageArt(), { x: Math.round((CW - 720) / 2), y: Math.round((RH - 452) / 2) - 10 });
  const zoom = V2.c.zoom.createInstance();
  add(canvas, zoom, { x: Math.round((CW - zoom.width) / 2), y: RH - 16 - zoom.height });
  const stageInfo = mkFrame('Stage info', { dir: 'h', cross: 'CENTER', gap: 'space/2' });
  const sb = make(V2.set.badge, { Type: 'Neutral' }); setText(sb, 'Label', 'Label', '960 × 600');
  add(stageInfo, sb);
  add(canvas, stageInfo, { x: CW - 16 - 100, y: 16 });

  // ----------------------------------------------------------- inspector --
  const insp = mkFrame('Inspector', { dir: 'v', w: INSP, h: RH, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 'l', clip: true });
  add(win, insp, { x: W2 - INSP, y: TOP });

  const props = mkFrame('Properties', { dir: 'v', w: INSP });
  add(insp, props, { fillW: true });
  const ph = V2.c.panelHeader.createInstance(); setText(ph, 'Label', 'Label', 'Properties');
  add(props, ph, { fillW: true });
  const pb = mkFrame('Body', { dir: 'v', gap: 'space/3', pad: 'space/4', w: INSP });
  add(props, pb, { fillW: true });
  const seg = V2.c.segmented.createInstance();
  seg.findAll(n => n.type === 'INSTANCE' && n.name.indexOf('Selected=') === 0).forEach((n, i) => setText(n, 'Label', 'Label', i === 0 ? 'Selection' : 'Tool'));
  add(pb, seg);
  add(pb, v2Section('Stage', true), { fillW: true });
  const w2 = INSP - 32;
  add(pb, v2Field('Name', 'Ideas in motion', w2), { fillW: true });
  const sz = mkFrame('Size', { dir: 'h', gap: 'space/2', cross: 'CENTER' });
  add(sz, v2Field('W', '960', 120)); add(sz, v2Field('H', '600', 120)); add(sz, v2IconBtn('link', 'Ghost', 'SM'));
  add(pb, sz);
  const bgr = mkFrame('Background', { dir: 'h', gap: 'space/2', cross: 'CENTER' });
  const bsw = V2.c.swatch.createInstance();
  add(bgr, bsw);
  add(bgr, v2Field(null, '#131824', 116)); add(bgr, v2Field('α', '100 %', 134));
  add(pb, bgr);
  add(pb, v2Section('Playback', true), { fillW: true });
  const pl = mkFrame('Playback row', { dir: 'h', gap: 'space/2' });
  add(pl, v2Field('FPS', '24', 140)); add(pl, v2Field('Frames', '120', 140));
  add(pb, pl);
  add(pb, v2Section('Publish', true), { fillW: true });
  const pub = mkFrame('Publish row', { dir: 'v', gap: 'space/2' });
  const hb = make(V2.set.badge, { Type: 'Accent' }); setText(hb, 'Label', 'Label', 'HTML5 · Rust / WebAssembly');
  add(pub, hb);
  const pbtn = v2Btn('Publish settings…', 'Secondary');
  add(pub, pbtn);
  add(pb, pub);

  const lib = mkFrame('Library', { dir: 'v', w: INSP, stroke: 'color/border/subtle', sides: 't' });
  add(insp, lib, { fillW: true, fillH: true });
  const lh = V2.c.panelHeader.createInstance(); setText(lh, 'Label', 'Label', 'Library');
  add(lib, lh, { fillW: true });
  const lb = mkFrame('Body', { dir: 'v', gap: 'space/2', pad: ['space/3', 'space/2'], w: INSP });
  add(lib, lb, { fillW: true });
  const se = make(V2.set.search, { State: 'Empty' }); se.resize(INSP - 16, 32);
  setText(se, 'Label', 'Label', 'Search library');
  add(lb, se, { fillW: true });
  const treeRow = (type, sel, label, indent) => {
    const t = make(V2.set.tree, { Type: type, Selected: sel ? 'True' : 'False' });
    setText(t, 'Label', 'Label', label);
    if (indent) {
      const w = mkFrame('indent', { dir: 'h', pad: [0, 0, 0, indent] });
      add(w, t); return w;
    }
    return t;
  };
  add(lb, treeRow('Folder Open', true, 'Library root'), { fillW: true });
  add(lb, treeRow('Symbol', false, 'Orbit', 20), { fillW: true });
  add(lb, treeRow('Symbol', false, 'Spark', 20), { fillW: true });
  add(lb, treeRow('Folder Closed', false, 'Resource files (0)'), { fillW: true });

  // ---------------------------------------------------------------- dock --
  const dock = mkFrame('Bottom dock', { dir: 'v', w: W2, h: DOCK, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 't', clip: true });
  add(win, dock, { x: 0, y: H2 - DOCK - STATUS });
  const dh = mkFrame('Dock tabs', { dir: 'h', cross: 'CENTER', pad: [0, 'space/2', 0, 'space/2'], w: W2, h: 40, stroke: 'color/border/subtle', sides: 'b' });
  add(dock, dh, { fillW: true });
  add(dh, v2Tab('Timeline', 'film', tab === 'timeline'));
  add(dh, v2Tab('Actions', 'code', tab === 'actions'));
  add(dh, v2Tab('Output', 'terminal', false));
  add(dh, v2Tab('Accessibility', 'access', false));
  add(dh, v2Spacer(), { grow: true });
  add(dh, v2IconBtn('chevdown', 'Ghost', 'SM'));
  add(dh, v2IconBtn('more', 'Ghost', 'SM'));

  const BODY = DOCK - 40;
  if (tab === 'timeline') v2Timeline(dock, BODY);
  else v2Actions(dock, BODY);

  // ----------------------------------------------------------- status bar --
  const st = mkFrame('Status bar', { dir: 'h', cross: 'CENTER', gap: 'space/3', pad: [0, 'space/4'], w: W2, h: STATUS, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 't' });
  add(win, st, { x: 0, y: H2 - STATUS });
  add(st, mkEllipse('Ready dot', { w: 6, fill: 'color/success/default' }));
  add(st, T2('NF2/Small', 'Ready', 'color/text/secondary'));
  add(st, T2('NF2/Small', 'Stage 960 × 600 · 24 fps · 120 frames', 'color/text/tertiary'));
  add(st, v2Spacer(), { grow: true });
  [['F5', 'frame'], ['F6', 'keyframe'], ['F7', 'blank keyframe'], ['⌘ ↵', 'Test Movie']].forEach(k => {
    const g = mkFrame('Hint', { dir: 'h', cross: 'CENTER', gap: 6 });
    add(g, v2Kbd(k[0]));
    add(g, T2('NF2/Small', k[1], 'color/text/tertiary'));
    add(st, g);
  });

  // tooltip demo next to the active tool
  const tip = V2.c.tooltip.createInstance();
  tip.findAll(n => n.type === 'TEXT').forEach(t => { if (t.name === 'Label') t.characters = 'Selection'; if (t.name === 'Key') t.characters = 'V'; });
  add(win, tip, { x: RAIL + 6, y: TOP + 12 });
  return win;
}

function v2Timeline(dock, BODY) {
  const FW = V2.FW, FR = V2.FRAMES;
  const LCOL = 260;
  const bar = mkFrame('Timeline controls', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 'space/3'], w: W2, h: 44, stroke: 'color/border/subtle', sides: 'b' });
  add(dock, bar, { fillW: true });
  add(bar, v2IconBtn('first', 'Ghost', 'SM'));
  add(bar, v2IconBtn('play', 'Primary', 'MD'));
  add(bar, v2IconBtn('last', 'Ghost', 'SM'));
  add(bar, v2IconBtn('loop', 'Ghost', 'SM'));
  const tc = mkFrame('Timecode', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 'space/2'] });
  add(tc, T2('NF2/Mono', '00:00.00', 'color/text/primary'));
  add(tc, T2('NF2/Mono Small', '1 / 120 F', 'color/text/tertiary'));
  add(bar, tc);
  add(bar, v2Divider());
  add(bar, v2Select('Scene 1', 120));
  add(bar, v2Field('FPS', '24', 96));
  add(bar, v2Field('Duration', '5.00 s', 150));
  add(bar, v2Spacer(), { grow: true });
  add(bar, v2Btn('Onion skin', 'Secondary', 'onion'));
  add(bar, v2IconBtn('chevdown', 'Ghost', 'SM'));
  add(bar, v2Divider());
  add(bar, v2IconBtn('minus', 'Ghost', 'SM'));
  add(bar, V2.c.slider.createInstance());
  add(bar, v2IconBtn('plus', 'Ghost', 'SM'));
  add(bar, v2Btn('Fit', 'Ghost'));

  const body = mkFrame('Timeline body', { w: W2, h: BODY - 44, clip: true });
  add(dock, body, { fillW: true });
  // layers column
  const lh = mkFrame('Layers header', { dir: 'h', cross: 'CENTER', gap: 'space/1', pad: [0, 'space/2', 0, 'space/4'], w: LCOL, h: 32, stroke: 'color/border/subtle', sides: 'br', fill: 'color/bg/panel' });
  add(body, lh, { x: 0, y: 0 });
  add(lh, T2('NF2/Overline', 'Layers', 'color/text/secondary'), { grow: true });
  add(lh, v2IconBtn('plus', 'Ghost', 'SM'));
  add(lh, v2IconBtn('trash', 'Ghost', 'SM'));
  const layers = [['Orbit instances', 'color/layer/green', false], ['Accents', 'color/layer/red', false], ['Typography', 'color/layer/blue', true]];
  layers.forEach((l, i) => {
    const r = make(V2.set.layer, { Selected: l[2] ? 'True' : 'False' });
    setText(r, 'Label', 'Label', l[0]);
    r.findOne(n => n.name === 'Dot').fills = [solid(l[1])];
    add(body, r, { x: 0, y: 32 + i * 32 });
  });
  const addRow = mkFrame('Add layer', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 'space/4'], w: LCOL, h: 32 });
  add(addRow, icon('plus', 16, 'color/text/tertiary'));
  add(addRow, T2('NF2/Small', 'Add layer', 'color/text/tertiary'));
  add(body, addRow, { x: 0, y: 32 + 3 * 32 });
  add(body, mkFrame('Layers fill', { w: LCOL, h: BODY - 44 - 160, fill: 'color/bg/panel', stroke: 'color/border/subtle', sides: 'r' }), { x: 0, y: 160 });
  // frames
  add(body, V2.c.ruler.createInstance(), { x: LCOL, y: 0 });
  for (let i = 0; i < 5; i++) {
    const r = make(V2.set.row, { Selected: i === 2 ? 'True' : 'False' });
    if (i < 3) {
      const tok = layers[i][1];
      r.findOne(n => n.name === 'Span').fills = [solid(tok, 0.22)];
      r.findOne(n => n.name === 'End').fills = [solid(tok)];
      r.findOne(n => n.name === 'Keyframe').fills = [solid(tok)];
    } else {
      ['Span', 'End', 'Keyframe'].forEach(nm => { r.findOne(n => n.name === nm).visible = false; });
    }
    add(body, r, { x: LCOL, y: 32 + i * 32 });
  }
  add(body, V2.c.playhead.createInstance(), { x: LCOL + 6.5 - 12, y: 0 });
}

function v2Actions(dock, BODY) {
  const bar = mkFrame('Script controls', { dir: 'h', cross: 'CENTER', gap: 'space/2', pad: [0, 'space/3'], w: W2, h: 44, stroke: 'color/border/subtle', sides: 'b' });
  add(dock, bar, { fillW: true });
  add(bar, v2Select('Scene 1 · Frame 1 · script', 280));
  add(bar, v2Select('JavaScript', 140));
  add(bar, v2Spacer(), { grow: true });
  add(bar, v2Btn('Format', 'Ghost', 'code'));
  add(bar, v2Btn('Run', 'Secondary', 'play'));
  const wrap = mkFrame('Editor wrap', { dir: 'v', pad: 'space/3', w: W2, h: BODY - 44 });
  add(dock, wrap, { fillW: true });
  const ed = V2.c.codeEditor.createInstance();
  ed.resize(W2 - 24, BODY - 44 - 24);
  add(wrap, ed, { fillW: true });
}

// ------------------------------------------------------------------ dialogs --
function v2DialogShell(name, w) {
  return mkFrame(name, { dir: 'v', w: w, fill: 'color/bg/raised', stroke: 'color/border/default', r: 'radius/xl', clip: true, shadow: { y: 16, blur: 48, a: 0.5 } });
}

function v2BuildUnsaved() {
  const d = v2DialogShell('NF2 / Dialog — Unsaved changes', 480);
  const body = mkFrame('Body', { dir: 'v', gap: 'space/5', pad: 'space/5', w: 480 });
  add(d, body, { fillW: true });
  const top = mkFrame('Top', { dir: 'h', gap: 'space/4' });
  add(body, top);
  const ic = mkFrame('Warning icon', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 40, h: 40, fill: 'color/warning/subtle', r: 'radius/full' });
  add(ic, T2('NF2/Title', '!', 'color/warning/default'));
  add(top, ic);
  const col = mkFrame('Text', { dir: 'v', gap: 'space/1', w: 372 });
  add(top, col, { grow: true });
  add(col, T2('NF2/Dialog Title', 'Save changes before continuing?', 'color/text/primary', { w: 372 }));
  add(col, T2('NF2/Dialog Body', 'Your changes to “Ideas in motion” will be lost if you discard them.', 'color/text/secondary', { w: 372 }));
  const ft = mkFrame('Footer', { dir: 'h', cross: 'CENTER', gap: 'space/2', w: 432 });
  add(body, ft, { fillW: true });
  add(ft, v2Btn('Discard', 'Danger'));
  add(ft, v2Spacer(), { grow: true });
  add(ft, v2Btn('Cancel', 'Secondary'));
  add(ft, v2Btn('Save', 'Primary'));
  return d;
}

function v2BuildConvert() {
  const d = v2DialogShell('NF2 / Dialog — Convert to Symbol', 520);
  const hd = V2.c.dialogHeader.createInstance(); setText(hd, 'Label', 'Label', 'Convert to Symbol');
  add(d, hd, { fillW: true });
  const body = mkFrame('Body', { dir: 'v', gap: 'space/4', pad: [0, 24, 24, 24], w: 520 });
  add(d, body, { fillW: true });
  const IW = 472;
  add(body, v2FieldGroup('Name', v2Field(null, 'Badge symbol', IW, 'Focus')), { fillW: true });
  // type segments
  const seg = mkFrame('Type segments', { dir: 'h', gap: 2, pad: 2, fill: 'color/bg/field', stroke: 'color/border/subtle', r: 'radius/md' });
  ['Movie Clip', 'Graphic', 'Button'].forEach((l, i) => { const s = make(V2.set.segment, { Selected: i === 0 ? 'True' : 'False' }); setText(s, 'Label', 'Label', l); add(seg, s); });
  add(body, v2FieldGroup('Type', seg));
  add(body, v2FieldGroup('Folder', v2Select('Library root', IW)), { fillW: true });
  // registration
  const reg = mkFrame('Registration', { dir: 'h', gap: 'space/4', cross: 'CENTER' });
  const grid = mkFrame('Grid', { dir: 'v', gap: 'space/1' });
  for (let r = 0; r < 3; r++) {
    const row = mkFrame('row', { dir: 'h', gap: 'space/1' });
    for (let c = 0; c < 3; c++) add(row, make(V2.set.reg, { Selected: r === 0 && c === 0 ? 'True' : 'False' }));
    add(grid, row);
  }
  add(reg, grid);
  add(reg, T2('NF2/Small', 'Top-left corner is the symbol’s origin for transforms.', 'color/text/tertiary', { w: 300 }));
  add(body, v2FieldGroup('Registration', reg));
  add(body, T2('NF2/Small', 'Movie Clips have an independent timeline.', 'color/text/tertiary'));
  const adv = mkFrame('Advanced', { dir: 'v', w: IW, stroke: 'color/border/subtle', sides: 't', pad: [8, 0, 0, 0] });
  add(adv, v2Section('Advanced — ActionScript linkage', false), { fillW: true });
  add(body, adv, { fillW: true });
  const ft = mkFrame('Footer', { dir: 'h', main: 'MAX', cross: 'CENTER', gap: 'space/2', w: IW });
  add(ft, v2Btn('Cancel', 'Secondary'));
  add(ft, v2Btn('Convert', 'Primary'));
  add(body, ft, { fillW: true });
  return d;
}

function v2WithOverlay(main, dialog, name) {
  const c = main.clone();
  c.name = name;
  add(c, mkRect('Overlay', { w: W2, h: H2, fill: 'color/overlay' }), { x: 0, y: 0 });
  add(c, dialog, { x: Math.round((W2 - dialog.width) / 2), y: Math.round((H2 - dialog.height) / 2) });
  return c;
}

function v2BuildScreens(page) {
  CTX = V2.tokens;
  ICON_PREFIX = 'NF2 Icon'; ICON_COMPS = V2.icons;
  const head = mkText('NF2 — Interface (v2)', { font: 'i600', size: 32, color: '#111111' });
  page.appendChild(head); head.x = 0; head.y = -90;
  const sub = mkText('Redesign based on the audit. Built only from NF2 components and NF2 · Tokens. Dark and Light are two modes of one variable collection.', { font: 'i400', size: 14, color: '#666666' });
  page.appendChild(sub); sub.x = 0; sub.y = -40;

  const main = v2BuildEditor({ name: 'Dark', dockTab: 'timeline' });
  page.appendChild(main); main.x = 0; main.y = 0;
  V2.main = main;

  // Light theme = same frame, other variable mode
  const light = main.clone();
  light.name = 'NF2 / Editor — Light';
  page.appendChild(light); light.x = 2020; light.y = 0;
  try { light.setExplicitVariableModeForCollection(V2.tokens.coll, V2.tokens.modeIds[1]); }
  catch (e) { light.setExplicitVariableModeForCollection(V2.tokens.coll.id, V2.tokens.modeIds[1]); }

  const act = v2BuildEditor({ name: 'Actions tab', dockTab: 'actions' });
  page.appendChild(act); act.x = 0; act.y = 1180;

  const d1 = v2WithOverlay(main, v2BuildUnsaved(), 'NF2 / Editor — Unsaved changes dialog');
  page.appendChild(d1); d1.x = 2020; d1.y = 1180;
  const d2 = v2WithOverlay(main, v2BuildConvert(), 'NF2 / Editor — Convert to Symbol dialog');
  page.appendChild(d2); d2.x = 0; d2.y = 2360;
}
// ============================================================================
// Orchestration — every stage is isolated, so a failure in one stage keeps the others.
// ============================================================================
(async () => {
  const errors = [];
  const all0 = () => errors.length + ERRLOG.length;
  const stage = async (label, fn) => {
    try { figma.notify(label + '…', { timeout: 2500 }); await fn(); return true; }
    catch (e) { console.error(label, e); errors.push(label + ': ' + (e && e.message ? e.message : e)); return false; }
  };
  try {
    if (figma.root.children.some(p => p.name === 'v1 · Interface' || p.name === 'v2 · Interface')) {
      figma.closePlugin('This file already contains the NeoFlash pages. Run the plugin in a new empty file.');
      return;
    }
    await loadAllFonts();

    const pC1 = figma.root.children[0];
    pC1.name = 'v1 · Components';
    const pI1 = await newPage('v1 · Interface');
    const pAu = await newPage('Audit · v1 → v2');
    const pC2 = await newPage('v2 · System & Components');
    const pI2 = await newPage('v2 · Interface');
    const show = async (p) => { await gotoPage(p); try { figma.viewport.scrollAndZoomIntoView(p.children); } catch (e) { /* cosmetic */ } };

    const okC1 = await stage('Building v1 components', async () => { await gotoPage(pC1); v1BuildTokens(); v1BuildComponents(pC1); await show(pC1); });
    const okI1 = await stage('Building v1 interface', async () => { await gotoPage(pI1); v1BuildScreens(pI1); await show(pI1); });
    await stage('Writing audit', async () => { await gotoPage(pAu); auditBuild(pAu, V1.main); await show(pAu); });
    const okC2 = await stage('Building v2 system', async () => { await gotoPage(pC2); v2BuildTokens(); v2BuildComponents(pC2); await show(pC2); });
    await stage('Building v2 interface', async () => { await gotoPage(pI2); v2BuildScreens(pI2); await show(pI2); });

    await flushStyles();
    const all = errors.concat(ERRLOG);
    if (all.length) {
      const log = await newPage('Build log');
      await gotoPage(log);
      const t = figma.createText();
      t.fontName = FONTS.i400; t.fontSize = 13;
      t.characters = all.join('\n\n');
      t.x = 0; t.y = 0;
      log.appendChild(t);
    }
    await gotoPage(okI1 ? pI1 : pC1);
    figma.closePlugin(all0() ? 'Finished with errors — see page "Build log". First: ' + (errors.concat(ERRLOG)[0] || '').split('\n')[0] : 'Done: 5 pages created (v1 components / interface, audit, v2 system / interface).');
  } catch (e) {
    console.error(e);
    figma.closePlugin('NeoFlash UI Builder failed: ' + (e && e.message ? e.message : e));
  }
})();
