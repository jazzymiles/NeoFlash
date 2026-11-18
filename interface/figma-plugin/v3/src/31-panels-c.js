// NF3 panels C (group P2): Ease, Motion editor, Timeline frame views and button-symbol timeline, the View settings,
// Swatches and Function search popovers, Test Movie window. Graphs, curves, frame cells and thumbnails are artwork
// with literal sizes; curves are plotted as dots because the builder uses no vector paths.
P2.art = (name, w, h, fill) => { const f = figma.createFrame(); f.name = name; f.resize(w, h); f.fills = fillOf(fill); f.clipsContent = true; return f; };
P2.put = (f, n, x, y) => { f.appendChild(n); n.x = x; n.y = y; return n; };
// fn(t) = [x, y] in 0..1; (x0, y0) is the point of [0, 0], w and h the size of one unit.
P2.trace = (f, fn, x0, y0, w, h, tone, n) => { for (let i = 0; i <= n; i++) { const p = fn(i / n); P2.put(f, dot(2, tone), x0 + p[0] * w - 1, y0 - p[1] * h - 1); } };
P2.bez = b => s => { const u = 1 - s, a = 3 * u * u * s, c = 3 * u * s * s, d = s * s * s; return [a * b[0] + c * b[2] + d, a * b[1] + c * b[3] + d]; };
P2.key = tone => { const r = rect(6, 6, tone); r.rotation = 45; return box({ name: 'Keyframe', w: 10, h: 10, justify: 'CENTER' }, [r]); };
P2.playhead = (parent, x) => abs(parent, rect(1, Math.max(parent.height, 1), 'timeline/playhead', { name: 'Playhead' }), x, 0, { horizontal: 'MIN', vertical: 'STRETCH' });
// ---- Ease
P2.PRESETS = { 'Linear': [0.3, 0.3, 0.7, 0.7], 'Ease In': [0.6, 0, 1, 0.6], 'Ease Out': [0, 0.4, 0.4, 1], 'Ease In-Out': [0.6, 0, 0.4, 1], 'Back': [0.3, 0, 0.55, 1.3], 'Bounce': [0.2, 1.2, 0.5, 0.9], 'Elastic': [0.1, 1.4, 0.4, 0.9], 'Custom': [0.6, 0.02, 0.4, 0.98] };
P2.preset = (name, sel) => {
  const a = P2.art('Curve', 36, 24, null); P2.trace(a, P2.bez(P2.PRESETS[name]), 2, 20, 32, 14, sel ? 'accent/default' : 'text/secondary', 12);
  const t = col({ name: 'Ease preset', pad: [4, 0], gap: 2, align: 'CENTER', radius: 'radius/control', fill: sel ? 'bg/selected' : 'bg/field', stroke: sel ? 'border/focus' : 'border/default' }, [a, K.small(name, sel ? 'text/primary' : 'text/secondary')]);
  markGrow(t, true, false); return t;
};
// b = cubic bezier [x1, y1, x2, y2]; wide = value axis -20 .. 120 % with the out-of-range bands tinted.
P2.easeGraph = (b, wide) => {
  const X = 40, PW = 280, top = 14, PH = 150, lo = wide ? -0.2 : 0, hi = wide ? 1.2 : 1, sy = PH / (hi - lo), Y0 = top + hi * sy;
  const f = P2.art('Ease graph', 336, 196, 'bg/field');
  if (wide) { P2.put(f, rect(PW, 0.2 * sy, 'warning/subtle'), X, top); P2.put(f, rect(PW, 0.2 * sy, 'warning/subtle'), X, Y0); }
  for (let i = 0; i <= (wide ? 7 : 5); i++) {
    const v = lo + i * 0.2, y = Y0 - v * sy, pct = Math.round(v * 100);
    P2.put(f, rect(PW, 1, pct === 0 || pct === 100 ? 'border/strong' : 'border/subtle'), X, y); P2.put(f, txt(pct + '%', 'Mono Small', 'text/tertiary'), 4, y - 7);
  }
  for (let n = 0; n <= 40; n += 5) { const x = X + n / 40 * PW; P2.put(f, rect(1, PH, 'border/subtle'), x, top); P2.put(f, txt(String(n), 'Mono Small', 'text/tertiary'), x - 5, top + PH + 8); }
  P2.trace(f, s => [b[0] * s, b[1] * s], X, Y0, PW, sy, 'text/tertiary', 10); P2.trace(f, s => [1 + (b[2] - 1) * s, 1 + (b[3] - 1) * s], X, Y0, PW, sy, 'text/tertiary', 10);
  P2.trace(f, P2.bez(b), X, Y0, PW, sy, 'accent/default', 90);
  P2.put(f, rect(7, 7, 'text/primary'), X - 3, Y0 - 3); P2.put(f, rect(7, 7, 'text/primary'), X + PW - 3, Y0 - sy - 3);
  const handle = (x, y) => P2.put(f, dot(8, 'bg/field', { stroke: y < 0 || y > 1 ? 'warning/default' : 'accent/default', strokeWeight: 2 }), X + x * PW - 4, Y0 - y * sy - 4);
  handle(b[0], b[1]); handle(b[2], b[3]);
  return f;
};
P2.ease = (caption, sel, vals, wide, focus) => {
  const names = Object.keys(P2.PRESETS), tiles = from => row({ name: 'Presets', w: 'fill', gap: 'gap/control' }, names.slice(from, from + 4).map(n => P2.preset(n, n === sel)));
  return K.panel('Ease', { w: 360 }, [
    P2.body([K.small(caption, 'text/secondary'),
      K.line([K.select('All', { label: 'Property', w: 'width/field-m' }), K.check('One curve for all', true)]),
      tiles(0), tiles(4), P2.easeGraph(vals.map(Number), wide),
      K.line(['X1', 'Y1', 'X2', 'Y2'].map((l, i) => K.num(l, vals[i], null, { state: i === focus ? 'Focus' : 'Default' }))),
      row({ name: 'Preview track', w: 'fill', h: 'size/control', pad: [0, 'pad/field'], justify: 'SPACE_BETWEEN', radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' },
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9].map(i => i === 7 ? dot(10, 'accent/default') : dot(6, 'border/strong'))),
      K.line([K.ibtn('play'), K.ibtn('stop'), txt('Frame 30 / 40  ·  88 %', 'Mono Small', 'text/secondary')])]),
    P2.foot([K.btn('Reset', { kind: 'Ghost' }), spacer(), K.btn('Cancel'), K.btn('Apply', { kind: 'Primary' })]),
  ]);
};
// ---- Motion editor: [name, value, ease, curve fn, key positions, expanded] or [section title]
P2.ME = [['Basic motion'], ['X', '412.0 px', 'Simple (slow)', t => 0.15 + 0.7 * t + 0.2 * Math.sin(t * 6.28), [0, 0.3, 0.65, 1], 1], ['Y', '160.0 px', 'No ease', t => 0.6 - 0.3 * Math.sin(t * 3.14), [0, 0.5, 1]],
  ['Rotation Z', '0°', 'No ease', t => 0.5, [0, 1]], ['Transformation'], ['Scale X', '100 %', 'Bounce', t => 0.3 + 0.4 * Math.sin(t * 3.14), [0, 0.4, 1]], ['Scale Y', '100 %', 'Bounce', t => 0.3 + 0.4 * Math.sin(t * 3.14), [0, 0.4, 1]],
  ['Colour effect'], ['Alpha', '100 %', 'No ease', t => 0.2 + 0.6 * Math.min(1, t * 5), [0, 0.2, 1]], ['Filters']];
P2.meRow = (h, sel, kids, o) => row(Object.assign({ name: 'Row', w: 'fill', h: h, pad: [0, 'pad/panel'], gap: 'gap/control', fill: sel ? 'bg/selected' : null, stroke: 'border/subtle', sides: [0, 0, 1, 0], clip: true }, o || {}), kids);
P2.meCurve = (m, h) => {
  const a = P2.art('Curve / ' + m[0], 600, h, null), sel = !!m[5], tone = sel ? 'accent/default' : 'text/secondary';
  if (m[0] === 'Rotation Z') P2.put(a, rect(590, 1, tone), 5, h / 2); else P2.trace(a, t => [t, m[3](t)], 5, h - 5, 590, h - 10, tone, 118);
  m[4].forEach(t => P2.put(a, P2.key(tone), 5 + t * 590 - 5, h - 5 - m[3](t) * (h - 10) - 5));
  return a;
};
P2.motionEditor = () => {
  const cap = (s, w) => row({ name: s, w: w }, [txt(s, 'Overline', 'text/secondary')]), H = 'size/timeline-controls';
  const left = col({ name: 'Properties', w: 'width/dialog-m', h: 'fill', stroke: 'border/subtle', sides: [0, 1, 0, 0] }, [
    P2.meRow('size/timeline-ruler', false, [txt('Property', 'Overline', 'text/secondary', { grow: true }), cap('Value', 'width/field-s'), cap('Ease', 'width/field-m'), cap('Keyframe', 'width/field-s')])]
    .concat(P2.ME.map(m => m.length === 1
      ? P2.meRow('size/section', false, [icon('chevdown', 'text/tertiary', 12), txt(m[0], 'Overline', 'text/secondary', { grow: true }), K.ibtn('plus')], { gap: 4 })
      : P2.meRow(m[5] ? 88 : H, m[5], [txt(m[0], 'Body', 'text/primary', { grow: true }), K.field('', m[1], { w: 'width/field-s' }), K.select(m[2], { w: 'width/field-m' }),
        row({ name: 'Keyframe nav', w: 'width/field-s', gap: 'gap/control', justify: 'CENTER' }, [K.ibtn('stepback'), P2.key(m[5] ? 'accent/default' : 'text/secondary'), K.ibtn('stepfwd')])]))));
  const ruler = [1, 6, 11, 16, 21, 26, 31, 36, 41, 46, 51, 56].map(n => row({ name: String(n), w: 50, pad: [0, 3] }, [txt(String(n), 'Mono Small', 'text/tertiary')]));
  const graph = col({ name: 'Graph', w: 'fill', h: 'fill', clip: true }, [P2.meRow('size/timeline-ruler', false, ruler, { pad: 0, gap: 0, fill: 'bg/panel' })]
    .concat(P2.ME.map(m => m.length === 1 ? P2.meRow('size/section', false, [], { pad: 0 }) : P2.meRow(m[5] ? 88 : H, m[5], [P2.meCurve(m, m[5] ? 80 : 20)], { pad: 0 }))));
  P2.playhead(graph, 295);
  return K.panel('Motion editor', { tabs: ['Motion editor', 'Timeline'], w: 1120, h: 480 }, [
    P2.dockBar([txt('Wiggler', 'Body Strong', 'text/primary'), K.num('Freq', '3.0', 'Hz', { w: 'width/field-s' }), K.num('Mag', '12.0', 'px', { w: 'width/field-s' }), K.btn('Apply wiggler'), divider(true),
      txt('Motion sketch', 'Body Strong', 'text/primary'), K.num('Smooth', '0.0', null, { w: 'width/field-s' }), K.btn('Capture sketch')]),
    P2.main([left, graph]),
    row({ name: 'Footer', w: 'fill', h: 'size/timeline-controls', pad: [0, 'pad/panel'], gap: 'gap/section', stroke: 'border/subtle', sides: [1, 0, 0, 0] },
      [K.t('Graph size'), K.slider({ w: 96 }), K.t('Expanded'), K.slider({ w: 96 }), K.num('Frames', '60', null, { w: 'width/field-s', stepper: true }),
        K.small('Click a curve to edit · ⌥-drag a keyframe to copy · ⇧ snaps to frames', null, { grow: true })]),
  ]);
};
P2.motionEmpty = () => K.panel('Motion editor', { tabs: ['Motion editor', 'Timeline'], w: 480, h: 200 }, [P2.empty('tween-motion', 'No motion tween', 'Select a motion tween span on the Timeline')]);
// ---- Timeline frame views (30 frames, px points per frame)
P2.SPAN = { motion: 'timeline/motion-tween', shape: 'timeline/shape-tween', classic: 'timeline/classic-tween', static: 'timeline/static', empty: 'timeline/empty' };
P2.LAYERS = [[['motion', 9], ['static', 9], ['shape', 12]], [['static', 30]], [['empty', 4], ['classic', 18], ['static', 8]]];
P2.thumb = (hex, ctx, i) => {
  const f = P2.art('Thumbnail', 56, 32, '#141926');
  if (ctx) P2.put(f, dot(22, null, { stroke: '#44546D' }), 28, 5);
  P2.put(f, dot(10, hex), ctx ? 8 + (i % 4) * 5 : 23, 11); return f;
};
// o: { plain: tween spans not tinted, short: 14 pt rows, preview: thumbnails, ctx: thumbnails in the stage context }
P2.span = (s, px, o, li) => {
  const kind = s[0], w = s[1] * px, tw = kind !== 'static' && kind !== 'empty', key = kind === 'empty' ? dot(5, null, { stroke: 'timeline/marker' }) : dot(5, 'timeline/marker');
  const kids = o.preview ? (kind === 'empty' ? [key] : Array.from({ length: Math.floor((w - 2) / 60) }, (x, i) => P2.thumb(li === 0 ? '#FF8E4F' : '#66A0FF', o.ctx, i)))
    : [key].concat(tw ? [rect('fill', 1, 'timeline/marker'), icon('triright', 'timeline/marker', 12)] : []);
  const sp = row({ name: 'Span / ' + kind, w: w, pad: [0, 2], gap: o.preview ? 4 : 2, fill: tw && o.plain ? P2.SPAN.static : P2.SPAN[kind], clip: true }, kids);
  markGrow(sp, false, true); return sp;
};
P2.frames = (px, o) => {
  o = o || {};
  const cells = [row({ name: '1', w: 2 * px, pad: [0, 2] }, [txt('1', 'Mono Small', 'text/secondary')])];
  for (let n = 5; n <= 30; n += 5) cells.push(row({ name: String(n), w: 5 * px, justify: 'CENTER' }, [txt(String(n), 'Mono Small', 'text/tertiary')]));
  const c = col({ name: 'Frames ' + px, w: 30 * px, clip: true }, [
    row({ name: 'Frame ruler', w: 'fill', h: 'size/timeline-ruler', fill: 'bg/panel', stroke: 'border/subtle', sides: [0, 0, 1, 0], clip: true, align: 'MAX', pad: [0, 0, 3, 0] }, cells)]
    .concat(P2.LAYERS.map((l, li) => row({ name: 'Frames', w: 'fill', h: o.preview ? 40 : o.short ? 14 : 'size/layer-row', fill: 'timeline/empty', stroke: 'timeline/line', sides: [0, 0, 1, 0], clip: true }, l.map(s => P2.span(s, px, o, li))))));
  P2.playhead(c, 12.5 * px);
  return c;
};
P2.mode = (name, hint, node) => col({ name: 'Mode / ' + name, gap: 6 }, [row({ name: 'Head', gap: 'gap/section' }, [txt(name, 'Body Strong', 'text/primary'), K.small(hint)]), node]);
P2.frameViews = () => {
  const c = (l, on, o) => K.mi(l, Object.assign({ check: !!on }, o || {}));
  return P2.states([
    P2.state('Panel menu (⋯)', K.menu('Timeline view', [c('Placement', 0, { sub: true, state: 'Hover' }), K.msep(), c('Tiny'), c('Small'), c('Normal', 1), c('Medium'), c('Large'), K.msep(), c('Preview'), c('Preview in context'), K.msep(),
      c('Short'), c('Tinted frames', 1), K.msep(), c('Show seconds on ruler', 1), c('Snap playhead to keyframes'), K.msep(), c('Help', 0, { key: 'F1' }), c('Close panel')], { w: 'width/inspector' })),
    P2.state('Placement submenu', K.menu('Placement', [c('Bottom (docked)', 1), c('Top'), c('Left'), c('Right'), K.msep(), c('Floating window')])),
    col({ name: 'Modes', gap: 20 }, [
      P2.mode('Tiny', '6 pt per frame', P2.frames(6)), P2.mode('Small', '9 pt per frame', P2.frames(9)), P2.mode('Normal (default)', '12 pt per frame · rows size/layer-row', P2.frames(12)),
      P2.mode('Medium', '16 pt per frame', P2.frames(16)), P2.mode('Large', '22 pt per frame', P2.frames(22)), P2.mode('Short', 'rows 14 pt — more layers on screen', P2.frames(12, { short: true })),
      P2.mode('Tinted frames off', 'tween spans not colour-coded, only arrows', P2.frames(12, { plain: true })),
      P2.mode('Preview', 'thumbnails of each keyframe span, scaled to fit', P2.frames(22, { preview: true })),
      P2.mode('Preview in context', 'thumbnails show the object inside the full stage', P2.frames(22, { preview: true, ctx: true }))])]);
};
// ---- Timeline of a button symbol: four state frames, 84 pt each
P2.buttonTimeline = () => {
  const layer = (name, ic, sel) => row({ name: 'Layer row', w: 'fill', h: 'size/layer-row', pad: [0, 6, 0, 8], gap: 4, fill: sel ? 'timeline/row-selected' : null, stroke: 'timeline/line', sides: [0, 0, 1, 0] },
    [icon(ic, 'text/secondary', 12), txt(name, 'Small', 'text/primary', { grow: true }), dot(4, 'text/tertiary'), dot(4, 'text/tertiary'), rect(8, 8, 'layer/blue', { radius: 2 })]);
  const cell = on => { const r = row({ name: 'State frame', w: 84, pad: [0, 4], fill: on ? 'timeline/static' : 'timeline/empty', stroke: 'timeline/line', sides: [0, 1, 0, 0] }, [on ? dot(5, 'timeline/marker') : dot(5, null, { stroke: 'timeline/marker' })]); markGrow(r, false, true); return r; };
  const fr = cells => row({ name: 'Frames', h: 'size/layer-row', stroke: 'timeline/line', sides: [0, 0, 1, 0] }, cells.map(cell));
  const layers = col({ name: 'Layer column', w: 'width/layer-column', stroke: 'border/subtle', sides: [0, 1, 0, 0] }, [
    row({ name: 'Layers header', w: 'fill', h: 'size/timeline-ruler', pad: [0, 6, 0, 8], stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [txt('Layers', 'Overline', 'text/secondary', { grow: true })]),
    layer('label', 'layer'), layer('shape', 'layer', true), layer('hit area', 'guide')]);
  const frames = col({ name: 'Frame area' }, [
    row({ name: 'States', h: 'size/timeline-ruler', fill: 'bg/panel', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, ['Up', 'Over', 'Down', 'Hit'].map((s, i) => row({ name: s, w: 84, pad: [0, 4] }, [txt(s, 'Small Strong', i === 1 ? 'text/primary' : 'text/secondary')]))),
    fr([1, 1, 1, 0]), fr([1, 1, 1, 1]), fr([0, 0, 0, 1])]);
  P2.playhead(frames, 126);
  const state = (label, fill, text, tone, on) => col({ name: 'State ' + label, gap: 6, align: 'CENTER' }, [
    box({ name: 'Button', w: 160, h: 44, radius: 8, fill: fill, stroke: on == null ? 'accent/default' : null, justify: 'CENTER' }, [txt(text, 'Small Strong', tone)]), K.small(label, on ? 'accent/default' : 'text/tertiary')]);
  return col({ name: 'Button symbol', gap: 16 }, [
    K.panel('Timeline — button symbol', { tabs: ['Timeline'], w: 720 }, [
      row({ name: 'Breadcrumb', w: 'fill', h: 'size/timeline-controls', pad: [0, 'pad/panel'], gap: 'gap/inline', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [icon('scene'), K.t('Scene 1'), icon('chevright', 'text/tertiary', 12),
        row({ name: 'Current', h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/control', fill: 'bg/raised', stroke: 'border/default' }, [icon('button-symbol', 'accent/default'), txt('cta_btn', 'Body Strong', 'text/primary')])]),
      row({ name: 'Timeline body', w: 'fill', align: 'MIN', clip: true }, [layers, frames])]),
    row({ name: 'State previews', gap: 12, align: 'MIN' }, [state('Up', '#1E2333', 'BUILT TO BE INTERACTIVE', '#F1F4FA', false), state('Over  ·  editing', '#35416B', 'BUILT TO BE INTERACTIVE', '#F1F4FA', true),
      state('Down', '#141926', 'BUILT TO BE INTERACTIVE', '#8B9AB1', false), state('Hit', 'accent/subtle', 'hit area (invisible)', 'accent/default')]),
    K.small('Button symbols get a 4-frame timeline: Up · Over · Down · Hit. Hit is never drawn — it only defines the clickable area.', 'text/secondary')]);
};
// ---- Popovers
P2.viewSettings = () => P2.popover('View settings', {}, [
  row({ name: 'Header', w: 'fill', h: 'size/tab', pad: [0, 4, 0, 'pad/panel'], stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [txt('Rulers, guides & grid', 'Body Strong', 'text/primary', { grow: true }), K.ibtn('x')]),
  K.section('Rulers'),
  K.body([K.check('Show rulers', true), K.select('Pixels', { label: 'Units' }), K.line([K.t('Labels'), K.seg(['Auto', 'Every'], 0), K.num('', '100', 'px', { state: 'Disabled' })]),
    K.small('Auto picks 10 / 50 / 100 / 500 units so labels never overlap at the current zoom.', null, { grow: true, wrap: true })]),
  K.section('Guides'),
  K.body([K.line([K.check('Show', true), K.check('Snap', true), K.check('Lock', false)]), K.line([K.swatch('#2EC4E6'), S.hex('#2EC4E6')]), K.select('Normal', { label: 'Snap' })]),
  K.section('Grid', true, { action: 'link' }),
  K.body([K.line([K.check('Show', true), K.check('Snap', false), K.check('Over objects', false)]), K.pair(K.num('↔', '20', 'px'), K.num('↕', '20', 'px')),
    K.line([K.swatch('#FFFFFF'), S.hex('#FFFFFF', 14)]), K.select('Normal', { label: 'Snap' })]),
  P2.foot([K.btn('Clear guides', { kind: 'Ghost' }), spacer(), K.btn('Save as default')]),
]);
P2.FN = [['Window > Panels > Color', '⌥⌘C', 1], ['Modify > Shape > Convert lines to fills'], ['Edit > Find and replace > Colour', '⌘F'], ['Window > Panels > Swatches', '⌘F9'], ['Document > Document settings > Background', '⌘J'], ['Help > NeoFlash Help & Reference…']];
P2.functionSearch = () => P2.popover('Function search', { w: 'width/dialog-m' }, [
  row({ name: 'Search', w: 'fill', pad: ['pad/panel', 'pad/panel', 'gap/row', 'pad/panel'] }, [K.search('color', { state: 'Focus', clear: true })]),
  P2.title('Menu items'),
  P2.list(P2.FN.map(f => K.mi(f[0], { key: f[1], state: f[2] ? 'Hover' : 'Default' }))),
  row({ name: 'Keyboard help', w: 'fill', pad: ['gap/control', 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [1, 0, 0, 0] },
    [K.kbd('↑↓'), K.small('move'), K.kbd('↵'), K.small('run'), K.kbd('Esc'), K.small('close'), spacer(), K.kbd('F1'), K.btn('Help & Reference…')]),
]);
// ---- Test Movie window
P2.testMovie = () => col({ name: 'Window / Test Movie', w: 800, fill: 'bg/panel', stroke: 'border/default', radius: 'radius/panel', shadow: 'dialog', clip: true }, [
  row({ name: 'Title bar', w: 'fill', h: 'size/top-bar', pad: [0, 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [0, 0, 1, 0] },
    [dot(10, '#FF5F57'), dot(10, '#FEBC2E'), dot(10, '#28C840'), txt('Ideas in motion — Test Movie', 'Body Strong', 'text/primary'), spacer(), K.badge('24 / 24 fps · 38 MB'), K.ibtn('globe'), K.ibtn('sidebar', { state: 'Selected' })]),
  row({ name: 'Stage area', w: 'fill', pad: [24, 0], justify: 'CENTER', fill: 'bg/canvas' }, [S.stageArt()]),
  row({ name: 'Transport', w: 'fill', h: 'size/timeline-controls', pad: [0, 'pad/panel'], gap: 'gap/control', stroke: 'border/subtle', sides: [1, 0, 1, 0] },
    [K.ibtn('first'), K.ibtn('stepback'), K.ibtn('pause', { kind: 'Outlined', state: 'Selected' }), K.ibtn('stepfwd'), K.ibtn('last'), K.ibtn('loop', { state: 'Selected' }), K.progress(),
      txt('Frame 30 / 120 · 00:01.21', 'Mono Small', 'text/secondary'), K.select('1×', { label: 'Speed', w: 'width/field-m' }), K.ibtn('refresh')]),
  col({ name: 'Output drawer', w: 'fill', pad: ['pad/menu', 0], fill: 'bg/field' },
    [['info', null, 'trace: NeoFlash: the stage is ready'], ['info', null, 'trace: animator started (60 frames)'], ['warning', null, 'Font "Inter Semi Bold" not embedded — fallback']].map(P2.logRow)),
]);
async function nf3BuildPanelsC() {
  nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Panel / Ease', 'Explicit width 360 pt. Property select with its label and One curve for all in one row, eight preset tiles in two rows, the graph, four equal X1 Y1 X2 Y2 fields, the preview track with Play / Stop, and Reset · Cancel · Apply in the footer. Overshoot and undershoot widen the value axis to −20 … 120 % and tint the out-of-range bands.',
    [P2.states([P2.state('Normal, 0 – 100 %', P2.ease('Classic tween · ball · frames 1–40', 'Custom', ['0.60', '0.02', '0.40', '0.98'], false, -1)),
      P2.state('Overshoot above 100 %', P2.ease('Back out · overshoots to 112 %', 'Back', ['0.30', '0.00', '0.55', '1.30'], true, 3)),
      P2.state('Undershoot below 0 %', P2.ease('Anticipation · dips to −8 % first', 'Back', ['0.35', '-0.30', '0.70', '1.00'], true, 1))])], { noteW: 1100 }));
  out.push(nfBoard(page, 'Panel / Motion editor', 'Bottom-dock panel, 1120 pt wide here. The tools row (Wiggler, Motion sketch) is the first row and exists only while a motion tween is in context; property rows are size/timeline-controls with Value, Ease and keyframe navigation in fixed columns, the selected property expands its graph; Graph size, Expanded, Frames and the hint in the footer.',
    [P2.states([P2.state('Motion tween in context', P2.motionEditor()), P2.state('No motion tween (no tools row)', P2.motionEmpty())])], { noteW: 1100 }));
  out.push(nfBoard(page, 'Panel / Timeline — frame views', 'Frame view modes of the Timeline ⋯ menu. Frame width is 6 / 9 / 12 / 16 / 22 pt (artwork); rows follow size/layer-row, Short uses fixed 14 pt rows, Preview rows are 40 pt. The menu is checkable, so every item has the check column.', [P2.frameViews()], { noteW: 1100 }));
  out.push(nfBoard(page, 'Panel / Timeline — button symbol', 'A button symbol shows four named state frames (Up · Over · Down · Hit) instead of the numbered ruler; layer rows and the breadcrumb follow the density.', [P2.buttonTimeline()], { noteW: 720 }));
  out.push(nfBoard(page, 'Popover / View settings', 'Inspector width. Rulers, Guides and Grid are sections; options are checkboxes, the label interval is a segmented choice with its field, colours are one swatch + #HEX row, the grid link moved into the section header.', [P2.viewSettings()], { noteW: 420 }));
  out.push(nfBoard(page, 'Popover / Swatches', 'The same sections as the Swatches panel tab, without Used colours.', [P2.popover('Swatches', {}, P2.swatchKids())], { noteW: 420 }));
  out.push(nfBoard(page, 'Popover / Function search', 'Search field, one row per result at size/row with its shortcut, keyboard hints as key caps and Help & Reference… at the end.', [P2.functionSearch()], { noteW: 520 }));
  out.push(nfBoard(page, 'Screen / Test Movie window', 'Window frame: title row with the performance badge, Open in browser and the Output drawer toggle; the stage; one transport row with the frame / time status, Speed and Restart; the output drawer.', [P2.testMovie()], { noteW: 800 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
