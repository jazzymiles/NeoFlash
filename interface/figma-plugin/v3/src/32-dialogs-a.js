// NF3 dialogs A (group D1): the eight remaining Preferences pages — Code editor, Timeline, Drawing, Clipboard,
// Shortcuts, Publish, AI, Updates. General and Text live in 21-surfaces-b.js (S.prefPages).
const D1 = {};
D1.off = { state: 'Disabled' };
// Body of a dialog that hugs its content (no fixed height): the padded form column.
D1.body = kids => col({ name: 'Body', w: 'fill', gap: 'gap/row', pad: ['pad/panel', 'pad/dialog'] }, kids);
// Footer: optional left items, then Cancel and the single Primary.
D1.okCancel = (ok, left) => (left || []).concat([spacer(), K.btn('Cancel'), K.btn(ok || 'OK', { kind: 'Primary' })]);
// ---- Code editor: syntax colours and the sample
D1.TOKENS = [['Keyword', 'C6A0F6'], ['Type / class', '8BD5CA'], ['Function', '8AADF4'], ['String', 'A6DA95'], ['Number', 'F5A97F'], ['Comment', '6E7891'], ['XML tag', '7DC4E4'],
  ['Attribute', 'EED49F'], ['Error', 'FF6B6B'], ['Warning', 'F5B94A'], ['Breakpoint', 'FF9A5C'], ['Selection', '2A3A63'], ['Current line', '171C27']];
D1.token = t => row({ name: 'Token / ' + t[0], w: 'fill', h: 'size/swatch', gap: 'gap/inline' },
  [K.swatch('#' + t[1]), txt(t[0], 'Body', 'text/primary', { grow: true }), txt(t[1], 'Mono Small', 'text/tertiary')]);
D1.codeSample = () => {
  const L = [['1', null, '// orbit setup', 'syntax/comment'], ['2', 'breakpoint', 'var speed:Number = 2.5;', 'syntax/keyword'], ['3', null, 'animator.play();', 'syntax/function'],
    ['4', 'warning', '<Keyframe index="0"/>', 'syntax/tag'], ['5', 'error', 'mc.rotation += speed;', 'text/primary']];
  const mark = { breakpoint: 'syntax/breakpoint', warning: 'warning/default', error: 'danger/default' };
  return col({ name: 'Preview', w: 'fill', pad: [6, 0], radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle', clip: true }, L.map(l =>
    row({ name: 'Line ' + l[0], w: 'fill', gap: 'gap/inline', pad: [0, 8, 0, 4] }, [
      row({ name: 'Gutter', w: 30, gap: 2, justify: 'MAX' }, [l[1] ? icon(l[1], mark[l[1]], 12) : null, txt(l[0], 'Mono Small', 'text/tertiary')]),
      txt(l[2], 'Mono', l[3], { grow: true })])));
};
D1.schemeBlock = () => row({ name: 'Syntax colours', w: 'fill', gap: 'gap/section', align: 'MIN' }, [
  col({ name: 'Tokens', w: 'fill', gap: 'gap/row' }, [
    row({ name: 'Token grid', w: 'fill', gap: 'gap/section', align: 'MIN' }, [
      col({ name: 'Column', w: 'fill', gap: 'gap/tool' }, D1.TOKENS.slice(0, 7).map(D1.token)),
      col({ name: 'Column', w: 'fill', gap: 'gap/tool' }, D1.TOKENS.slice(7).map(D1.token))]),
    row({ name: 'Token actions', gap: 'gap/control' }, [K.btn('Reset colours', { kind: 'Ghost' })])]),
  col({ name: 'Sample', w: 'width/field-l', gap: 'gap/row' }, [D1.codeSample(),
    K.small('Red is reserved for errors by default. Click any swatch to customise; Reset colours restores theme defaults.', null, { grow: true, wrap: true })])]);
// ---- Shortcuts: one row per command
D1.shortcutRow = (name, keys, o) => {
  o = o || {};
  const cell = o.rec ? [row({ name: 'Recording', w: 'width/field-m', h: 'fill', pad: [0, 'pad/field'], radius: 'radius/check', fill: 'bg/field', stroke: 'border/focus' }, [txt('Press keys…', 'Body', 'text/tertiary')])]
    : keys.map(k => K.kbd(k));
  return row({ name: 'Shortcut / ' + name, w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/control', fill: o.rec ? 'bg/selected' : null, stroke: 'border/subtle', sides: [0, 0, 1, 0] },
    [txt(name, 'Body', 'text/primary', { grow: true }), o.conflict ? K.badge(o.conflict, 'Warning') : null].concat(cell));
};
D1.shortcutsPage = () => {
  const list = col({ name: 'Commands', radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle', clip: true }, [
    D1.shortcutRow('Insert ▸ Frame', ['F5']),
    D1.shortcutRow('Insert ▸ Keyframe', [], { rec: true }),
    D1.shortcutRow('Insert ▸ Blank keyframe', ['F7']),
    D1.shortcutRow('Clear keyframe', ['⇧', 'F6']),
    D1.shortcutRow('Convert to keyframes', ['⌥', 'F6'], { conflict: 'Conflicts with Insert ▸ Keyframe' }),
    D1.shortcutRow('Edit ▸ Timeline ▸ Remove frames', ['⇧', 'F5'])]);
  markGrow(list, true, true);
  return [
    row({ name: 'Shortcut set', w: 'fill', gap: 'gap/control' }, [K.select('NeoFlash default', { w: 'width/field-m' }), K.btn('Duplicate', { kind: 'Ghost' }),
      K.btn('Rename', { kind: 'Ghost', state: 'Disabled' }), K.btn('Delete', { kind: 'Ghost' }), spacer(), K.btn('Export as HTML…')]),
    row({ name: 'Filter', w: 'fill', gap: 'gap/control' }, [K.search('keyframe', { clear: true }), K.select('All commands', { w: 'width/field-m' }), K.check('Only assigned', false), K.badge('1 conflict', 'Warning')]),
    list,
    row({ name: 'List actions', w: 'fill', gap: 'gap/control' }, [K.btn('Reset command', { kind: 'Ghost' })])];
};
// ---- AI: one row per connection
D1.aiRow = (name, sub, badge, tone, sel) => row({ name: 'Connection / ' + name, w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/control', fill: sel ? 'bg/selected' : null, stroke: 'border/subtle', sides: [0, 0, 1, 0] },
  [txt(name, 'Body', 'text/primary'), txt(sub, 'Small', 'text/secondary', { grow: true }), K.badge(badge, tone)]);
// ---- Pages
D1.prefPages = {
  'Code editor': () => [S.group('Font'),
    K.formRow('Font', [K.select('Roboto Mono', { w: 'width/field-m' }), K.num('Size', '13', null, { w: 'width/field-s', stepper: true }), K.num('Line', '1.6', null, { w: 'width/field-s', stepper: true })]),
    S.group('Editing'),
    K.formRow('Display', [K.check('Line numbers', true), K.check('Word wrap', false), K.check('Highlight current line', true)]),
    K.formRow('Typing', [K.check('Auto-close brackets', true), K.check('Format on save', true)]),
    K.formRow('Hints', [K.check('Code hints', true), K.t('Delay'), K.slider({ w: 'width/field-m' }), K.small('300 ms')]),
    K.formRow('Indentation', [K.select('Tab: 4 spaces', { w: 'width/field-m' })]),
    S.group('Colour scheme'),
    K.formRow('Scheme', [K.select('NF2 Night', { w: 'width/field-m', state: 'Disabled' }), K.btn('Duplicate', { kind: 'Ghost', state: 'Disabled' }), K.btn('Import…', { kind: 'Ghost', state: 'Disabled' })]),
    D1.schemeBlock()],
  Timeline: () => [S.group('Selection & view'),
    K.formRow('Selection', [K.check('Span-based selection', true)]),
    K.formRow('Frame view', [K.select('Normal', { w: 'width/field-m' }), K.select('Row: Normal', { w: 'width/field-m' })]),
    K.formRow('Ruler', [K.check('Show seconds', true), K.check('Snap playhead to keyframes', false)]),
    K.formRow('Scenes', [K.check('Named anchor on each scene', false, D1.off)]),
    S.group('Onion skin defaults'),
    K.formRow('Range', [K.num('Before', '3', null, { w: 'width/field-s', stepper: true }), K.num('After', '3', null, { w: 'width/field-s', stepper: true })]),
    K.formRow('Tint', [K.t('Before'), K.swatch('#2F61EF'), K.t('After'), K.swatch('#147D4F')], { opacity: 0.4 }),
    K.formRow('Opacity falloff', [K.slider({ w: 'width/field-m' }), K.small('70 → 15 %')]),
    S.group('Animation'),
    K.formRow('Auto keyframe', [K.check('Create keyframe on transform', true)]),
    K.formRow('Default ease', [K.select('Ease In-Out', { w: 'width/field-m' })]),
    K.formRow('Tween on drag', [K.select('Classic tween', { w: 'width/field-m' })])],
  Drawing: () => [S.group('Default mode'),
    K.formRow('New shapes', [K.seg(['Merge drawing', 'Object drawing'], 1)]),
    S.group('Pen tool'),
    K.formRow('Pen', [K.check('Show pen preview', true), K.check('Show solid points', false, D1.off)]),
    K.formRow('Cursors', [K.check('Precise cursors', false)]),
    S.group('Recognition')].concat(['Connect lines', 'Smooth curves', 'Lines', 'Shapes', 'Click accuracy'].map(l => K.formRow(l, [K.select('Normal', { w: 'width/field-m' })])), [
    S.group('Snapping'),
    K.formRow('Tolerance', [K.num('', '8', 'px', { w: 'width/field-s', stepper: true })]),
    K.formRow('Snap', [K.check('Snap to pixels', false), K.check('Snap align', true, D1.off)])]),
  Clipboard: () => [S.group('Copying'),
    K.formRow('Bitmaps', [K.select('Match screen (144 ppi)', { w: 'width/field-l', state: 'Disabled' })]),
    K.formRow('Gradients', [K.select('Normal', { w: 'width/field-m', state: 'Disabled' })]),
    K.formRow('Other apps', [K.check('Copy as SVG to other apps', true, D1.off), K.check('Keep layer names', true, D1.off)]),
    S.group('Pasting'),
    K.formRow('Default paste', [K.seg(['In centre', 'In place'], 0)]),
    K.formRow('Vectors', [K.check('Import vector paste as editable paths', true)]),
    K.formRow('Bitmaps', [K.check('Ask before pasting large bitmaps', true, D1.off)])],
  Shortcuts: () => D1.shortcutsPage(),
  Publish: () => [S.group('Target'),
    K.formRow('Default target', [K.select('HTML5 · Rust / WebAssembly', { w: 'width/field-l' })]),
    K.formRow('Output folder', [K.field('', '~/Projects/ideas/publish'), K.btn('Browse…')]),
    K.formRow('Files', [K.check('HTML wrapper', true), K.check('Assets folder', true), K.check('Source maps', false)]),
    S.group('Preview & publish'),
    K.formRow('Preview mode', [K.seg(['Technical bar', 'Clean stage'], 0), K.check('Clean publish output', false)]),
    K.formRow('Technical bar', [K.check('FPS & frames', true), K.check('Stage size (W×H)', true), K.check('Playback controls', true)]),
    K.formRow('', [K.check('Weight & size report', true), K.check('Trace & diagnostics', true)]),
    K.formRow('Compiler', [K.check('Strict mode', true), K.check('Treat warnings as errors', false), K.check('Publish on save', false)]),
    S.group('Compression'),
    K.formRow('Images', [K.slider({ w: 'width/field-m' }), K.small('JPEG 80')]),
    K.formRow('Audio', [K.select('MP3 128 kbps', { w: 'width/field-m' }), K.select('GPU (WebGL)', { label: 'Acceleration', w: 'width/field-l' })]),
    K.formRow('Sequences', [K.select('Keep individual frames', { w: 'width/field-l' })]),
    K.formRow('Sequence import', [K.select('Keyframes on layer', { w: 'width/field-l' }), K.num('Step', '1', null, { w: 'width/field-s' })])],
  AI: () => [S.group('Connections'),
    col({ name: 'Connections', w: 'fill', radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle', clip: true }, [
      D1.aiRow('Work — Anthropic', 'Anthropic · claude-sonnet-5-5', 'Ready', 'Accent', true),
      D1.aiRow('Local Ollama', 'Local (OpenAI-compatible) · llama3.1', 'Not tested', 'Neutral'),
      D1.aiRow('OpenAI', 'OpenAI · gpt-4o', 'No key', 'Warning')]),
    row({ name: 'Connection actions', w: 'fill', gap: 'gap/control' }, [K.btn('Add connection…'), K.btn('Edit…', { kind: 'Ghost' }), K.btn('Test', { kind: 'Ghost' }), K.btn('Remove', { kind: 'Ghost' })]),
    S.group('Default'),
    K.formRow('Connection', [K.select('Work — Anthropic', { w: 'width/field-l' })]),
    S.group('Privacy'),
    K.msg('Nothing is sent to a provider until you press Send in the review dialog. Every request lists what will leave this computer.', 'Info'),
    K.msg('API keys are kept in the system keychain. They are never written to the project, to preferences or to logs.', 'Info')],
  Updates: () => [S.group('Version'),
    K.formRow('Version', [txt('NeoFlash 0.1.0-beta.1 · build 412', 'Body', 'text/primary', { grow: true })]),
    K.formRow('Updates', [K.btn('Check now'), K.btn('Release notes', { kind: 'Ghost' }), K.small('Up to date · Last checked just now', null, { grow: true })]),
    K.formRow('Channel', [K.seg(['Stable', 'Beta'], 0)]),
    K.formRow('Automatic', [K.check('Check for updates automatically', true)]),
    K.formRow('Background', [K.check('Download in background', false, D1.off)]),
    S.group('Privacy'),
    K.formRow('Statistics', [K.check('Send anonymous usage statistics', false, D1.off)]),
    K.formRow('Crash reports', [K.check('Send crash reports', false, D1.off)]),
    K.msg('Usage telemetry and crash reports are disabled; no data is collected or transmitted.', 'Info'),
    S.group('Extensions'),
    K.formRow('Installed', [K.t('3 extensions installed', 'text/primary'), K.btn('Manage…', D1.off)]),
    K.small('Extension update checks are not available yet.')],
};
// Same shell as S.preferences: one size for every page, the same sidebar with the page selected.
D1.preferences = pg => K.dialog('Preferences', { w: 'width/dialog-l', h: 'height/dialog-l' }, [S.dialogBody([
  S.side(S.PREFS.map(p => S.navRow(p[0], p[1], p[1] === pg))), S.page(D1.prefPages[pg]())])],
  [K.btn('Reset to defaults', { kind: 'Ghost' }), spacer(), K.btn('Cancel'), K.btn('Save', { kind: 'Primary' })]);
async function nf3BuildDialogsA() {
  nf3MoreVars();
  const page = await nfMaster(), out = [];
  const two = (a, b) => [row({ gap: 40, align: 'MIN' }, [D1.preferences(a), D1.preferences(b)])];
  out.push(nfBoard(page, 'Dialog / Preferences — Code editor, Timeline', 'Same shell and form grid as General. Code editor: Size and Line share the Font row, the 13 syntax colours are one compact two-column list beside the sample, Reset colours sits under the list. Options are checkboxes; controls that are not available yet stay in place, disabled.',
    two('Code editor', 'Timeline'), { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Preferences — Drawing, Clipboard', 'Every option has a row label in the label column; the five Recognition selects share one width. Nothing removed against v2.25.',
    two('Drawing', 'Clipboard'), { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Preferences — Shortcuts, Publish', 'Shortcuts: set select with Duplicate / Rename / Delete and Export as HTML… in the page header row, search and filters in the second row, one size/row per command (name · conflict badge · key caps) in a list that fills the page, Reset command under the list. Publish: Acceleration sits beside Audio with its label inside the field.',
    two('Shortcuts', 'Publish'), { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Preferences — AI, Updates', 'AI: one row per connection (name · provider and model · status badge), actions under the list. Updates: beta wording of the application — version line, Check now, channel, and the telemetry and extension options that are not available yet shown disabled.',
    two('AI', 'Updates'), { noteW: 1100 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
