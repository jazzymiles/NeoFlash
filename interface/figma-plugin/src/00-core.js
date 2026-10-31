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
