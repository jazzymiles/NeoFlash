// ============================================================================
// Orchestration — every stage is isolated, so a failure in one stage keeps the others.
// ============================================================================
(async () => {
  const errors = [];
  const all0 = () => errors.length + ERRLOG.length;
  const stage = async (label, fn) => {
    try { figma.notify(label + '…', { timeout: 2500 }); await fn(); return true; }
    catch (e) { console.error(label, e); errors.push(label + ': ' + (e && e.message ? e.message : e)); return false; }
  };
  try {
    if (figma.root.children.some(p => p.name === 'v1 · Interface' || p.name === 'v2 · Interface')) {
      figma.closePlugin('This file already contains the NeoFlash pages. Run the plugin in a new empty file.');
      return;
    }
    await loadAllFonts();

    const pC1 = figma.root.children[0];
    pC1.name = 'v1 · Components';
    const pI1 = await newPage('v1 · Interface');
    const pAu = await newPage('Audit · v1 → v2');
    const pC2 = await newPage('v2 · System & Components');
    const pI2 = await newPage('v2 · Interface');
    const show = async (p) => { await gotoPage(p); try { figma.viewport.scrollAndZoomIntoView(p.children); } catch (e) { /* cosmetic */ } };

    const okC1 = await stage('Building v1 components', async () => { await gotoPage(pC1); v1BuildTokens(); v1BuildComponents(pC1); await show(pC1); });
    const okI1 = await stage('Building v1 interface', async () => { await gotoPage(pI1); v1BuildScreens(pI1); await show(pI1); });
    await stage('Writing audit', async () => { await gotoPage(pAu); auditBuild(pAu, V1.main); await show(pAu); });
    const okC2 = await stage('Building v2 system', async () => { await gotoPage(pC2); v2BuildTokens(); v2BuildComponents(pC2); await show(pC2); });
    await stage('Building v2 interface', async () => { await gotoPage(pI2); v2BuildScreens(pI2); await show(pI2); });

    await flushStyles();
    const all = errors.concat(ERRLOG);
    if (all.length) {
      const log = await newPage('Build log');
      await gotoPage(log);
      const t = figma.createText();
      t.fontName = FONTS.i400; t.fontSize = 13;
      t.characters = all.join('\n\n');
      t.x = 0; t.y = 0;
      log.appendChild(t);
    }
    await gotoPage(okI1 ? pI1 : pC1);
    figma.closePlugin(all0() ? 'Finished with errors — see page "Build log". First: ' + (errors.concat(ERRLOG)[0] || '').split('\n')[0] : 'Done: 5 pages created (v1 components / interface, audit, v2 system / interface).');
  } catch (e) {
    console.error(e);
    figma.closePlugin('NeoFlash UI Builder failed: ' + (e && e.message ? e.message : e));
  }
})();
