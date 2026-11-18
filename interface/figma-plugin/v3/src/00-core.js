// NF3 builder core — portable Figma Plugin API only (no MCP-only helpers).
// Every script: await nfInit() … build … await nfFlush().
const NF = { v: {}, ts: {}, icons: {}, atoms: {}, pend: [], grow: new Map(), warn: [] };
const NF_ATOMS_PAGE = 'v3 · Atoms';

async function nfInit(opts) {
  opts = opts || {};
  await Promise.all([['Inter', 'Regular'], ['Inter', 'Medium'], ['Inter', 'Semi Bold'], ['Roboto Mono', 'Regular']]
    .map(f => figma.loadFontAsync({ family: f[0], style: f[1] })));
  const cols = await figma.variables.getLocalVariableCollectionsAsync();
  NF.themeColl = cols.find(c => c.name === 'NF2 · Tokens');
  NF.densColl = cols.find(c => c.name === 'NF3 · Density');
  for (const v of await figma.variables.getLocalVariablesAsync())
    if (v.variableCollectionId === NF.themeColl.id || v.variableCollectionId === NF.densColl.id) NF.v[v.name] = v;
  for (const s of await figma.getLocalTextStylesAsync()) if (s.name.indexOf('NF2/') === 0) NF.ts[s.name.slice(4)] = s;
  NF.atomsPage = figma.root.children.find(p => p.name === NF_ATOMS_PAGE);
  if (NF.atomsPage && opts.atoms !== false) {
    await NF.atomsPage.loadAsync();
    for (const c of NF.atomsPage.findAllWithCriteria({ types: ['COMPONENT', 'COMPONENT_SET'] })) {
      if (c.name.indexOf('NF2 Icon/') === 0) NF.icons[c.name.slice(9)] = c;
      else if (c.name.indexOf('NF3/') === 0) NF.atoms[c.name.slice(4)] = c;
    }
  }
}
async function nfFlush() {
  const p = NF.pend; NF.pend = [];
  for (let i = 0; i < p.length; i += 200) await Promise.all(p.slice(i, i + 200).map(x => x[0].setTextStyleIdAsync(x[1])));
}
function nfPage(name) {
  let p = figma.root.children.find(x => x.name === name);
  if (!p) { p = figma.createPage(); p.name = name; }
  return p;
}
function nfMode(node, theme, density) {
  if (theme) node.setExplicitVariableModeForCollection(NF.themeColl, NF.themeColl.modes.find(m => m.name === theme).modeId);
  if (density) node.setExplicitVariableModeForCollection(NF.densColl, NF.densColl.modes.find(m => m.name === density).modeId);
}

// ------------------------------------------------------------ primitives ----
function V(n) { const v = NF.v[n]; if (!v) throw new Error('No variable ' + n); return v; }
function paint(tok, op) {
  const p = { type: 'SOLID', color: { r: 0.5, g: 0.5, b: 0.5 } };
  if (op != null) p.opacity = op;
  return figma.variables.setBoundVariableForPaint(p, 'color', V('color/' + tok));
}
function rawPaint(hex, op) {
  const h = hex.replace('#', '');
  const p = { type: 'SOLID', color: { r: parseInt(h.slice(0, 2), 16) / 255, g: parseInt(h.slice(2, 4), 16) / 255, b: parseInt(h.slice(4, 6), 16) / 255 } };
  if (op != null) p.opacity = op;
  return p;
}
function fillOf(x, op) { return x == null ? [] : [x[0] === '#' ? rawPaint(x, op) : paint(x, op)]; }
function bind(node, prop, val) {
  if (val == null) return;
  if (typeof val === 'string') node.setBoundVariable(prop, V(val)); else node[prop] = val;
}
function isFixed(x) { return typeof x === 'number' || (typeof x === 'string' && x !== 'fill' && x !== 'hug'); }
function markGrow(n, w, h) { if (w || h) NF.grow.set(n.id, { w: !!w, h: !!h }); }
function applyGrow(k) {
  const d = NF.grow.get(k.id); if (!d) return;
  if (d.w) k.layoutSizingHorizontal = 'FILL';
  if (d.h) k.layoutSizingVertical = 'FILL';
  NF.grow.delete(k.id);
}
function add(parent, kids) {
  for (const k of [].concat(kids || [])) { if (!k) continue; parent.appendChild(k); applyGrow(k); }
  return parent;
}
// box({name, dir:'h'|'v', w, h, gap, pad, fill, stroke, sides:[t,r,b,l], radius, align, justify, clip, component, wrap, shadow, opacity, minH}, kids)
// w / h: number | density variable name | 'fill' | 'hug' (default). pad: n | [v, h] | [t, r, b, l], items may be variable names.
function box(o, kids) {
  o = o || {};
  const f = o.component ? figma.createComponent() : figma.createFrame();
  f.name = o.name || (o.dir === 'v' ? 'Column' : 'Row');
  f.layoutMode = o.dir === 'v' ? 'VERTICAL' : 'HORIZONTAL';
  const horiz = o.dir !== 'v';
  if (typeof o.w === 'number' || typeof o.h === 'number')
    f.resize(typeof o.w === 'number' ? Math.max(o.w, 0.01) : 100, typeof o.h === 'number' ? Math.max(o.h, 0.01) : 100);
  const wMode = isFixed(o.w) ? 'FIXED' : 'AUTO', hMode = isFixed(o.h) ? 'FIXED' : 'AUTO';
  f.primaryAxisSizingMode = horiz ? wMode : hMode;
  f.counterAxisSizingMode = horiz ? hMode : wMode;
  if (typeof o.w === 'string' && isFixed(o.w)) f.setBoundVariable('width', V(o.w));
  if (typeof o.h === 'string' && isFixed(o.h)) f.setBoundVariable('height', V(o.h));
  markGrow(f, o.w === 'fill', o.h === 'fill');
  if (o.minH != null) bind(f, 'minHeight', o.minH);
  if (o.minW != null) bind(f, 'minWidth', o.minW);
  f.fills = fillOf(o.fill, o.fillOpacity);
  if (o.stroke) {
    f.strokes = fillOf(o.stroke); f.strokeAlign = 'INSIDE'; f.strokeWeight = o.strokeWeight || 1;
    if (o.sides) { f.strokeTopWeight = o.sides[0]; f.strokeRightWeight = o.sides[1]; f.strokeBottomWeight = o.sides[2]; f.strokeLeftWeight = o.sides[3]; }
  }
  bind(f, 'itemSpacing', o.gap || 0);
  let p = o.pad == null ? [0, 0, 0, 0] : [].concat(o.pad);
  if (p.length === 1) p = [p[0], p[0], p[0], p[0]]; else if (p.length === 2) p = [p[0], p[1], p[0], p[1]];
  bind(f, 'paddingTop', p[0]); bind(f, 'paddingRight', p[1]); bind(f, 'paddingBottom', p[2]); bind(f, 'paddingLeft', p[3]);
  if (o.radius != null) for (const c of ['topLeftRadius', 'topRightRadius', 'bottomLeftRadius', 'bottomRightRadius']) bind(f, c, o.radius);
  f.counterAxisAlignItems = o.align || (horiz ? 'CENTER' : 'MIN');
  f.primaryAxisAlignItems = o.justify || 'MIN';
  f.clipsContent = !!o.clip;
  if (o.wrap) { f.layoutWrap = 'WRAP'; bind(f, 'counterAxisSpacing', o.wrapGap != null ? o.wrapGap : (o.gap || 0)); }
  if (o.shadow) f.effects = [{ type: 'DROP_SHADOW', color: { r: 0, g: 0, b: 0, a: o.shadow === 'dialog' ? 0.45 : 0.35 },
    offset: { x: 0, y: o.shadow === 'dialog' ? 16 : 12 }, radius: o.shadow === 'dialog' ? 48 : 32, spread: 0, visible: true, blendMode: 'NORMAL' }];
  if (o.opacity != null) f.opacity = o.opacity;
  return add(f, kids);
}
const row = (o, kids) => box(Object.assign({ dir: 'h' }, o), kids);
const col = (o, kids) => box(Object.assign({ dir: 'v' }, o), kids);
function spacer() { const f = box({ name: 'Spacer', h: 1 }); markGrow(f, true, false); return f; }
function rect(w, h, fill, o) {
  o = o || {}; const r = figma.createRectangle(); r.name = o.name || 'Rect';
  r.resize(typeof w === 'number' ? w : 10, typeof h === 'number' ? h : 10);
  if (typeof w === 'string' && w !== 'fill') r.setBoundVariable('width', V(w));
  if (typeof h === 'string' && h !== 'fill') r.setBoundVariable('height', V(h));
  markGrow(r, w === 'fill', h === 'fill');
  r.fills = fillOf(fill, o.fillOpacity);
  if (o.stroke) { r.strokes = fillOf(o.stroke); r.strokeAlign = 'INSIDE'; r.strokeWeight = 1; }
  if (o.radius != null) { if (typeof o.radius === 'string') for (const c of ['topLeftRadius', 'topRightRadius', 'bottomLeftRadius', 'bottomRightRadius']) r.setBoundVariable(c, V(o.radius)); else r.cornerRadius = o.radius; }
  return r;
}
function dot(size, fill, o) {
  o = o || {}; const e = figma.createEllipse(); e.name = o.name || 'Dot';
  e.resize(typeof size === 'number' ? size : 10, typeof size === 'number' ? size : 10);
  if (typeof size === 'string') { e.setBoundVariable('width', V(size)); e.setBoundVariable('height', V(size)); }
  e.fills = fillOf(fill); if (o.stroke) { e.strokes = fillOf(o.stroke); e.strokeAlign = 'INSIDE'; e.strokeWeight = o.strokeWeight || 1; }
  return e;
}
// txt('Label', 'Body', 'text/primary', {grow, wrap, w, align, name})
function txt(s, style, color, o) {
  o = o || {}; const st = NF.ts[style || 'Body']; if (!st) throw new Error('No text style ' + style);
  const t = figma.createText();
  t.fontName = st.fontName; t.fontSize = st.fontSize; t.lineHeight = st.lineHeight; t.letterSpacing = st.letterSpacing; t.textCase = st.textCase;
  t.characters = String(s); t.fills = fillOf(color || 'text/primary');
  t.name = o.name || 'Text';
  NF.pend.push([t, st.id]);
  if (o.w) { t.textAutoResize = 'HEIGHT'; t.resize(o.w, t.height); }
  if (o.grow) { t.textAutoResize = 'HEIGHT'; markGrow(t, true, false); if (!o.wrap) { t.textTruncation = 'ENDING'; t.maxLines = 1; } }
  if (o.align) t.textAlignHorizontal = o.align;
  return t;
}
function tint(inst, color, comp) {
  if (!inst) return inst;
  const p = paint(color), kinds = ['VECTOR', 'ELLIPSE', 'RECTANGLE', 'LINE', 'BOOLEAN_OPERATION', 'STAR', 'POLYGON'];
  const pred = x => kinds.indexOf(x.type) >= 0;
  const ns = inst.findAll(pred), ms = comp ? comp.findAll(pred) : null;
  ns.forEach((n, i) => {
    const m = ms && ms[i] ? ms[i] : n;
    const hasS = m.strokes && m.strokes.some(f => f.visible !== false);
    const hasF = m.fills !== figma.mixed && m.fills && m.fills.some(f => f.visible !== false && (f.opacity == null || f.opacity > 0.01));
    n.strokes = hasS ? [p] : []; n.fills = hasF ? [p] : [];
  });
  return inst;
}
function iconComp(name, size) {
  size = size || 16;
  return NF.icons[size + '/' + name] || NF.icons[size + '/tool-' + name] || NF.icons['16/' + name] || NF.icons['20/' + name] || null;
}
function icon(name, color, size) {
  size = size || 16; const c = iconComp(name, size);
  if (!c) {
    NF.warn.push('icon ' + size + '/' + name);
    const f = figma.createFrame(); f.resize(size, size); f.fills = []; f.name = 'Icon/missing-' + name;
    const e = dot(size - 6, null, { stroke: color || 'text/secondary' }); f.appendChild(e); e.x = 3; e.y = 3; return f;
  }
  const i = c.createInstance(); i.name = 'Icon';
  if (color && color !== 'text/secondary') tint(i, color, c);
  return i;
}
function divider(vertical) {
  const r = figma.createRectangle(); r.name = 'Divider'; r.fills = fillOf('border/subtle');
  if (vertical) { r.resize(1, 10); markGrow(r, false, true); } else { r.resize(10, 1); markGrow(r, true, false); }
  return r;
}

// ------------------------------------------------------- component helpers ----
function variantName(props) { return Object.keys(props).map(k => k + '=' + props[k]).join(', '); }
// makeSet('Button', [{props:{Kind:'Primary',State:'Default'}, node: ComponentNode}], parent, width)
function makeSet(name, variants, parent, width) {
  for (const v of variants) v.node.name = variantName(v.props);
  const set = figma.combineAsVariants(variants.map(v => v.node), parent);
  set.name = 'NF3/' + name;
  set.layoutMode = 'HORIZONTAL'; set.layoutWrap = 'WRAP'; set.itemSpacing = 16; set.counterAxisSpacing = 16;
  set.paddingTop = set.paddingBottom = set.paddingLeft = set.paddingRight = 16;
  set.resize(width || 720, 100); set.primaryAxisSizingMode = 'FIXED'; set.counterAxisSizingMode = 'AUTO';
  set.fills = fillOf('bg/panel'); set.strokes = fillOf('border/subtle'); set.strokeWeight = 1; set.cornerRadius = 8;
  NF.atoms[name] = set;
  return set;
}
function linkText(set, prop, nodeName, def) {
  const key = set.addComponentProperty(prop, 'TEXT', def);
  for (const t of set.findAll(n => n.type === 'TEXT' && n.name === nodeName)) t.componentPropertyReferences = Object.assign({}, t.componentPropertyReferences, { characters: key });
  return key;
}
function linkBool(set, prop, nodeName, def) {
  const key = set.addComponentProperty(prop, 'BOOLEAN', def);
  for (const t of set.findAll(n => n.name === nodeName)) t.componentPropertyReferences = Object.assign({}, t.componentPropertyReferences, { visible: key });
  return key;
}
function linkSwap(set, prop, nodeName, defComp) {
  const key = set.addComponentProperty(prop, 'INSTANCE_SWAP', defComp.id);
  for (const t of set.findAll(n => n.type === 'INSTANCE' && n.name === nodeName)) t.componentPropertyReferences = Object.assign({}, t.componentPropertyReferences, { mainComponent: key });
  return key;
}
// inst('Button', {Kind:'Primary', Label:'Publish'}, {w:'fill'}) — property names without the #id suffix.
function inst(atom, props, o) {
  o = o || {}; const c = NF.atoms[atom]; if (!c) throw new Error('No atom ' + atom);
  const i = (c.type === 'COMPONENT_SET' ? c.defaultVariant : c).createInstance();
  if (props) {
    const defs = i.componentProperties, out = {};
    for (const k of Object.keys(props)) {
      if (props[k] == null) continue;
      const key = Object.keys(defs).find(d => d === k || d.split('#')[0] === k);
      if (!key) { NF.warn.push('prop ' + k + ' on ' + atom); continue; }
      out[key] = props[k] && props[k].id ? props[k].id : props[k];
    }
    i.setProperties(out);
  }
  if (o.name) i.name = o.name;
  if (typeof o.w === 'number') { i.resize(o.w, i.height); }
  else if (typeof o.w === 'string' && o.w !== 'fill') i.setBoundVariable('width', V(o.w));
  markGrow(i, o.w === 'fill', o.h === 'fill');
  return i;
}
function nfReport(extra) { return Object.assign({ warnings: Array.from(new Set(NF.warn)).slice(0, 40) }, extra || {}); }
// Layout lint for a built board (real Figma only: the mock has no layout engine). Reports children that stick
// out of their auto-layout parent and nodes that collapsed to nothing. Vertical overflow inside a clipping
// container is treated as scrolling content; artwork containers are skipped by name.
function nfLint(root, max) {
  const out = []; max = max || 12;
  const art = /^(Frame ruler|Frames|Span|Stage|Stage area|Canvas|Art|Artwork|Waveform|Graph|Preview)/;
  const path = n => { const p = []; let x = n; while (x && x !== root.parent) { p.unshift(x.name); x = x.parent; } return p.slice(-4).join(' > '); };
  const walk = n => {
    if (out.length >= max || !('children' in n) || art.test(n.name)) return;
    const auto = n.layoutMode && n.layoutMode !== 'NONE';
    for (const k of n.children) {
      if (k.visible === false) continue;
      if (auto && k.layoutPositioning !== 'ABSOLUTE') {
        const ox = Math.max(k.x + k.width - n.width, -k.x), oy = Math.max(k.y + k.height - n.height, -k.y);
        if (ox > 1.5) out.push('overflow-x ' + Math.round(ox) + ': ' + path(k));
        else if (oy > 1.5 && !n.clipsContent) out.push('overflow-y ' + Math.round(oy) + ': ' + path(k));
        if (k.width < 1.5 && k.type !== 'RECTANGLE' && k.type !== 'LINE' && k.name !== 'Spacer' && !(k.type === 'TEXT' && !k.characters)) out.push('collapsed: ' + path(k));
      }
      walk(k);
    }
  };
  walk(root);
  return out;
}
