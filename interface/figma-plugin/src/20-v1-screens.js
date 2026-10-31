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
