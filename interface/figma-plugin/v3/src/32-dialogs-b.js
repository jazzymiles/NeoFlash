// NF3 dialogs B (group D1): Document settings, Layer properties, confirmations, Convert to Symbol,
// Sound properties, Edit envelope, Stroke style. Uses D1.body / D1.okCancel / D1.off from 32-dialogs-a.js.
// Nine-point registration picker (artwork: 4 pt dots in a size/control box).
D1.regPicker = sel => col({ name: 'Registration picker', w: 'size/control', h: 'size/control', gap: 3, align: 'CENTER', justify: 'CENTER', radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' },
  [0, 1, 2].map(r => row({ name: 'Row', gap: 3 }, [0, 1, 2].map(c => rect(4, 4, r * 3 + c === sel ? 'accent/default' : 'text/tertiary', { radius: 1 })))));
// Collapsible caption inside a dialog body, aligned with the label column.
D1.disclosure = (title, open) => row({ name: 'Section Header / ' + title, w: 'fill', h: 'size/section', gap: 4 },
  [icon(open ? 'chevdown' : 'chevright', 'text/tertiary', 12), txt(title, 'Overline', 'text/secondary', { grow: true })]);
// ---- Document settings: 0 normal, 1 scripts in other languages, 2 no compatible language
D1.docSettings = v => {
  const warn = K.msg('5 scripts are in other languages and stay in the document: 2 compile with a warning, 3 are skipped when publishing.', 'Warning');
  if (v !== 1) { warn.opacity = 0; warn.name = 'Reserved / warning line'; }
  return K.dialog('Document settings', { w: 'width/dialog-m' }, [D1.body([
  K.formRow('Stage size', [K.num('W', '960', 'px', { w: 'width/field-s' }), K.num('H', '600', 'px', { w: 'width/field-s' }), K.ibtn('link', { state: 'Selected' })]),
  K.formRow('', [K.btn('Match contents', { kind: 'Ghost' }), K.btn('Match printer', { kind: 'Ghost' })]),
  K.formRow('Scale', [K.check('Scale content with stage', true), D1.regPicker(0)]),
  K.formRow('Units', [K.select('Pixels', { w: 'width/field-m' })]),
  K.formRow('Background', [K.swatch('#131824'), K.field('#', '131824', { unit: '100 %', w: 'width/field-m' })]),
  K.formRow('Frame rate', [K.num('', '24.00', 'fps', { w: 'width/field-s', stepper: true }), K.small('120 frames · 5.00 s')]),
  K.formRow('Script', [K.select(v === 1 ? 'TypeScript' : v === 2 ? 'ActionScript 3.0' : 'ActionScript 3.0 · strict', { w: 'width/field-l' })]),
  v === 1 ? K.formRow('', [K.check('Also compile JavaScript', true)]) : v === 2 ? K.formRow('', [K.check('Also compile compatible languages', false, D1.off)]) : K.formRow('', [], { name: 'Reserved / option row' }),
  warn,
])], D1.okCancel('OK', [K.btn('Make default', { kind: 'Ghost' })]));
};
// ---- Layer properties
D1.layerProps = () => K.dialog('Layer properties', { w: 'width/dialog-m' }, [D1.body([
  K.formRow('Name', [K.field('', 'hand', { state: 'Focus' })]),
  K.formRow('', [K.check('Visible', true), K.check('Locked', false)]),
  K.formRow('Type', ['Normal', 'Guide', 'Guided', 'Mask'].map(t => K.radio(t, false))),
  K.formRow('', [K.radio('Masked', true), K.radio('Folder', false), K.radio('Armature', false, D1.off)]),
  K.formRow('Layer height', [K.select('100 %', { w: 'width/field-m' })]),
  K.formRow('Blending', [K.select('Normal', { w: 'width/field-m' })]),
  K.formRow('', [K.check('Sync layer to timeline', false)]),
  S.group('Outline'),
  K.formRow('Colour', [K.swatch('#2F61EF'), K.check('View layer as outlines', false)]),
  S.group('Camera'),
  K.formRow('Z-depth', [K.num('', '0', 'px', { w: 'width/field-s' }), K.check('Attach layer to camera', false)]),
])], D1.okCancel());
// ---- Confirmations: the question is the dialog title, one warning line, the destructive action on the left
D1.confirm = (title, text, footer) => K.dialog(title, { w: 'width/dialog-s' }, [D1.body([
  row({ name: 'Message', w: 'fill', gap: 'gap/section', align: 'MIN' }, [icon('warning', 'warning/default'), S.para(text)])])], footer);
D1.confirmations = () => [
  D1.confirm('Save changes before continuing?', 'Your changes to “Ideas in motion” will be lost if you discard them.',
    [K.btn('Discard', { kind: 'Danger' }), spacer(), K.btn('Cancel'), K.btn('Save', { kind: 'Primary' })]),
  D1.confirm('Revert to saved version?', 'Your unsaved changes to “Ideas in motion” will be lost and the last saved version on disk will be reloaded.',
    [spacer(), K.btn('Cancel'), K.btn('Revert', { kind: 'Danger' })]),
  D1.confirm('File modified on disk', '“Ideas in motion” was modified on disk by another program while you have unsaved changes in NeoFlash. Reload from disk, keep editing your current version, or Save As a new file.',
    [K.btn('Reload', { kind: 'Danger' }), spacer(), K.btn('Save As…'), K.btn('Keep editing', { kind: 'Primary' })]),
  D1.confirm('Recover unsaved work?', 'An autosaved copy of “Ideas in motion” from a previous session was found. Restore it or discard the recovery file.',
    [K.btn('Discard', { kind: 'Danger' }), spacer(), K.btn('Restore', { kind: 'Primary' })])];
// ---- Convert to Symbol: 0 basic, 1 advanced, 2 type menu open
D1.convertSymbol = v => K.dialog('Convert to Symbol', { w: 'width/dialog-m' }, [D1.body([
  K.formRow('Name', [K.field('', 'Badge symbol', { state: 'Focus' })]),
  K.formRow('Type', [K.select('Movie clip', { w: 'width/field-m', state: v === 2 ? 'Open' : 'Default' })]),
  K.formRow('', [K.small('Movie clips have their own timeline and can be scripted.', null, { grow: true, wrap: true })]),
  K.formRow('Folder', [K.select('Library root', { w: 'width/field-l' })]),
  K.formRow('Registration', [D1.regPicker(4), K.t('Centre')]),
  D1.disclosure('Advanced', v === 1)].concat(v !== 1 ? [] : [
  K.formRow('', [K.check('Enable guides for 9-slice scaling', false)]),
  S.group('ActionScript linkage'),
  K.formRow('', [K.check('Export for ActionScript', true), K.check('Export in frame 1', true)]),
  K.formRow('Identifier', [K.field('', 'badge_symbol')]),
  K.formRow('Class', [K.field('', 'Badge'), K.ibtn('check'), K.ibtn('pencil')]),
  K.formRow('Base', [K.field('', 'flash.display.MovieClip'), K.ibtn('check'), K.ibtn('pencil')]),
  S.group('Runtime shared library'),
  K.formRow('', [K.check('Export for runtime sharing', false)]),
  K.formRow('', [K.check('Import for runtime sharing', false)]),
  K.formRow('URL', [K.field('', 'shared/ui.swf', { state: 'Disabled' })]),
  S.group('Authortime sharing'),
  K.formRow('File', [K.btn('Source file…'), K.t('—')]),
  K.formRow('Symbol', [K.btn('Symbol…'), K.t('—')]),
  K.formRow('', [K.check('Update automatically', false)])]))], D1.okCancel('Convert'));
D1.symbolTypeMenu = () => K.menu('Symbol type', [K.mi('Movie clip', { check: true, key: 'own timeline' }), K.mi('Button', { check: false, key: 'up · over · down · hit', state: 'Hover' }),
  K.mi('Graphic', { check: false, key: 'synced to parent' })], { w: 'width/tree' });
// ---- Sound: waveform artwork (bars from a fixed formula, no random)
D1.wave = (n, h, seed) => {
  const out = [];
  for (let i = 0; i < n; i++) out.push(rect(2, Math.max(2, Math.round(h * (0.25 + 0.75 * Math.abs(Math.sin(i * 0.37 + seed) * Math.cos(i * 0.11 + seed * 2))))), 'success/default'));
  return out;
};
D1.soundProps = () => K.dialog('Sound properties', { w: 'width/dialog-m' }, [D1.body([
  row({ name: 'Sound', w: 'fill', gap: 'gap/section', align: 'MIN' }, [
    row({ name: 'Waveform', w: 120, h: 64, gap: 2, pad: [0, 4], clip: true, radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle' }, D1.wave(28, 52, 1)),
    col({ name: 'Info', w: 'fill', gap: 'gap/row' }, [K.field('', 'music.mp3'), K.small('~/Projects/ideas/audio/music.mp3', null, { grow: true }), K.small('44 kHz · Stereo · 16 bit · 32.4 s · 5.7 MB', null, { grow: true })])]),
  S.group('Export'),
  K.formRow('Compression', [K.select('MP3', { w: 'width/field-m' })]),
  K.formRow('Bit rate', [K.select('128 kbps', { w: 'width/field-m' })]),
  K.formRow('Quality', [K.select('Best', { w: 'width/field-m' })]),
  K.formRow('', [K.check('Convert stereo to mono', false)]),
  K.formRow('Result', [txt('128 kbps stereo, 518.6 kB — 9 % of original', 'Body', 'text/secondary', { grow: true })]),
])], D1.okCancel('OK', [K.btn('Update', { kind: 'Ghost' }), K.btn('Import…', { kind: 'Ghost' }), K.btn('Test', { kind: 'Ghost', icon: 'play' })]));
// One channel lane: 676 pt of waveform (clipped by the lane in narrower densities) with the envelope drawn over it.
D1.envLane = (ch, pts, sel) => {
  const W = 676, H = 72, art = row({ name: 'Channel ' + ch, w: W, h: H, gap: 2, clip: true }, D1.wave(169, 56, ch === 'L' ? 1 : 2));
  const line = (a, b) => {
    if (a[1] === b[1]) { abs(art, rect(b[0] - a[0], 2, 'accent/default', { name: 'Envelope' }), a[0], a[1] - 1); return; }
    const n = Math.ceil((b[0] - a[0]) / 4);
    for (let i = 0; i < n; i++) abs(art, rect(4, 2, 'accent/default', { name: 'Envelope' }), a[0] + i * 4, Math.round(a[1] + (b[1] - a[1]) * (i + 0.5) / n) - 1);
  };
  for (let i = 1; i < pts.length; i++) line(pts[i - 1], pts[i]);
  pts.forEach((p, i) => abs(art, rect(7, 7, i === sel ? 'accent/default' : 'bg/panel', { name: 'Handle', stroke: 'accent/default', radius: 1 }), Math.min(Math.max(p[0] - 3, 0), W - 7), p[1] - 3));
  abs(art, txt(ch, 'Small Strong', 'text/secondary'), 6, 4);
  return row({ name: 'Waveform lane ' + ch, w: 'fill', h: H, clip: true, radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle' }, [art]);
};
D1.editEnvelope = () => K.dialog('Edit envelope', { w: 'width/dialog-l' }, [D1.body([
  row({ name: 'Options', w: 'fill', gap: 'gap/control' }, [K.select('Fade in', { label: 'Effect', w: 'width/field-l' }), spacer(), K.seg(['Seconds', 'Frames'], 0)]),
  D1.envLane('L', [[0, 62], [120, 12], [470, 12], [660, 44]], 1),
  row({ name: 'Time ruler', w: 'fill', justify: 'SPACE_BETWEEN' }, ['0s', '4s', '8s', '12s', '16s', '20s', '24s', '28s', '32s'].map(s => txt(s, 'Mono Small', 'text/tertiary'))),
  D1.envLane('R', [[0, 62], [150, 12], [470, 12], [660, 40]], -1),
])], [K.ibtn('play'), K.ibtn('stop'), K.ibtn('zoomin'), K.ibtn('zoomout'), K.small('Drag handles to shape volume · click a lane to add a point'), spacer(), K.btn('Cancel'), K.btn('OK', { kind: 'Primary' })]);
// ---- Stroke style
D1.strokeStyle = () => {
  const dashes = [];
  for (let i = 0; i < 16; i++) dashes.push(rect(12, 4, 'text/primary', { name: 'Dash' }));
  return K.dialog('Stroke style', { w: 'width/dialog-m' }, [D1.body([
    row({ name: 'Preview', w: 'fill', h: 72, gap: 8, justify: 'CENTER', clip: true, radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle' }, dashes),
    K.formRow('Type', [K.select('Dashed', { w: 'width/field-m' })]),
    K.formRow('Thickness', [K.num('', '4', 'pt', { w: 'width/field-s' }), K.sw('4× zoom', true)]),
    K.formRow('Dash', [K.num('', '12', 'pt', { w: 'width/field-s', stepper: true }), K.num('Gap', '8', 'pt', { w: 'width/field-s', stepper: true })]),
    K.formRow('', [K.check('Sharp corners', false, D1.off)]),
  ])], D1.okCancel('OK', [K.btn('Reset', { kind: 'Ghost' })]));
};
async function nf3BuildDialogsB() {
  nf3MoreVars();
  const page = await nfMaster(), out = [];
  const side = kids => [row({ gap: 40, align: 'MIN' }, kids)];
  out.push(nfBoard(page, 'Dialog / Document settings', 'Label column with controls at natural width; Make default stays in the footer. States: normal · scripts in other languages (Also compile JavaScript and the warning line) · no compatible language (the option stays in place, disabled). One height in all three states: the option row and the warning line keep their space when they are not shown.',
    side([0, 1, 2].map(D1.docSettings)), { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Layer properties', 'Visible, Locked, Sync and Attach to camera are checkboxes (the dialog applies on OK); Type is the label of two radio rows; Layer height and Blending get their own labelled rows. With Type = Mask the application adds a Mask mode select and Invert mask under the Type rows.',
    [D1.layerProps()], { noteW: 600 }));
  out.push(nfBoard(page, 'Dialog / Confirmations', 'width/dialog-s. The question is the dialog title, one warning line below; the destructive action sits alone on the left, Cancel and the single Primary on the right. Unsaved changes · Revert · File modified on disk (with unsaved changes) · Recover unsaved work.',
    side(D1.confirmations()), { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Convert to symbol', 'One label column instead of three stacked captions; Advanced is a collapsible caption of the same dialog. Surfaces: basic · Advanced open · Type menu open (the menu is drawn beside the dialog).',
    side([D1.convertSymbol(0), D1.convertSymbol(1), row({ name: 'Type menu open', gap: 12, align: 'MIN' }, [D1.convertSymbol(2), D1.symbolTypeMenu()])]), { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Sound properties, Edit envelope', 'Sound properties: Compression, Bit rate and Quality are labelled rows; Update, Import… and Test stay on the left of the footer. Edit envelope: Effect select and the Seconds / Frames switch above the two channel lanes, transport and zoom icon buttons with the hint in the footer.',
    side([D1.soundProps(), D1.editEnvelope()]), { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Stroke style', 'Preview on top, then Type, Thickness with the 4× zoom switch (it changes the preview at once), Dash and Gap, Sharp corners (not available yet, disabled); Reset on the left of the footer.',
    [D1.strokeStyle()], { noteW: 600 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
