// NF3 dialogs E (group D2): AI connection, Send to AI, AI answer, productivity dialogs. Uses the D2 helpers of 33-dialogs-d.js.
D2.cancelThen = (label) => [spacer(), K.btn('Cancel'), K.btn(label, { kind: 'Primary' })];

// ---- AI connection (cloud provider / local provider with a failed test)
D2.aiConnection = local => K.dialog('AI connection', { w: 'width/dialog-m' }, [D2.body([
  K.formRow('Provider', [K.select(local ? 'Local (OpenAI-compatible)' : 'Anthropic', { w: 'width/field-l' })]),
  K.formRow('Name', [K.field('', local ? 'Local Ollama' : 'Work — Anthropic')]),
  K.formRow('Base URL', [K.field('', local ? 'http://localhost:11434/v1' : 'https://api.anthropic.com')]),
  K.formRow('Model', [K.field('', local ? 'llama3.1' : 'claude-sonnet-5-5', { w: 'width/field-l' })]),
  K.formRow('API key', [K.field('', local ? 'Not required' : '••••••••••••••••••••', { state: local ? 'Disabled' : 'Default' })]),
  D2.opt([D2.note(local ? 'A local provider may use http:// on this computer. Cloud providers must use https://.' : 'Stored in the system keychain. The key is never shown again and never saved with the project.')]),
  S.group('Advanced'),
  K.formRow('Timeout', [K.num('', '15', 's', { w: 'width/field-s' }), K.num('Retries', '2', null, { w: 'width/field-s' })]),
  K.formRow('Options', [K.check('Streaming responses', true), K.check('Image input', false)]),
  K.formRow('', [K.btn('Test connection'), local ? K.msg('Provider unreachable (offline): connection refused', 'Error') : K.msg('Connected · claude-sonnet-5-5', 'Success')]),
])], D2.cancelThen('Save'));

// ---- Send to AI (review of what leaves the computer)
D2.SENT = [['Project and symbol names', 'Ideas in motion · Scene 1 · frame 30'], ['Selected elements (2)', 'name, type and position'], ['Script excerpt', 'ActionScript 3.0 · 1.2 KB'], ['Asset list (2)', 'name, type and size only']];
D2.sentRow = r => row({ name: 'Row / ' + r[0], w: 'fill', minH: 'size/row', gap: 'gap/control' }, [K.check(r[0], true), txt(r[1], 'Small', 'text/tertiary', { grow: true, align: 'RIGHT' })]);
D2.sendToAi = () => K.dialog('Send to AI', { w: 'width/dialog-m' }, [D2.body([
  K.formRow('Connection', [K.select('Work — Anthropic · claude-sonnet-5-5')]),
  S.group('Request'),
  K.formRow('Ask', [K.field('', 'Why does the orbit clip stop at frame 30?')]),
  S.group('What will be sent'),
  col({ name: 'Context table', w: 'fill', pad: [2, 'pad/field'], radius: 'radius/control', stroke: 'border/subtle' }, D2.SENT.map(D2.sentRow)),
  D2.note('Not sent: intro.mp4 (video is not an allowed type), theme.mp3 (larger than 64 KB).'),
  K.msg('Keys, tokens and passwords found in the text are replaced with [REDACTED_SECRET] before sending.', 'Info'),
  D2.note('Destination: api.anthropic.com over HTTPS. The answer is a proposal: nothing in the document changes until you apply it.'),
])], D2.cancelThen('Send'));

// ---- AI answer: one size in every state, the answer area scrolls
D2.aiText = lines => col({ name: 'Answer', w: 'fill', gap: 'gap/row', pad: ['gap/section', 'pad/field'], radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle' },
  lines.map((l, i) => txt(l, i === 0 && lines.length > 1 ? 'Body Strong' : 'Body', 'text/primary', { grow: true, wrap: true })));
D2.aiCard = (name, kids) => col({ name: 'Proposal / ' + name, w: 'fill', gap: 'gap/row', pad: ['pad/panel', 'pad/panel'], radius: 'radius/control', fill: 'bg/app', stroke: 'border/subtle' }, kids);
D2.aiTr = (k, v) => row({ name: 'Table row', w: 'fill', h: 'size/row', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [row({ name: 'Property', w: 'width/dialog-label' }, [K.t(k)]), txt(v, 'Body', 'text/primary', { grow: true })]);
D2.AI = {
  waiting: () => ({ foot: [spacer(), K.btn('Stop')], body: [
    K.t('Waiting for the provider…', 'text/primary'), K.progress(),
    K.msg('The provider limits requests. Trying again in 12 s (attempt 2 of 3).', 'Info')] }),
  streaming: () => ({ foot: [spacer(), K.btn('Stop')], body: [
    K.t('Receiving the answer…', 'text/primary'), K.progress(),
    D2.aiText(['The clip stops on a stop() call left in frame 30 of orbit_mc. It was needed for the intro, and the loop label after it is never reached, so'])] }),
  proposals: () => ({ foot: [K.btn('New request…', { kind: 'Ghost' }), spacer(), K.btn('Done', { kind: 'Primary' })], body: [
    D2.aiText(['The clip stops on a leftover stop() in frame 30.', 'It was needed for the intro, and the loop label after it is never reached. Jump back to the label instead of stopping, and ease the ball out before the hand-off so the loop does not jerk.']),
    S.group('Proposals'),
    D2.aiCard('code — applied', [
      K.line([K.badge('ActionScript 3.0'), txt('Frame script · orbit_mc, frame 30', 'Body', 'text/primary', { grow: true }), K.btn('Undo', { kind: 'Ghost' })]),
      S.code(['// return to the loop instead of stopping', 'gotoAndPlay("loop");']),
      D2.note('Replaces the stop() call so the clip returns to the loop label.'),
      K.msg('Inserted into Actions at the caret.', 'Success')]),
    D2.aiCard('timing', [
      K.line([K.badge('Timing'), txt('Layer ball', 'Body', 'text/primary', { grow: true }), K.btn('Apply to timeline'), K.btn('Dismiss', { kind: 'Ghost' })]),
      col({ name: 'Table', w: 'fill' }, [D2.aiTr('Frames', '1 – 30'), D2.aiTr('Easing', 'Ease out')]),
      D2.note('Slows the ball before the hand-off at frame 30.')]),
    K.msg('NeoFlash does not check AI output. Read a proposal before applying it; each one is a single Undo step.', 'Warning')] }),
  failed: () => ({ foot: [K.btn('Edit connection…', { kind: 'Ghost' }), spacer(), K.btn('Close'), K.btn('Try again', { kind: 'Primary' })], body: [
    K.msg('HTTP 401: invalid x-api-key', 'Error'),
    D2.note('Nothing was changed in the document.')] }),
};
D2.aiAnswer = st => {
  const s = D2.AI[st]();
  return K.dialog('AI answer', { w: 'width/dialog-m', h: 'height/dialog-xl' }, [S.page([K.small('Work — Anthropic · claude-sonnet-5-5')].concat(s.body))], s.foot);
};

// ---- Productivity dialogs
D2.batchRename = () => K.dialog('Batch rename library items', { w: 'width/dialog-m' }, [D2.body([
  S.group('Scope'),
  K.formRow('Items', [K.select('Selected items / folder', { w: 'width/field-l' })]),
  D2.opt([K.check('Also rename matching Stage instances', true)]),
  S.group('Affixes and find / replace'),
  K.formRow('Prefix', [K.field('', 'hero_', { w: 'width/field-l' })]),
  K.formRow('Suffix', [K.field('', ' ', { w: 'width/field-l' })]),
  K.formRow('Find', [K.field('', 'copy', { w: 'width/field-l' })]),
  K.formRow('Replace', [K.field('', ' ', { w: 'width/field-l' })]),
  S.group('Sequential numbering'),
  D2.opt([K.check('Append sequential numbering', true)]),
  K.formRow('Start', [K.num('', '1', null, { w: 'width/num' }), K.num('Digits', '2', null, { w: 'width/field-s' })]),
])], D2.cancelThen('Rename'));
D2.batchLinkage = () => K.dialog('Batch linkage (Export for ActionScript)', { w: 'width/dialog-m' }, [D2.body([
  K.formRow('Scope', [K.select('All library symbols and sounds')]),
  S.group('ActionScript linkage'),
  D2.opt([K.check('Export for ActionScript', true)]),
  D2.opt([K.check('Export in frame 1', true)]),
  K.formRow('Prefix', [K.field('', 'com.example.', { w: 'width/field-l' })]),
  D2.opt([K.small('Example class: com.example.HeroWalk')]),
])], D2.cancelThen('Apply'));
D2.batchBitmaps = () => K.dialog('Batch bitmap compression and smoothing', { w: 'width/dialog-m' }, [D2.body([
  K.formRow('Target', [K.select('All bitmap resources', { w: 'width/field-l' })]),
  S.group('Compression and quality'),
  K.formRow('Compression', [K.select('Photo (JPEG)', { w: 'width/field-m' })]),
  K.formRow('Quality', [K.num('', '80', null, { w: 'width/num' })]),
  D2.opt([K.check('Allow smoothing', true)]),
])], D2.cancelThen('Apply'));
D2.instanceNamer = () => K.dialog('Auto instance namer', { w: 'width/dialog-m' }, [D2.body([
  S.group('Naming pattern'),
  K.formRow('Pattern', [K.select('Prefix + sequence (<prefix><index>)')]),
  K.formRow('Prefix', [K.field('', 'item_', { w: 'width/field-l' })]),
  K.formRow('Start', [K.num('', '1', null, { w: 'width/num' }), K.num('Digits', '2', null, { w: 'width/field-s' })]),
  K.line([K.check('Only rename unnamed instances when nothing is selected', false)]),
])], D2.cancelThen('Name instances'));
D2.range = (label, a, b, unit) => K.formRow(label, [K.num('min', a, unit, { w: 'width/field-s' }), K.num('max', b, unit, { w: 'width/field-s' })]);
D2.randomize = () => K.dialog('Randomize selection', { w: 'width/dialog-m' }, [D2.body([
  K.formRow('Seed', [K.num('', '1234', null, { w: 'width/field-s' })]),
  S.group('Transform and alpha ranges'),
  D2.range('ΔX', '−20', '20'),
  D2.range('ΔY', '−20', '20'),
  D2.range('Scale', '80', '120', '%'),
  D2.opt([K.check('Uniform X/Y scale', true)]),
  D2.range('Rotation', '−15', '15', '°'),
  D2.range('Alpha', '60', '100', '%'),
])], D2.cancelThen('Randomize'));

async function nf3BuildDialogsE() {
  if (!NF.v['width/field-m']) nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Dialog / AI connection', 'Prefix-in-field rows became the dialog form grid; the Advanced group is always shown, so the dialog keeps one layout for every provider. A local provider disables the API key (“Not required”); the test result sits next to “Test connection” as an inline message.',
    [D2.states([D2.state('Cloud provider', D2.aiConnection(false)), D2.state('Local provider, failed test', D2.aiConnection(true))])], { noteW: 1000 }));
  out.push(nfBoard(page, 'Dialog / Send to AI', 'Review before sending: connection, request, then one row per context item (checkbox · what exactly is sent), followed by the “Not sent”, redaction and destination notes.',
    [D2.sendToAi()], { noteW: 480 }));
  out.push(nfBoard(page, 'Dialog / AI answer', 'One size in all four states (width/dialog-m × height/dialog-xl) instead of a window that grows with the answer: the body scrolls. Proposal actions (Undo · Apply to timeline · Dismiss) moved to the end of each proposal header row.',
    [D2.states([D2.state('Waiting, retry scheduled', D2.aiAnswer('waiting')), D2.state('Streaming', D2.aiAnswer('streaming')), D2.state('Proposals', D2.aiAnswer('proposals')), D2.state('Failed', D2.aiAnswer('failed'))])], { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Batch rename, Batch linkage', 'Section headers became group captions; every field has its own labelled row at a natural width (Prefix, Suffix, Find, Replace), Start and Digits share a row.',
    [D2.states([D2.batchRename(), D2.batchLinkage()])], { noteW: 1000 }));
  out.push(nfBoard(page, 'Dialog / Batch bitmap compression', 'Target, compression and quality as form rows; the selects no longer stretch to the dialog.',
    [D2.batchBitmaps()], { noteW: 480 }));
  out.push(nfBoard(page, 'Dialog / Auto instance namer, Randomize selection', 'Randomize: one row per property with min and max fields (ΔX, ΔY, Scale, Rotation, Alpha) instead of ten prefixed fields; the Seed caption is the row label.',
    [D2.states([D2.instanceNamer(), D2.randomize()])], { noteW: 1000 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
