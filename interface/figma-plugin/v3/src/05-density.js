// NF3 · Density — the density collection. Idempotent: creates missing variables, updates values of existing ones.
// name: [Normal, Compact, Classic Flash, scope]
// The Rust side is crates/neoflash-ui/src/metrics.rs; crates/neoflash-ui/tests/density_parity.rs fails when
// the two disagree. Keep one entry per `'name': [n, n, n, 'S|G|R']` so that test can read this file.
const NF3_DENSITY = {
  'size/control': [26, 22, 20, 'S'], 'size/row': [24, 20, 18, 'S'], 'size/tab': [30, 26, 22, 'S'],
  'size/section': [24, 22, 20, 'S'], 'size/top-bar': [36, 32, 28, 'S'], 'size/status': [22, 20, 18, 'S'],
  'size/dialog-header': [44, 38, 32, 'S'], 'size/tool': [28, 24, 22, 'S'], 'size/icon-tool': [20, 16, 16, 'S'],
  'size/toggle': [18, 16, 16, 'S'], 'size/check': [14, 14, 12, 'S'], 'size/switch-w': [26, 24, 22, 'S'],
  'size/switch-h': [14, 12, 12, 'S'], 'size/knob': [10, 8, 8, 'S'], 'size/slider-thumb': [12, 10, 10, 'S'],
  'size/badge': [18, 16, 16, 'S'], 'size/swatch': [18, 16, 14, 'S'], 'size/timeline-controls': [34, 30, 26, 'S'],
  'size/layer-row': [22, 18, 16, 'S'], 'size/timeline-ruler': [24, 22, 20, 'S'], 'size/stage-ruler': [18, 16, 16, 'S'],
  'size/stage-bar': [32, 28, 24, 'S'], 'size/menu-separator': [9, 7, 5, 'S'],
  'pad/field': [8, 6, 5, 'G'], 'pad/button': [10, 8, 6, 'G'], 'pad/panel': [12, 8, 6, 'G'], 'pad/dialog': [20, 16, 12, 'G'],
  'pad/tools': [6, 6, 5, 'G'], 'pad/menu': [4, 4, 3, 'G'],
  'gap/control': [6, 4, 4, 'G'], 'gap/row': [6, 4, 3, 'G'], 'gap/section': [8, 6, 4, 'G'], 'gap/tool': [2, 2, 1, 'G'],
  'gap/inline': [6, 5, 4, 'G'],
  'radius/control': [5, 4, 3, 'R'], 'radius/panel': [8, 6, 4, 'R'], 'radius/check': [3, 3, 2, 'R'],
  'width/inspector': [264, 240, 224, 'S'], 'width/label': [72, 64, 60, 'S'], 'width/dialog-label': [112, 100, 92, 'S'],
  'width/layer-column': [300, 276, 256, 'S'], 'width/num': [64, 56, 52, 'S'], 'width/tools-1': [40, 36, 32, 'S'],
  'width/tools-2': [72, 64, 57, 'S'], 'width/menu': [208, 188, 172, 'S'], 'width/sidebar': [160, 148, 136, 'S'],
  'width/dialog-s': [360, 328, 304, 'S'], 'width/dialog-m': [480, 440, 408, 'S'], 'width/dialog-l': [720, 660, 620, 'S'],
  'width/dialog-xl': [880, 820, 760, 'S'], 'height/dialog-l': [576, 500, 440, 'S'], 'height/dialog-xl': [640, 600, 560, 'S'],
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
