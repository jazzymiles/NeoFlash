// NF3 dialogs D (group D2): export dialogs (video, audio, image) and import dialogs (video, PSD to Library).
const D2 = {};
// Hug-height dialog body (one column). Fixed-height dialogs use S.page / D2.split(.., .., true) instead.
D2.body = (kids, o) => col(Object.assign({ name: 'Body', w: 'fill', gap: 'gap/row', pad: ['pad/panel', 'pad/dialog'] }, o || {}), kids);
// Preview column (artwork width) + settings column (fills).
D2.split = (left, right, fillH) => {
  const r = row({ name: 'Body', w: 'fill', align: 'MIN', gap: 'pad/dialog', pad: ['pad/panel', 'pad/dialog'] }, [left, right]);
  if (fillH) markGrow(r, true, true);
  return r;
};
D2.aside = (w, kids) => col({ name: 'Preview column', w: w, gap: 'gap/row' }, kids);
D2.form = kids => col({ name: 'Settings', w: 'fill', gap: 'gap/row' }, kids);
D2.art = (w, h, kids, o) => row(Object.assign({ name: 'Preview', w: w, h: h, gap: 16, justify: 'CENTER', fill: 'bg/canvas', stroke: 'border/subtle', radius: 'radius/control', clip: true }, o || {}), kids);
D2.note = s => K.small(s, null, { grow: true, wrap: true });
D2.opt = kids => K.formRow('', kids);
// Footer: optional ghost button, summary (truncates) or spacer, then the buttons.
D2.foot = (summary, btns, lead) => [lead || null, summary ? K.small(summary, null, { grow: true }) : spacer()].concat(btns);
D2.state = (label, node) => col({ name: label, gap: 12 }, [txt(label, 'Title', 'text/primary'), node]);
D2.states = kids => row({ name: 'States', gap: 40, align: 'MIN' }, kids);
D2.stageThumb = (w, h) => D2.art(w, h, [rect(72, 8, 'text/tertiary', { radius: 2 }), dot(36, 'accent/default')]);
D2.sizeRow = (w, h, scale) => K.formRow('Size', [K.num('W', w, 'px', { w: 'width/field-s' }), K.num('H', h, 'px', { w: 'width/field-s' }), K.ibtn('link', { state: 'Selected' }), K.select(scale, { w: 'width/num' })]);

// ---- Export video (idle / exporting)
D2.exportVideo = busy => K.dialog('Export video', { w: 'width/dialog-xl' }, [D2.split(
  D2.aside(288, [D2.stageThumb(288, 180)].concat(busy ? [
    row({ name: 'Progress label', w: 'fill', gap: 'gap/control' }, [txt('Exporting frames 72 / 120', 'Small', 'text/secondary', { grow: true }), txt('60 %', 'Mono Small', 'text/tertiary')]),
    K.progress(),
    D2.note('Preview while rendering · Pause to hold')] : [D2.note('Preview · frame 1')])),
  D2.form([
    K.line([K.seg(['MP4', 'WebM', 'MOV', 'GIF', 'PNG seq.'], 0)]),
    K.formRow('Range', [K.select('Whole movie (all scenes)', { w: 'width/field-l' })]),
    K.formRow('Frames', [K.num('From', '1', null, { w: 'width/field-s', state: 'Disabled' }), K.num('To', '120', null, { w: 'width/field-s', state: 'Disabled' })]),
    S.group('Video'),
    D2.sizeRow('1920', '1200', '2×'),
    K.formRow('Frame rate', [K.select('24 fps (document)', { w: 'width/field-m' })]),
    K.formRow('Codec', [K.select('H.264', { w: 'width/field-m' })]),
    K.formRow('Quality', [K.slider({ w: 'width/field-m' }), K.small('High · ~8 Mbps')]),
    K.formRow('Background', [K.check('Transparent bg (MOV/WebM)', false)]),
    S.group('Content'),
    K.line([K.check('Render nested movie clip animation and scripts (runtime capture)', true)]),
    K.formRow('Duration', [K.radio('Stop at last frame', true), K.radio('Stop after', false), K.num('', '10', 's', { w: 'width/num', state: 'Disabled' })]),
    S.group('Audio'),
    K.formRow('', [K.check('Include audio', true), K.select('AAC 192 kbps', { w: 'width/field-m' })]),
  ]))],
  D2.foot('~ 12.1 MB · 00:05 · 1920×1200', [K.btn('Cancel'), K.btn('Export', { kind: 'Primary', icon: 'upload', state: busy ? 'Disabled' : 'Default' })],
    busy ? K.btn('Pause', { kind: 'Ghost', icon: 'pause' }) : null));

// ---- Export audio (waveform / no sounds)
D2.wave = () => {
  const bars = [];
  for (let i = 0; i < 96; i++) bars.push(rect(2, 4 + Math.round(36 * Math.abs(Math.sin(i * 0.37) * Math.sin(i * 0.11 + 1))), 'success/default', { radius: 1 }));
  return row({ name: 'Waveform', w: 'fill', h: 48, pad: [0, 4], justify: 'SPACE_BETWEEN', fill: 'bg/field', stroke: 'border/subtle', radius: 'radius/control', clip: true }, bars);
};
D2.noSound = () => col({ name: 'Empty state', w: 'fill', h: 48, gap: 2, align: 'CENTER', justify: 'CENTER', fill: 'bg/field', stroke: 'border/subtle', radius: 'radius/control' },
  [icon('sound', 'text/tertiary'), K.small('This document has no sounds to export.')]);
D2.exportAudio = empty => K.dialog('Export audio', { w: 'width/dialog-m' }, [D2.body([
  K.line([K.seg(['WAV', 'MP3', 'OGG', 'AAC'], 1)]),
  empty ? D2.noSound() : D2.wave(),
  S.group('Source'),
  K.formRow('Sound', [K.select('Timeline mix (all sound layers)')]),
  K.formRow('Range', [K.select('Whole movie', { w: 'width/field-m' })]),
  S.group('Format'),
  K.formRow('Sample rate', [K.select('44.1 kHz', { w: 'width/field-s' }), K.select('16-bit', { w: 'width/field-s' })]),
  K.formRow('Channels', [K.select('Stereo', { w: 'width/field-s' }), K.select('192 kbps CBR', { w: 'width/field-m' })]),
  S.group('Processing'),
  K.formRow('', [K.check('Normalize to −1 dB', true), K.check('Trim silence', false)]),
  K.formRow('Fade out', [K.num('', '1.0', 's', { w: 'width/field-s', stepper: true })]),
])], D2.foot(empty ? 'No sounds' : 'music.mp3 + sfx · 00:32.4 · ~ 760 kB',
  [K.btn('Cancel'), K.btn('Export', { kind: 'Primary', icon: 'upload', state: empty ? 'Disabled' : 'Default' })]));

// ---- Export image (AI / PSD / SVG): one size for every format
D2.IMG = {
  AI: { sel: 3, bg: 0, sum: '~ 260 kB · 6 layers', rows: () => [
    K.formRow('Version', [K.select('Illustrator CS6+', { w: 'width/field-m' })]),
    K.formRow('Layers', [K.radio('Each layer → AI layer', true)]),
    D2.opt([K.radio('Each keyframe → artboard', false)]),
    K.formRow('Strokes', [K.check('Strokes as strokes (not outlines)', true)]),
    K.formRow('Text', [K.check('Convert text to outlines', false)])] },
  PSD: { sel: 2, bg: 0, sum: '~ 4.2 MB · 6 layers', rows: () => [
    K.formRow('Layers', [K.radio('Keep timeline layers as Photoshop layers', true)]),
    D2.opt([K.radio('Flatten to one layer', false)]),
    K.formRow('Convert', [K.check('Folders → layer groups', true)]),
    D2.opt([K.check('Symbols → smart objects', true)]),
    D2.opt([K.check('Keep text editable', true)]),
    K.formRow('Colour', [K.select('sRGB 8-bit', { w: 'width/field-m' })]),
    K.formRow('Resolution', [K.select('144 ppi', { w: 'width/field-s' })])] },
  SVG: { sel: 1, bg: 1, sum: '~ 18 kB · 42 paths', rows: () => [
    K.formRow('Images', [K.select('Embed', { w: 'width/field-m' })]),
    K.formRow('Text', [K.select('Convert to outlines', { w: 'width/field-l' })]),
    K.formRow('Output', [K.check('Minify output', true)]),
    D2.opt([K.check('Use CSS classes for styles', false)]),
    D2.opt([K.check('Include hidden & guide layers', false)]),
    K.formRow('Precision', [K.num('', '2', null, { w: 'width/num' })]),
    K.formRow('ID prefix', [K.field('', 'nf-', { w: 'width/field-s' })])] },
};
D2.exportImage = f => {
  const d = D2.IMG[f];
  return K.dialog('Export image', { w: 'width/dialog-l', h: 'height/dialog-l' }, [D2.split(
    D2.aside(208, [D2.stageThumb(208, 130), D2.note('Frame 30 of 120 · 960 × 600')]),
    D2.form([
      K.line([K.seg(['PNG', 'SVG', 'PSD', 'AI', 'PDF'], d.sel)]),
      K.formRow('Range', [K.select('Current frame', { w: 'width/field-m' }), K.num('Frames', '30', null, { w: 'width/field-s', state: 'Disabled' })]),
      D2.sizeRow('960', '600', '1×'),
      K.formRow('Background', [K.radio('Stage colour', d.bg === 0), K.radio('Transparent', d.bg === 1)]),
      S.group(f + ' options')].concat(d.rows())), true)],
    D2.foot(d.sum, [K.btn('Cancel'), K.btn('Export ' + f, { kind: 'Primary', icon: 'upload' })]));
};

// ---- Import video
D2.choice = (label, on, hint) => col({ name: 'Option', w: 'fill' }, [K.radio(label, on), D2.note(hint)]);
D2.importVideo = () => K.dialog('Import video', { w: 'width/dialog-l' }, [D2.split(
  D2.aside(240, [
    D2.art(240, 135, [icon('video', 'text/tertiary')]),
    D2.art(240, 24, ['#3A4E8C', '#9A5B2E', '#2F6B55', '#6B3A78', '#3A4E8C', '#9A5B2E', '#2F6B55'].map(c => rect(30, 16, c, { radius: 2 })), { name: 'Filmstrip', gap: 3, justify: 'MIN', pad: [0, 3], radius: 2 }),
    K.pair(K.field('In', '00:00.00'), K.field('Out', '00:24.00')),
    D2.note('Drag the in / out handles on the strip to trim.')]),
  D2.form([
    K.formRow('File', [K.field('', '~/Movies/intro.mp4'), K.btn('Browse…')]),
    S.group('Deployment'),
    D2.choice('Embed in timeline', true, 'Frames sync with the timeline; best for short clips'),
    D2.choice('Load external video', false, 'Streams with a player component; keeps .nfl small'),
    D2.choice('Progressive download', false, 'Plays while downloading from a URL'),
    S.group('Embedding'),
    K.formRow('Symbol', [K.select('Movie clip', { w: 'width/field-m' })]),
    K.formRow('Codec', [K.select('H.264', { w: 'width/field-m' })]),
    K.formRow('Placement', [K.check('Place on stage', true)]),
    D2.opt([K.check('Expand timeline if needed', true)]),
    K.formRow('Audio', [K.check('Include audio', true)]),
    K.formRow('Frame rate', [K.check('Match document frame rate', true)]),
  ]))],
  D2.foot('1280×720 · 30 fps → 24 fps · 24.0 s · ~6.1 MB', [K.btn('Cancel'), K.btn('Import', { kind: 'Primary' })]));

// ---- Import PSD to Library: layer list (one row per layer) + options of the selected layer
// name, type icon, checked, nested, thumbnail colour
D2.PSD = [['Title', 'font', true, 0, null], ['Logo', 'folderopen', true, 0, null], ['Mark', 'bitmap', true, 1, '#C97B4A'], ['Wordmark', 'pen', true, 1, '#C97B4A'],
  ['Glow', 'bitmap', false, 0, '#C97B4A'], ['Background', 'bitmap', true, 0, '#5A5F8A']];
D2.psdRow = l => row({ name: 'Layer / ' + l[0], w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/check', fill: l[0] === 'Mark' ? 'bg/selected' : null }, [
  l[3] ? rect(12, 1, null, { name: 'Indent' }) : null, K.check('', l[2]), rect(22, 14, l[4] || 'bg/canvas', { name: 'Thumbnail', radius: 2, stroke: 'border/default' }),
  txt(l[0], l[0] === 'Mark' ? 'Body Strong' : 'Body', 'text/primary', { grow: true }), icon(l[1], l[0] === 'Mark' ? 'accent/default' : 'text/tertiary')]);
D2.regPicker = () => col({ name: 'Registration picker', w: 'size/control', h: 'size/control', gap: 3, align: 'CENTER', justify: 'CENTER', radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' },
  [0, 1, 2].map(r => row({ name: 'Dots', gap: 3 }, [0, 1, 2].map(c => dot(3, r === 1 && c === 1 ? 'accent/default' : 'text/tertiary')))));
D2.importPsd = () => K.dialog('Import "Poster.psd" to library', { w: 'width/dialog-xl', h: 'height/dialog-xl' }, [S.dialogBody([
  S.side([row({ name: 'Group caption', w: 'fill', minH: 'size/section', pad: [0, 'pad/field'] }, [txt('Layers to import', 'Overline', 'text/tertiary')])]
    .concat(D2.PSD.map(D2.psdRow))
    .concat([row({ name: 'Layer actions', w: 'fill', gap: 'gap/control', pad: ['gap/section', 0, 0, 0] }, [K.btn('Merge layers'), K.btn('Select all', { kind: 'Ghost' })])]), true),
  S.page([
    txt('Options for "Mark"', 'Title', 'text/primary'),
    S.group('Import this layer as'),
    D2.opt([K.radio('Bitmap with editable layer styles', false)]),
    D2.opt([K.radio('Flattened bitmap image', true)]),
    D2.opt([K.radio('Vector shapes (if possible)', false)]),
    S.group('Symbol'),
    D2.opt([K.check('Create movie clip for this layer', true)]),
    K.formRow('Instance', [K.field('', 'mark_mc', { w: 'width/field-l' }), K.t('Reg.'), D2.regPicker()]),
    S.group('Publish settings'),
    K.formRow('Compression', [K.select('Lossy', { w: 'width/field-m' })]),
    K.formRow('Quality', [K.radio('Use publish quality', true), K.radio('Custom', false), K.num('', '90', null, { w: 'width/num', state: 'Disabled' }), K.btn('Calculate bitmap size', { kind: 'Ghost' })]),
    S.group('Placement'),
    K.formRow('Convert to', [K.select('Timeline layers', { w: 'width/field-l' })]),
    D2.opt([K.check('Place layers at original position', true)]),
    D2.opt([K.check('Set stage size to canvas (1280 × 720)', false)]),
  ]),
])], D2.foot('5 of 6 layers · 3 bitmaps · 1 text · 1 vector', [K.btn('Cancel'), K.btn('Import', { kind: 'Primary' })], K.btn('AI options…', { kind: 'Ghost' })));

async function nf3BuildDialogsD() {
  if (!NF.v['width/field-m']) nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Dialog / Export video', 'Preview column + one form grid at width/dialog-xl; the format switch heads the form, “Transparent bg” moved from the audio row into the Video group, “Include audio” is a checkbox. The progress bar, its label and the ghost Pause button exist only while exporting: the idle state shows “Preview · frame 1”, the footer is always Cancel, then Export.',
    [D2.states([D2.state('Idle', D2.exportVideo(false)), D2.state('Exporting', D2.exportVideo(true))])], { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Export audio', 'Single-column form at width/dialog-m: the four unlabelled format selects became two labelled rows, Normalize is a checkbox. With no sounds the waveform strip keeps its height and shows the empty state, the summary reads “No sounds” and Export is disabled.',
    [D2.states([D2.state('Timeline mix', D2.exportAudio(false)), D2.state('No sounds', D2.exportAudio(true))])], { noteW: 900 }));
  out.push(nfBoard(page, 'Dialog / Export image', 'One size for every format (width/dialog-l × height/dialog-l): the common rows (range, size, background) never move, only the “… options” group changes; the SVG switches became checkboxes.',
    [D2.states(['AI', 'PSD', 'SVG'].map(f => D2.state(f, D2.exportImage(f))))], { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Import video', 'Preview, filmstrip and In / Out stay in the artwork column; deployment, embedding and the four options follow the dialog form grid with one option per row.',
    [D2.importVideo()], { noteW: 720 }));
  out.push(nfBoard(page, 'Dialog / Import PSD to Library', 'Fixed size (width/dialog-xl × height/dialog-xl): the layer list is a sidebar with one size/row per layer (checkbox · thumbnail · name · type), the options of the selected layer use the form grid; “Use publish quality / Custom / Calculate bitmap size” share one Quality row, “AI options…” stays a ghost button in the footer.',
    [D2.importPsd()], { noteW: 880 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
