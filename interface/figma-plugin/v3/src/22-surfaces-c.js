// NF3 surfaces C: Scripting Reference (API browser, Guides) and the Flash 8 Help study.
S.code = lines => col({ name: 'Code', w: 'fill', pad: [6, 8], gap: 1, radius: 'radius/control', fill: 'bg/field', stroke: 'border/subtle' }, lines.map(l => txt(l, 'Mono', 'text/primary', { grow: true, wrap: true })));
S.para = (s, color) => txt(s, 'Dialog Body', color || 'text/secondary', { grow: true, wrap: true });
S.status = k => dot(6, k === 'W' ? 'success/default' : k === 'P' ? 'warning/default' : 'text/disabled', { name: 'Status' });
S.member = (m, sel) => row({ name: 'Member', w: 'fill', h: 'size/row', pad: [0, 'pad/field'], gap: 'gap/inline', radius: 'radius/check', fill: sel ? 'bg/selected' : null },
  [S.status(m[0]), txt(m[1], 'Mono', m[0] === 'M' ? 'text/tertiary' : 'text/primary'), txt(m[2], 'Mono Small', 'text/tertiary', { grow: true })]);
S.kv = (k, v) => row({ name: 'Parameter', w: 'fill', gap: 'gap/control', align: 'MIN' }, [row({ w: 'width/field-s' }, [txt(k, 'Mono', 'text/primary')]), txt(v, 'Body', 'text/secondary', { grow: true, wrap: true })]);
S.refBar = (lang, hint) => row({ name: 'Reference bar', w: 'fill', pad: ['pad/panel', 'pad/dialog'], gap: 'gap/section', stroke: 'border/subtle', sides: [0, 0, 1, 0] },
  [K.seg(['ActionScript 2', 'ActionScript 3', 'JavaScript / TypeScript'], lang), K.search(hint), K.badge('Project: ActionScript 3', 'Accent')]);
S.refTree = (tab, kids) => { const c = col({ name: 'Reference tree', w: 'width/tree', fill: 'bg/app', stroke: 'border/subtle', sides: [0, 1, 0, 0] },
  [row({ name: 'Tabs', w: 'fill', h: 'size/tab', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [K.tab('API', tab === 0), K.tab('Guides', tab === 1)]), col({ name: 'Tree', w: 'fill', pad: 'pad/menu', gap: 1 }, kids)]); markGrow(c, false, true); return c; };
S.refFooter = s => row({ name: 'Reference footer', w: 'fill', h: 'size/status', pad: [0, 'pad/dialog'], gap: 'gap/control', fill: 'bg/app', stroke: 'border/subtle', sides: [1, 0, 0, 0] },
  [K.small(s), spacer(), K.kbd('↑↓'), K.small('move'), K.kbd('↵'), K.small('open'), K.kbd('⌘↵'), K.small('insert at cursor')]);
S.referenceApi = () => {
  const tree = S.refTree(0, [K.tree('Top level', { chev: false, icon: 'package' }), K.tree('flash.display', { chev: true, icon: 'package' })]
    .concat(['Bitmap', 'DisplayObject', 'Graphics', 'Loader', 'MovieClip', 'Shape', 'Sprite', 'Stage'].map(c => K.tree(c, { level: 1, icon: 'class', state: c === 'MovieClip' ? 'Selected' : 'Default' })))
    .concat(['flash.events', 'flash.filters', 'flash.geom', 'flash.media', 'flash.net', 'flash.text', 'flash.utils'].map(p => K.tree(p, { chev: false, icon: 'package' }))));
  const P = [['W', 'currentFrame', ': int  ·  read-only'], ['W', 'currentLabel', ': String  ·  read-only'], ['P', 'currentScene', ': Scene  ·  read-only'], ['W', 'totalFrames', ': int  ·  read-only']];
  const M = [['W', 'gotoAndPlay', '(frame:Object, scene:String = null):void'], ['W', 'gotoAndStop', '(frame:Object, scene:String = null):void'], ['W', 'nextFrame', '():void'], ['W', 'play', '():void'], ['M', 'prevScene', '():void'], ['W', 'stop', '():void']];
  const mid = col({ name: 'Class', gap: 'gap/row', pad: ['pad/panel', 'pad/dialog'], clip: true }, [
    row({ w: 'fill', gap: 'gap/control' }, [icon('class', 'accent/default'), txt('MovieClip', 'Heading', 'text/primary'), K.badge('Partial', 'Warning'), spacer(), K.btn('Open in browser', { kind: 'Ghost', icon: 'external' })]),
    txt('flash.display  ·  MovieClip → Sprite → DisplayObjectContainer → InteractiveObject → DisplayObject → EventDispatcher', 'Mono Small', 'text/tertiary', { grow: true }),
    S.para('A display object with its own timeline. Frame scripts run on it; instances are placed on the Stage or created from a Library symbol with a linkage class.'),
    row({ w: 'fill', gap: 'gap/control', pad: ['gap/row', 0, 0, 0] }, [K.seg(['All', 'Properties', 'Methods', 'Events'], 0), K.check('Hide missing', false)]),
    K.small('48 members · 31 working · 9 partial · 8 missing', null, { grow: true }),
    S.group('Properties'), col({ name: 'Members', w: 'fill', gap: 1 }, P.map(m => S.member(m))), S.group('Methods'), col({ name: 'Members', w: 'fill', gap: 1 }, M.map(m => S.member(m, m[1] === 'gotoAndPlay')))]);
  markGrow(mid, true, true);
  const det = col({ name: 'Member detail', w: 300, gap: 'gap/row', pad: ['pad/panel', 'pad/panel'], fill: 'bg/app', stroke: 'border/subtle', sides: [0, 0, 0, 1], clip: true }, [
    row({ w: 'fill', gap: 'gap/control' }, [txt('gotoAndPlay()', 'Title', 'text/primary'), K.badge('Working', 'Success'), spacer(), K.small('method')]),
    S.code(['public function gotoAndPlay(', '    frame:Object,', '    scene:String = null', '):void']),
    S.para('Moves the playhead to the frame and plays from it.'),
    S.group('Parameters'), S.kv('frame', 'Frame number (1-based) or frame label.'), S.kv('scene', 'Optional scene name; resolved through the flattened main timeline.'),
    S.group('In NeoFlash'), K.msg('Runs in HTML5 publish and Test Movie. A missing label reports a runtime diagnostic instead of failing silently.', 'Success'),
    S.group('Example'), S.code(['button.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {', '    gotoAndPlay("intro");', '});']),
    row({ w: 'fill', gap: 'gap/control' }, [K.btn('Copy', { icon: 'copy' }), K.btn('Insert at cursor', { kind: 'Primary' })]),
    S.group('See also'), txt('gotoAndStop()  ·  play()  ·  currentFrame  ·  Scene', 'Mono Small', 'accent/default', { grow: true, wrap: true })]);
  markGrow(det, false, true);
  return K.dialog('Scripting Reference', { w: 'width/reference', h: 'height/dialog-xl' }, [S.refBar(1, 'Search ActionScript 3: classes, methods, properties, events'), S.dialogBody([tree, mid, det]), S.refFooter('ActionScript 3 · 563 classes · 5 173 members')]);
};
S.referenceGuides = () => {
  const G = ['Language differences', 'Frame, object and module scripts', 'Events', 'Display list', 'Timeline control', 'Text and fonts', 'Sound', 'Loading and networking', 'JavaScript runtime', 'TypeScript', 'Browser and Web APIs', 'Unsupported features and diagnostics'];
  const tree = S.refTree(1, G.map((g, i) => K.tree(g, { icon: 'file-code', state: i === 1 ? 'Selected' : 'Default' })));
  const cell = (s, head) => row({ name: 'Cell', pad: [4, 8], w: 'fill', align: 'MIN' }, [txt(s, head ? 'Small Strong' : 'Mono Small', head ? 'text/secondary' : 'text/primary', { grow: true, wrap: true })]);
  const tr = (cells, head) => row({ name: 'Table row', w: 'fill', align: 'MIN', stroke: 'border/subtle', sides: [0, 0, 1, 0], fill: head ? 'bg/app' : null }, cells.map(c => cell(c, head)));
  const art = col({ name: 'Article', gap: 'gap/section', pad: ['pad/dialog', 'pad/dialog'], clip: true }, [
    txt('Frame, object and module scripts', 'Heading', 'text/primary'),
    S.para('Where a script lives decides what “this” is and when the code runs. The table lists the three contexts in the current project language; the other languages are one click away in the language switch above.'),
    col({ name: 'Table', w: 'fill', stroke: 'border/subtle', radius: 'radius/control', clip: true }, [tr(['Context', 'this', 'Runs'], true), tr(['Frame script', 'owning MovieClip', 'each time the playhead enters the keyframe']), tr(['Object script', 'target DisplayObject', 'when its event fires']), tr(['Class / module', 'instance or module scope', 'on construction / import'])]),
    txt('Frame script', 'Title', 'text/primary'),
    S.para('Attach to a keyframe in the Actions panel (F9). Timeline methods are in scope.'),
    S.code(['stop();', 'nextButton.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {', '    gotoAndPlay("chapter2");', '});']),
    row({ gap: 'gap/control' }, [K.btn('Copy', { icon: 'copy' }), K.btn('Insert at cursor'), K.btn('Open MovieClip in API', { kind: 'Ghost' })])]);
  markGrow(art, true, true);
  const toc = col({ name: 'On this page', w: 'width/sidebar-wide', gap: 'gap/row', pad: ['pad/dialog', 'pad/panel'], stroke: 'border/subtle', sides: [0, 0, 0, 1] }, [txt('On this page', 'Overline', 'text/tertiary'), K.t('Contexts', 'accent/default'), K.t('Frame script'), K.t('Object script'), K.t('Class and module'), K.t('Common mistakes')]);
  markGrow(toc, false, true);
  return K.dialog('Scripting Reference', { w: 'width/reference', h: 'height/dialog-xl' }, [S.refBar(1, 'Search guides for ActionScript 3'), S.dialogBody([tree, art, toc]), S.refFooter('Guides · 12 chapters · written for the project language')]);
};
S.helpFlash8 = () => {
  const B = [['Getting started with NeoFlash', 0, false], ['Using NeoFlash', 0, true], ['Workspace', 1, null], ['Drawing', 1, null], ['Symbols, instances and the Library', 1, null], ['Timeline and animation', 1, null], ['Learning ActionScript 2.0', 0, false],
    ['ActionScript 2.0 Language Reference', 0, true], ['Global functions', 1, false], ['Classes', 1, true], ['Button', 2, null], ['Color', 2, null], ['MovieClip', 2, null], ['ActionScript 3.0 Language Reference', 0, false], ['JavaScript and TypeScript API', 0, false], ['Extending NeoFlash (JSFL)', 0, false]];
  const tree = col({ name: 'Books', w: 'width/sidebar-wide', fill: 'bg/app', stroke: 'border/subtle', sides: [0, 1, 0, 0] }, [
    row({ name: 'Tabs', w: 'fill', h: 'size/tab', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [K.tab('Contents', true), K.tab('Search', false), K.tab('Bookmarks', false)]),
    col({ name: 'Tree', w: 'fill', pad: 'pad/menu', gap: 1 }, B.map(b => K.tree(b[0], { level: b[1], chev: b[2], icon: b[2] == null ? 'file-code' : 'bookmark', state: b[0] === 'MovieClip' ? 'Selected' : 'Default' })))]);
  tree.resize(280, tree.height); markGrow(tree, false, true);
  const art = col({ name: 'Topic', gap: 'gap/section', pad: ['pad/dialog', 'pad/dialog'], clip: true }, [
    txt('ActionScript 2.0 Language Reference  ›  Classes  ›  MovieClip', 'Small', 'text/tertiary', { grow: true }),
    txt('MovieClip.gotoAndPlay()', 'Heading', 'text/primary'),
    row({ gap: 'gap/control' }, [K.badge('Flash Player 5', 'Neutral'), K.badge('Working in NeoFlash', 'Success')]),
    txt('Usage', 'Title', 'text/primary'), S.code(['my_mc.gotoAndPlay(frame:Object) : Void']),
    txt('Parameters', 'Title', 'text/primary'), S.kv('frame', 'A number representing the frame number, or a string representing the label of the frame, to which the playhead is sent.'),
    txt('Description', 'Title', 'text/primary'), S.para('Method; starts playing the SWF file at the specified frame. To indicate a scene as well as a frame, use the global gotoAndPlay().'),
    txt('Example', 'Title', 'text/primary'), S.code(['stop();', 'myBtn_btn.onRelease = function() {', '    gotoAndPlay("intro");', '};']),
    row({ w: 'fill', gap: 'gap/control', pad: ['gap/section', 0, 0, 0] }, [K.btn('MovieClip.getURL()', { kind: 'Ghost', icon: 'chevleft' }), spacer(), K.btn('MovieClip.gotoAndStop()', { kind: 'Ghost' })])]);
  markGrow(art, true, true);
  const bar = row({ name: 'Help bar', w: 'fill', pad: ['pad/panel', 'pad/dialog'], gap: 'gap/control', stroke: 'border/subtle', sides: [0, 0, 1, 0] },
    [K.ibtn('chevleft', { kind: 'Outlined' }), K.ibtn('chevright', { kind: 'Outlined', state: 'Disabled' }), K.ibtn('bookmark', { kind: 'Outlined' }), K.search('Search all books'), K.btn('Print')]);
  return K.dialog('NeoFlash Help', { w: 'width/reference', h: 'height/dialog-xl' }, [bar, S.dialogBody([tree, art])]);
};
async function nf3BuildC() {
  const hv = V('height/dialog-l'); ['Normal', 'Compact', 'Classic Flash'].forEach((m, i) => hv.setValueForMode(NF.densColl.modes.find(x => x.name === m).modeId, [576, 500, 440][i]));
  const page = await nfMaster(), out = [];
  out.push(nfBoard(page, 'Dialog / Scripting Reference — API', 'Implemented variant. Always exactly one language (defaults to the project language), one search field whose placeholder names its scope, three fixed columns: packages and classes · members with support status · member detail with signature, parameters, NeoFlash status, example and Insert at cursor. One size (width/reference × height/dialog-xl).', [S.referenceApi()], { noteW: 900 }));
  out.push(nfBoard(page, 'Dialog / Scripting Reference — Guides', 'Second tab of the same window: plain articles for the selected language with a table of contents; no tag clouds, cards or decorative blocks.', [S.referenceGuides()], { noteW: 900 }));
  out.push(nfBoard(page, 'Dialog / Help — Flash 8 study (not implemented)', 'Design study requested by the owner: a modern reading of the Flash 8 Help panel — one tree of books (manual, learning guides, language references), Contents / Search / Bookmarks, topic pages in the classic Usage · Parameters · Description · Example order. The application ships the API browser above.', [S.helpFlash8()], { noteW: 900 }));
  nfArrange(page);
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
