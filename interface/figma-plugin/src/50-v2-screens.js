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
