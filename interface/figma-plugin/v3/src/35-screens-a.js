// NF3 screens A: parameterised editor shell, Stage chrome, timeline artwork, and the first six editor screens.
// A screen is a composition of the builders in files 20–34 (S, P1, P2, D1, M3); only Stage and timeline artwork is drawn here.
const SC = {};
// The mock has no layout engine (hugging frames read 100, positions read 0): use the measured value only when it is real.
SC.real = (v, lo, fb) => (v > lo && v !== 100 ? v : fb);
SC.setProp = (i, name, value) => {
  const k = Object.keys(i.componentProperties).find(d => d.split('#')[0] === name), p = {};
  if (!k) { NF.warn.push('prop ' + name + ' on ' + i.name); return; }
  p[k] = value; i.setProperties(p);
};
// Replaces the tabs of a K.panel header. tabs: 'Label' or ['Label', icon].
SC.retab = (p, tabs, sel) => {
  const head = p.children[0];
  head.children.slice().forEach(k => k.remove());
  add(head, tabs.map((t, i) => Array.isArray(t) ? K.tab(t[0], i === (sel || 0), t[1]) : K.tab(t, i === (sel || 0))).concat([spacer(), K.ibtn('more')]));
  return p;
};
// Tools palette that fits the Stage: keeps the first tools, the rest stay behind the closing "more" button.
// room: the height the palette may take. The palette is measured; where it cannot be (mock), 205 + 30 per tool row (Normal) is used.
SC.fitTools = (cols, room) => {
  const t = S.toolsPalette(cols), g = t.children[1].children, rows = Math.ceil(g.length / cols);
  const over = Math.ceil((SC.real(t.height, 120, 205 + 30 * rows) - room) / 30);
  if (over > 0) g.slice(Math.max(2, (rows - over) * cols) - 1, g.length - 1).forEach(k => k.remove());
  return t;
};
SC.crumb = () => row({ name: 'Breadcrumb', h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', fill: 'bg/raised', stroke: 'border/default', radius: 'radius/panel' },
  [icon('scene'), txt('Scene 1', 'Body', 'text/secondary'), icon('chevright', 'text/tertiary', 12), icon('movieclip', 'accent/default'), txt('Orbit', 'Body Strong', 'text/primary'), icon('chevdown', 'text/tertiary', 12)]);
// Builders reused here leave text nodes behind when a part of them is removed: drop those from the pending text-style list before nfFlush.
SC.purge = () => {
  NF.pend = NF.pend.filter(x => { try { for (let n = x[0]; n; n = n.parent) { if (n.removed) return false; if (n.type === 'PAGE') return true; } return false; } catch (e) { return false; } });
};
SC.float = p => col({ name: 'Floating panel', radius: 'radius/panel', stroke: 'border/default', shadow: 'popover', clip: true }, [p]);

// ---- Stage artwork (literal sizes: user content, not chrome)
SC.selBox = (f, x, y, w, h) => {
  P2.put(f, rect(w, h, null, { name: 'Selection box', stroke: 'accent/default' }), x, y);
  [[0, 0], [1, 0], [0, 1], [1, 1]].forEach(c => P2.put(f, rect(6, 6, '#FFFFFF', { name: 'Handle', stroke: 'accent/default' }), x + c[0] * w - 3, y + c[1] * h - 3));
};
// The document of S.stageArt at scale k. o.dim: the parent artwork is dimmed and the Orbit symbol stays at full opacity
// (edit in place); o.sel: [x, y, w, h] selection box in document points; o.over(f, k): extra artwork.
SC.art = (k, o) => {
  o = o || {}; k = k || 1;
  const f = figma.createFrame(); f.name = 'Stage'; f.resize(720 * k, 450 * k); f.fills = [rawPaint('#141926')]; f.clipsContent = true;
  const dim = n => { if (o.dim) n.opacity = 0.3; return n; };
  const T = (s, size, hex, x, y) => { const t = figma.createText(); t.fontName = { family: 'Inter', style: 'Regular' }; t.fontSize = size * k; t.characters = s; t.fills = [rawPaint(hex)]; dim(P2.put(f, t, x * k, y * k)); };
  const E = (d, x, y, fill, stroke) => { const e = figma.createEllipse(); e.resize(d * k, d * k); e.fills = fill ? [rawPaint(fill)] : []; if (stroke) { e.strokes = [rawPaint(stroke)]; e.strokeWeight = 1.5; } P2.put(f, e, x * k, y * k); };
  dim(P2.put(f, rect(34 * k, 3, '#FF8E4F'), 56 * k, 44 * k));
  T('Ideas in motion.', 40, '#F1F4FA', 54, 70); T('One symbol. Many possibilities.', 16, '#8B9AB1', 56, 138); T('A familiar timeline. A modern canvas.', 16, '#8B9AB1', 56, 164);
  E(190, 470, 90, null, '#44546D'); E(112, 509, 129, null, '#2D3C52'); E(52, 539, 159, '#66A0FF'); E(18, 556, 82, '#FF8E4F');
  if (o.over) o.over(f, k);
  if (o.sel) SC.selBox(f, o.sel[0] * k, o.sel[1] * k, o.sel[2] * k, o.sel[3] * k);
  return f;
};

// ---- Timeline artwork: 12 pt per frame, colours from the timeline/* tokens
SC.FILL = { static: 'timeline/static', classic: 'timeline/classic-tween', shape: 'timeline/shape-tween', broken: 'timeline/classic-tween', motion: 'timeline/motion-tween',
  selected: 'timeline/selected', video: 'timeline/static', sound: 'timeline/static' };
SC.FILM = ['#8E9BC0', '#C0A58E', '#8EC0A5', '#B08EC0'];
// s = [frames, kind, o]. kind: static | empty | classic | shape | broken | motion | selected | video | sound.
// o.key: 'b' blank keyframe (default for empty), 'a' action keyframe, 0 none (default: keyframe with content); o.end: span end; o.props: n property
// keyframes; o.label / o.comment / o.anchor: frame label text; o.arrow: tween arrow on a selected span; o.thumb: Preview thumbnail.
SC.span = s => {
  const n = s[0], kind = s[1], o = s[2] || {}, m = kind === 'selected' ? 'timeline/marker-inverse' : 'timeline/marker';
  const key = o.key === 'b' || (kind === 'empty' && o.key == null) ? dot(5, null, { name: 'Blank keyframe', stroke: m })
    : o.key === 'a' ? col({ name: 'Action keyframe', align: 'CENTER' }, [txt('a', 'Mono Small', m), dot(5, m)])
      : o.key === 0 ? null : dot(5, m, { name: 'Keyframe' });
  let kids = [o.thumb ? P2.thumb('#66A0FF', true, 0) : key];
  if (kind === 'video') kids = Array.from({ length: Math.floor((n * 12 - 6) / 26) }, (x, i) => rect(24, 'fill', SC.FILM[i % 4], { name: 'Film frame', radius: 2 }));
  else if (kind === 'sound') kids = D1.wave(Math.floor((n * 12 - 6) / 6), o.h || 12, 1);
  else if (kind === 'classic' || kind === 'shape' || o.arrow) kids.push(rect('fill', 1, m, { name: 'Tween line' }), icon('triright', m, 12));
  else if (kind === 'broken') {
    const d = row({ name: 'Dashes', gap: 3, clip: true }, Array.from({ length: 48 }, () => rect(4, 1, m)));
    markGrow(d, true, false); kids.push(d);
  } else {
    if (o.label) kids.push(icon('flag', 'timeline/label', 12), txt(o.label, 'Mono Small', 'timeline/label'));
    if (o.comment) kids.push(txt('// ' + o.comment, 'Mono Small', 'timeline/comment'));
    if (o.anchor) kids.push(icon('anchor', m, 12), txt(o.anchor, 'Mono Small', 'text/primary'));
    for (let i = 0; i < (o.props || 0); i++) kids.push(spacer(), P2.key(m));
    if (o.end) kids.push(spacer(), rect(4, 7, null, { name: 'Span end', stroke: m }));
  }
  // The height is a placeholder: the span fills its layer row (children that fill need a parent with a set height).
  const sp = row({ name: 'Span / ' + kind, w: n * 12, h: 16, pad: kind === 'video' ? [3, 3] : [0, 3], gap: kind === 'sound' ? 4 : 2, fill: SC.FILL[kind], clip: true }, kids);
  markGrow(sp, false, true);
  return sp;
};
// One layer of frames; spans may also be ready nodes. o: { w, h, fill }.
SC.frames = (spans, o) => {
  o = o || {};
  return row({ name: 'Frames', w: o.w || 'fill', h: o.h || 'size/layer-row', fill: o.fill || 'timeline/empty', stroke: 'timeline/line', sides: [0, 0, 1, 0], clip: true },
    spans.map(s => Array.isArray(s) ? SC.span(s) : s));
};
// Layer row as S.layerRow, with the layer kind. o: { icon, level, chev, sel, mode (false hides it), hidden, locked, outline, color, w, h }.
SC.layer = (name, o) => {
  o = o || {};
  const folder = o.icon === 'folder' || o.icon === 'folderopen';
  return row({ name: 'Layer row', w: o.w || 'fill', h: o.h || 'size/layer-row', pad: [0, 6, 0, 8 + 12 * (o.level || 0)], gap: 4, fill: o.sel ? 'timeline/row-selected' : null, stroke: 'timeline/line', sides: [0, 0, 1, 0] }, [
    o.chev == null ? null : icon(o.chev ? 'chevdown' : 'chevright', 'text/tertiary', 12),
    icon(o.icon || 'layer', folder ? 'brand/orange' : o.sel ? 'accent/default' : 'text/secondary', 12),
    txt(name, 'Small', 'text/primary', { grow: true }),
    o.sel ? icon('pencil', 'accent/default', 12) : null,
    o.mode === false ? null : txt(o.mode || 'Normal', 'Small', 'text/tertiary'),
    o.hidden ? icon('x', 'danger/default', 12) : dot(4, 'text/tertiary'),
    o.locked ? icon('lock', 'text/secondary', 12) : dot(4, 'text/tertiary'),
    o.outline ? rect(8, 8, null, { radius: 2, stroke: o.color || 'layer/blue' }) : rect(8, 8, o.color || 'layer/blue', { radius: 2 })]);
};
// S.timeline with its three sample layers replaced. rows: [name, layer options, spans]; o: { h: row height, head: playhead frame }.
SC.timeline = (rows, o) => {
  o = o || {};
  const tl = S.timeline(), lay = tl.findOne(n => n.name === 'Layer column'), fr = tl.findOne(n => n.name === 'Frame area');
  lay.children.slice(1).forEach(k => k.remove());
  fr.children.slice(1).forEach(k => k.remove());
  add(lay, rows.map(r => SC.layer(r[0], Object.assign({ h: o.h }, r[1]))));
  add(fr, rows.map(r => SC.frames(r[2], { h: o.h })));
  if (o.head) P2.playhead(fr, (o.head - 0.5) * 12);
  return tl;
};
// The sample document: labels, two tweened layers around the layer "hand", a sound layer. hand = the spans of "hand".
SC.rows = hand => [
  ['labels', {}, [[29, 'static', { key: 'a', label: 'intro' }], [50, 'static', { comment: 'loop' }], [41, 'static', { label: 'outro', end: true }]]],
  ['ball', { mode: 'Screen' }, [[29, 'classic'], [30, 'classic'], [16, 'static', { end: true }], [45, 'empty', { key: 'b', end: true }]]],
  ['hand', { sel: true, icon: 'masked' }, hand],
  ['body', { mode: 'Multiply' }, [[18, 'classic'], [20, 'broken'], [82, 'static', { end: true }]]],
  ['music.mp3', { icon: 'sound', mode: false }, [[120, 'sound']]]];
// Timeline at the minimum window width: the zoom slider leaves the controls row (it is in the "more" menu there).
SC.narrowTimeline = () => { const tl = S.timeline(), c = tl.children[1].children; c[c.length - 2].remove(); return tl; };
// The Actions panel as a bottom-dock tab, scrolled to lines 13–20 so the code hint and the error peek are in view.
SC.actionsDock = h => {
  const p = SC.retab(P2.actions('full'), [['Timeline', 'film'], ['Actions', 'code'], 'Output', 'Compiler errors'], 1);
  const ed = p.findOne(n => n.name === 'Editor');
  ed.children.slice().forEach(k => { if (k.layoutPositioning === 'ABSOLUTE') k.y -= 240; else if (Number(k.name.slice(5)) < 13) k.remove(); });
  p.resize(p.width, h); markGrow(p, true, false);
  return p;
};

// ---- Shell
// o: { inspector: node | [nodes], bottom: node, bottomH: its height when it is not height/bottom-dock, tools: 1 | 2 | 0, stageArt: node,
//      w, h: window size literals (default width/window × height/window), split: height of the lower dock group, crumb: node replacing
//      the scene pill, zoom: text of the Stage toolbar, side: tight margins and the document centred right of the rail (default: when the Stage is lower than 500 pt) }.
// Returns { root, stage, dock, … }; call SC.chrome(sh) after the root is inside a board, then place overlays with abs on sh.root.
SC.shell = o => {
  o = o || {};
  const stage = box({ name: 'Stage area', fill: 'bg/canvas', clip: true }); markGrow(stage, true, true);
  const list = [].concat(o.inspector || S.propsDocument());
  const groups = list.map((p, i) => {
    markGrow(p, false, true);
    if (i === 0) return p;
    const g = col({ name: 'Dock group', w: 'fill', h: o.split || 330, clip: true }, [p]), l = p.children.find(n => n.name === 'List');
    if (l) { l.layoutSizingVertical = 'FILL'; l.clipsContent = true; }
    return g;
  });
  if (list.length === 1) groups.push(row({ name: 'Collapsed group', w: 'fill', h: 'size/tab', pad: [0, 4, 0, 'pad/panel'], fill: 'bg/panel', stroke: 'border/subtle', sides: [1, 0, 0, 0] },
    [txt('Library', 'Body Strong', 'text/primary', { grow: true }), K.ibtn('chevup'), K.ibtn('more')]));
  const dock = col({ name: 'Right dock', w: 'width/inspector', stroke: 'border/subtle', sides: [0, 0, 0, 1], clip: true }, groups); markGrow(dock, false, true);
  const mid = row({ name: 'Workspace', w: 'fill', align: 'MIN' }, [stage, dock]); markGrow(mid, true, true);
  const bar = S.menuBar(), status = S.statusBar(), bottom = o.bottom || S.timeline();
  const root = col({ name: 'Editor window', w: o.w || 'width/window', h: o.h || 'height/window', fill: 'bg/app', stroke: 'border/default', clip: true }, [bar, S.docTabs(), mid, bottom, status]);
  return { root, stage, dock, mid, bar, status, bottom, o, W: o.w || 1440, H: o.h || 900 };
};
// Rulers, artwork, scene pill, Tools and Stage toolbar, as S.shellChrome. Returns the measured Stage: { W, H, X, Y (in the window), rail, pill, bar, RW, RH }.
SC.chrome = sh => {
  const o = sh.o, st = sh.stage, R = 18;
  const W = SC.real(st.width, 200, sh.W - 264), H = SC.real(st.height, 150, sh.H - 88 - (o.bottomH || 248));
  const top = abs(st, rect(W, R, 'bg/panel', { name: 'Ruler top', stroke: 'border/subtle' }), 0, 0, { horizontal: 'STRETCH', vertical: 'MIN' });
  top.setBoundVariable('height', V('size/stage-ruler'));
  const left = abs(st, rect(R, H, 'bg/panel', { name: 'Ruler left', stroke: 'border/subtle' }), 0, 0, { horizontal: 'MIN', vertical: 'STRETCH' });
  left.setBoundVariable('width', V('size/stage-ruler'));
  const cols = o.tools == null ? 1 : o.tools, side = o.side != null ? o.side : H < 500, m = side ? 5 : 12, ty = R + m;
  // As S.shellChrome, the rail starts under the ruler with the scene pill beside it; it keeps the tool rows that fit above a 4 pt margin.
  const rail = cols ? SC.fitTools(cols, H - ty - 4) : null;
  const x0 = rail ? R + m + SC.real(rail.width, 20, cols === 2 ? 72 : 40) + (side ? 8 : 12) : R + 12;
  const art = o.stageArt || S.stageArt();
  abs(st, art, side ? x0 + Math.round((W - x0 - art.width) / 2) : Math.round((W - art.width) / 2) + 9, Math.round((H - art.height) / 2) + 9, { horizontal: 'CENTER', vertical: 'CENTER' });
  const pill = abs(st, o.crumb || S.scenePill(), x0, R + m);
  if (rail) abs(st, rail, R + m, ty);
  const bar = S.stageToolbar();
  if (o.zoom) bar.children.find(n => n.name === 'Zoom').children[0].characters = o.zoom;
  abs(st, bar, W - SC.real(bar.width, 150, 330) - 14, H - SC.real(bar.height, 10, 32) - 14, { horizontal: 'MAX', vertical: 'MAX' });
  return { W, H, R, X: 0, Y: SC.real(sh.mid.y, 1, 66), rail, pill, bar, RW: SC.real(sh.root.width, 200, sh.W), RH: SC.real(sh.root.height, 200, sh.H) };
};
// Modal dialog: scrim over the whole window, then the dialog in its centre. fw / fh: size used when it cannot be measured.
SC.modal = (sh, c, d, fw, fh) => {
  abs(sh.root, rect(c.RW, c.RH, 'overlay', { name: 'Scrim' }), 0, 0, { horizontal: 'STRETCH', vertical: 'STRETCH' });
  return abs(sh.root, d, Math.round((c.RW - SC.real(d.width, 200, fw)) / 2), Math.round((c.RH - SC.real(d.height, 60, fh)) / 2), { horizontal: 'CENTER', vertical: 'CENTER' });
};
// One screen board: the window, its Stage chrome, then the overlays placed by after(sh, c).
SC.board = (page, out, title, note, sh, after) => {
  out.push(nfBoard(page, title, note, [sh.root], { noteW: 1100 }));
  const c = SC.chrome(sh);
  if (after) after(sh, c);
};

// ---- Overlays of the first screens
SC.editMenus = (sh, c) => {
  const menus = sh.bar.children[2], it = menus.children.find(n => n.name === 'Edit');
  it.fills = fillOf('bg/selected'); it.children[0].fills = fillOf('text/primary');
  for (const r of ['topLeftRadius', 'topRightRadius', 'bottomLeftRadius', 'bottomRightRadius']) it.setBoundVariable(r, V('radius/control'));
  const x = SC.real(menus.x, 1, 84) + SC.real(it.x, 1, 40), y = SC.real(sh.bar.height, 10, 36);
  const a = M3.edit(), menu = abs(sh.root, a.children[1], x, y); a.remove();
  // "Timeline" is item 14 of Edit: its submenu opens beside it, one menu padding higher.
  const b = M3.editTimeline(), tl = menu.children[14];
  abs(sh.root, b.children[1], x + SC.real(menu.width, 100, 240) - 4, y + SC.real(tl ? tl.y : 0, 10, 295) - 4); b.remove();
};
SC.unsaved = (sh, c) => {
  const s = sh.status.children; s[s.length - 1].characters = 'Unsaved changes';
  const all = D1.confirmations(); all.slice(1).forEach(n => n.remove());
  SC.modal(sh, c, all[0], 360, 160);
};
SC.symbolPicker = () => P2.popover('Symbols and scenes', {}, [
  P2.body([K.search('pla', { state: 'Focus', clear: true })]),
  P2.list([M3.sec('Scenes · 4'), K.tree('Scene 1', { icon: 'scene' }),
    M3.sec('Matches in symbols · 3 of 248'), K.tree('planet', { icon: 'graphic', state: 'Selected' }), K.tree('planet_shadow', { icon: 'graphic' }), K.tree('planets_bg', { icon: 'movieclip' }),
    M3.sec('Recent'), K.tree('orbit', { icon: 'movieclip' }), K.tree('cta_btn', { icon: 'button-symbol' }),
    M3.sec('Browse'), K.tree('Backgrounds', { chev: false, meta: '41' }), K.tree('Characters', { chev: false, meta: '96' }), K.tree('UI', { chev: false, meta: '111' })]),
  P2.note('↑↓ move  ·  ↵ edit  ·  ⌥↵ edit in place  ·  ⌘↵ new window')]);

async function nf3BuildScreensA() {
  const page = await nfMaster(), out = [];
  SC.board(page, out, 'Screen / Editor — minimum window 1024 × 720', 'Smallest supported window (R-078), 1024 × 720 as literals. Dock height cap: the bottom dock keeps height/bottom-dock, which leaves a Stage row of 384 pt in Normal (422 Compact, 456 Classic Flash), above the 360 pt minimum; a taller dock is capped at window height − bars − 360. Tools overflow: the one-column rail starts under the ruler beside the scene pill, keeps the first four tools and puts the rest behind ⋯, while Hand / Zoom, the colour chips and Snap stay pinned. Stage fit: the document is shown at Fit (56 %) in the area right of the rail. The Timeline zoom slider moves into the ⋯ menu of the controls row at this width.',
    SC.shell({ w: 1024, h: 720, bottom: SC.narrowTimeline(), stageArt: SC.art(0.75), zoom: '56%' }));
  SC.board(page, out, 'Screen / Editor — Actions', 'The Actions panel is a tab of the bottom dock (380 pt here) and reuses Panel / Actions unchanged: one toolbar row, toolbox and script navigator in the sidebar, the code scrolled to the caret with the code hint and the error peek, pinned script tabs with the caret position in the footer. The Stage keeps the document at 60 %.',
    SC.shell({ bottom: SC.actionsDock(380), bottomH: 380, stageArt: SC.art(0.8), zoom: '60%' }));
  SC.board(page, out, 'Screen / Editor — Edit menu open', 'The open menu hangs under its top-bar item, which takes the selected fill; Edit ▸ Timeline opens beside its row. Both menus are the shipped Menu / File, Edit boards (Copy motion, Paste motion and Send to AI… included).',
    SC.shell(), SC.editMenus);
  SC.board(page, out, 'Screen / Editor — Convert to symbol', 'Modal state: the overlay scrim covers the whole window and Dialog / Convert to symbol (label column, Advanced collapsed) sits in its centre; the artwork being converted stays selected on the Stage and Properties shows the selection.',
    SC.shell({ inspector: P1.propsSelection(), stageArt: SC.art(1, { sel: [466, 78, 198, 206] }) }), (sh, c) => SC.modal(sh, c, D1.convertSymbol(0), 480, 306));
  SC.board(page, out, 'Screen / Editor — Unsaved changes', 'Modal confirmation from Dialog / Confirmations on the scrim: the question is the dialog title, Discard alone on the left, Cancel and Save on the right; the status bar reads “Unsaved changes” instead of the v2.25 badge in the top bar.',
    SC.shell(), SC.unsaved);
  SC.board(page, out, 'Screen / Editor — editing a symbol in place', 'The scene pill becomes the breadcrumb Scene 1 › Orbit; the parent scene is dimmed on the Stage while the symbol artwork keeps full opacity, the Timeline shows the layers of the symbol and Properties the selected instance inside it. The symbols and scenes picker (search, scenes, matches, recent, browse) opens under the breadcrumb, which sits beside the Tools rail here so the picker does not cover it.',
    SC.shell({ inspector: P1.propsGraphic(), crumb: SC.crumb(), side: true, stageArt: SC.art(1, { dim: true, sel: [535, 155, 60, 60] }),
      bottom: SC.timeline([['satellite', {}, [[40, 'classic'], [41, 'static', { end: true }]]], ['planet', { sel: true, icon: 'graphic' }, [[81, 'static', { end: true }]]], ['rings', {}, [[81, 'static', { end: true }]]]], { head: 12 }) }),
    (sh, c) => abs(sh.root, SC.symbolPicker(), c.X + c.pill.x, c.Y + c.pill.y + SC.real(c.pill.height, 10, 26) + 4));
  nfArrange(page); SC.purge();
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
