// NF3 surfaces B: reference-page helpers, Publish settings, Preferences.
const NF_MASTER = 'v3 · Normal · Light';
function nf3MoreVars() {
  const defs = { 'width/sidebar-wide': [216, 196, 180], 'width/field-m': [168, 152, 140], 'width/field-l': [232, 208, 192], 'width/field-s': [104, 92, 84],
    'width/reference': [980, 920, 880], 'width/tree': [248, 224, 208], 'width/studio': [1280, 1200, 1120], 'height/studio': [760, 700, 640] };
  const ids = ['Normal', 'Compact', 'Classic Flash'].map(n => NF.densColl.modes.find(m => m.name === n).modeId);
  for (const name of Object.keys(defs)) {
    let v = NF.v[name];
    if (!v) { v = figma.variables.createVariable(name, NF.densColl, 'FLOAT'); NF.v[name] = v; }
    v.scopes = ['WIDTH_HEIGHT']; v.setVariableCodeSyntax('WEB', 'var(--nf-' + name.replace('/', '-') + ')');
    ids.forEach((id, i) => v.setValueForMode(id, defs[name][i]));
  }
}
async function nfMaster() {
  nf3MoreVars();
  const page = nfPage(NF_MASTER);
  await figma.setCurrentPageAsync(page);
  nfMode(page, 'Light', 'Normal');
  return page;
}
function nfBoard(page, title, note, kids, o) {
  const old = page.children.find(n => n.name === title && n.type === 'FRAME' && n.children.length && n.children[0].name === 'Board title'); if (old) old.remove();
  const b = K.board(title, note, kids, o); page.appendChild(b); return b;
}
function nfArrange(page, keep) {
  for (const n of page.children.slice()) if (n.type !== 'FRAME' || !n.children.length || n.children[0].name !== 'Board title' || n.name.indexOf('Debug') === 0) n.remove();
  const order = ['Screen', 'Panel', 'Dialog', 'Menu', 'Popover', 'Board'];
  let y = 0;
  for (const g of order) {
    const bs = page.children.filter(n => n.name.indexOf(g + ' / ') === 0).sort((a, b) => a.name < b.name ? -1 : 1);
    let x = 0, rowH = 0;
    for (const b of bs) { if (x > 0 && x + b.width > 12000) { x = 0; y += rowH + 160; rowH = 0; } b.x = x; b.y = y; x += b.width + 160; rowH = Math.max(rowH, b.height); }
    if (bs.length) y += rowH + 480;
  }
  return page.children.length;
}
S.group = t => row({ name: 'Group caption', w: 'fill', minH: 'size/section', align: 'MAX', pad: [0, 0, 4, 0] }, [txt(t, 'Overline', 'text/tertiary')]);
S.navRow = (ic, label, sel, checked) => row({ name: 'Nav row', w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/check', fill: sel ? 'bg/selected' : null },
  [checked == null ? null : K.check('', checked), icon(ic, sel ? 'accent/default' : 'text/secondary'), txt(label, sel ? 'Body Strong' : 'Body', 'text/primary', { grow: true })]);
S.dialogBody = kids => { const r = row({ name: 'Dialog body', w: 'fill', align: 'MIN' }, kids); markGrow(r, true, true); return r; };
S.page = kids => { const c = col({ name: 'Page', gap: 'gap/row', pad: ['pad/panel', 'pad/dialog'], clip: true }, kids); markGrow(c, true, true); return c; };
S.side = (kids, wide) => { const c = col({ name: 'Sidebar', w: wide ? 'width/sidebar-wide' : 'width/sidebar', gap: 1, pad: 'pad/menu', fill: 'bg/app', stroke: 'border/subtle', sides: [0, 1, 0, 0] }, kids); markGrow(c, false, true); return c; };
// ---- Publish settings
S.PUB = [['globe', 'HTML5', true], ['desktop', 'Desktop app', false], ['video', 'Video (MP4)', true], ['grid', 'Image sequence', true], ['code', 'SVG animation', true]];
S.pubOutput = (label, value) => [S.group('Output'), K.formRow(label, [K.field('', value), K.btn('Browse…')])];
S.pubPages = {
  HTML5: () => [txt('HTML5 · Rust / WebAssembly', 'Title', 'text/primary')].concat(S.pubOutput('Folder', '~/Sites/ideas-in-motion'), [
    K.formRow('', [K.check('Include HTML wrapper', true)]),
    S.group('Runtime'),
    K.formRow('Size level', [K.seg(['Full', 'Balanced', 'Compact', 'Shared'], 1)]),
    K.formRow('Budget', [K.num('Max gzip', '900', 'KB', { w: 'width/field-m' }), K.small('On disk 1.36 MB · transfer 412 KB')]),
    S.group('Playback'),
    K.formRow('', [K.check('Loop', true), K.check('Autoplay', true), K.check('Preloader', false), K.check('Fullscreen', false)]),
    S.group('Assets'),
    K.formRow('JPEG quality', [K.slider({ w: 'width/field-m' }), K.num('', '80', '%', { w: 'width/num' })]),
    K.formRow('Bitmaps', [K.check('Sprite sheets', true), K.check('Embed fonts (subset)', true)]),
    K.formRow('Sequences', [K.select('Keep individual frames', { w: 'width/field-l' })]),
    S.group('Scripts'),
    K.formRow('', [K.check('Strict mode', true), K.check('Include trace()', false), K.check('Minify', true), K.check('Source maps', false)]),
  ]),
  'Video (MP4)': () => [txt('Video (MP4)', 'Title', 'text/primary')].concat(S.pubOutput('File', '~/Movies/ideas-in-motion.mp4'), [
    S.group('Video'),
    K.formRow('Size', [K.num('W', '960', 'px', { w: 'width/field-s' }), K.num('H', '600', 'px', { w: 'width/field-s' }), K.ibtn('link', { state: 'Selected' })]),
    K.formRow('Frame rate', [K.num('', '24', 'fps', { w: 'width/field-s', stepper: true })]),
    K.formRow('Codec', [K.select('H.264', { w: 'width/field-m' })]),
    K.formRow('Quality', [K.slider({ w: 'width/field-m' }), K.small('High (~2 Mbps)')]),
    S.group('Content'),
    K.formRow('', [K.check('Render nested movie clip animation and scripts', true)]),
    K.formRow('Duration', [K.radio('Stop at last frame', true), K.radio('Stop after', false), K.num('', '10', 's', { w: 'width/num', state: 'Disabled' })]),
    S.group('Audio'),
    K.formRow('', [K.check('Include audio', true), K.select('AAC 192 kbps', { w: 'width/field-m' })]),
  ]),
  'SVG animation': () => [txt('SVG animation', 'Title', 'text/primary')].concat(S.pubOutput('File', 'Choose a file'), [K.formRow('', [K.small('Exports vector SVG 1.1 (960 × 600 px).')])]),
};
S.publish = target => K.dialog('Publish settings', { w: 'width/dialog-xl', h: 'height/dialog-l' }, [S.dialogBody([
  S.side([row({ name: 'Profile', w: 'fill', gap: 'gap/control', pad: [0, 0, 'gap/row', 0] }, [K.select('Default', { label: 'Profile' }), K.ibtn('more', { kind: 'Outlined' })])]
    .concat(S.PUB.map(t => S.navRow(t[0], t[1], t[1] === target || (target === 'HTML5' && t[1] === 'HTML5'), t[2]))), true),
  S.page(S.pubPages[target]()),
])], [K.small(target === 'HTML5' ? 'HTML5 · 1.36 MB on disk · 412 KB gzip' : target === 'Video (MP4)' ? 'MP4 (H.264) · ~1.36 MB · 00:05 · 960 × 600' : 'SVG 1.1 · 960 × 600'), spacer(), K.btn('Cancel'), K.btn('OK'), K.btn('Publish', { kind: 'Primary', icon: 'upload' })]);
// ---- Preferences
S.PREFS = [['settings', 'General'], ['code', 'Code editor'], ['film', 'Timeline'], ['pen', 'Drawing'], ['text', 'Text'], ['paste', 'Clipboard'], ['keyboard', 'Shortcuts'], ['upload', 'Publish'], ['wand', 'AI'], ['history', 'Updates']];
S.prefPages = {
  General: () => [S.group('Interface'),
    K.formRow('Theme', [K.select('Light', { w: 'width/field-m' }), K.check('Follow system', false, { state: 'Disabled' })]),
    K.formRow('Density', [K.seg(['Normal', 'Compact', 'Classic Flash'], 0)]),
    K.formRow('UI scale', [K.select('100 %', { w: 'width/field-s' })]),
    K.formRow('Accent', ['#2F61EF', '#147D4F', '#D97706', '#D32626', '#7C3AED'].map(K.swatch)),
    K.formRow('Language', [K.select('English', { w: 'width/field-m', state: 'Disabled' })]),
    S.group('Workspace'),
    K.formRow('On launch', [K.select('Open the example', { w: 'width/field-l' })]),
    K.formRow('Undo levels', [K.num('', '150', null, { w: 'width/field-s', stepper: true })]),
    K.formRow('Autosave', [K.sw('Every', true), K.num('', '5', 'min', { w: 'width/field-s', stepper: true })]),
    K.formRow('Tooltips', [K.check('Show shortcuts in tooltips', true)]),
    S.group('Selection'),
    K.formRow('Shift-select', [K.check('Add to the selection with Shift', true)]),
    K.formRow('Highlight', [K.select('Layer colour', { w: 'width/field-m', state: 'Disabled' })])],
  Text: () => [S.group('Defaults'),
    K.formRow('Font', [K.select('Arial', { w: 'width/field-l' }), K.num('Size', '32', null, { w: 'width/field-s', stepper: true })]),
    K.formRow('Anti-alias', [K.select('For readability', { w: 'width/field-l', state: 'Disabled' })]),
    K.formRow('Substitution', [K.select('Show warning', { w: 'width/field-l' })]),
    S.group('Behaviour'),
    K.formRow('Fonts', [K.check('Embed used glyphs automatically', true, { state: 'Disabled' })]),
    K.formRow('Editing', [K.check('Spell check', true, { state: 'Disabled' })]),
    K.formRow('Vertical text', [K.check('Right-to-left flow', false, { state: 'Disabled' }), K.check('No kerning', true, { state: 'Disabled' })]),
    K.formRow('Input method', [K.select('System default', { w: 'width/field-l', state: 'Disabled' })])],
};
S.preferences = pg => K.dialog('Preferences', { w: 'width/dialog-l', h: 'height/dialog-l' }, [S.dialogBody([
  S.side(S.PREFS.map(p => S.navRow(p[0], p[1], p[1] === pg))), S.page(S.prefPages[pg]())])],
  [K.btn('Reset to defaults', { kind: 'Ghost' }), spacer(), K.btn('Cancel'), K.btn('Save', { kind: 'Primary' })]);
async function nf3BuildB() {
  nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Dialog / Publish settings', 'One size for every target (width/dialog-xl × height/dialog-l). The target list, the Output row (field + “Browse…”) and the footer never move; a short page leaves empty space instead of shrinking the dialog. Profile actions (Duplicate, Import JSON, Export JSON) live in the ⋯ menu.',
    [row({ gap: 40, align: 'MIN' }, ['HTML5', 'Video (MP4)', 'SVG animation'].map(t => S.publish(t)))], { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Preferences', 'One size for every category (width/dialog-l × height/dialog-l). Label column + controls at their natural width; checkboxes for options, a switch only for Autosave.',
    [row({ gap: 40, align: 'MIN' }, ['General', 'Text'].map(p => S.preferences(p)))], { noteW: 1100 }));
  out.push(nfBoard(page, 'Panel / Properties — Movie clip', null, [S.propsMovieClip()]));
  out.push(nfBoard(page, 'Panel / Properties — Document, Used colours', 'R-106: one row per colour (swatch · #HEX with alpha · usage · select on stage), “Only on stage” in the section header, Replace as one bar, Import palette… in the ⋯ menu.', [S.propsDocument()], { noteW: 420 }));
  out.push(nfBoard(page, 'Panel / Tools', 'Grip instead of a clipped title; ⋯ closes the tool grid; options block has a fixed size.', [row({ gap: 24, align: 'MIN' }, [S.toolsPalette(2), S.toolsPalette(1)])], { noteW: 420 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
