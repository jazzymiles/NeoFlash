'use strict';
// Offline mock of the part of the Figma Plugin API that interface/figma-plugin/v3/src/*.js uses.
// Purpose: smoke-test generated builder code without Figma. It models the document tree, components,
// variants, component properties, variables and text styles, and it is strict where Figma is strict:
// unknown node properties, invalid enum values, illegal FILL / HUG sizing, unknown component properties
// and invalid variant values all throw. It has NO layout engine: frame sizes only change through resize().
const fs = require('fs');
const path = require('path');
const util = require('util');

const RAW = Symbol('mock-figma.raw');

const FRAMELIKE = ['FRAME', 'COMPONENT', 'COMPONENT_SET', 'INSTANCE'];
const SHAPES = ['RECTANGLE', 'ELLIPSE', 'POLYGON', 'STAR', 'LINE', 'VECTOR'];
const SCENE = FRAMELIKE.concat(SHAPES, ['TEXT']);
const CONTAINERS = ['DOCUMENT', 'PAGE'].concat(FRAMELIKE);
const ALL = ['DOCUMENT', 'PAGE'].concat(SCENE);
const CORNERS = ['topLeftRadius', 'topRightRadius', 'bottomLeftRadius', 'bottomRightRadius'];
const SIDES = ['strokeTopWeight', 'strokeRightWeight', 'strokeBottomWeight', 'strokeLeftWeight'];
const PADDINGS = ['paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft'];
const BLEND = ['PASS_THROUGH', 'NORMAL', 'DARKEN', 'MULTIPLY', 'LINEAR_BURN', 'COLOR_BURN', 'LIGHTEN', 'SCREEN', 'LINEAR_DODGE',
  'COLOR_DODGE', 'OVERLAY', 'SOFT_LIGHT', 'HARD_LIGHT', 'DIFFERENCE', 'EXCLUSION', 'HUE', 'SATURATION', 'COLOR', 'LUMINOSITY'];
const SCOPES = ['ALL_SCOPES', 'TEXT_CONTENT', 'CORNER_RADIUS', 'WIDTH_HEIGHT', 'GAP', 'ALL_FILLS', 'FRAME_FILL', 'SHAPE_FILL', 'TEXT_FILL',
  'STROKE_COLOR', 'STROKE_FLOAT', 'EFFECT_FLOAT', 'EFFECT_COLOR', 'OPACITY', 'FONT_FAMILY', 'FONT_STYLE', 'FONT_WEIGHT', 'FONT_SIZE',
  'LINE_HEIGHT', 'LETTER_SPACING', 'PARAGRAPH_SPACING', 'PARAGRAPH_INDENT'];

const COLOR_NAMES = ('bg/app bg/panel bg/raised bg/field bg/hover bg/selected bg/canvas bg/stage border/subtle border/default border/strong ' +
  'border/focus text/primary text/secondary text/tertiary text/disabled text/on-accent text/inverse accent/default accent/hover ' +
  'accent/subtle brand/orange danger/default danger/subtle success/default warning/default warning/subtle layer/green layer/red ' +
  'layer/blue timeline/cell timeline/cell-alt timeline/line timeline/row-selected timeline/playhead timeline/empty timeline/empty-5th ' +
  'timeline/grid timeline/static timeline/classic-tween timeline/shape-tween timeline/motion-tween timeline/selected timeline/marker ' +
  'timeline/marker-inverse timeline/label timeline/comment timeline/folder timeline/onion syntax/function syntax/string syntax/keyword ' +
  'syntax/type syntax/comment syntax/number syntax/tag syntax/attr syntax/error syntax/warning syntax/breakpoint syntax/selection ' +
  'syntax/current-line overlay tooltip/bg guide guide-selected stage-grid').split(' ');
const THEME_MODES = ['Dark', 'Light', 'Mid · Neutral Gray', 'Dark · Graphite'];
const DENSITY_MODES = ['Normal', 'Compact', 'Classic Flash'];
// name: [family, style, size, line height, letter spacing %, upper]
const TEXT_STYLES = {
  'Heading': ['Inter', 'Semi Bold', 20, 28], 'Title': ['Inter', 'Semi Bold', 16, 24], 'Body': ['Inter', 'Regular', 13, 20],
  'Body Strong': ['Inter', 'Medium', 13, 20], 'Small': ['Inter', 'Regular', 12, 16], 'Small Strong': ['Inter', 'Medium', 12, 16],
  'Overline': ['Inter', 'Semi Bold', 11, 16, 6, true], 'Mono': ['Roboto Mono', 'Regular', 12, 20], 'Mono Small': ['Roboto Mono', 'Regular', 11, 14],
  'Dialog Title': ['Inter', 'Semi Bold', 18, 26], 'Dialog Body': ['Inter', 'Regular', 14, 22], 'Caption': ['Inter', 'Regular', 11, 14],
};

function show(v) {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol' || typeof v === 'function' || v === undefined) return String(v);
  try { const s = JSON.stringify(v); return s && s.length > 120 ? s.slice(0, 117) + '...' : String(s); } catch (e) { return String(v); }
}
function fail(where, msg) { return new Error('in ' + where + ': ' + msg); }
function deepFreeze(o) {
  if (o && typeof o === 'object' && !Object.isFrozen(o)) { Object.freeze(o); for (const k of Object.keys(o)) deepFreeze(o[k]); }
  return o;
}
function frozenCopy(v) { return deepFreeze(JSON.parse(JSON.stringify(v))); }
const isNum = v => typeof v === 'number' && Number.isFinite(v);
function parseVariant(name) {
  const o = {};
  for (const part of String(name).split(',')) { const i = part.indexOf('='); if (i > 0) o[part.slice(0, i).trim()] = part.slice(i + 1).trim(); }
  return o;
}
function variantLabel(o) { return Object.keys(o).map(k => k + '=' + o[k]).join(', '); }
function hashColor(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); }
  return { r: ((h >>> 16) & 255) / 255, g: ((h >>> 8) & 255) / 255, b: (h & 255) / 255, a: 1 };
}
// A plain API object (variable, collection, style, figma.*) whose unknown properties cannot be written.
function sealed(label, target, settable) {
  return new Proxy(target, {
    get(o, prop) { return prop === RAW ? o : o[prop]; },
    set(o, prop, value) {
      const s = settable && settable[prop];
      if (!s) {
        if (prop in o) throw new Error("Cannot assign to read only property '" + String(prop) + "' of " + label);
        throw new Error('object is not extensible: ' + String(prop));
      }
      s(value, o);
      return true;
    },
    defineProperty(o, prop) { throw new Error('object is not extensible: ' + String(prop)); },
    deleteProperty(o, prop) { throw new Error('Cannot delete property ' + String(prop) + ' of ' + label); },
  });
}

function createFigma(options) {
  options = options || {};
  let seq = 0;
  const nodes = new Map();        // id -> raw node
  const variables = new Map();    // id -> variable API object
  const collections = new Map();  // id -> collection API object
  const textStyles = new Map();   // id -> text style API object
  const loadedFonts = new Set();
  const MIXED = Symbol('figma.mixed');
  let currentPage = null;
  const nextId = () => '1:' + (++seq);
  const fontKey = f => f.family + '|' + f.style;

  // ------------------------------------------------------------------ tree helpers
  const raw = n => (n && typeof n === 'object' ? n[RAW] : undefined);
  const isAL = t => !!t && FRAMELIKE.indexOf(t.type) >= 0 && t.p.layoutMode !== 'NONE';
  function alive(t) { if (t.removed) throw new Error('The node with id "' + t.id + '" does not exist (it was removed)'); return t; }
  function insideInstance(t) { for (let n = t.parent; n; n = n.parent) if (n.type === 'INSTANCE') return true; return false; }
  function pageOf(t) { for (let n = t; n; n = n.parent) if (n.type === 'PAGE') return n; return null; }
  function walk(t, fn) { if (!t.kids) return; for (const k of t.kids.slice()) { fn(k); walk(k, fn); } }
  function detach(t) { if (t.parent) { const i = t.parent.kids.indexOf(t); if (i >= 0) t.parent.kids.splice(i, 1); t.parent = null; } }
  function drop(t) { t.removed = true; t.parent = null; nodes.delete(t.id); if (t.kids) for (const k of t.kids) drop(k); }
  function describe(t) { return t.type + ' "' + t.p.name + '"'; }
  // The auto-layout sizing-mode property that governs the horizontal (or vertical) axis of a frame.
  function axisProp(t, horizontal) { return (t.p.layoutMode === 'VERTICAL') === horizontal ? 'counterAxisSizingMode' : 'primaryAxisSizingMode'; }

  function attach(parent, child, index, where) {
    alive(parent); alive(child);
    if (!parent.kids) throw fail(where, describe(parent) + ' cannot have children');
    if (child === parent) throw fail(where, 'Cannot move node into itself');
    for (let n = parent.parent; n; n = n.parent) if (n === child) throw fail(where, 'Cannot move node into its own descendant');
    if (parent.type === 'INSTANCE' || insideInstance(parent))
      throw fail(where, 'Cannot move node. New parent ' + describe(parent) + ' is an instance or is inside of an instance');
    if (insideInstance(child)) throw fail(where, 'Cannot move node. ' + describe(child) + ' is inside of an instance');
    if ((parent.type === 'DOCUMENT') !== (child.type === 'PAGE')) throw fail(where, 'A ' + child.type + ' cannot be a child of a ' + parent.type);
    if (child.type === 'DOCUMENT') throw fail(where, 'The document cannot be moved');
    if (parent.type === 'COMPONENT_SET' && child.type !== 'COMPONENT') throw fail(where, 'A component set can only contain components, got ' + describe(child));
    if (child.type === 'COMPONENT' || child.type === 'COMPONENT_SET')
      for (let n = parent; n; n = n.parent)
        if (n.type === 'COMPONENT' || (n.type === 'COMPONENT_SET' && !(n === parent && child.type === 'COMPONENT')))
          throw fail(where, 'Cannot move node. A component cannot be nested inside another component (' + describe(child) + ' into ' + describe(n) + ')');
    if (index != null && (!Number.isInteger(index) || index < 0 || index > parent.kids.length - (child.parent === parent ? 1 : 0)))
      throw fail(where, 'index ' + show(index) + ' is out of range');
    detach(child);
    if (index == null) parent.kids.push(child); else parent.kids.splice(index, 0, child);
    child.parent = parent;
    if (!isAL(parent) && child.fill) {
      child.fill.h = child.fill.v = false;
      if (child.p.layoutPositioning === 'ABSOLUTE') child.p.layoutPositioning = 'AUTO';
    }
  }

  // ------------------------------------------------------------------ variables
  function getVar(v, where) {
    const id = v && typeof v === 'object' ? v.id : v;
    const found = typeof id === 'string' ? variables.get(id) : null;
    if (!found) throw fail(where, 'Expected a Variable, got ' + show(v));
    return found;
  }
  function getColl(c, where) {
    const id = c && typeof c === 'object' ? c.id : c;
    const found = typeof id === 'string' ? collections.get(id) : null;
    if (!found) throw fail(where, 'Expected a VariableCollection, got ' + show(c));
    return found;
  }
  function modeFor(t, coll) { for (let n = t; n; n = n.parent) if (n.modes[coll.id]) return n.modes[coll.id]; return coll.defaultModeId; }
  function resolveVar(v, t, depth) {
    const coll = collections.get(v.variableCollectionId), vals = v[RAW].values;
    let val = vals[modeFor(t, coll)];
    if (val === undefined) val = vals[coll.defaultModeId];
    if (val && val.type === 'VARIABLE_ALIAS') { const a = variables.get(val.id); return a && (depth || 0) < 8 ? resolveVar(a, t, (depth || 0) + 1) : undefined; }
    return val;
  }
  function checkVarValue(type, value, where) {
    if (value && typeof value === 'object' && value.type === 'VARIABLE_ALIAS') {
      const a = getVar(value.id, where);
      if (a.resolvedType !== type) throw fail(where, 'Alias to a ' + a.resolvedType + ' variable cannot be the value of a ' + type + ' variable');
      return frozenCopy({ type: 'VARIABLE_ALIAS', id: a.id });
    }
    const ok = type === 'FLOAT' ? isNum(value) : type === 'BOOLEAN' ? typeof value === 'boolean' : type === 'STRING' ? typeof value === 'string'
      : !!value && typeof value === 'object' && ['r', 'g', 'b'].every(k => isNum(value[k]) && value[k] >= 0 && value[k] <= 1) && (value.a === undefined || isNum(value.a));
    if (!ok) throw fail(where, 'Value ' + show(value) + ' is not valid for a ' + type + ' variable');
    return type === 'COLOR' ? frozenCopy({ r: value.r, g: value.g, b: value.b, a: value.a === undefined ? 1 : value.a }) : value;
  }
  function makeCollection(name) {
    if (typeof name !== 'string' || !name) throw fail('createVariableCollection', 'Expected a name, got ' + show(name));
    const o = { id: 'VariableCollectionId:1:' + (++seq), name: name, remote: false, hiddenFromPublishing: false, modeList: [], varIds: [] };
    o.key = 'mock-' + o.id;
    const addMode = n => { const m = { modeId: '1:' + (++seq), name: n }; o.modeList.push(m); return m.modeId; };
    o.defaultModeId = addMode('Mode 1');
    Object.defineProperty(o, 'modes', { enumerable: true, get: () => o.modeList.map(m => ({ modeId: m.modeId, name: m.name })) });
    Object.defineProperty(o, 'variableIds', { enumerable: true, get: () => o.varIds.slice() });
    o.addMode = n => {
      if (typeof n !== 'string' || !n) throw fail('addMode', 'Expected a mode name, got ' + show(n));
      if (o.modeList.some(m => m.name === n)) throw fail('addMode', 'Duplicate mode name ' + show(n) + ' in collection ' + show(o.name));
      const id = addMode(n);
      for (const vid of o.varIds) { const vr = variables.get(vid)[RAW]; vr.values[id] = vr.values[o.defaultModeId]; }
      return id;
    };
    o.renameMode = (modeId, n) => {
      const m = o.modeList.find(x => x.modeId === modeId);
      if (!m) throw fail('renameMode', 'Mode ' + show(modeId) + ' does not exist in collection ' + show(o.name));
      if (typeof n !== 'string' || !n) throw fail('renameMode', 'Expected a mode name, got ' + show(n));
      m.name = n;
    };
    o.removeMode = modeId => {
      const i = o.modeList.findIndex(x => x.modeId === modeId);
      if (i < 0) throw fail('removeMode', 'Mode ' + show(modeId) + ' does not exist in collection ' + show(o.name));
      if (o.modeList.length === 1) throw fail('removeMode', 'Cannot remove the last mode');
      o.modeList.splice(i, 1); if (o.defaultModeId === modeId) o.defaultModeId = o.modeList[0].modeId;
    };
    o.remove = () => { for (const vid of o.varIds.slice()) variables.delete(vid); collections.delete(o.id); };
    const api = sealed('VariableCollection', o, {
      name: (v, t) => { if (typeof v !== 'string') throw fail('set_name', 'Expected string'); t.name = v; },
      hiddenFromPublishing: (v, t) => { t.hiddenFromPublishing = !!v; },
    });
    collections.set(o.id, api);
    return api;
  }
  function makeVariable(name, collection, type) {
    const coll = getColl(collection, 'createVariable');
    if (typeof name !== 'string' || !name) throw fail('createVariable', 'Expected a variable name, got ' + show(name));
    if (['BOOLEAN', 'COLOR', 'FLOAT', 'STRING'].indexOf(type) < 0) throw fail('createVariable', 'Expected resolvedType BOOLEAN | COLOR | FLOAT | STRING, got ' + show(type));
    const c = coll[RAW];
    if (c.varIds.some(id => variables.get(id).name === name)) throw fail('createVariable', 'duplicate variable name ' + show(name) + ' in collection ' + show(c.name));
    const o = { id: 'VariableID:1:' + (++seq), name: name, resolvedType: type, variableCollectionId: c.id, remote: false, description: '',
      hiddenFromPublishing: false, scopes: type === 'BOOLEAN' ? [] : ['ALL_SCOPES'], values: {}, syntax: {} };
    o.key = 'mock-' + o.id;
    const zero = type === 'FLOAT' ? 0 : type === 'BOOLEAN' ? false : type === 'STRING' ? '' : deepFreeze({ r: 1, g: 1, b: 1, a: 1 });
    for (const m of c.modeList) o.values[m.modeId] = zero;
    Object.defineProperty(o, 'valuesByMode', { enumerable: true, get: () => Object.assign({}, o.values) });
    Object.defineProperty(o, 'codeSyntax', { enumerable: true, get: () => Object.assign({}, o.syntax) });
    o.setValueForMode = (modeId, value) => {
      if (!c.modeList.some(m => m.modeId === modeId)) throw fail('setValueForMode', 'Mode ' + show(modeId) + ' does not exist in collection ' + show(c.name) + ' (variable ' + show(o.name) + ')');
      o.values[modeId] = checkVarValue(type, value, 'setValueForMode(' + o.name + ')');
    };
    o.setVariableCodeSyntax = (platform, value) => {
      if (['WEB', 'ANDROID', 'iOS'].indexOf(platform) < 0) throw fail('setVariableCodeSyntax', 'Expected platform WEB | ANDROID | iOS, got ' + show(platform));
      if (typeof value !== 'string') throw fail('setVariableCodeSyntax', 'Expected a string, got ' + show(value));
      o.syntax[platform] = value;
    };
    o.removeVariableCodeSyntax = platform => { delete o.syntax[platform]; };
    o.remove = () => { variables.delete(o.id); const i = c.varIds.indexOf(o.id); if (i >= 0) c.varIds.splice(i, 1); };
    const api = sealed('Variable', o, {
      name: (v, t) => { if (typeof v !== 'string' || !v) throw fail('set_name', 'Expected a non-empty string'); t.name = v; },
      description: (v, t) => { if (typeof v !== 'string') throw fail('set_description', 'Expected string'); t.description = v; },
      hiddenFromPublishing: (v, t) => { t.hiddenFromPublishing = !!v; },
      scopes: (v, t) => {
        if (!Array.isArray(v)) throw fail('set_scopes', 'Expected an array of scopes, got ' + show(v));
        for (const s of v) if (SCOPES.indexOf(s) < 0) throw fail('set_scopes', 'Unknown variable scope ' + show(s));
        t.scopes = Object.freeze(v.slice());
      },
    });
    variables.set(o.id, api); c.varIds.push(o.id);
    return api;
  }

  // ------------------------------------------------------------------ value checkers
  const cStr = (t, v, p) => { if (typeof v !== 'string') throw fail('set_' + p, 'Expected string, got ' + show(v)); return v; };
  const cBool = (t, v, p) => { if (typeof v !== 'boolean') throw fail('set_' + p, 'Expected boolean, got ' + show(v)); return v; };
  const cEnum = list => (t, v, p) => { if (list.indexOf(v) < 0) throw fail('set_' + p, 'Expected one of ' + list.map(show).join(' | ') + ', got ' + show(v)); return v; };
  const cNum = (min, max) => (t, v, p) => {
    if (!isNum(v)) throw fail('set_' + p, 'Expected number, got ' + show(v));
    if (min != null && v < min) throw fail('set_' + p, 'Expected a number >= ' + min + ', got ' + v);
    if (max != null && v > max) throw fail('set_' + p, 'Expected a number <= ' + max + ', got ' + v);
    return v;
  };
  const cInt = min => (t, v, p) => { if (!Number.isInteger(v) || v < min) throw fail('set_' + p, 'Expected an integer >= ' + min + ', got ' + show(v)); return v; };
  const cNumOrNull = min => (t, v, p) => (v === null ? null : cNum(min)(t, v, p));
  function cColor(c, where, alpha) {
    const keys = alpha ? ['r', 'g', 'b', 'a'] : ['r', 'g', 'b'];
    if (!c || typeof c !== 'object') throw fail(where, 'Expected a color object, got ' + show(c));
    for (const k of Object.keys(c)) if (keys.indexOf(k) < 0) throw fail(where, 'Unrecognized key ' + show(k) + ' in color ' + show(c));
    for (const k of keys) if (!isNum(c[k]) || c[k] < 0 || c[k] > 1) throw fail(where, 'color.' + k + ' must be a number between 0 and 1, got ' + show(c[k]));
  }
  const PAINT_KEYS = ['type', 'color', 'opacity', 'visible', 'blendMode', 'boundVariables'];
  function cPaint(x, where) {
    if (!x || typeof x !== 'object') throw fail(where, 'Expected a paint object, got ' + show(x));
    if (x.type === 'SOLID') {
      for (const k of Object.keys(x)) if (PAINT_KEYS.indexOf(k) < 0) throw fail(where, 'Unrecognized key ' + show(k) + ' in SOLID paint');
      cColor(x.color, where, false);
      if (x.opacity !== undefined && (!isNum(x.opacity) || x.opacity < 0 || x.opacity > 1)) throw fail(where, 'opacity must be a number between 0 and 1, got ' + show(x.opacity));
      if (x.visible !== undefined && typeof x.visible !== 'boolean') throw fail(where, 'visible must be boolean');
      if (x.blendMode !== undefined && BLEND.indexOf(x.blendMode) < 0) throw fail(where, 'Unknown blendMode ' + show(x.blendMode));
      const b = x.boundVariables && x.boundVariables.color;
      if (b) { const v = getVar(b.id, where); if (v.resolvedType !== 'COLOR') throw fail(where, 'Variable ' + show(v.name) + ' is ' + v.resolvedType + ', a paint colour needs COLOR'); }
    } else if (['GRADIENT_LINEAR', 'GRADIENT_RADIAL', 'GRADIENT_ANGULAR', 'GRADIENT_DIAMOND', 'IMAGE', 'VIDEO', 'PATTERN'].indexOf(x.type) < 0) {
      throw fail(where, 'Unknown paint type ' + show(x.type));
    }
  }
  const cPaints = (t, v, p) => {
    if (!Array.isArray(v)) throw fail('set_' + p, 'Expected an array of paints, got ' + show(v));
    v.forEach((x, i) => cPaint(x, 'set_' + p + '[' + i + ']'));
    return frozenCopy(v);
  };
  const cEffects = (t, v, p) => {
    if (!Array.isArray(v)) throw fail('set_' + p, 'Expected an array of effects, got ' + show(v));
    v.forEach((e, i) => {
      const where = 'set_' + p + '[' + i + ']';
      if (!e || typeof e !== 'object') throw fail(where, 'Expected an effect object, got ' + show(e));
      if (e.type === 'DROP_SHADOW' || e.type === 'INNER_SHADOW') {
        cColor(e.color, where, true);
        if (!e.offset || !isNum(e.offset.x) || !isNum(e.offset.y)) throw fail(where, 'offset must be {x, y}');
        if (!isNum(e.radius) || e.radius < 0) throw fail(where, 'radius must be a number >= 0');
        if (e.spread !== undefined && !isNum(e.spread)) throw fail(where, 'spread must be a number');
        if (typeof e.visible !== 'boolean') throw fail(where, 'visible is required (boolean)');
        if (BLEND.indexOf(e.blendMode) < 0) throw fail(where, 'blendMode is required, got ' + show(e.blendMode));
      } else if (e.type === 'LAYER_BLUR' || e.type === 'BACKGROUND_BLUR') {
        if (!isNum(e.radius) || e.radius < 0) throw fail(where, 'radius must be a number >= 0');
        if (typeof e.visible !== 'boolean') throw fail(where, 'visible is required (boolean)');
      } else throw fail(where, 'Unknown effect type ' + show(e.type));
    });
    return frozenCopy(v);
  };
  const CONSTRAINT = ['MIN', 'CENTER', 'MAX', 'STRETCH', 'SCALE'];
  const cConstraints = (t, v, p) => {
    if (!v || CONSTRAINT.indexOf(v.horizontal) < 0 || CONSTRAINT.indexOf(v.vertical) < 0)
      throw fail('set_' + p, 'Expected {horizontal, vertical} with ' + CONSTRAINT.join(' | ') + ', got ' + show(v));
    return frozenCopy({ horizontal: v.horizontal, vertical: v.vertical });
  };
  const cMinMax = (t, v, p) => {
    if (v !== null) cNum(0)(t, v, p);
    if (!isAL(t) && !isAL(t.parent)) throw fail('set_' + p, 'Can only set ' + p + ' on auto-layout frames and their children (' + describe(t) + ')');
    return v;
  };
  function fontLoaded(t, p, font) {
    const f = font || t.p.fontName;
    if (!loadedFonts.has(fontKey(f)))
      throw fail('set_' + p, 'Cannot write to node with unloaded font "' + f.family + ' ' + f.style + '". Please call figma.loadFontAsync({ family: "' + f.family + '", style: "' + f.style + '" }) and await the returned promise first.');
  }
  const textProp = check => (t, v, p) => { const out = check(t, v, p); fontLoaded(t, p); return out; };
  const cFontName = (t, v, p) => {
    if (!v || typeof v.family !== 'string' || typeof v.style !== 'string') throw fail('set_' + p, 'Expected {family, style}, got ' + show(v));
    fontLoaded(t, p, v);
    return frozenCopy({ family: v.family, style: v.style });
  };
  const cLineHeight = (t, v, p) => {
    if (v && v.unit === 'AUTO') return frozenCopy({ unit: 'AUTO' });
    if (!v || ['PIXELS', 'PERCENT'].indexOf(v.unit) < 0 || !isNum(v.value)) throw fail('set_' + p, 'Expected {unit: "AUTO"} or {unit: "PIXELS" | "PERCENT", value}, got ' + show(v));
    return frozenCopy({ unit: v.unit, value: v.value });
  };
  const cLetterSpacing = (t, v, p) => {
    if (!v || ['PIXELS', 'PERCENT'].indexOf(v.unit) < 0 || !isNum(v.value)) throw fail('set_' + p, 'Expected {unit: "PIXELS" | "PERCENT", value}, got ' + show(v));
    return frozenCopy({ unit: v.unit, value: v.value });
  };
  const cMaxLines = (t, v, p) => { if (v !== null && (!Number.isInteger(v) || v < 1)) throw fail('set_' + p, 'Expected null or an integer >= 1, got ' + show(v)); return v; };

  // ------------------------------------------------------------------ component properties
  // The node that owns the property definitions of a component: its set, or the component itself.
  const defOwner = comp => (comp.parent && comp.parent.type === 'COMPONENT_SET' ? comp.parent : comp);
  function variantDefs(set) {
    const out = {};
    for (const c of set.kids) { const pv = parseVariant(c.p.name); for (const k of Object.keys(pv)) { out[k] = out[k] || []; if (out[k].indexOf(pv[k]) < 0) out[k].push(pv[k]); } }
    return out;
  }
  function getComponent(id, where) {
    const c = typeof id === 'string' ? nodes.get(id) : null;
    if (!c || c.type !== 'COMPONENT') throw fail(where, 'expects the string id of an existing component, got ' + (raw(id) ? 'a ' + raw(id).type + ' node object (pass node.id)' : show(id)));
    return c;
  }
  function checkPropValue(type, value, where, label) {
    if (type === 'BOOLEAN' && typeof value !== 'boolean') throw fail(where, 'Property ' + label + ' is BOOLEAN and expects a boolean, got ' + show(value));
    if (type === 'TEXT' && typeof value !== 'string') throw fail(where, 'Property ' + label + ' is TEXT and expects a string, got ' + show(value));
    if (type === 'INSTANCE_SWAP') { try { getComponent(value, where); } catch (e) { throw fail(where, 'Property ' + label + ' is INSTANCE_SWAP and ' + e.message.split(': ').slice(1).join(': ')); } }
  }
  const cRefs = (t, v, p) => {
    if (v === null || v === undefined) return null;
    if (typeof v !== 'object') throw fail('set_' + p, 'Expected an object, got ' + show(v));
    let comp = null;
    for (let n = t.parent; n; n = n.parent) { if (n.type === 'INSTANCE') break; if (n.type === 'COMPONENT') { comp = n; break; } }
    if (!comp || t.type === 'COMPONENT' || t.type === 'COMPONENT_SET')
      throw fail('set_' + p, 'Can only set component property references on a sublayer of a component that is not inside a nested instance (' + describe(t) + ')');
    const owner = defOwner(comp), out = {};
    for (const k of Object.keys(v)) {
      const want = { visible: 'BOOLEAN', characters: 'TEXT', mainComponent: 'INSTANCE_SWAP' }[k];
      if (!want) throw fail('set_' + p, 'Unknown reference field ' + show(k) + ' (expected visible | characters | mainComponent)');
      if (v[k] == null) continue;
      const def = owner.defs && owner.defs[v[k]];
      if (!def) throw fail('set_' + p, 'Component property ' + show(v[k]) + ' does not exist on ' + describe(owner));
      if (def.type !== want) throw fail('set_' + p, 'Reference "' + k + '" needs a ' + want + ' property, but ' + show(v[k]) + ' is ' + def.type);
      if (k === 'characters' && t.type !== 'TEXT') throw fail('set_' + p, 'A "characters" reference can only be set on a text node, not on ' + describe(t));
      if (k === 'mainComponent' && t.type !== 'INSTANCE') throw fail('set_' + p, 'A "mainComponent" reference can only be set on an instance, not on ' + describe(t));
      out[k] = v[k];
    }
    return Object.keys(out).length ? Object.freeze(out) : null;
  };
  // A linked sublayer of a component shows the default value of its property.
  function applyRefDefaults(t) {
    const r = t.p.componentPropertyReferences; if (!r) return;
    let comp = null;
    for (let n = t.parent; n; n = n.parent) if (n.type === 'COMPONENT') { comp = n; break; }
    if (!comp) return;
    const defs = defOwner(comp).defs || {};
    if (r.characters && defs[r.characters]) t.p.characters = defs[r.characters].defaultValue;
    if (r.visible && defs[r.visible]) t.p.visible = defs[r.visible].defaultValue;
    if (r.mainComponent && defs[r.mainComponent] && t.main.id !== defs[r.mainComponent].defaultValue) fillInstance(t, nodes.get(defs[r.mainComponent].defaultValue), 'swap');
  }
  function applyProp(inst, key, value, type) {
    walk(inst, n => {
      if (n.removed) return;
      const r = n.p.componentPropertyReferences; if (!r) return;
      if (type === 'TEXT' && r.characters === key && n.type === 'TEXT') n.p.characters = value;
      else if (type === 'BOOLEAN' && r.visible === key) n.p.visible = value;
      else if (type === 'INSTANCE_SWAP' && r.mainComponent === key && n.type === 'INSTANCE' && n.main.id !== value) fillInstance(n, nodes.get(value), 'swap');
    });
  }
  const KEEP_ON_REBUILD = ['name', 'visible', 'locked', 'x', 'y', 'rotation', 'layoutPositioning', 'constraints', 'layoutAlign', 'layoutGrow', 'componentPropertyReferences'];
  // (Re)builds the subtree of an instance from a component. mode: 'new' | 'variant' (keeps property values) | 'swap'.
  function fillInstance(inst, comp, mode) {
    for (const k of inst.kids) drop(k);
    inst.kids = [];
    const old = inst.p;
    inst.p = Object.assign({}, comp.p);
    inst.p.componentPropertyReferences = null;
    delete inst.p.description;
    if (mode === 'new') inst.p.name = defOwner(comp).p.name;
    else for (const k of KEEP_ON_REBUILD) inst.p[k] = old[k];
    if (mode === 'new') { inst.p.x = 0; inst.p.y = 0; }
    if (mode === 'new' || !inst.resized) { inst.w = comp.w; inst.h = comp.h; }
    inst.bound = Object.assign({}, comp.bound, inst.ownBound || {});
    inst.main = comp;
    if (mode !== 'variant') inst.props = {};
    const base = inst.id.replace(/^I/, '');
    for (const k of comp.kids) { const c = cloneRaw(k, s => 'I' + base + ';' + s.id.replace(/^I/, '')); c.parent = inst; inst.kids.push(c); }
    if (mode === 'variant') { const defs = defOwner(comp).defs || {}; for (const key of Object.keys(inst.props)) if (defs[key]) applyProp(inst, key, inst.props[key], defs[key].type); }
  }
  function cloneRaw(src, idFor) {
    const t = newRaw(src.type, idFor ? idFor(src) : undefined);
    t.p = Object.assign({}, src.p); t.w = src.w; t.h = src.h;
    t.bound = Object.assign({}, src.bound); t.modes = Object.assign({}, src.modes); t.fill = Object.assign({}, src.fill);
    if (src.type === 'INSTANCE') { t.main = src.main; t.props = Object.assign({}, src.props); t.ownBound = Object.assign({}, src.ownBound); t.resized = src.resized; }
    if (src.type === 'TEXT') t.styleId = src.styleId;
    if (src.kids) for (const k of src.kids) { const c = cloneRaw(k, idFor); c.parent = t; t.kids.push(c); }
    return t;
  }

  // ------------------------------------------------------------------ sizes
  function textSize(t) {
    const fsz = t.p.fontSize, lh = t.p.lineHeight;
    const line = lh.unit === 'PIXELS' ? lh.value : lh.unit === 'PERCENT' ? fsz * lh.value / 100 : fsz * 1.3;
    const lines = String(t.p.characters).split('\n'), mode = t.p.textAutoResize;
    if (mode === 'WIDTH_AND_HEIGHT') return { w: Math.max.apply(null, lines.map(l => l.length * fsz * 0.55)), h: line * lines.length };
    if (mode === 'HEIGHT') {
      let n = 0;
      for (const l of lines) n += Math.max(1, Math.ceil(l.length * fsz * 0.55 / Math.max(t.w, 1)));
      if (t.p.textTruncation === 'ENDING' && t.p.maxLines) n = Math.min(n, t.p.maxLines);
      return { w: t.w, h: n * line };
    }
    return { w: t.w, h: t.h };
  }
  function sizeOf(t, dim) {
    const b = t.bound[dim];
    if (b && variables.has(b)) { const v = resolveVar(variables.get(b), t); if (isNum(v)) return v; }
    if (t.type === 'TEXT') return textSize(t)[dim === 'width' ? 'w' : 'h'];
    return dim === 'width' ? t.w : t.h;
  }
  function getSizing(t, horizontal) {
    if (t.fill[horizontal ? 'h' : 'v'] && isAL(t.parent)) return 'FILL';
    if (isAL(t)) return t.p[axisProp(t, horizontal)] === 'AUTO' ? 'HUG' : 'FIXED';
    if (t.type === 'TEXT') { const m = t.p.textAutoResize; return m === 'WIDTH_AND_HEIGHT' || (!horizontal && m === 'HEIGHT') ? 'HUG' : 'FIXED'; }
    return 'FIXED';
  }
  function setSizing(t, horizontal, v, p) {
    if (v === 'FILL' && !isAL(t.parent))
      throw fail('set_' + p, 'FILL can only be set on children of auto-layout frames (' + describe(t) + ' has ' + (t.parent ? 'parent ' + describe(t.parent) + (FRAMELIKE.indexOf(t.parent.type) >= 0 ? ' with layoutMode NONE' : '') : 'no parent') + ')');
    if (v === 'HUG' && !isAL(t) && t.type !== 'TEXT')
      throw fail('set_' + p, 'HUG can only be set on auto-layout frames and text nodes (' + describe(t) + ')');
    t.fill[horizontal ? 'h' : 'v'] = v === 'FILL';
    if (isAL(t)) t.p[axisProp(t, horizontal)] = v === 'HUG' ? 'AUTO' : 'FIXED';
    else if (t.type === 'TEXT') {
      const m = t.p.textAutoResize;
      if (horizontal) t.p.textAutoResize = v === 'HUG' ? 'WIDTH_AND_HEIGHT' : (m === 'WIDTH_AND_HEIGHT' ? 'HEIGHT' : m);
      else t.p.textAutoResize = v === 'HUG' ? (m === 'WIDTH_AND_HEIGHT' ? m : 'HEIGHT') : (m === 'TRUNCATE' ? m : 'NONE');
    }
  }

  // ------------------------------------------------------------------ property table
  const PROPS = new Map();
  function prop(names, types, check, init, extra) {
    for (const n of [].concat(names)) PROPS.set(n, Object.assign({ types: new Set(types), check: check, init: init }, extra || {}));
  }
  const EMPTY = Object.freeze([]);
  const solid = (r, g, b) => deepFreeze([{ type: 'SOLID', visible: true, opacity: 1, blendMode: 'NORMAL', color: { r: r, g: g, b: b } }]);
  const WHITE = solid(1, 1, 1), GRAY = solid(0.85, 0.85, 0.85), BLACK = solid(0, 0, 0);

  prop('name', ALL, cStr, '');
  prop('visible', SCENE, cBool, true);
  prop(['locked', 'isMask'], SCENE, cBool, false);
  prop('opacity', SCENE, cNum(0, 1), 1);
  prop('blendMode', SCENE, cEnum(BLEND), 'PASS_THROUGH');
  prop('effects', SCENE, cEffects, EMPTY);
  prop(['x', 'y'], SCENE, cNum(), 0);
  prop('rotation', SCENE, cNum(-180, 180), 0);
  prop('layoutPositioning', SCENE, (t, v, p) => {
    cEnum(['AUTO', 'ABSOLUTE'])(t, v, p);
    if (v === 'ABSOLUTE' && !isAL(t.parent)) throw fail('set_' + p, 'Can only set layoutPositioning = ABSOLUTE if the parent node has layoutMode !== NONE (' + describe(t) + ')');
    return v;
  }, 'AUTO');
  prop('constraints', SCENE, cConstraints, deepFreeze({ horizontal: 'MIN', vertical: 'MIN' }));
  prop('layoutAlign', SCENE, cEnum(['MIN', 'CENTER', 'MAX', 'STRETCH', 'INHERIT']), 'INHERIT');
  prop('layoutGrow', SCENE, cEnum([0, 1]), 0);
  prop(['minWidth', 'maxWidth', 'minHeight', 'maxHeight'], SCENE, cMinMax, null);
  prop('layoutSizingHorizontal', SCENE, cEnum(['FIXED', 'HUG', 'FILL']), undefined, { get: t => getSizing(t, true), set: (t, v, p) => setSizing(t, true, v, p) });
  prop('layoutSizingVertical', SCENE, cEnum(['FIXED', 'HUG', 'FILL']), undefined, { get: t => getSizing(t, false), set: (t, v, p) => setSizing(t, false, v, p) });
  prop('componentPropertyReferences', SCENE, cRefs, null, { after: applyRefDefaults });
  prop('fills', SCENE, cPaints, type => (type === 'TEXT' ? BLACK : type === 'FRAME' || type === 'COMPONENT' ? WHITE : SHAPES.indexOf(type) >= 0 && type !== 'LINE' && type !== 'VECTOR' ? GRAY : EMPTY));
  prop('strokes', SCENE, cPaints, EMPTY);
  prop('strokeWeight', SCENE, cNum(0), 1, { after: t => { for (const s of SIDES) if (s in t.p) t.p[s] = t.p.strokeWeight; } });
  prop('strokeAlign', SCENE, cEnum(['CENTER', 'INSIDE', 'OUTSIDE']), type => (FRAMELIKE.indexOf(type) >= 0 ? 'INSIDE' : type === 'TEXT' ? 'OUTSIDE' : 'CENTER'));
  prop('strokeCap', SCENE, cEnum(['NONE', 'ROUND', 'SQUARE', 'ARROW_LINES', 'ARROW_EQUILATERAL', 'DIAMOND_FILLED', 'TRIANGLE_FILLED', 'CIRCLE_FILLED']), 'NONE');
  prop('strokeJoin', SCENE, cEnum(['MITER', 'BEVEL', 'ROUND']), 'MITER');
  prop('dashPattern', SCENE, (t, v, p) => { if (!Array.isArray(v) || !v.every(isNum)) throw fail('set_' + p, 'Expected an array of numbers'); return Object.freeze(v.slice()); }, EMPTY);
  prop(SIDES, FRAMELIKE.concat(['RECTANGLE']), cNum(0), 1);
  const ROUNDED = FRAMELIKE.concat(['RECTANGLE', 'ELLIPSE', 'POLYGON', 'STAR', 'VECTOR']);
  prop('cornerRadius', ROUNDED, cNum(0), 0, {
    get: t => (CORNERS[0] in t.p && !CORNERS.every(c => t.p[c] === t.p[CORNERS[0]]) ? MIXED : (CORNERS[0] in t.p ? t.p[CORNERS[0]] : t.p.cornerRadius)),
    after: t => { for (const c of CORNERS) if (c in t.p) t.p[c] = t.p.cornerRadius; },
  });
  prop('cornerSmoothing', ROUNDED, cNum(0, 1), 0);
  prop(CORNERS, FRAMELIKE.concat(['RECTANGLE']), cNum(0), 0);
  prop('layoutMode', FRAMELIKE, cEnum(['NONE', 'HORIZONTAL', 'VERTICAL']), 'NONE', {
    after: t => { if (t.p.layoutMode === 'NONE') for (const k of t.kids) { k.fill.h = k.fill.v = false; if (k.p.layoutPositioning === 'ABSOLUTE') k.p.layoutPositioning = 'AUTO'; } },
  });
  prop('primaryAxisSizingMode', FRAMELIKE, cEnum(['FIXED', 'AUTO']), 'AUTO', { after: t => { if (t.p.primaryAxisSizingMode === 'AUTO') t.fill[t.p.layoutMode === 'VERTICAL' ? 'v' : 'h'] = false; } });
  prop('counterAxisSizingMode', FRAMELIKE, cEnum(['FIXED', 'AUTO']), 'AUTO', { after: t => { if (t.p.counterAxisSizingMode === 'AUTO') t.fill[t.p.layoutMode === 'VERTICAL' ? 'h' : 'v'] = false; } });
  prop('primaryAxisAlignItems', FRAMELIKE, cEnum(['MIN', 'CENTER', 'MAX', 'SPACE_BETWEEN']), 'MIN');
  prop('counterAxisAlignItems', FRAMELIKE, cEnum(['MIN', 'CENTER', 'MAX', 'BASELINE']), 'MIN');
  prop('counterAxisAlignContent', FRAMELIKE, cEnum(['AUTO', 'SPACE_BETWEEN']), 'AUTO');
  prop('layoutWrap', FRAMELIKE, (t, v, p) => {
    cEnum(['NO_WRAP', 'WRAP'])(t, v, p);
    if (v === 'WRAP' && t.p.layoutMode !== 'HORIZONTAL') throw fail('set_' + p, 'WRAP is only available on horizontal auto-layout frames (' + describe(t) + ' has layoutMode ' + t.p.layoutMode + ')');
    return v;
  }, 'NO_WRAP');
  prop('itemSpacing', FRAMELIKE, cNum(), 0);
  prop('counterAxisSpacing', FRAMELIKE, cNumOrNull(0), null);
  prop(PADDINGS, FRAMELIKE, cNum(0), 0);
  prop('clipsContent', FRAMELIKE, cBool, true);
  prop(['itemReverseZIndex', 'strokesIncludedInLayout'], FRAMELIKE, cBool, false);
  prop('description', ['COMPONENT', 'COMPONENT_SET'], cStr, '');
  prop('characters', ['TEXT'], textProp(cStr), '');
  prop('fontName', ['TEXT'], cFontName, deepFreeze({ family: 'Inter', style: 'Regular' }));
  prop('fontSize', ['TEXT'], textProp(cNum(1)), 12);
  prop('lineHeight', ['TEXT'], textProp(cLineHeight), deepFreeze({ unit: 'AUTO' }));
  prop('letterSpacing', ['TEXT'], textProp(cLetterSpacing), deepFreeze({ unit: 'PERCENT', value: 0 }));
  prop('textCase', ['TEXT'], textProp(cEnum(['ORIGINAL', 'UPPER', 'LOWER', 'TITLE', 'SMALL_CAPS', 'SMALL_CAPS_FORCED'])), 'ORIGINAL');
  prop('textDecoration', ['TEXT'], textProp(cEnum(['NONE', 'UNDERLINE', 'STRIKETHROUGH'])), 'NONE');
  prop('textAutoResize', ['TEXT'], textProp(cEnum(['NONE', 'WIDTH_AND_HEIGHT', 'HEIGHT', 'TRUNCATE'])), 'WIDTH_AND_HEIGHT', {
    after: t => { const m = t.p.textAutoResize; if (m === 'WIDTH_AND_HEIGHT') t.fill.h = t.fill.v = false; else if (m === 'HEIGHT') t.fill.v = false; },
  });
  prop('textTruncation', ['TEXT'], textProp(cEnum(['DISABLED', 'ENDING'])), 'DISABLED');
  prop('maxLines', ['TEXT'], textProp(cMaxLines), null);
  prop('textAlignHorizontal', ['TEXT'], textProp(cEnum(['LEFT', 'CENTER', 'RIGHT', 'JUSTIFIED'])), 'LEFT');
  prop('textAlignVertical', ['TEXT'], textProp(cEnum(['TOP', 'CENTER', 'BOTTOM'])), 'TOP');
  prop(['paragraphSpacing', 'paragraphIndent', 'listSpacing'], ['TEXT'], textProp(cNum(0)), 0);
  prop('pointCount', ['POLYGON', 'STAR'], cInt(3), type => (type === 'STAR' ? 5 : 3));
  prop('innerRadius', ['STAR'], cNum(0, 1), 0.382);
  prop('selection', ['PAGE'], (t, v, p) => {
    if (!Array.isArray(v) || !v.every(n => raw(n) && pageOf(raw(n)) === t)) throw fail('set_' + p, 'Expected an array of nodes on this page');
    return Object.freeze(v.slice());
  }, EMPTY);
  prop('mainComponent', ['INSTANCE'], (t, v, p) => {
    const c = raw(v);
    if (!c || c.type !== 'COMPONENT' || c.removed) throw fail('set_' + p, 'Expected an existing ComponentNode, got ' + show(v));
    if (insideInstance(c)) throw fail('set_' + p, 'Component is inside an instance');
    return c;
  }, undefined, { get: t => (t.main ? t.main.proxy : null), set: (t, c) => { if (c !== t.main) fillInstance(t, c, 'swap'); } });

  const DEFAULTS = {};
  for (const type of ALL) {
    const d = DEFAULTS[type] = {};
    for (const [name, def] of PROPS) if (def.types.has(type) && !def.set) d[name] = typeof def.init === 'function' ? def.init(type) : def.init;
  }

  // Bindable fields: field -> variable type.
  const BIND = {};
  for (const type of SCENE) {
    const b = BIND[type] = { visible: 'BOOLEAN', opacity: 'FLOAT', width: 'FLOAT', height: 'FLOAT', minWidth: 'FLOAT', maxWidth: 'FLOAT', minHeight: 'FLOAT', maxHeight: 'FLOAT', strokeWeight: 'FLOAT' };
    if (FRAMELIKE.indexOf(type) >= 0) for (const f of ['itemSpacing', 'counterAxisSpacing'].concat(PADDINGS)) b[f] = 'FLOAT';
    if (FRAMELIKE.indexOf(type) >= 0 || type === 'RECTANGLE') for (const f of CORNERS.concat(SIDES)) b[f] = 'FLOAT';
    if (type === 'TEXT') Object.assign(b, { characters: 'STRING', fontFamily: 'STRING', fontStyle: 'STRING', fontWeight: 'FLOAT', fontSize: 'FLOAT', lineHeight: 'FLOAT', letterSpacing: 'FLOAT', paragraphSpacing: 'FLOAT', paragraphIndent: 'FLOAT' });
  }

  // ------------------------------------------------------------------ getters and methods
  const GETTERS = {};
  function getter(names, types, get) { for (const n of [].concat(names)) GETTERS[n] = { types: new Set(types), get: get }; }
  getter('id', ALL, t => t.id);
  getter('type', ALL, t => t.type);
  getter('removed', ALL, t => t.removed);
  getter('parent', ALL, t => (t.parent ? t.parent.proxy : null));
  getter('children', CONTAINERS, t => Object.freeze(t.kids.map(k => k.proxy)));
  getter('width', SCENE, t => sizeOf(t, 'width'));
  getter('height', SCENE, t => sizeOf(t, 'height'));
  getter('boundVariables', SCENE, t => { const o = {}; for (const k of Object.keys(t.bound)) o[k] = { type: 'VARIABLE_ALIAS', id: t.bound[k] }; return deepFreeze(o); });
  getter('explicitVariableModes', SCENE.concat(['PAGE']), t => Object.freeze(Object.assign({}, t.modes)));
  getter('textStyleId', ['TEXT'], t => t.styleId || '');
  getter('remote', ['COMPONENT', 'COMPONENT_SET'], () => false);
  getter('key', ['COMPONENT', 'COMPONENT_SET'], t => 'mock-key-' + t.id);
  getter('defaultVariant', ['COMPONENT_SET'], t => (t.kids[0] ? t.kids[0].proxy : null));
  getter('variantProperties', ['COMPONENT', 'INSTANCE'], t => {
    const c = t.type === 'INSTANCE' ? t.main : t;
    return c && c.parent && c.parent.type === 'COMPONENT_SET' ? Object.freeze(parseVariant(c.p.name)) : null;
  });
  getter('componentPropertyDefinitions', ['COMPONENT', 'COMPONENT_SET'], t => {
    if (t.type === 'COMPONENT' && t.parent && t.parent.type === 'COMPONENT_SET')
      throw fail('get_componentPropertyDefinitions', 'Can only get component property definitions of a component set or non-variant component');
    const out = {};
    if (t.type === 'COMPONENT_SET') {
      const vd = variantDefs(t), first = t.kids[0] ? parseVariant(t.kids[0].p.name) : {};
      for (const k of Object.keys(vd)) out[k] = { type: 'VARIANT', defaultValue: first[k], variantOptions: vd[k].slice() };
    }
    for (const k of Object.keys(t.defs)) out[k] = { type: t.defs[k].type, defaultValue: t.defs[k].defaultValue };
    return deepFreeze(out);
  });
  getter('componentProperties', ['INSTANCE'], t => {
    const owner = defOwner(t.main), out = {};
    if (owner.type === 'COMPONENT_SET') { const pv = parseVariant(t.main.p.name); for (const k of Object.keys(pv)) out[k] = { type: 'VARIANT', value: pv[k] }; }
    for (const k of Object.keys(owner.defs)) out[k] = { type: owner.defs[k].type, value: k in t.props ? t.props[k] : owner.defs[k].defaultValue };
    return deepFreeze(out);
  });

  const METHODS = {};
  function method(names, types, fn) { for (const n of [].concat(names)) METHODS[n] = { types: new Set(types), fn: fn }; }
  const self = (that, where) => { const t = raw(that); if (!t) throw fail(where, 'called without a node as "this"'); return alive(t); };
  const descendants = t => { const out = []; walk(t, n => out.push(n)); return out; };
  const predOf = (fn, where) => { if (fn !== undefined && typeof fn !== 'function') throw fail(where, 'Expected a callback, got ' + show(fn)); return fn || (() => true); };

  method('toString', ALL, function () { const t = raw(this); return '[Node ' + t.type + ' ' + t.id + ']'; });
  method('remove', ALL, function () {
    const t = raw(this); if (t.removed) return;
    if (t.type === 'DOCUMENT') throw fail('remove', 'The document cannot be removed');
    if (insideInstance(t)) throw fail('remove', 'Removing this node is not allowed (' + describe(t) + ' is inside an instance)');
    if (t === currentPage) throw fail('remove', 'Cannot remove the current page');
    detach(t); drop(t);
  });
  method('getPluginData', ALL, function (k) { return self(this, 'getPluginData').plugin[k] || ''; });
  method('setPluginData', ALL, function (k, v) { if (typeof v !== 'string') throw fail('setPluginData', 'Expected a string value'); self(this, 'setPluginData').plugin[k] = v; });
  method('appendChild', CONTAINERS, function (child) {
    const c = raw(child); if (!c) throw fail('appendChild', 'Expected a node, got ' + show(child));
    attach(self(this, 'appendChild'), c, null, 'appendChild');
  });
  method('insertChild', CONTAINERS, function (index, child) {
    const c = raw(child); if (!c) throw fail('insertChild', 'Expected a node, got ' + show(child));
    attach(self(this, 'insertChild'), c, index, 'insertChild');
  });
  method('findAll', CONTAINERS, function (fn) { const f = predOf(fn, 'findAll'); return descendants(self(this, 'findAll')).map(n => n.proxy).filter(n => f(n)); });
  method('findOne', CONTAINERS, function (fn) { const f = predOf(fn, 'findOne'); for (const n of descendants(self(this, 'findOne'))) if (f(n.proxy)) return n.proxy; return null; });
  method('findChildren', CONTAINERS, function (fn) { const f = predOf(fn, 'findChildren'); return self(this, 'findChildren').kids.map(n => n.proxy).filter(n => f(n)); });
  method('findChild', CONTAINERS, function (fn) { const f = predOf(fn, 'findChild'); for (const n of self(this, 'findChild').kids) if (f(n.proxy)) return n.proxy; return null; });
  method('findAllWithCriteria', CONTAINERS, function (criteria) {
    if (!criteria || !Array.isArray(criteria.types)) throw fail('findAllWithCriteria', 'Expected {types: [...]}, got ' + show(criteria));
    for (const k of Object.keys(criteria)) if (k !== 'types') throw fail('findAllWithCriteria', 'Criteria key ' + show(k) + ' is not supported by the mock');
    return descendants(self(this, 'findAllWithCriteria')).filter(n => criteria.types.indexOf(n.type) >= 0).map(n => n.proxy);
  });
  method('loadAsync', ['PAGE'], function () { self(this, 'loadAsync'); return Promise.resolve(); });
  function resize(that, w, h, where) {
    const t = self(that, where);
    if (!isNum(w) || !isNum(h)) throw fail(where, 'Expected two numbers, got ' + show(w) + ', ' + show(h));
    if (w < 0.01 || h < 0.01) throw fail(where, 'Width and height must be at least 0.01, got ' + w + ' x ' + h + ' (' + describe(t) + ')');
    t.w = w; t.h = h;
    if (t.type === 'INSTANCE') t.resized = true;
    if (isAL(t)) { t.p.primaryAxisSizingMode = 'FIXED'; t.p.counterAxisSizingMode = 'FIXED'; }
    if (t.type === 'TEXT' && t.p.textAutoResize === 'WIDTH_AND_HEIGHT') t.p.textAutoResize = 'NONE';
  }
  method('resize', SCENE, function (w, h) { resize(this, w, h, 'resize'); });
  method('resizeWithoutConstraints', SCENE, function (w, h) { resize(this, w, h, 'resizeWithoutConstraints'); });
  method('setBoundVariable', SCENE, function (field, variable) {
    const t = self(this, 'setBoundVariable');
    if (!variable) throw fail('setBoundVariable', 'No variable given for field ' + show(field) + ' on ' + describe(t) + ' (got ' + show(variable) + ')');
    const v = getVar(variable, 'setBoundVariable');
    const want = BIND[t.type][field];
    if (!want) throw fail('setBoundVariable', 'Field ' + show(field) + ' cannot be bound to a variable on a ' + t.type + ' node');
    if (v.resolvedType !== want) throw fail('setBoundVariable', 'Field ' + show(field) + ' needs a ' + want + ' variable, but ' + show(v.name) + ' is ' + v.resolvedType);
    if (/^(min|max)(Width|Height)$/.test(field) && !isAL(t) && !isAL(t.parent))
      throw fail('setBoundVariable', 'Can only set ' + field + ' on auto-layout frames and their children (' + describe(t) + ')');
    t.bound[field] = v.id;
    if (t.type === 'INSTANCE') t.ownBound[field] = v.id;
  });
  method('setExplicitVariableModeForCollection', SCENE.concat(['PAGE']), function (collection, modeId) {
    const t = self(this, 'setExplicitVariableModeForCollection'), coll = getColl(collection, 'setExplicitVariableModeForCollection');
    if (!coll[RAW].modeList.some(m => m.modeId === modeId))
      throw fail('setExplicitVariableModeForCollection', 'Mode ' + show(modeId) + ' does not exist in collection ' + show(coll.name));
    t.modes[coll.id] = modeId;
  });
  method('clearExplicitVariableModeForCollection', SCENE.concat(['PAGE']), function (collection) {
    delete self(this, 'clearExplicitVariableModeForCollection').modes[getColl(collection, 'clearExplicitVariableModeForCollection').id];
  });
  method('clone', SCENE, function () {
    const t = self(this, 'clone');
    if (t.type === 'COMPONENT_SET') throw fail('clone', 'Cloning a component set is not supported by the mock');
    const c = cloneRaw(t);
    if (c.type === 'COMPONENT') c.defs = Object.assign({}, t.defs);
    const inInstance = insideInstance(t);
    if (inInstance) c.p.componentPropertyReferences = null;
    const host = t.parent && !inInstance ? t.parent : currentPage;
    c.parent = host; host.kids.push(c);
    return c.proxy;
  });
  method('createInstance', ['COMPONENT'], function () {
    const t = self(this, 'createInstance');
    const i = newRaw('INSTANCE');
    fillInstance(i, t, 'new');
    i.parent = currentPage; currentPage.kids.push(i);
    return i.proxy;
  });
  method('addComponentProperty', ['COMPONENT', 'COMPONENT_SET'], function (name, type, defaultValue) {
    const t = self(this, 'addComponentProperty');
    if (t.type === 'COMPONENT' && t.parent && t.parent.type === 'COMPONENT_SET')
      throw fail('addComponentProperty', 'Can only add a component property to a component set or a non-variant component (' + describe(t) + ' is a variant)');
    if (typeof name !== 'string' || !name) throw fail('addComponentProperty', 'Expected a property name, got ' + show(name));
    if (['BOOLEAN', 'TEXT', 'INSTANCE_SWAP'].indexOf(type) < 0) throw fail('addComponentProperty', 'Expected type BOOLEAN | TEXT | INSTANCE_SWAP, got ' + show(type) + ' (VARIANT properties come from variant names)');
    checkPropValue(type, defaultValue, 'addComponentProperty', show(name));
    const key = name + '#' + t.id.split(':').pop() + ':' + (t.defSeq++);
    t.defs[key] = { type: type, defaultValue: defaultValue };
    return key;
  });
  method('setProperties', ['INSTANCE'], function (props) {
    const t = self(this, 'setProperties');
    if (!props || typeof props !== 'object') throw fail('setProperties', 'Expected an object, got ' + show(props));
    const owner = defOwner(t.main), vd = owner.type === 'COMPONENT_SET' ? variantDefs(owner) : {};
    const want = {}, others = [];
    for (const key of Object.keys(props)) {
      const value = props[key];
      if (Object.prototype.hasOwnProperty.call(vd, key)) {
        if (typeof value !== 'string') throw fail('setProperties', 'Variant property ' + show(key) + ' of ' + owner.p.name + ' expects a string, got ' + show(value));
        if (vd[key].indexOf(value) < 0) throw fail('setProperties', 'Variant property ' + show(key) + ' of ' + owner.p.name + ' has no value ' + show(value) + '. Valid values: ' + vd[key].join(', '));
        want[key] = value; continue;
      }
      const def = owner.defs[key];
      if (!def) throw fail('setProperties', 'Could not find a component property with name ' + show(key) + ' on ' + owner.p.name + '. Existing: ' + Object.keys(vd).concat(Object.keys(owner.defs)).join(', '));
      checkPropValue(def.type, value, 'setProperties', show(key) + ' of ' + owner.p.name);
      others.push([key, value, def.type]);
    }
    let rebuilt = false;
    if (Object.keys(want).length) {
      const full = Object.assign(parseVariant(t.main.p.name), want), names = Object.keys(full);
      const target = owner.kids.find(c => { const pv = parseVariant(c.p.name); return Object.keys(pv).length === names.length && names.every(k => pv[k] === full[k]); });
      if (!target) throw fail('setProperties', 'No variant "' + variantLabel(full) + '" in ' + owner.p.name);
      if (target !== t.main) { for (const o of others) t.props[o[0]] = o[1]; fillInstance(t, target, 'variant'); rebuilt = true; }
    }
    if (!rebuilt) for (const o of others) { t.props[o[0]] = o[1]; applyProp(t, o[0], o[1], o[2]); }
  });
  method('getMainComponentAsync', ['INSTANCE'], function () { const t = self(this, 'getMainComponentAsync'); return Promise.resolve(t.main ? t.main.proxy : null); });
  method('swapComponent', ['INSTANCE'], function (comp) { this.mainComponent = comp; });
  method('detachInstance', ['INSTANCE'], function () {
    const t = self(this, 'detachInstance');
    if (insideInstance(t)) throw fail('detachInstance', 'Cannot detach an instance that is inside another instance');
    t.type = 'FRAME'; delete t.main; delete t.props;
    walk(t, n => { n.p.componentPropertyReferences = null; });
    return t.proxy;
  });
  method('setTextStyleIdAsync', ['TEXT'], function (id) {
    let t;
    try { t = self(this, 'setTextStyleIdAsync'); } catch (e) { return Promise.reject(e); }
    const s = textStyles.get(id);
    if (!s) return Promise.reject(fail('setTextStyleIdAsync', 'Text style ' + show(id) + ' does not exist'));
    if (!loadedFonts.has(fontKey(s.fontName))) return Promise.reject(fail('setTextStyleIdAsync', 'Font "' + s.fontName.family + ' ' + s.fontName.style + '" of style ' + s.name + ' is not loaded'));
    Object.assign(t.p, { fontName: s.fontName, fontSize: s.fontSize, lineHeight: s.lineHeight, letterSpacing: s.letterSpacing, textCase: s.textCase });
    t.styleId = id;
    return Promise.resolve();
  });

  const READONLY_HINT = {
    textStyleId: 'use setTextStyleIdAsync(id)', fillStyleId: 'use setFillStyleIdAsync(id)', strokeStyleId: 'use setStrokeStyleIdAsync(id)',
    width: 'use resize(w, h) or setBoundVariable("width", v)', height: 'use resize(w, h) or setBoundVariable("height", v)',
  };

  // Real Plugin API members that the mock does not model. Touching one is reported as a mock gap, not as a builder bug.
  const UNIMPLEMENTED = new Set(('absoluteBoundingBox absoluteRenderBounds absoluteTransform relativeTransform exportAsync exportSettings fillStyleId strokeStyleId ' +
    'effectStyleId gridStyleId layoutGrids backgrounds guides exposedInstances isExposedInstance overrides instances getInstancesAsync resetOverrides ' +
    'editComponentProperty deleteComponentProperty documentationLinks vectorPaths vectorNetwork arcData hyperlink reactions constrainProportions ' +
    'lockAspectRatio unlockAspectRatio targetAspectRatio rescale flatten outlineStroke getStyledTextSegments insertCharacters deleteCharacters ' +
    'getRangeFontName getRangeFontSize getRangeFills setRangeFontName setRangeFontSize setRangeFills setRangeTextCase setRangeLineHeight ' +
    'setRangeLetterSpacing setRangeTextStyleIdAsync setRangeHyperlink setRangeBoundVariable setFillStyleIdAsync setStrokeStyleIdAsync ' +
    'setEffectStyleIdAsync setGridStyleIdAsync getSharedPluginData setSharedPluginData getPluginDataKeys setRelaunchData getRelaunchData ' +
    'strokeMiterLimit strokeGeometry fillGeometry leadingTrim hangingPunctuation hangingList autoRename fontWeight openTypeFeatures textStyleRange ' +
    'expanded stuckNodes attachedConnectors inferredAutoLayout inferredVariables resolvedVariableModes annotations devStatus isAsset getCSSAsync ' +
    'getDevResourcesAsync numberOfFixedChildren overflowDirection overlayPositionType overlayBackground prototypeStartNode flowStartingPoints ' +
    'findWidgetNodesByWidgetId scaleFactor componentPropertyDefinitionsAsync variantGroupProperties setDevResourcePreviewAsync').split(' '));
  const gap = (t, key) => new Error('mock-figma: ' + t.type + ' node member "' + key + '" exists in the Plugin API but is not implemented in test/mock-figma.js');

  // ------------------------------------------------------------------ node proxy
  const SHOWN = ['id', 'type', 'name'];
  const handler = {
    get(t, key) {
      if (key === RAW) return t;
      if (typeof key === 'symbol') return undefined;
      const g = GETTERS[key];
      if (g && g.types.has(t.type)) return g.get(t);
      const def = PROPS.get(key);
      if (def && def.types.has(t.type)) {
        if (def.get) return def.get(t);
        const b = t.bound[key];
        if (b && variables.has(b)) { const v = resolveVar(variables.get(b), t); if (v !== undefined) return v; }
        return t.p[key];
      }
      const m = METHODS[key];
      if (m && m.types.has(t.type)) return m.fn;
      if (UNIMPLEMENTED.has(key) && !READONLY_HINT[key]) throw gap(t, key);
      return undefined;
    },
    set(t, key, value) {
      const def = typeof key === 'string' ? PROPS.get(key) : null;
      if (!def || !def.types.has(t.type)) {
        const known = typeof key === 'string' && ((GETTERS[key] && GETTERS[key].types.has(t.type)) || (METHODS[key] && METHODS[key].types.has(t.type)) || READONLY_HINT[key]);
        if (known) throw new Error("Cannot assign to read only property '" + key + "' of " + t.type + ' node' + (READONLY_HINT[key] ? ' (' + READONLY_HINT[key] + ')' : ''));
        if (typeof key === 'string' && UNIMPLEMENTED.has(key)) throw gap(t, key);
        throw new Error('object is not extensible: ' + String(key));
      }
      alive(t);
      const v = def.check(t, value, key);
      if (def.set) def.set(t, v, key); else t.p[key] = v;
      if (def.after) def.after(t);
      return true;
    },
    has(t, key) {
      return typeof key === 'string' && !!((GETTERS[key] && GETTERS[key].types.has(t.type)) || (PROPS.has(key) && PROPS.get(key).types.has(t.type)) || (METHODS[key] && METHODS[key].types.has(t.type)));
    },
    ownKeys() { return SHOWN; },
    getOwnPropertyDescriptor(t, key) {
      return SHOWN.indexOf(key) >= 0 ? { value: key === 'name' ? t.p.name : t[key], enumerable: true, configurable: true, writable: true } : undefined;
    },
    defineProperty(t, key) { throw new Error('object is not extensible: ' + String(key)); },
    deleteProperty(t, key) { throw new Error('Cannot delete property ' + String(key) + ' of ' + t.type + ' node'); },
    setPrototypeOf() { throw new Error('object is not extensible: prototype'); },
  };
  function newRaw(type, id) {
    const t = { id: id || nextId(), type: type, parent: null, removed: false, p: Object.assign({}, DEFAULTS[type]), w: 100, h: 100, bound: {}, modes: {}, plugin: {}, fill: { h: false, v: false } };
    if (CONTAINERS.indexOf(type) >= 0) t.kids = [];
    if (type === 'COMPONENT' || type === 'COMPONENT_SET') { t.defs = {}; t.defSeq = 0; }
    if (type === 'INSTANCE') { t.props = {}; t.ownBound = {}; t.resized = false; }
    Object.defineProperty(t, util.inspect.custom, { configurable: true, value: () => '[' + t.type + ' ' + t.id + ' ' + JSON.stringify(t.p.name) + ']' });
    t.proxy = new Proxy(t, handler);
    nodes.set(t.id, t);
    return t;
  }
  function create(type, name, init) {
    const t = newRaw(type);
    t.p.name = name;
    if (init) init(t);
    t.parent = currentPage; currentPage.kids.push(t);
    return t.proxy;
  }

  // ------------------------------------------------------------------ document
  const root = newRaw('DOCUMENT', '0:0'); root.p.name = 'Document';
  function createPage(name) {
    const t = newRaw('PAGE', '0:' + (++seq));
    t.p.name = name || 'Page ' + (root.kids.length + 1);
    t.parent = root; root.kids.push(t);
    return t;
  }
  const atomsPage = createPage('v3 · Atoms');
  currentPage = atomsPage;

  // Collections, variables and text styles of the real file.
  function collectionWithModes(name, modeNames) {
    const c = makeCollection(name);
    c.renameMode(c.modes[0].modeId, modeNames[0]);
    for (const m of modeNames.slice(1)) c.addMode(m);
    return c;
  }
  const theme = collectionWithModes('NF2 · Tokens', THEME_MODES);
  for (const name of COLOR_NAMES) {
    const v = makeVariable('color/' + name, theme, 'COLOR');
    theme.modes.forEach(m => v.setValueForMode(m.modeId, hashColor(name + '|' + m.name)));
  }
  const density = collectionWithModes('NF3 · Density', DENSITY_MODES);
  const densityFile = options.densityFile;
  let densityTable = {};
  if (densityFile) {
    if (!fs.existsSync(densityFile)) throw new Error('mock-figma: density file not found: ' + densityFile);
    densityTable = new Function(fs.readFileSync(densityFile, 'utf8') + '\n;return typeof NF3_DENSITY === "undefined" ? null : NF3_DENSITY;')();
    if (!densityTable) throw new Error('mock-figma: ' + densityFile + ' does not define NF3_DENSITY');
  }
  const scopeOf = { S: ['WIDTH_HEIGHT'], G: ['GAP'], R: ['CORNER_RADIUS'] };
  for (const name of Object.keys(densityTable)) {
    const d = densityTable[name], v = makeVariable(name, density, 'FLOAT');
    if (scopeOf[d[3]]) v.scopes = scopeOf[d[3]];
    density.modes.forEach((m, i) => v.setValueForMode(m.modeId, d[i]));
  }
  const boolDefs = { 'show/tool-20': [true, false, false], 'show/tool-16': [false, true, true] };
  for (const name of Object.keys(boolDefs)) {
    const v = makeVariable(name, density, 'BOOLEAN');
    density.modes.forEach((m, i) => v.setValueForMode(m.modeId, boolDefs[name][i]));
  }
  for (const name of Object.keys(TEXT_STYLES)) {
    const s = TEXT_STYLES[name];
    const o = deepFreeze({ id: 'S:' + (++seq).toString(16).padStart(8, '0') + ',', type: 'TEXT', name: 'NF2/' + name, remote: false, description: '',
      fontName: { family: s[0], style: s[1] }, fontSize: s[2], lineHeight: { unit: 'PIXELS', value: s[3] },
      letterSpacing: { unit: 'PERCENT', value: s[4] || 0 }, textCase: s[5] ? 'UPPER' : 'ORIGINAL', textDecoration: 'NONE', paragraphSpacing: 0, paragraphIndent: 0 });
    textStyles.set(o.id, sealed('TextStyle', Object.assign({}, o)));
  }

  // Icon masters: one component per exported SVG, on the atoms page.
  const iconsDir = options.iconsDir;
  let iconCount = 0;
  if (iconsDir && fs.existsSync(iconsDir)) {
    let y = -400;
    for (const size of [12, 16, 20]) {
      const dir = path.join(iconsDir, String(size));
      if (!fs.existsSync(dir)) continue;
      let x = -2000;
      for (const file of fs.readdirSync(dir).filter(f => f.endsWith('.svg')).sort()) {
        const c = newRaw('COMPONENT');
        Object.assign(c.p, { name: 'NF2 Icon/' + size + '/' + file.slice(0, -4), fills: EMPTY, clipsContent: false, x: x, y: y });
        c.w = c.h = size; c.parent = atomsPage; atomsPage.kids.push(c);
        const v = newRaw('VECTOR');
        Object.assign(v.p, { name: 'Vector', fills: EMPTY, strokes: solid(0.5, 0.5, 0.5), strokeWeight: 1.5, strokeAlign: 'CENTER', constraints: deepFreeze({ horizontal: 'SCALE', vertical: 'SCALE' }) });
        v.w = v.h = size; v.parent = c; c.kids.push(v);
        x += 40; iconCount++;
      }
      y += 60;
    }
  }

  // ------------------------------------------------------------------ figma
  const ignore = new Set(['then', 'toJSON', 'constructor', 'inspect', 'nodeType', 'tagName', 'asymmetricMatch', '$$typeof']);
  function api(label, target, settable) {
    return new Proxy(target, {
      get(o, key) {
        if (typeof key === 'symbol' || key in o || ignore.has(key)) return o[key];
        throw new Error('mock-figma: ' + label + '.' + key + ' is not available (not implemented in test/mock-figma.js, or not a Plugin API member)');
      },
      set(o, key, value) {
        if (settable && settable[key]) { settable[key](value); return true; }
        if (key in o) throw new Error("Cannot assign to read only property '" + String(key) + "' of " + label);
        throw new Error('object is not extensible: ' + String(key));
      },
    });
  }
  const variablesApi = api('figma.variables', {
    getLocalVariableCollectionsAsync: () => Promise.resolve(Array.from(collections.values())),
    getLocalVariablesAsync: type => Promise.resolve(Array.from(variables.values()).filter(v => !type || v.resolvedType === type)),
    getVariableByIdAsync: id => Promise.resolve(variables.get(id) || null),
    getVariableCollectionByIdAsync: id => Promise.resolve(collections.get(id) || null),
    createVariableCollection: name => makeCollection(name),
    createVariable: (name, collection, type) => makeVariable(name, collection, type),
    createVariableAlias: v => deepFreeze({ type: 'VARIABLE_ALIAS', id: getVar(v, 'createVariableAlias').id }),
    setBoundVariableForPaint: (paint, field, variable) => {
      if (field !== 'color') throw fail('setBoundVariableForPaint', 'Only the "color" field can be bound, got ' + show(field));
      if (!paint || paint.type !== 'SOLID') throw fail('setBoundVariableForPaint', 'Expected a SOLID paint, got ' + show(paint));
      if (!variable) throw fail('setBoundVariableForPaint', 'No variable given (got ' + show(variable) + ')');
      const v = getVar(variable, 'setBoundVariableForPaint');
      if (v.resolvedType !== 'COLOR') throw fail('setBoundVariableForPaint', 'Variable ' + show(v.name) + ' is ' + v.resolvedType + ', expected COLOR');
      cPaint(paint, 'setBoundVariableForPaint');
      const out = JSON.parse(JSON.stringify(paint));
      out.boundVariables = { color: { type: 'VARIABLE_ALIAS', id: v.id } };
      return deepFreeze(out);
    },
  });
  const shape = (type, name, w, h) => () => create(type, name, t => { t.w = w; t.h = h; });
  const figma = api('figma', {
    mixed: MIXED,
    editorType: 'figma',
    root: root.proxy,
    variables: variablesApi,
    viewport: { center: { x: 0, y: 0 }, zoom: 1, scrollAndZoomIntoView() {} },
    get currentPage() { return currentPage.proxy; },
    setCurrentPageAsync: page => {
      const p = raw(page);
      if (!p || p.type !== 'PAGE' || p.removed) return Promise.reject(fail('setCurrentPageAsync', 'Expected an existing PageNode, got ' + show(page)));
      currentPage = p;
      return Promise.resolve();
    },
    createPage: () => createPage().proxy,
    createFrame: shape('FRAME', 'Frame', 100, 100),
    createComponent: shape('COMPONENT', 'Component', 100, 100),
    createRectangle: shape('RECTANGLE', 'Rectangle', 100, 100),
    createEllipse: shape('ELLIPSE', 'Ellipse', 100, 100),
    createPolygon: shape('POLYGON', 'Polygon', 100, 100),
    createStar: shape('STAR', 'Star', 100, 100),
    createLine: () => create('LINE', 'Line', t => { t.w = 100; t.h = 0; t.p.strokes = BLACK; }),
    createVector: shape('VECTOR', 'Vector', 100, 100),
    createText: () => create('TEXT', 'Text', t => { t.w = 0; t.h = 0; }),
    combineAsVariants: (components, parent, index) => {
      if (!Array.isArray(components) || !components.length) throw fail('combineAsVariants', 'Expected a non-empty array of components');
      const host = raw(parent);
      if (!host || !host.kids || host.type === 'DOCUMENT') throw fail('combineAsVariants', 'Expected a page or frame as parent, got ' + show(parent));
      alive(host);
      const page = pageOf(host);
      const list = components.map(c => {
        const t = raw(c);
        if (!t || t.type !== 'COMPONENT') throw fail('combineAsVariants', 'Expected ComponentNodes, got ' + (t ? describe(t) : show(c)));
        alive(t);
        if (t.parent && t.parent.type === 'COMPONENT_SET') throw fail('combineAsVariants', describe(t) + ' already belongs to a component set');
        if (pageOf(t) !== page) throw fail('combineAsVariants', 'Component "' + t.p.name + '" is on page "' + (pageOf(t) ? pageOf(t).p.name : '(none)') + '" but the new parent is on page "' + (page ? page.p.name : '(none)') + '": grouped nodes must be on the same page as the parent');
        return t;
      });
      const set = newRaw('COMPONENT_SET');
      Object.assign(set.p, { name: 'Component 1', fills: EMPTY, clipsContent: true });
      attach(host, set, index == null ? null : index, 'combineAsVariants');
      for (const t of list) attach(set, t, null, 'combineAsVariants');
      set.w = Math.max.apply(null, list.map(t => sizeOf(t, 'width'))); set.h = Math.max.apply(null, list.map(t => sizeOf(t, 'height')));
      return set.proxy;
    },
    getNodeByIdAsync: id => Promise.resolve(nodes.has(id) ? nodes.get(id).proxy : null),
    loadFontAsync: font => {
      if (!font || typeof font.family !== 'string' || typeof font.style !== 'string') return Promise.reject(fail('loadFontAsync', 'Expected {family, style}, got ' + show(font)));
      loadedFonts.add(fontKey(font));
      return Promise.resolve();
    },
    getLocalTextStylesAsync: () => Promise.resolve(Array.from(textStyles.values())),
    getLocalPaintStylesAsync: () => Promise.resolve([]),
    getLocalEffectStylesAsync: () => Promise.resolve([]),
    getStyleByIdAsync: id => Promise.resolve(textStyles.get(id) || null),
    notify: () => ({ cancel() {} }),
    closePlugin: () => {},
    // Harness-only information; not part of the Plugin API.
    __mock: { iconCount: iconCount, iconsDir: iconsDir || null, nodeCount: () => nodes.size },
  }, {
    currentPage: () => { throw new Error('Cannot set figma.currentPage directly (documentAccess: dynamic-page); use await figma.setCurrentPageAsync(page)'); },
  });

  // Document-level methods live on the root node; pages are created through figma.createPage().
  return figma;
}

module.exports = { createFigma };
