// NF3 dialogs F (group D2): Commands set, Beta feedback, Paste motion special. Uses the D2 helpers of 33-dialogs-d.js / -e.js.
// ---- Commands set
D2.commandsSet = () => K.dialog('Commands', { w: 'width/dialog-s' }, [D2.body([
  K.formRow('Set', [K.radio('User commands', false)]),
  D2.opt([K.radio('Commands folder', true)]),
  D2.opt([K.radio('API examples', false)]),
  D2.opt([D2.note('Commands are script files (.nfsl) run on the document. User commands live in your profile; the commands folder is shared by the team.')]),
  K.formRow('Action', [K.select('Load (replace list)')]),
])], D2.cancelThen('OK'));

// ---- Beta feedback: optional Email / Name rows, text, attachments
D2.FILES = [['Screenshot 2026-11-15 at 10.42.png', '412 KB'], ['timeline-jump.png', '1.2 MB']];
D2.attachment = f => row({ name: 'Attachment', w: 'fill', minH: 'size/control', gap: 'gap/inline' }, [icon('bitmap'), txt(f[0], 'Body', 'text/primary', { grow: true }), K.small(f[1]), K.ibtn('x')]);
D2.feedback = (email, name, files) => K.dialog('Beta feedback', { w: 'width/dialog-m' }, [D2.body([
  email ? K.formRow('Email', [K.field('', 'ada@example.com')]) : null,
  name ? K.formRow('Name', [K.field('', 'Ada Lovelace')]) : null,
  col({ name: 'Text area', w: 'fill', minH: 88, pad: ['gap/section', 'pad/field'], radius: 'radius/control', fill: 'bg/field', stroke: 'border/default' },
    [txt('The playhead jumps back to frame 1 when I press Enter inside a motion tween. I expected playback to start from the current frame.', 'Body', 'text/primary', { grow: true, wrap: true })]),
  files ? col({ name: 'Attachments', w: 'fill' }, D2.FILES.map(D2.attachment)) : null,
])], [K.btn('Attach image…', { kind: 'Ghost' }), spacer(), K.btn('Cancel'), K.btn('Send', { kind: 'Primary' })]);
D2.bugChip = () => row({ name: 'Beta feedback chip', h: 'size/control', pad: [0, 'pad/field'], gap: 'gap/inline', fill: 'bg/raised', stroke: 'border/default', radius: 'radius/panel' },
  [icon('debug'), txt('Bug?', 'Body', 'text/primary')]);

// ---- Paste motion special (content from panels/paste_motion.rs)
D2.two = (a, b) => row({ name: 'Pair', w: 'fill', gap: 'gap/control', minH: 'size/control' }, [row({ name: 'Cell', w: 'fill' }, [a]), row({ name: 'Cell', w: 'fill' }, [b])]);
D2.pasteMotion = () => K.dialog('Paste motion special', { w: 'width/dialog-m' }, [D2.body([
  S.group('Properties'),
  D2.two(K.check('X position', true), K.check('Y position', true)),
  D2.two(K.check('Horizontal scale', true), K.check('Vertical scale', true)),
  D2.two(K.check('Rotation and skew', true), K.check('Colour', true)),
  D2.two(K.check('Filters', true), K.check('Blend mode', true)),
  K.line([K.check('Ease', true)]),
  S.group('Options'),
  K.line([K.check('Override target scale properties', false)]),
  K.line([K.check('Override target rotation and skew properties', false)]),
])], [K.btn('Select none', { kind: 'Ghost' }), spacer(), K.btn('Cancel'), K.btn('Paste', { kind: 'Primary' })]);

async function nf3BuildDialogsF() {
  if (!NF.v['width/field-m']) nf3MoreVars();
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Dialog / Commands set', 'Small dialog (width/dialog-s) on the form grid: Set as three radio rows with the explanation under them, Action as a select.',
    [D2.commandsSet()], { noteW: 420 }));
  out.push(nfBoard(page, 'Dialog / Beta feedback', 'The four variants differ only in which optional rows are shown (Email, Name; the attachment list appears once an image is attached); text area, “Attach image…” and Cancel / Send never move. The “Bug?” chip in the Stage corner opens the dialog.',
    [D2.states([D2.state('Text only', D2.feedback(false, false, false)), D2.state('Name and text', D2.feedback(false, true, false)), D2.state('Email and text', D2.feedback(true, false, false)),
      D2.state('Email, name and text, two attachments', D2.feedback(true, true, true)), D2.state('Stage chip', D2.bugChip())])], { noteW: 1100 }));
  out.push(nfBoard(page, 'Dialog / Paste motion special', 'Added after v2.25 (content from the editor code): nine property checkboxes in two equal columns, two override options, “Select none” as the ghost button on the left of the footer.',
    [D2.pasteMotion()], { noteW: 480 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
