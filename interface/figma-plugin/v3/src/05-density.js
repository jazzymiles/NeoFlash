// NF3 · Density — the density collection. Idempotent: creates missing variables, updates values of existing ones.
// name: [Normal, Compact, Classic Flash, scope]
// The Rust side is crates/neoflash-ui/src/metrics.rs; crates/neoflash-ui/tests/density_parity.rs fails when
// the two disagree. Keep one entry per `'name': [n, n, n, 'S|G|R']` so that test can read this file.
const NF3_DENSITY = {
  'size/control': [28, 24, 22, 'S'], 'size/row': [26, 22, 20, 'S'], 'size/tab': [34, 30, 26, 'S'],
  'size/section': [28, 24, 22, 'S'], 'size/top-bar': [40, 34, 30, 'S'], 'size/status': [24, 22, 20, 'S'],
  'size/dialog-header': [48, 40, 34, 'S'], 'size/tool': [32, 28, 24, 'S'], 'size/icon-tool': [20, 20, 16, 'S'],
  'size/toggle': [20, 18, 16, 'S'], 'size/check': [16, 14, 14, 'S'], 'size/switch-w': [28, 26, 24, 'S'],
  'size/switch-h': [16, 14, 14, 'S'], 'size/knob': [12, 10, 10, 'S'], 'size/slider-thumb': [12, 12, 10, 'S'],
  'size/badge': [20, 18, 16, 'S'], 'size/swatch': [18, 16, 16, 'S'], 'size/timeline-controls': [38, 32, 28, 'S'],
  'size/layer-row': [24, 20, 18, 'S'], 'size/timeline-ruler': [26, 22, 20, 'S'], 'size/stage-ruler': [18, 16, 16, 'S'],
  'size/stage-bar': [34, 30, 26, 'S'], 'size/menu-separator': [9, 7, 7, 'S'],
  'pad/field': [10, 8, 8, 'G'], 'pad/button': [12, 10, 10, 'G'], 'pad/panel': [14, 12, 10, 'G'], 'pad/dialog': [24, 18, 14, 'G'],
  'pad/tools': [6, 6, 5, 'G'], 'pad/menu': [4, 4, 3, 'G'],
  'gap/control': [8, 6, 6, 'G'], 'gap/row': [8, 6, 5, 'G'], 'gap/section': [14, 12, 10, 'G'], 'gap/tool': [2, 2, 1, 'G'],
  'gap/inline': [8, 6, 6, 'G'],
  'radius/control': [6, 5, 4, 'R'], 'radius/panel': [8, 8, 6, 'R'], 'radius/check': [4, 3, 3, 'R'],
  'width/inspector': [288, 264, 248, 'S'], 'width/label': [80, 72, 64, 'S'], 'width/dialog-label': [120, 108, 96, 'S'],
  'width/layer-column': [324, 300, 284, 'S'], 'width/num': [68, 60, 56, 'S'], 'width/tools-1': [44, 40, 34, 'S'],
  'width/tools-2': [80, 72, 61, 'S'], 'width/menu': [216, 196, 180, 'S'], 'width/sidebar': [168, 152, 140, 'S'],
  'width/dialog-s': [380, 344, 320, 'S'], 'width/dialog-m': [500, 456, 424, 'S'], 'width/dialog-l': [740, 680, 640, 'S'],
  'width/dialog-xl': [900, 840, 780, 'S'], 'height/dialog-l': [600, 520, 470, 'S'], 'height/dialog-xl': [660, 610, 570, 'S'],
  'width/window': [1440, 1440, 1440, 'S'], 'height/window': [900, 900, 900, 'S'], 'height/bottom-dock': [248, 220, 196, 'S'],
};
async function nf3BuildDensity() {
  const cols = await figma.variables.getLocalVariableCollectionsAsync();
  let coll = cols.find(c => c.name === 'NF3 · Density');
  if (!coll) {
    coll = figma.variables.createVariableCollection('NF3 · Density');
    coll.renameMode(coll.modes[0].modeId, 'Normal');
    coll.addMode('Compact'); coll.addMode('Classic Flash');
  }
  const modeId = n => coll.modes.find(m => m.name === n).modeId;
  const ids = [modeId('Normal'), modeId('Compact'), modeId('Classic Flash')];
  const existing = {};
  for (const id of coll.variableIds) { const v = await figma.variables.getVariableByIdAsync(id); if (v) existing[v.name] = v; }
  const scopes = { S: ['WIDTH_HEIGHT'], G: ['GAP'], R: ['CORNER_RADIUS'] };
  let created = 0;
  for (const name of Object.keys(NF3_DENSITY)) {
    const d = NF3_DENSITY[name];
    let v = existing[name];
    if (!v) { v = figma.variables.createVariable(name, coll, 'FLOAT'); created++; }
    v.scopes = scopes[d[3]];
    v.setVariableCodeSyntax('WEB', 'var(--nf-' + name.replace('/', '-') + ')');
    ids.forEach((id, i) => v.setValueForMode(id, d[i]));
  }
  return { collectionId: coll.id, modes: coll.modes, created, total: coll.variableIds.length };
}
