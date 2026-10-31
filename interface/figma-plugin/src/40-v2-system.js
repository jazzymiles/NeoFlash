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
