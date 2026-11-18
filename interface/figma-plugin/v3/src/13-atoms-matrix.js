// NF3 atoms matrix: every atom in its main states, in each of the 12 theme × density combinations.
function nf3Specimen() {
  const line = kids => row({ name: 'Specimen row', gap: 'gap/control', w: 'fill', minH: 'size/control' }, kids);
  const menu = K.menu('Specimen', [K.mi('Undo', { key: '⌘Z', check: false }), K.mi('Snap to guides', { check: true, key: '⇧⌘;' }), K.msep(), K.mi('Arrange', { sub: true, check: false, state: 'Hover' }), K.mi('Paste in place', { check: false, state: 'Disabled' })]);
  const tree = col({ name: 'Tree', w: 'width/tree', gap: 1 }, [K.tree('Library', { chev: true, icon: 'folderopen' }), K.tree('Orbit', { level: 1, icon: 'movieclip', state: 'Selected', meta: '4' }), K.tree('Accent', { level: 1, icon: 'graphic', meta: '2' }), K.tree('Sounds', { chev: false, icon: 'folder' })]);
  return col({ name: 'Specimen', w: 520, gap: 'gap/section', pad: 'pad/dialog', fill: 'bg/panel', stroke: 'border/subtle', radius: 'radius/panel' }, [
    line([K.btn('Publish', { kind: 'Primary', icon: 'upload' }), K.btn('Cancel'), K.btn('Reset', { kind: 'Ghost' }), K.btn('Delete', { kind: 'Danger' }), K.btn('Disabled', { state: 'Disabled' })]),
    line([K.ibtn('layerplus'), K.ibtn('trash', { state: 'Hover' }), K.ibtn('loop', { state: 'Selected' }), K.ibtn('undo', { state: 'Disabled' }), K.ibtn('more', { kind: 'Outlined' }), spacer(), K.tool('select', 'Selected'), K.tool('pen', 'Default', true), K.tool('rect', 'Hover'), K.tool('eraser', 'Disabled')]),
    line([K.num('X', '640'), K.num('W', '216', 'px', { state: 'Focus' }), K.num('', '24', 'fps', { stepper: true }), K.field('', 'Mixed', { state: 'Disabled' })]),
    line([K.field('Name', 'mainOrbit'), K.select('Movie Clip'), K.select('Alpha', { label: 'Style', state: 'Open' })]),
    line([K.search('Search'), K.field('#', 'FF8E4F', { unit: '100 %', state: 'Error' })]),
    line([K.check('Visible', true), K.check('Cache as bitmap', false), K.check('Mixed', 'mixed'), K.check('Off', false, { state: 'Disabled' }), K.radio('Loop', true), K.radio('Once', false)]),
    line([K.sw('Autosave', true), K.sw('Snapping', false), K.slider({ w: 'width/field-m' }), K.num('', '60', '%', { w: 'width/num' })]),
    line([K.seg(['Normal', 'Compact', 'Classic Flash'], 1), K.badge('Partial', 'Warning'), K.badge('Working', 'Success'), K.badge('HTML5', 'Accent'), K.badge('Missing', 'Neutral'), K.badge('Error', 'Danger'), K.kbd('⌘K'), K.kbd('F6')]),
    row({ name: 'Tabs', w: 'fill', h: 'size/tab', stroke: 'border/subtle', sides: [0, 0, 1, 0] }, [K.tab('Properties', true), K.tab('Transform', false), K.tab('Timeline', false, 'film'), spacer(), K.ibtn('more')]),
    K.section('Position and size', true, { action: 'link' }), K.section('Filters', false),
    row({ name: 'Lists', w: 'fill', gap: 'gap/section', align: 'MIN' }, [menu, tree]),
    line([K.swatch('#8B9AB1'), K.swatch('#FF8E4F'), K.swatch('#66A0FF'), K.swatch('#233043'), K.progress({ w: 'width/field-m' }), K.tip('Selection tool', 'V')]),
    K.msg('Scripts in other languages are skipped when publishing.', 'Warning'), K.msg('Label “intro” was not found.', 'Error'),
  ]);
}
async function nf3AtomsMatrix() {
  if (typeof nf3MoreVars === 'function') nf3MoreVars();
  await figma.setCurrentPageAsync(NF.atomsPage);
  const old = NF.atomsPage.findChild(n => n.name === 'NF3 / Atoms matrix'); if (old) old.remove();
  const grid = col({ name: 'NF3 / Atoms matrix', gap: 32, pad: 48, fill: 'bg/app' }, [txt('NF3 · Atoms in every theme and density', 'Heading', 'text/primary')]);
  NF.atomsPage.appendChild(grid);
  const shelf = NF.atomsPage.findChild(n => n.name === 'NF3 / Atoms');
  grid.x = shelf ? shelf.x + shelf.width + 200 : 2000; grid.y = 0;
  for (const theme of ['Light', 'Dark', 'Mid · Neutral Gray', 'Dark · Graphite']) {
    const r = row({ name: theme, gap: 32, align: 'MIN', pad: 24, fill: 'bg/app', radius: 12 }, []);
    for (const density of ['Normal', 'Compact', 'Classic Flash']) {
      const cell = col({ name: theme + ' · ' + density, gap: 8 }, [txt(theme + ' · ' + density, 'Title', 'text/primary'), nf3Specimen()]);
      nfMode(cell, null, density); add(r, cell);
    }
    nfMode(r, theme, null); add(grid, r);
  }
  return { id: grid.id, w: Math.round(grid.width), h: Math.round(grid.height) };
}
