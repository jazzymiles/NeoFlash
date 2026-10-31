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
