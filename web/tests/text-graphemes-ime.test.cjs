// End-to-end verification of T-326: UAX #29 grapheme-safe caret/selection/delete/replace,
// mixed TextFormatRun preservation across IME preedit/commit/cancel & rich clipboard,
// TextInteractionMode + TEXT_INPUT/CHANGE/onChanged contracts, real SFNT cmap Format 4 & Format 12
// Font.hasGlyphs, and HTML5 Canvas + DOM publish modes including Restart.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { execFileSync, spawn } = require('node:child_process');

const repoRoot = path.resolve(__dirname, '../..');
const runtimeDir = path.join(repoRoot, 'target/web-runtime');
const exportBin = path.join(repoRoot, 'target/debug/neoflash-export');

function findCargoBinary() {
  const localCargo = path.join(os.homedir(), '.cargo/bin/cargo');
  return process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
}

function findChromeBinary() {
  const candidates = [
    process.env.CHROME_BIN,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
  ].filter(Boolean);
  for (const bin of candidates) {
    if (fs.existsSync(bin)) return bin;
  }
  return null;
}

function ensureExportBinary() {
  execFileSync(
    findCargoBinary(),
    ['build', '--quiet', '--locked', '-p', 'neoflash-export', '--bin', 'neoflash-export'],
    { cwd: repoRoot, stdio: 'inherit' }
  );
  return exportBin;
}

function u16be(v) {
  const b = Buffer.alloc(2);
  b.writeUInt16BE(v & 0xffff, 0);
  return b;
}

function i16be(v) {
  const b = Buffer.alloc(2);
  b.writeInt16BE(v, 0);
  return b;
}

function u32be(v) {
  const b = Buffer.alloc(4);
  b.writeUInt32BE(v >>> 0, 0);
  return b;
}

function buildUtf16BeNameTable(family, subfamily) {
  const enc = (s) => {
    const parts = [];
    for (let i = 0; i < s.length; i++) parts.push(u16be(s.charCodeAt(i)));
    return Buffer.concat(parts);
  };
  const fam = enc(family);
  const sub = enc(subfamily);
  const storageOff = 6 + 2 * 12;
  return Buffer.concat([
    u16be(0),
    u16be(2),
    u16be(storageOff),
    // Record 0: nameID 1 (Family)
    u16be(3),
    u16be(1),
    u16be(0x0409),
    u16be(1),
    u16be(fam.length),
    u16be(0),
    // Record 1: nameID 2 (Subfamily)
    u16be(3),
    u16be(1),
    u16be(0x0409),
    u16be(2),
    u16be(sub.length),
    u16be(fam.length),
    fam,
    sub,
  ]);
}

function buildSfntFromTables(flavorBytes, rawTables) {
  const tables = [...rawTables].sort((a, b) => Buffer.compare(Buffer.from(a[0], 'ascii'), Buffer.from(b[0], 'ascii')));
  const numTables = tables.length;
  const headerLen = 12 + numTables * 16;
  const dirParts = [
    Buffer.from(flavorBytes),
    u16be(numTables),
    u16be(0),
    u16be(0),
    u16be(0),
  ];
  const bodyParts = [];
  let offset = headerLen;
  for (const [tag, buf] of tables) {
    const padLen = (4 - (buf.length % 4)) % 4;
    const padded = padLen ? Buffer.concat([buf, Buffer.alloc(padLen)]) : buf;
    dirParts.push(Buffer.from(tag, 'ascii'), u32be(0), u32be(offset), u32be(buf.length));
    bodyParts.push(padded);
    offset += padded.length;
  }
  return Buffer.concat([...dirParts, ...bodyParts]);
}

function buildTestSfntFormat4() {
  // Format 4 TrueType font mapping space (0x20), 'A'..'Z' (0x41..0x5A),
  // Greek 'Ω' (0x03A9) via non-zero idRangeOffset -> glyphIdArray[0]=30,
  // Cyrillic 'Я' (0x042F) via idDelta, Hiragana 'あ' (0x3042) via idDelta, and sentinel 0xFFFF.
  const segCount = 6;
  const endCodes = [0x0020, 0x005a, 0x03a9, 0x042f, 0x3042, 0xffff];
  const startCodes = [0x0020, 0x0041, 0x03a9, 0x042f, 0x3042, 0xffff];
  const idDeltas = [1 - 0x0020, 2 - 0x0041, 0, 40 - 0x042f, 50 - 0x3042, 1];
  const idRangeOffsets = [0, 0, 8, 0, 0, 0];
  const subLen = 16 + 4 * segCount * 2 + 2;
  const fmt4 = Buffer.concat([
    u16be(4),
    u16be(subLen),
    u16be(0),
    u16be(segCount * 2),
    u16be(8),
    u16be(2),
    u16be(4),
    ...endCodes.map(u16be),
    u16be(0),
    ...startCodes.map(u16be),
    ...idDeltas.map(i16be),
    ...idRangeOffsets.map(u16be),
    u16be(30),
  ]);
  const cmap = Buffer.concat([u16be(0), u16be(1), u16be(3), u16be(1), u32be(12), fmt4]);
  const head = Buffer.alloc(54);
  head.writeUInt16BE(1000, 18);
  head.writeUInt16BE(0x0003, 44); // macStyle bold + italic
  return buildSfntFromTables([0, 1, 0, 0], [
    ['cmap', cmap],
    ['head', head],
    ['name', buildUtf16BeNameTable('NeoBmpSans', 'Bold Italic')],
  ]);
}

function buildTestSfntFormat12() {
  // Format 12 OpenType/CFF font mapping space (0x20), BMP '漢' (0x6F22),
  // SMP emoji '😀'..'😅' (0x1F600..0x1F605), and SMP CJK Extension B '𠮷' (0x20BB7).
  const groups = [
    [0x0020, 0x0020, 1],
    [0x6f22, 0x6f22, 2],
    [0x1f600, 0x1f605, 10],
    [0x20bb7, 0x20bb7, 20],
  ];
  const cmap = Buffer.concat([
    u16be(0),
    u16be(1),
    u16be(3),
    u16be(10),
    u32be(12),
    u16be(12),
    u16be(0),
    u32be(16 + groups.length * 12),
    u32be(0),
    u32be(groups.length),
    ...groups.flatMap(([s, e, g]) => [u32be(s), u32be(e), u32be(g)]),
  ]);
  const head = Buffer.alloc(54);
  head.writeUInt16BE(1000, 18);
  return buildSfntFromTables(Buffer.from('OTTO', 'ascii'), [
    ['CFF ', Buffer.from([1, 0, 4, 1])],
    ['cmap', cmap],
    ['head', head],
    ['name', buildUtf16BeNameTable('NeoSmpCff', 'Regular')],
  ]);
}

function makeTransform(overrides = {}) {
  return { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1, ...overrides };
}

function makeTextFormat(overrides = {}, fieldOptions = null) {
  const fmt = {
    font: 'Arial',
    size: 16,
    color: 0x1e293b,
    bold: false,
    italic: false,
    leading: 0,
    align: 'left',
    leftMargin: 0,
    rightMargin: 0,
    indent: 0,
    blockIndent: 0,
    underline: false,
    letterSpacing: 0,
    kerning: null,
    bullet: false,
    tabStops: [],
    url: '',
    target: '',
    ...overrides,
  };
  if (fieldOptions) fmt.fieldOptions = fieldOptions;
  return fmt;
}

function buildTestTtcFace(familyName) {
  // Single-segment Format-4 BMP SFNT mapping space (0x20), 'A'..'C' (0x41..0x43), and 'Я' (0x042F).
  const segCount = 4;
  const segCountX2 = segCount * 2;
  const endCodes = [0x0020, 0x0043, 0x042f, 0xffff];
  const startCodes = [0x0020, 0x0041, 0x042f, 0xffff];
  const idDeltas = [1 - 0x0020, 2 - 0x0041, 10 - 0x042f, 1];
  const fmt4Len = 14 + segCountX2 + 2 + segCountX2 * 3;
  const fmt4 = Buffer.alloc(fmt4Len, 0);
  fmt4.writeUInt16BE(4, 0);
  fmt4.writeUInt16BE(fmt4Len, 2);
  fmt4.writeUInt16BE(segCountX2, 6);
  let p = 14;
  for (const v of endCodes) { fmt4.writeUInt16BE(v, p); p += 2; }
  p += 2;
  for (const v of startCodes) { fmt4.writeUInt16BE(v, p); p += 2; }
  for (const v of idDeltas) { fmt4.writeInt16BE(v, p); p += 2; }
  p += segCountX2; // idRangeOffsets = 0

  const cmap = Buffer.alloc(12 + fmt4.length);
  cmap.writeUInt16BE(0, 0);
  cmap.writeUInt16BE(1, 2);
  cmap.writeUInt16BE(3, 4);
  cmap.writeUInt16BE(1, 6);
  cmap.writeUInt32BE(12, 8);
  fmt4.copy(cmap, 12);

  const head = Buffer.alloc(54, 0);
  head.writeUInt16BE(1000, 18);
  return buildSfntFromTables(Buffer.from([0, 1, 0, 0]), [
    ['cmap', cmap],
    ['head', head],
    ['name', buildUtf16BeNameTable(familyName, 'Regular')],
  ]);
}

function packSfntsIntoTtc(faces) {
  const headerLen = 12 + faces.length * 4;
  const chunks = [Buffer.alloc(headerLen, 0)];
  chunks[0].write('ttcf', 0, 'ascii');
  chunks[0].writeUInt16BE(1, 4);
  chunks[0].writeUInt16BE(0, 6);
  chunks[0].writeUInt32BE(faces.length, 8);
  let curLen = headerLen;
  faces.forEach((face, idx) => {
    const pad = (4 - (curLen % 4)) % 4;
    if (pad) {
      chunks.push(Buffer.alloc(pad, 0));
      curLen += pad;
    }
    const base = curLen;
    chunks[0].writeUInt32BE(base, 12 + idx * 4);
    const relocated = Buffer.from(face);
    const numTables = relocated.readUInt16BE(4);
    for (let t = 0; t < numTables; t++) {
      const recOff = 12 + t * 16 + 8;
      const origOff = relocated.readUInt32BE(recOff);
      relocated.writeUInt32BE(base + origOff, recOff);
    }
    chunks.push(relocated);
    curLen += relocated.length;
  });
  return Buffer.concat(chunks);
}

test('Node/WASM: UAX #29 graphemes, FTE atoms, AS2/AS3 mixed runs, TextInteractionMode, and SFNT cmap Format 4 & 12 Font.hasGlyphs', async () => {
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');

  const sfnt4 = buildTestSfntFormat4();
  const sfnt12 = buildTestSfntFormat12();

  const project = {
    format_version: 7,
    name: 'T326NodeWasmSuite',
    width: 640,
    height: 400,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: [
      {
        id: 'font-bmp-4',
        path: 'fonts/NeoBmpSans.ttf',
        media_type: 'font/ttf',
        data_base64: sfnt4.toString('base64'),
      },
      {
        id: 'font-smp-12',
        path: 'fonts/NeoSmpCff.otf',
        media_type: 'font/otf',
        data_base64: sfnt12.toString('base64'),
      },
      {
        id: 'font-ttc-bundle',
        path: 'fonts/NeoCollectionTtc.ttc',
        media_type: 'font/collection',
        data_base64: packSfntsIntoTtc([buildTestTtcFace('NeoCollectionTtc')]).toString('base64'),
      },
    ],
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'l1',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [{ frame: 0, label: '', tween: null, elements: [] }],
            },
          ],
          scripts: [],
        },
      },
    ],
  };

  const player = new runtimeWasm.WebPlayer(JSON.stringify(project));
  const { host } = createRuntimeHost(repoRoot, player, [], {
    project,
    bundle: { project },
    context: { measureText: (line) => ({ width: line.length * 8 }) },
  });
  const prog = host.program();
  const scene = host.proxyFor([]);

  try {
    // 1. SFNT cmap Format 4 (BMP idDelta + idRangeOffset) & Format 12 (SMP) Font.enumerateFonts + Font.hasGlyphs
    const Font = prog.type('flash.text.Font');
    const embedded = Font.enumerateFonts(false);
    const bmpFont = embedded.find((f) => f.fontName === 'NeoBmpSans');
    const smpFont = embedded.find((f) => f.fontName === 'NeoSmpCff');
    assert.ok(bmpFont, 'Format 4 TrueType font must be enumerated');
    assert.equal(bmpFont.fontStyle, 'boldItalic');
    assert.equal(bmpFont.fontType, 'embedded');
    assert.equal(bmpFont.hasGlyphs('AZЯあΩ '), true);
    assert.equal(bmpFont.hasGlyphs('AЖ'), false, 'unmapped BMP Cyrillic glyph must return false');
    assert.equal(bmpFont.hasGlyphs('A😀'), false, 'SMP glyph absent from Format 4 must return false');
    assert.equal(bmpFont.hasGlyphs('A\nB'), false, 'newline control character must return false');

    assert.ok(smpFont, 'Format 12 OpenType CFF font must be enumerated');
    assert.equal(smpFont.fontStyle, 'regular');
    assert.equal(smpFont.fontType, 'embeddedCFF');
    assert.equal(smpFont.hasGlyphs('漢 𠮷 😀'), true, 'Format 12 must resolve BMP + SMP U+20BB7 + U+1F600');
    assert.equal(smpFont.hasGlyphs('漢🚀'), false, 'unmapped SMP U+1F680 must return false');

    assert.equal(host.fte3d().fontHasGlyphs(sfnt12, '漢 𠮷 😀'), true);
    assert.equal(host.fte3d().fontHasGlyphs(sfnt12, '🚀'), false);

    // 1b. OpenType/TrueType Collection (`ttcf`) + `Font.registerFont` with explicit SFNT/TTC bytes
    const regFace = buildTestTtcFace('NeoRegisteredTtc');
    const ttcBundle = packSfntsIntoTtc([regFace, sfnt12]);
    class RegisteredTtcFont extends Font {}
    RegisteredTtcFont.__nfFontBytes = new Uint8Array(ttcBundle);
    Font.registerFont(RegisteredTtcFont);
    const afterReg = Font.enumerateFonts(false);
    const regInst = afterReg.find((f) => f.fontName === 'NeoRegisteredTtc');
    assert.ok(regInst, 'Font.registerFont must parse TTC/SFNT name via 2-arg sfnt_font_info_json');
    assert.equal(regInst.fontStyle, 'regular');
    assert.equal(regInst.hasGlyphs('ABCЯ '), true, 'TTC face 0 mapped glyphs must return true');
    assert.equal(regInst.hasGlyphs('AD'), false, 'TTC face 0 unmapped glyph D must return false');
    assert.equal(regInst.hasGlyphs('😀'), false, 'TTC face 0 unmapped SMP glyph must return false');

    // 2. AS3 TextField UAX #29 grapheme boundaries, selection clamping, replaceText, maxChars & mixed runs
    const TextField = prog.type('flash.text.TextField');
    const TextFormat = prog.type('flash.text.TextFormat');
    const TextFieldType = prog.type('flash.text.TextFieldType');
    const TextInteractionMode = prog.type('flash.text.TextInteractionMode');

    assert.equal(TextInteractionMode.NORMAL, 'normal');
    assert.equal(TextInteractionMode.SELECTION, 'selection');

    const tf = new TextField();
    tf.type = TextFieldType.INPUT;
    scene.addChild(tf);

    // Sample clusters:
    // "e\u0301" (0..2) | "и\u0306" (2..4) | "か\u3099" (4..6) | "🇺🇸" (6..10) | "👨‍👩‍👧‍👦" (10..21) | "🏳️‍🌈" (21..27) | "👩🏽‍💻" (27..34)
    const sample = 'e\u0301и\u0306か\u3099🇺🇸👨‍👩‍👧‍👦🏳️‍🌈👩🏽‍💻';
    assert.equal(sample.length, 34);
    assert.deepEqual(
      host.text().graphemeBoundaries(sample),
      [0, 2, 4, 6, 10, 21, 27, 34]
    );
    assert.deepEqual(host.text().graphemeBoundaries('\u{1E922}\u{1E944}'), [0, 4], 'modern Adlam Extend must stay with its base');
    assert.deepEqual(host.text().graphemeBoundaries('\u0915\u094D\u0937'), [0, 3], 'UAX #29 GB9c Indic conjunct must remain one cluster');

    tf.text = sample;
    assert.equal(tf.length, 34);

    // Mid-cluster setSelection snaps floor/ceil cleanly and never lands inside surrogates/combining/ZWJ!
    tf.setSelection(1, 1); // inside "e\u0301" -> snaps to 0..0
    assert.equal(tf.selectionBeginIndex, 0);
    assert.equal(tf.selectionEndIndex, 0);
    assert.equal(tf.caretIndex, 0);

    tf.setSelection(1, 15); // mid "e\u0301" (1->0) to mid "👨‍👩‍👧‍👦" (15->10) -> snaps cleanly to cluster boundaries [0, 10]
    assert.equal(tf.selectionBeginIndex, 0);
    assert.equal(tf.selectionEndIndex, 10);
    tf.setSelection(1, 21);
    assert.equal(tf.selectionBeginIndex, 0);
    assert.equal(tf.selectionEndIndex, 21);

    // Splitting a UTF-16 surrogate pair in replaceText still throws /Unicode/ (existing contract).
    assert.throws(() => tf.replaceText(6, 7, 'X'), /Unicode/);

    // Mid-cluster scalar boundary in replaceText (e.g. index 1 between 'e' and '\u0301', or index 12 inside ZWJ family)
    // snaps cleanly to whole grapheme clusters instead of leaving orphaned combining marks or broken ZWJ sequences!
    tf.replaceText(12, 15, '!');
    assert.equal(tf.text, 'e\u0301и\u0306か\u3099🇺🇸!🏳️‍🌈👩🏽‍💻');

    // maxChars enforces UTF-16 budget per whole grapheme cluster without splitting surrogates/ZWJ.
    const capped = new TextField();
    capped.type = TextFieldType.INPUT;
    capped.maxChars = 10; // fits "и\u0306" (2) + "🇺🇸" (4) = 6, rejects 11-unit "👨‍👩‍👧‍👦" cleanly!
    capped.text = '';
    scene.addChild(capped);
    const cappedRef = JSON.stringify(capped.toJSON().object_ref);
    const cappedRev = JSON.parse(player.object_text_info(cappedRef)).revision;
    const cappedAfter = JSON.parse(
      player.object_text_input(
        cappedRef,
        JSON.stringify({
          expected_revision: cappedRev,
          begin: 0,
          end: 0,
          text: 'и\u0306🇺🇸👨‍👩‍👧‍👦',
        })
      )
    );
    assert.equal(cappedAfter.text, 'и\u0306🇺🇸');
    assert.equal(cappedAfter.text.length, 6);

    // Rich input with `restrict` + `maxChars` + single-line newline: `restrict` MUST filter BEFORE `maxChars`
    // while remapping `runs` ("123\nABCDE" -> strips "123\n", keeps "ABC" under maxChars=3 with "AB" bold red + "C" italic blue).
    const restrictedRich = new TextField();
    restrictedRich.type = TextFieldType.INPUT;
    restrictedRich.multiline = false;
    restrictedRich.restrict = 'A-Z';
    restrictedRich.maxChars = 3;
    restrictedRich.text = '';
    scene.addChild(restrictedRich);
    const restrictedRef = JSON.stringify(restrictedRich.toJSON().object_ref);
    const restrictedRev = JSON.parse(player.object_text_info(restrictedRef)).revision;
    const baseWireFormat = JSON.parse(player.object_text_info(restrictedRef)).format;
    const fmtRichRed = { ...baseWireFormat, font: 'Arial', size: 16, color: 0xff0000, bold: true, italic: false };
    const fmtRichBlue = { ...baseWireFormat, font: 'Georgia', size: 18, color: 0x0055ff, bold: false, italic: true };
    const restrictedCommitted = JSON.parse(
      player.object_text_rich_input(
        restrictedRef,
        JSON.stringify({
          expected_revision: restrictedRev,
          begin: 0,
          end: 0,
          text: '123\nABCDE',
          runs: [
            { start: 0, end: 6, format: fmtRichRed }, // covers "123\nAB"
            { start: 6, end: 9, format: fmtRichBlue }, // covers "CDE"
          ],
        })
      )
    );
    assert.equal(restrictedCommitted.text, 'ABC', 'object_text_rich_input must filter restrict before maxChars');
    assert.equal(restrictedRich.getTextFormat(0, 2).bold, true);
    assert.equal(restrictedRich.getTextFormat(0, 2).color, 0xff0000);
    assert.equal(restrictedRich.getTextFormat(2, 3).italic, true);
    assert.equal(restrictedRich.getTextFormat(2, 3).color, 0x0055ff);

    // textInteractionMode is read-only. Only a host/user transition dispatches
    // textInteractionModeChange, and the Event is non-bubbling.
    const modeEvents = [];
    capped.addEventListener('textInteractionModeChange', (event) => {
      modeEvents.push({ mode: capped.textInteractionMode, bubbles: event.bubbles });
    });
    assert.throws(() => {
      capped.textInteractionMode = TextInteractionMode.SELECTION;
    }, /read-only/);
    assert.equal(capped.textInteractionMode, 'normal');
    assert.equal(host.text().userInteractionModeChanged(cappedRef, TextInteractionMode.SELECTION), true);
    assert.equal(capped.textInteractionMode, 'selection');
    assert.deepEqual(modeEvents, [{ mode: 'selection', bubbles: false }]);
    const editedState = JSON.parse(
      player.object_text_input(
        cappedRef,
        JSON.stringify({
          expected_revision: JSON.parse(player.object_text_info(cappedRef)).revision,
          begin: 0,
          end: 0,
          text: 'Z',
        })
      )
    );
    assert.equal(editedState.text, 'Zи\u0306🇺🇸', 'host interaction state must not become a script-set read-only lock');
    assert.equal(host.text().userInteractionModeChanged(cappedRef, TextInteractionMode.NORMAL), true);

    // 3. AS3 FTE (flash.text.engine.*) atom & word boundaries preserve combining marks & ZWJ clusters
    const FontDescription = prog.type('flash.text.engine.FontDescription');
    const ElementFormat = prog.type('flash.text.engine.ElementFormat');
    const TextElement = prog.type('flash.text.engine.TextElement');
    const GroupElement = prog.type('flash.text.engine.GroupElement');
    const TextBlock = prog.type('flash.text.engine.TextBlock');

    const ef = new ElementFormat(new FontDescription('Arial'), 16, 0x112233);
    const fteText = 'e\u0301👨‍👩‍👧‍👦 и\u0306か\u3099';
    // Clusters: "e\u0301" (0..2), "👨‍👩‍👧‍👦" (2..13), " " (13..14), "и\u0306" (14..16), "か\u3099" (16..18)
    const te = new TextElement(fteText, ef);
    const tb = new TextBlock(te);
    const line = tb.createTextLine(null, 500);
    assert.equal(line.atomCount, 5);
    assert.equal(tb.findNextAtomBoundary(0), 2);
    assert.equal(tb.findNextAtomBoundary(2), 13);
    assert.equal(tb.findPreviousAtomBoundary(13), 2);
    assert.equal(tb.findNextWordBoundary(0), 2);

    // GroupElement.splitTextElement snaps mid-cluster split index cleanly instead of corrupting ZWJ cluster
    const grpElem = new TextElement('e\u0301👨‍👩‍👧‍👦', ef);
    const grp = new GroupElement([grpElem]);
    const splitRight = grp.splitTextElement(0, 5); // mid-ZWJ -> snaps to cluster boundary 2!
    assert.equal(grp.getElementAt(0).text, 'e\u0301');
    assert.equal(splitRight.text, '👨‍👩‍👧‍👦');

    // 4. AS2 TextField mixed TextFormatRun preservation across user edits + onChanged contract
    const as2Player = new runtimeWasm.WebPlayer(JSON.stringify(project));
    const { host: as2Host } = createRuntimeHost(
      repoRoot,
      as2Player,
      [{ script: { language: 'action_script2', source: 'stop();' }, code: 'stop();', location: 'Main:1' }],
      {
        project,
        bundle: { project },
        context: { measureText: (line) => ({ width: line.length * 8 }) },
      }
    );
    try {
      const as2Scene = as2Host.proxyFor([]);
      const as2Tf = as2Scene.createTextField('as2Mixed', 10, 20, 20, 260, 40);
      as2Tf.type = 'input';
      as2Tf.text = 'Red Blue';
      const As2TextFormat = as2Host.legacy().TextFormat;
      const boldRed = new As2TextFormat('Arial', 16, 0xff0000, true, false);
      const italicBlue = new As2TextFormat('Georgia', 18, 0x0055ff, false, true);
      as2Tf.setTextFormat(0, 3, boldRed);
      as2Tf.setTextFormat(4, 8, italicBlue);

      let as2ChangedCalls = 0;
      as2Tf.onChanged = function (fieldArg) {
        if (fieldArg === as2Tf) as2ChangedCalls++;
      };
      // Programmatic .htmlText assignment must NOT fire onChanged
      const savedHtml = as2Tf.htmlText;
      as2Tf.htmlText = savedHtml;
      assert.equal(as2ChangedCalls, 0);

      // User edit at index 3 ("Red" -> "Redか\u3099") preserves bold red on 0..5 and italic blue on 6..10!
      as2Tf.setTextFormat(0, 3, boldRed);
      as2Tf.setTextFormat(4, 8, italicBlue);
      const as2Ref = JSON.stringify(as2Tf.toJSON().object_ref);
      const as2InfoBefore = JSON.parse(as2Player.object_text_info(as2Ref));
      assert.ok(as2InfoBefore.format_runs.length >= 3, 'AS2 setTextFormat(begin, end, fmt) must sync runs to Rust');
      as2Player.object_text_input(
        as2Ref,
        JSON.stringify({
          expected_revision: as2InfoBefore.revision,
          begin: 3,
          end: 3,
          text: 'か\u3099',
        })
      );
      const fmtPrefix = as2Tf.getTextFormat(0, 5);
      const fmtSuffix = as2Tf.getTextFormat(6, 10);
      assert.equal(fmtPrefix.bold, true);
      assert.equal(fmtPrefix.color, 0xff0000);
      assert.equal(fmtSuffix.italic, true);
      assert.equal(fmtSuffix.color, 0x0055ff);

      // AS2 flash.text.TextRenderer static surface & Rust CSM resolution
      const As2TextRenderer = as2Host.legacy().TextRenderer;
      assert.equal(As2TextRenderer, as2Host.program().type('flash.text.TextRenderer'));
      assert.equal(As2TextRenderer.displayMode, 'default');
      assert.equal(As2TextRenderer.maxLevel, 4);
      As2TextRenderer.displayMode = 'crt';
      As2TextRenderer.maxLevel = -5;
      assert.equal(As2TextRenderer.displayMode, 'crt');
      assert.equal(As2TextRenderer.maxLevel, 0);
      assert.throws(() => { As2TextRenderer.displayMode = 'bogus'; }, /displayMode/, 'AS2 invalid displayMode throws actionable validation error');
      As2TextRenderer.displayMode = 'default';
      As2TextRenderer.maxLevel = 4;
      As2TextRenderer.setAdvancedAntialiasingTable('Arial', 'none', 'dark', [
        { fontSize: 12, insideCutoff: 0.8, outsideCutoff: -0.3 },
        { fontSize: 24, insideCutoff: 0.4, outsideCutoff: -0.6 },
      ]);
      const resolvedCsm = as2Host.text().resolveRendererCsm({ font: 'Arial', bold: false, italic: false, size: 18, color: 0x111111 });
      assert.equal(resolvedCsm.hasCsm, true);
      assert.equal(resolvedCsm.colorType, 'dark');
      assert.ok(Math.abs(resolvedCsm.insideCutoff - 0.6) < 1e-6);
      assert.ok(Math.abs(resolvedCsm.outsideCutoff - -0.45) < 1e-6);
    } finally {
      as2Host.disposeFamily();
      as2Player.free();
    }

    // 5. AS3 CSMSettings, TextDisplayMode, TextRenderer + Canvas pixel/stroke rendering differences
    const CSMSettings = prog.type('flash.text.CSMSettings');
    const TextDisplayMode = prog.type('flash.text.TextDisplayMode');
    const TextRenderer = prog.type('flash.text.TextRenderer');
    const TextColorType = prog.type('flash.text.TextColorType');
    const FontStyle = prog.type('flash.text.FontStyle');
    const AntiAliasType = prog.type('flash.text.AntiAliasType');
    assert.deepEqual([TextDisplayMode.DEFAULT, TextDisplayMode.LCD, TextDisplayMode.CRT], ['default', 'lcd', 'crt']);
    const csm12 = new CSMSettings(12, 0.85, -0.25);
    const csm24 = new CSMSettings(24, 0.35, -0.75);
    assert.deepEqual([csm12.fontSize, csm12.insideCutoff, csm12.outsideCutoff], [12, 0.85, -0.25]);
    csm12.insideCutoff = 0.9;
    assert.equal(csm12.insideCutoff, 0.9);
    TextRenderer.setAdvancedAntiAliasingTable('Arial', FontStyle.REGULAR, TextColorType.DARK_COLOR, [csm12, csm24]);
    assert.throws(() => { TextRenderer.displayMode = 'plasma'; }, err => err.name === 'ArgumentError');
    TextRenderer.maxLevel = 999;
    assert.equal(TextRenderer.maxLevel, 64);
    TextRenderer.maxLevel = 4;

    const advTf = new TextField();
    advTf.antiAliasType = AntiAliasType.ADVANCED;
    advTf.thickness = 80;
    advTf.sharpness = 120;
    advTf.text = 'Subpixel';
    scene.addChild(advTf);
    const advInfo = JSON.parse(player.object_text_info(JSON.stringify(advTf.toJSON().object_ref)));
    const runDrawProbe = (mode, level) => {
      TextRenderer.displayMode = mode;
      TextRenderer.maxLevel = level;
      const ops = [];
      const probeCtx = {
        font: '', fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1, shadowBlur: 0, shadowColor: '', imageSmoothingEnabled: true,
        save() {}, restore() {}, beginPath() {}, rect() {}, clip() {}, fillRect() {}, strokeRect() {}, moveTo() {}, lineTo() {}, stroke() {},
        measureText: (s) => ({ width: s.length * 8 }),
        fillText(txt, x, y) { ops.push({ op: 'fill', style: String(this.fillStyle), alpha: this.globalAlpha, blur: this.shadowBlur, x, y, txt }); },
        strokeText(txt, x, y) { ops.push({ op: 'stroke', width: this.lineWidth, alpha: this.globalAlpha, x, y, txt }); },
      };
      host.text().draw(probeCtx, advInfo);
      return ops;
    };
    const defaultOps = runDrawProbe(TextDisplayMode.DEFAULT, 4);
    const lcdOps = runDrawProbe(TextDisplayMode.LCD, 4);
    const crtOps = runDrawProbe(TextDisplayMode.CRT, 4);
    const level0Ops = runDrawProbe(TextDisplayMode.DEFAULT, 0);
    TextRenderer.displayMode = TextDisplayMode.DEFAULT;
    TextRenderer.maxLevel = 4;
    assert.ok(lcdOps.filter(o => o.op === 'fill').length > defaultOps.filter(o => o.op === 'fill').length, 'LCD mode renders RGB subpixel passes');
    assert.ok(crtOps.some(o => o.blur > 0), 'CRT mode applies phosphor bloom shadowBlur');
    assert.equal(level0Ops.filter(o => o.op === 'stroke').length, 0, 'maxLevel=0 disables antialiased stroke smoothing');

    // 6. Device-font Font.hasGlyphs via .notdef tofu raster comparison
    const TextModule = require('../text.js');
    const devTextHost = TextModule.create({
      player,
      wasm: runtimeWasm,
      events: require('../events.js'),
      project,
      measureText: (s) => s.length * 8,
      createCanvas: (w, h) => {
        let drawn = '';
        return {
          width: w,
          height: h,
          getContext: () => ({
            font: '', fillStyle: '', textBaseline: 'top',
            clearRect() { drawn = ''; },
            measureText(s) {
              const isTofu = s === '\uFFFD' || s === '\uFFFF' || s === '\u0378' || s.charCodeAt(0) >= 0xe000;
              const familyWidth = String(this.font).includes('"Arial"') ? 12 : 10;
              return { width: isTofu ? 9 : s.length * familyWidth };
            },
            fillText(s) { drawn = s; },
            getImageData: () => {
              const buf = new Uint8ClampedArray(w * h * 4);
              const isTofu = drawn === '\uFFFD' || drawn === '\uFFFF' || drawn === '\u0378' || drawn.charCodeAt(0) >= 0xe000;
              const seed = isTofu ? 0x5a : (drawn.charCodeAt(0) & 0xff) || 0x33;
              buf[0] = seed; buf[1] = isTofu ? 0x11 : 0x99; buf[3] = 255;
              return { data: buf };
            },
          }),
        };
      },
    });
    const devArial = devTextHost.Font.enumerateFonts(true).find(f => f.fontType === 'device' && f.fontName === 'Arial');
    assert.ok(devArial, 'enumerateFonts(true) includes device Arial');
    assert.equal(devArial.hasGlyphs('Hello 2026!'), true, 'device font accepts supported characters with non-tofu rasters');
    assert.equal(devArial.hasGlyphs('Hello\u0378'), false, 'device font rejects unassigned codepoint matching .notdef tofu');
    assert.equal(devArial.hasGlyphs('Hello\uE999'), false, 'device font rejects unmapped PUA codepoint');
    assert.equal(devArial.hasGlyphs('Line1\nLine2'), false, 'device font rejects control characters');
    devTextHost.dispose();
  } finally {
    host.disposeFamily();
    player.free();
  }
});

function buildBrowserTextSuiteProject(renderMode, sfnt4, sfnt12) {
  const publishSettingsJson = JSON.stringify({ render_mode: renderMode });
  const frame0Script = [
    'import flash.events.Event;',
    'import flash.events.TextEvent;',
    'import flash.text.Font;',
    'import flash.text.TextFormat;',
    'import flash.text.TextInteractionMode;',
    'stop();',
    'var self:* = this;',
    'var richInput:* = self.getChildByName("richInput");',
    'var selOnlyField:* = self.getChildByName("selOnlyField");',
    'var dynSelect:* = self.getChildByName("dynSelect");',
    'var dynLocked:* = self.getChildByName("dynLocked");',
    'var pasteTarget:* = self.getChildByName("pasteTarget");',
    'self.changeLog = [];',
    'self.textInputLog = [];',
    'self.blockNextTextInput = false;',
    '',
    '// Configure mixed runs on richInput ("Red Blue"): 0..3 bold red, 4..8 italic blue.',
    'richInput.useRichTextClipboard = true;',
    'richInput.text = "Red Blue";',
    'var fmtRed:TextFormat = new TextFormat("Arial", 16, 0xFF0000, true, false, false);',
    'var fmtBlue:TextFormat = new TextFormat("Georgia", 18, 0x0055FF, false, true, false);',
    'richInput.setTextFormat(fmtRed, 0, 3);',
    'richInput.setTextFormat(fmtBlue, 4, 8);',
    '',
    '// Configure pasteTarget ("Start End"): 0..5 underline green, 6..9 regular green.',
    'pasteTarget.useRichTextClipboard = true;',
    'pasteTarget.text = "Start End";',
    'var fmtUnder:TextFormat = new TextFormat("Georgia", 16, 0x008844, false, false, true);',
    'pasteTarget.setTextFormat(fmtUnder, 0, 5);',
    '',
    '// Keep the legacy-authored selection state to prove it cannot become an editing lock.',
    'selOnlyField.text = "CopyOnly";',
    '',
    '// Attach event listeners AFTER programmatic setup; then do one programmatic update on dynSelect',
    '// and verify programmatic .text / .htmlText never fires Event.CHANGE on richInput!',
    'richInput.addEventListener(TextEvent.TEXT_INPUT, function(e:TextEvent):void {',
    '  self.textInputLog.push(e.text + "@len=" + richInput.length);',
    '  if (self.blockNextTextInput) {',
    '    e.preventDefault();',
    '  }',
    '});',
    'richInput.addEventListener(Event.CHANGE, function(e:Event):void {',
    '  self.changeLog.push(richInput.text);',
    '});',
    'dynSelect.htmlText = "<b>Selectable</b> Dynamic";',
    '',
    '// Probe embedded SFNT cmap Format 4 & Format 12 fonts.',
    'var fonts:Array = Font.enumerateFonts(false);',
    '  var f4:* = null;',
    '  var f12:* = null;',
    '  var fTtc:* = null;',
    '  for (var i:int = 0; i < fonts.length; i++) {',
    '    if (fonts[i].fontName === "NeoBmpSans") f4 = fonts[i];',
    '    if (fonts[i].fontName === "NeoSmpCff") f12 = fonts[i];',
    '    if (fonts[i].fontName === "NeoCollectionTtc") fTtc = fonts[i];',
    '  }',
    '  self.fontProbe = {',
    '    bmpOk: Boolean(f4 && f4.hasGlyphs("AZЯあΩ ") && !f4.hasGlyphs("A😀") && !f4.hasGlyphs("A\\nB")),',
    '    smpOk: Boolean(f12 && f12.hasGlyphs("漢 𠮷 😀") && !f12.hasGlyphs("漢🚀")),',
    '    ttcOk: Boolean(fTtc && fTtc.hasGlyphs("ABCЯ ") && !fTtc.hasGlyphs("AD") && !fTtc.hasGlyphs("😀"))',
    '  };',
    '',
    'self.inspectRich = function():Object {',
    '  var r0:TextFormat = richInput.length >= 3 ? richInput.getTextFormat(0, 3) : null;',
    '  var rTail:TextFormat = richInput.length >= 4 ? richInput.getTextFormat(richInput.length - 4, richInput.length) : null;',
    '  return {',
    '    text: richInput.text,',
    '    length: richInput.length,',
    '    caretIndex: richInput.caretIndex,',
    '    selBegin: richInput.selectionBeginIndex,',
    '    selEnd: richInput.selectionEndIndex,',
    '    interactionMode: richInput.textInteractionMode,',
    '    headBold: r0 ? Boolean(r0.bold) : null,',
    '    headColor: r0 ? Number(r0.color) : null,',
    '    tailItalic: rTail ? Boolean(rTail.italic) : null,',
    '    tailColor: rTail ? Number(rTail.color) : null,',
    '    changeCount: self.changeLog.length,',
    '    lastChange: self.changeLog.length ? self.changeLog[self.changeLog.length - 1] : null,',
    '    textInputLog: self.textInputLog.slice(),',
    '    selOnlyText: selOnlyField.text,',
    '    fontProbe: self.fontProbe',
    '  };',
    '};',
    'self.inspectPasteTarget = function():Object {',
    '  var u0:TextFormat = pasteTarget.getTextFormat(0, 5);',
    '  var midRed:TextFormat = pasteTarget.length >= 16 ? pasteTarget.getTextFormat(6, 8) : null;',
    '  var midBlue:TextFormat = pasteTarget.length >= 16 ? pasteTarget.getTextFormat(9, 13) : null;',
    '  return {',
    '    text: pasteTarget.text,',
    '    u0Underline: Boolean(u0.underline),',
    '    midBold: midRed ? Boolean(midRed.bold) : null,',
    '    midRedColor: midRed ? Number(midRed.color) : null,',
    '    midItalic: midBlue ? Boolean(midBlue.italic) : null,',
    '    midBlueColor: midBlue ? Number(midBlue.color) : null',
    '  };',
    '};',
    'self.doProgrammaticSet = function():void {',
    '  var keep:String = richInput.text;',
    '  richInput.text = keep;',
    '  richInput.setTextFormat(fmtRed, 0, 3);',
    '  richInput.setTextFormat(fmtBlue, 4, 8);',
    '};',
    'self.setBlockInput = function(flag:Boolean):void {',
    '  self.blockNextTextInput = flag;',
    '};',
    'self.probeTextInteractionModeWrite = function():Object {',
    '  var threw:Boolean = false;',
    '  try { richInput.textInteractionMode = TextInteractionMode.SELECTION; }',
    '  catch (error:*) { threw = true; }',
    '  return { threw: threw, mode: richInput.textInteractionMode };',
    '};',
    'self.resetPasteTarget = function(useRich:Boolean, restrictVal:String, maxCharsVal:int):void {',
    '  pasteTarget.restrict = null;',
    '  pasteTarget.maxChars = 0;',
    '  pasteTarget.useRichTextClipboard = useRich;',
    '  pasteTarget.text = "Start End";',
    '  var fmtGreen:TextFormat = new TextFormat("Georgia", 16, 0x008844, false, false, false);',
    '  pasteTarget.setTextFormat(fmtGreen, 0, 9);',
    '  pasteTarget.setTextFormat(fmtUnder, 0, 5);',
    '  if (restrictVal !== null && restrictVal !== "") pasteTarget.restrict = restrictVal;',
    '  if (maxCharsVal > 0) pasteTarget.maxChars = maxCharsVal;',
    '};',
    'self.setRichClipboardFlag = function(useRich:Boolean):void {',
    '  richInput.useRichTextClipboard = useRich;',
    '};',
    'self.inspectRestrictedTarget = function():Object {',
    '  var rAB:TextFormat = pasteTarget.length >= 2 ? pasteTarget.getTextFormat(0, 2) : null;',
    '  var rC:TextFormat = pasteTarget.length >= 3 ? pasteTarget.getTextFormat(2, 3) : null;',
    '  return {',
    '    text: pasteTarget.text,',
    '    abBold: rAB ? Boolean(rAB.bold) : null,',
    '    abColor: rAB ? Number(rAB.color) : null,',
    '    cItalic: rC ? Boolean(rC.italic) : null,',
    '    cColor: rC ? Number(rC.color) : null',
    '  };',
    '};',
    'self.prepareRestrictedScenario = function():void {',
    '  richInput.useRichTextClipboard = true;',
    '  richInput.text = "123ABCDE";',
    '  richInput.setTextFormat(fmtRed, 0, 5);',
    '  richInput.setTextFormat(fmtBlue, 5, 8);',
    '  pasteTarget.useRichTextClipboard = true;',
    '  pasteTarget.restrict = null;',
    '  pasteTarget.maxChars = 0;',
    '  pasteTarget.text = "";',
    '  pasteTarget.restrict = "A-Z";',
    '  pasteTarget.maxChars = 3;',
    '};',
    'self.restoreRichScenario = function():void {',
    '  richInput.useRichTextClipboard = true;',
    '  richInput.text = "Redи\\u0306 Blue";',
    '  richInput.setTextFormat(fmtRed, 0, 5);',
    '  richInput.setTextFormat(fmtBlue, 6, 10);',
    '  pasteTarget.restrict = null;',
    '  pasteTarget.maxChars = 0;',
    '};',
    'trace("T326_BOOT=" + String(self.fontProbe.bmpOk) + "," + String(self.fontProbe.smpOk) + "," + String(self.fontProbe.ttcOk));',
  ].join('\n');

  return {
    format_version: 7,
    name: `T326_${renderMode}`,
    width: 640,
    height: 360,
    fps: 24,
    background: [15, 23, 42, 255],
    root: 'root',
    resources: [
      {
        id: 'res-publish-settings',
        path: 'publish/settings.json',
        media_type: 'application/json',
        data_base64: Buffer.from(publishSettingsJson, 'utf8').toString('base64'),
      },
      {
        id: 'font-bmp-4',
        path: 'fonts/NeoBmpSans.ttf',
        media_type: 'font/ttf',
        data_base64: sfnt4.toString('base64'),
      },
      {
        id: 'font-smp-12',
        path: 'fonts/NeoSmpCff.otf',
        media_type: 'font/otf',
        data_base64: sfnt12.toString('base64'),
      },
      {
        id: 'font-ttc-bundle',
        path: 'fonts/NeoCollectionTtc.ttc',
        media_type: 'font/collection',
        data_base64: packSfntsIntoTtc([buildTestTtcFace('NeoCollectionTtc')]).toString('base64'),
      },
    ],
    symbols: [
      {
        id: 'root',
        name: 'MainScene',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'layer-ui',
              name: 'UI',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  label: 'main',
                  tween: null,
                  elements: [
                    {
                      id: 'el-rich-input',
                      name: 'richInput',
                      transform: makeTransform({ x: 24, y: 24 }),
                      kind: {
                        type: 'text_box',
                        text: 'Red Blue',
                        width: 360,
                        height: 40,
                        format: makeTextFormat(
                          { size: 16, color: 0x1e293b },
                          {
                            field_type: 'input',
                            selectable: true,
                            border: true,
                            border_color: 0x60a5fa,
                            background: true,
                            background_color: 0xffffff,
                          }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-paste-target',
                      name: 'pasteTarget',
                      transform: makeTransform({ x: 24, y: 80 }),
                      kind: {
                        type: 'text_box',
                        text: 'Start End',
                        width: 360,
                        height: 40,
                        format: makeTextFormat(
                          { font: 'Georgia', size: 16, color: 0x008844 },
                          {
                            field_type: 'input',
                            selectable: true,
                            border: true,
                            border_color: 0x34d399,
                            background: true,
                            background_color: 0xffffff,
                          }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-sel-only',
                      name: 'selOnlyField',
                      transform: makeTransform({ x: 24, y: 136 }),
                      kind: {
                        type: 'text_box',
                        text: 'CopyOnly',
                        width: 240,
                        height: 36,
                        format: makeTextFormat(
                          { size: 15, color: 0x0f172a },
                          {
                            field_type: 'input',
                            selectable: true,
                            interaction_mode: 'selection',
                            border: true,
                            border_color: 0x94a3b8,
                            background: true,
                            background_color: 0xf1f5f9,
                          }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-dyn-select',
                      name: 'dynSelect',
                      transform: makeTransform({ x: 24, y: 192 }),
                      kind: {
                        type: 'text_box',
                        text: 'Selectable Dynamic',
                        width: 260,
                        height: 34,
                        format: makeTextFormat(
                          { size: 14, color: 0xe2e8f0 },
                          { field_type: 'dynamic', selectable: true, html: true }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                    {
                      id: 'el-dyn-locked',
                      name: 'dynLocked',
                      transform: makeTransform({ x: 24, y: 238 }),
                      kind: {
                        type: 'text_box',
                        text: 'Locked Dynamic',
                        width: 260,
                        height: 34,
                        format: makeTextFormat(
                          { size: 14, color: 0x94a3b8 },
                          { field_type: 'dynamic', selectable: false }
                        ),
                        word_wrap: false,
                        multiline: false,
                      },
                      scripts: [],
                    },
                  ],
                },
              ],
            },
          ],
          scripts: [{ frame: 0, script: { language: 'action_script3', source: frame0Script } }],
        },
      },
    ],
  };
}

function startStaticServer(rootDir) {
  const mimeByExt = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.wasm': 'application/wasm',
    '.json': 'application/json; charset=utf-8',
    '.ttf': 'font/ttf',
    '.otf': 'font/otf',
  };
  const server = http.createServer((req, res) => {
    const rawPath = decodeURIComponent((req.url || '/').split('?')[0]);
    const rel = rawPath.replace(/^\/+/, '');
    const candidate = path.resolve(rootDir, rel);
    if (!candidate.startsWith(rootDir) || !fs.existsSync(candidate) || fs.statSync(candidate).isDirectory()) {
      res.writeHead(404);
      res.end('Not found');
      return;
    }
    const ext = path.extname(candidate).toLowerCase();
    res.writeHead(200, { 'Content-Type': mimeByExt[ext] || 'application/octet-stream' });
    fs.createReadStream(candidate).pipe(res);
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port }));
  });
}

async function launchCdpSession(chromeBin, userDataDir) {
  const chromeProc = spawn(
    chromeBin,
    [
      '--headless=new',
      '--remote-debugging-port=0',
      `--user-data-dir=${userDataDir}`,
      '--no-first-run',
      '--no-default-browser-check',
      'about:blank',
    ],
    { stdio: ['ignore', 'pipe', 'pipe'] }
  );

  const wsUrl = await new Promise((resolve, reject) => {
    let stderrBuf = '';
    const timer = setTimeout(() => reject(new Error(`Chrome DevTools timeout: ${stderrBuf}`)), 15000);
    chromeProc.stderr.on('data', async (chunk) => {
      stderrBuf += chunk.toString();
      const match = stderrBuf.match(/DevTools listening on (ws:\/\/\S+)/);
      if (match) {
        clearTimeout(timer);
        try {
          const devOrigin = match[1].replace(/^ws:/, 'http:').replace(/\/devtools\/.*/, '');
          const resp = await fetch(`${devOrigin}/json/list`);
          const targets = await resp.json();
          const page = targets.find((t) => t.type === 'page' && t.webSocketDebuggerUrl);
          if (!page) reject(new Error('No page CDP target'));
          else resolve(page.webSocketDebuggerUrl);
        } catch (err) {
          reject(err);
        }
      }
    });
    chromeProc.once('exit', (code) => {
      clearTimeout(timer);
      reject(new Error(`Chrome exited prematurely (${code}): ${stderrBuf}`));
    });
  });

  const ws = new WebSocket(wsUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', reject, { once: true });
  });

  let nextId = 1;
  const pending = new Map();
  const errors = [];
  let loadResolve = null;

  ws.addEventListener('message', (event) => {
    const msg = JSON.parse(String(event.data));
    if (msg.method === 'Page.loadEventFired' && loadResolve) {
      const fn = loadResolve;
      loadResolve = null;
      fn();
    } else if (msg.method === 'Runtime.exceptionThrown') {
      errors.push(`exception: ${JSON.stringify(msg.params.exceptionDetails)}`);
    } else if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
      errors.push(`console.error: ${JSON.stringify(msg.params.args)}`);
    } else if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(msg.error.message || JSON.stringify(msg.error)));
      else resolve(msg.result);
    }
  });

  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });

  await send('Page.enable');
  await send('Runtime.enable');

  const navigateAndWait = async (url) => {
    const loaded = new Promise((resolve) => {
      loadResolve = resolve;
    });
    await send('Page.navigate', { url });
    await loaded;
  };

  const evalJson = async (expression) => {
    const res = await send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (res.exceptionDetails) {
      throw new Error(`Eval failed: ${JSON.stringify(res.exceptionDetails)}`);
    }
    return res.result?.value;
  };

  const waitBooted = async (traceMarker) => {
    await evalJson(`
      new Promise((resolve, reject) => {
        const deadline = Date.now() + 15000;
        const tick = () => {
          const errEl = document.getElementById('error');
          if (errEl && errEl.textContent.trim()) {
            reject(new Error('Runtime error UI: ' + errEl.textContent));
            return;
          }
          const loadingEl = document.getElementById('loading');
          const traceText = document.getElementById('trace')?.textContent || '';
          if (
            loadingEl &&
            loadingEl.style.display === 'none' &&
            window.NeoFlashPlayer &&
            (${JSON.stringify(traceMarker)} === '' || traceText.includes(${JSON.stringify(traceMarker)}))
          ) {
            resolve(true);
            return;
          }
          if (Date.now() > deadline) {
            reject(new Error('Timeout waiting for boot (' + ${JSON.stringify(traceMarker)} + '): trace=' + traceText));
            return;
          }
          setTimeout(tick, 25);
        };
        tick();
      })
    `);
  };

  const pressKey = async (key, code, windowsVirtualKeyCode) => {
    const text = key === 'Enter' ? '\r' : key.length === 1 ? key : undefined;
    await send('Input.dispatchKeyEvent', {
      type: 'keyDown',
      key,
      code,
      windowsVirtualKeyCode,
      nativeVirtualKeyCode: windowsVirtualKeyCode,
      ...(text ? { text, unmodifiedText: text } : {}),
    });
    await send('Input.dispatchKeyEvent', {
      type: 'keyUp',
      key,
      code,
      windowsVirtualKeyCode,
      nativeVirtualKeyCode: windowsVirtualKeyCode,
    });
  };

  return { chromeProc, ws, send, navigateAndWait, evalJson, waitBooted, pressKey, errors };
}

test('Browser HTML5 (Canvas + DOM): graphemes, mixed-run IME commit/cancel, rich clipboard, TextInteractionMode, TEXT_INPUT preventDefault, Font.hasGlyphs, and Restart', async (t) => {
  const chromeBin = findChromeBinary();
  if (!chromeBin) {
    t.skip('Google Chrome / Chromium binary not available');
    return;
  }
  assert.ok(
    fs.existsSync(path.join(runtimeDir, 'neoflash_web_bg.wasm')),
    'target/web-runtime must be built via ./scripts/build-web.sh'
  );
  const bin = ensureExportBinary();

  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-text-t326-'));
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nf-chrome-t326-'));
  let serverInfo = null;
  let cdp = null;

  try {
    const sfnt4 = buildTestSfntFormat4();
    const sfnt12 = buildTestSfntFormat12();

    const canvasProjPath = path.join(tempRoot, 't326-canvas.nflash');
    const domProjPath = path.join(tempRoot, 't326-dom.nflash');
    fs.writeFileSync(canvasProjPath, JSON.stringify(buildBrowserTextSuiteProject('canvas', sfnt4, sfnt12)));
    fs.writeFileSync(domProjPath, JSON.stringify(buildBrowserTextSuiteProject('dom', sfnt4, sfnt12)));

    const outCanvas = path.join(tempRoot, 'bundle-canvas');
    const outDom = path.join(tempRoot, 'bundle-dom');
    execFileSync(bin, ['--runtime', runtimeDir, canvasProjPath, '--output', outCanvas, '--render-mode', 'canvas'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    execFileSync(bin, ['--runtime', runtimeDir, domProjPath, '--output', outDom, '--render-mode', 'dom'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });

    serverInfo = await startStaticServer(tempRoot);
    cdp = await launchCdpSession(chromeBin, userDataDir);
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: 900,
      height: 700,
      deviceScaleFactor: 1,
      mobile: false,
    });

    for (const mode of ['canvas', 'dom']) {
      await cdp.navigateAndWait(`http://127.0.0.1:${serverInfo.port}/bundle-${mode}/index.html`);
      await cdp.waitBooted('T326_BOOT=true,true,true');

      // Helper in page context to focus an authored TextField by name and return its active <input>/<textarea>.
      await cdp.evalJson(`(() => {
        window.__t326Scene = () => window.NeoFlashPlayer.root;
        window.__t326DomInput = (tf) => {
          const refKey = JSON.stringify(tf.toJSON().object_ref);
          const wanted = 'input:' + refKey;
          return Array.from(document.querySelectorAll('#stage-dom-root .nf-dom-input')).find(
            (n) => n.getAttribute('data-nf-id') === wanted
          ) || null;
        };
        window.__t326FocusField = (fieldName, selStart, selEnd) => {
          const scene = window.__t326Scene();
          const tf = scene.getChildByName(fieldName);
          scene.stage.focus = tf;
          if (typeof selStart === 'number' && typeof selEnd === 'number') {
            tf.setSelection(selStart, selEnd);
          }
          const el = ${
            mode === 'dom'
              ? `window.__t326DomInput(tf)`
              : `document.querySelector('[data-neoflash-text-input="true"][aria-label="' + fieldName + '"]')`
          };
          if (el) {
            el.focus();
            if (typeof selStart === 'number' && typeof selEnd === 'number' && typeof el.setSelectionRange === 'function') {
              el.setSelectionRange(selStart, selEnd, 'forward');
            }
          }
          return Boolean(el);
        };
        return true;
      })()`);

      // 1. Embedded cmap Format 4 & Format 12 probe + programmatic .text assignment does NOT fire Event.CHANGE
      const bootInfo = await cdp.evalJson(`(() => {
        const sc = window.__t326Scene();
        sc.doProgrammaticSet();
        return sc.inspectRich();
      })()`);
      assert.deepEqual(
        bootInfo.fontProbe,
        { bmpOk: true, smpOk: true, ttcOk: true },
        `${mode}: embedded SFNT/TTC cmap probe failed`
      );
      assert.equal(bootInfo.changeCount, 0, `${mode}: programmatic .text/.htmlText must NOT fire Event.CHANGE`);
      assert.equal(bootInfo.headBold, true);
      assert.equal(bootInfo.headColor, 0xff0000);
      assert.equal(bootInfo.tailItalic, true);
      assert.equal(bootInfo.tailColor, 0x0055ff);

      // 2. Non-selectable dynamic text vs selectable dynamic text + SELECTION mode field
      if (mode === 'dom') {
        const domSelectStyles = await cdp.evalJson(`(() => {
          const sc = window.__t326Scene();
          const texts = Array.from(document.querySelectorAll('#stage-dom-root .nf-dom-text'));
          const sel = texts.find((n) => n.textContent.includes('Selectable Dynamic'));
          const locked = texts.find((n) => n.textContent.includes('Locked Dynamic'));
          window.__t326FocusField('selOnlyField', 0, 4);
          const selOnly = window.__t326DomInput(sc.getChildByName('selOnlyField'));
          return {
            selUserSelect: sel ? getComputedStyle(sel).userSelect : null,
            lockedUserSelect: locked ? getComputedStyle(locked).userSelect : null,
            selOnlyReadOnly: selOnly ? selOnly.readOnly : null,
          };
        })()`);
        assert.equal(domSelectStyles.selUserSelect, 'text');
        assert.equal(domSelectStyles.lockedUserSelect, 'none');
        assert.equal(domSelectStyles.selOnlyReadOnly, false, 'legacy selection state must not lock desktop input');
      } else {
        const canvasOverlayCheck = await cdp.evalJson(`(() => {
          const sc = window.__t326Scene();
          sc.stage.focus = sc.getChildByName('dynSelect');
          const dynInputs = document.querySelectorAll('[data-neoflash-text-input="true"]').length;
          sc.stage.focus = sc.getChildByName('selOnlyField');
          const selInput = document.querySelector('[data-neoflash-text-input="true"][aria-label="selOnlyField"]');
          return { dynInputs, selOnlyReadOnly: selInput ? selInput.readOnly : null };
        })()`);
        assert.equal(canvasOverlayCheck.dynInputs, 0, 'Canvas dynamic TextField must not mount editable textarea overlay');
        assert.equal(canvasOverlayCheck.selOnlyReadOnly, false, 'Canvas legacy selection state must not lock desktop input');
      }

      // 3. TextEvent.TEXT_INPUT fires BEFORE mutation and preventDefault() cancels the edit cleanly!
      const focusedRich = await cdp.evalJson(`window.__t326FocusField('richInput', 3, 3)`);
      assert.equal(focusedRich, true, `${mode}: richInput must mount focusable input`);
      await cdp.evalJson(`window.__t326Scene().setBlockInput(true)`);
      await cdp.send('Input.insertText', { text: 'X' });
      await cdp.evalJson(`window.__t326Scene().setBlockInput(false)`);
      const afterBlockedInput = await cdp.evalJson(`window.__t326Scene().inspectRich()`);
      assert.equal(afterBlockedInput.text, 'Red Blue', `${mode}: preventDefault() on TEXT_INPUT must prevent text mutation`);
      assert.deepEqual(
        afterBlockedInput.textInputLog,
        ['X@len=8'],
        `${mode}: TEXT_INPUT must fire before text mutation (length still 8)`
      );
      assert.equal(afterBlockedInput.changeCount, 0, `${mode}: cancelled TEXT_INPUT must not fire Event.CHANGE`);
      assert.equal(afterBlockedInput.headBold, true);
      assert.equal(afterBlockedInput.tailItalic, true);

      // 4. IME composition preedit -> Escape cancel restores exact pre-composition text & mixed format_runs!
      await cdp.evalJson(`(() => {
        window.__t326FocusField('richInput', 3, 3);
        const el = document.activeElement;
        el.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }));
        el.value = 'Redにほんご Blue';
        el.setSelectionRange(7, 7, 'forward');
        el.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: 'にほんご' }));
        el.dispatchEvent(new InputEvent('input', { bubbles: true, isComposing: true, data: 'にほんご' }));
        return el.value;
      })()`);
      await cdp.pressKey('Escape', 'Escape', 27);
      await cdp.evalJson(`(() => {
        const el = document.activeElement;
        if (el) el.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '' }));
        return true;
      })()`);
      const afterImeCancel = await cdp.evalJson(`window.__t326Scene().inspectRich()`);
      assert.equal(afterImeCancel.text, 'Red Blue', `${mode}: cancelled IME composition must restore exact text`);
      assert.equal(afterImeCancel.changeCount, 0, `${mode}: cancelled IME composition must not fire Event.CHANGE`);
      assert.equal(afterImeCancel.headBold, true, `${mode}: cancelled IME must preserve bold red prefix`);
      assert.equal(afterImeCancel.headColor, 0xff0000);
      assert.equal(afterImeCancel.tailItalic, true, `${mode}: cancelled IME must preserve italic blue suffix`);
      assert.equal(afterImeCancel.tailColor, 0x0055ff);

      // 5. IME composition preedit -> commit of multi-grapheme ("か\u3099👨‍👩‍👧‍👦и\u0306", 15 UTF-16 units) at index 3
      //    inherits active bold-red run (0..18) and rebases italic-blue suffix (19..23)!
      await cdp.evalJson(`(() => {
        window.__t326FocusField('richInput', 3, 3);
        const el = document.activeElement;
        const committedCluster = 'か\\u3099👨‍👩‍👧‍👦и\\u0306';
        el.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }));
        el.value = 'Red' + committedCluster + ' Blue';
        el.setSelectionRange(18, 18, 'forward');
        el.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: committedCluster }));
        el.dispatchEvent(new InputEvent('input', { bubbles: true, isComposing: true, data: committedCluster }));
        el.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: committedCluster }));
        return true;
      })()`);
      const afterImeCommit = await cdp.evalJson(`window.__t326Scene().inspectRich()`);
      assert.equal(afterImeCommit.text, 'Redか\u3099👨‍👩‍👧‍👦и\u0306 Blue', `${mode}: IME commit text mismatch`);
      assert.equal(afterImeCommit.length, 23);
      assert.equal(afterImeCommit.changeCount, 1, `${mode}: IME commit must dispatch Event.CHANGE once`);
      assert.equal(afterImeCommit.headBold, true, `${mode}: IME commit must preserve bold red run`);
      assert.equal(afterImeCommit.headColor, 0xff0000);
      assert.equal(afterImeCommit.tailItalic, true, `${mode}: IME commit must rebase italic blue run cleanly`);
      assert.equal(afterImeCommit.tailColor, 0x0055ff);

      // 6. Grapheme-safe ArrowLeft, forward Delete (11-unit ZWJ family), and Backspace (2-unit kana+dakuten)!
      await cdp.pressKey('ArrowLeft', 'ArrowLeft', 37);
      const caretStep1 = await cdp.evalJson(`window.__t326Scene().inspectRich()`);
      assert.equal(caretStep1.caretIndex, 16, `${mode}: ArrowLeft must step over 2-unit decomposed Cyrillic и\\u0306`);

      await cdp.pressKey('ArrowLeft', 'ArrowLeft', 37);
      const caretStep2 = await cdp.evalJson(`window.__t326Scene().inspectRich()`);
      assert.equal(caretStep2.caretIndex, 5, `${mode}: ArrowLeft must step over 11-unit ZWJ family emoji`);

      await cdp.pressKey('Delete', 'Delete', 46);
      const afterForwardDelete = await cdp.evalJson(`window.__t326Scene().inspectRich()`);
      assert.equal(
        afterForwardDelete.text,
        'Redか\u3099и\u0306 Blue',
        `${mode}: forward Delete must remove the entire 11-unit ZWJ family cluster at once`
      );
      assert.equal(afterForwardDelete.tailItalic, true);
      assert.equal(afterForwardDelete.tailColor, 0x0055ff);

      await cdp.pressKey('Backspace', 'Backspace', 8);
      const afterBackspace = await cdp.evalJson(`window.__t326Scene().inspectRich()`);
      assert.equal(
        afterBackspace.text,
        'Redи\u0306 Blue',
        `${mode}: Backspace must remove the entire 2-unit か\\u3099 cluster at once`
      );
      assert.equal(afterBackspace.headBold, true);
      assert.equal(afterBackspace.tailItalic, true);

      // 7. Rich Clipboard Copy + Paste across fields preserves mixed runs ("и\u0306 Blue" = bold red + italic blue)!
      const pasteResult = await cdp.evalJson(`(() => {
        // Select "и\\u0306 Blue" (UTF-16 3..10) in richInput and dispatch Copy with a real DataTransfer.
        window.__t326FocusField('richInput', 3, 10);
        const srcEl = document.activeElement;
        const dt = new DataTransfer();
        const copyEvt = new ClipboardEvent('copy', { bubbles: true, cancelable: true, clipboardData: dt });
        srcEl.dispatchEvent(copyEvt);

        // Focus pasteTarget at UTF-16 index 6 ("Start |End") and dispatch Paste with the same DataTransfer!
        window.__t326FocusField('pasteTarget', 6, 6);
        const dstEl = document.activeElement;
        const pasteEvt = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt });
        dstEl.dispatchEvent(pasteEvt);

        return {
          copiedPlain: dt.getData('text/plain'),
          copiedHtml: dt.getData('text/html'),
          target: window.__t326Scene().inspectPasteTarget(),
        };
      })()`);
      assert.equal(pasteResult.copiedPlain, 'и\u0306 Blue', `${mode}: rich Copy plain text mismatch`);
      assert.ok(pasteResult.copiedHtml.includes('<font'), `${mode}: rich Copy must populate HTML clipboard`);
      assert.equal(pasteResult.target.text, 'Start и\u0306 BlueEnd', `${mode}: rich Paste text mismatch`);
      assert.equal(pasteResult.target.u0Underline, true, `${mode}: rich Paste must preserve prefix underline run`);
      assert.equal(pasteResult.target.midBold, true, `${mode}: rich Paste must preserve copied bold red run`);
      assert.equal(pasteResult.target.midRedColor, 0xff0000);
      assert.equal(pasteResult.target.midItalic, true, `${mode}: rich Paste must preserve copied italic blue run`);
      assert.equal(pasteResult.target.midBlueColor, 0x0055ff);

      // 7b. `useRichTextClipboard == false` on destination OR source MUST paste plain text inheriting destination format,
      //     and `restrict` + `maxChars` on rich paste MUST filter `restrict` before `maxChars`!
      const plainAndRestrictCheck = await cdp.evalJson(`(() => {
        const sc = window.__t326Scene();

        // Case A: source has useRichTextClipboard=true, destination has useRichTextClipboard=false.
        sc.setRichClipboardFlag(true);
        sc.resetPasteTarget(false, null, 0);
        window.__t326FocusField('richInput', 3, 10);
        const dtRich = new DataTransfer();
        document.activeElement.dispatchEvent(new ClipboardEvent('copy', { bubbles: true, cancelable: true, clipboardData: dtRich }));
        window.__t326FocusField('pasteTarget', 6, 6);
        document.activeElement.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dtRich }));
        const dstPlainWhenTargetFalse = sc.inspectPasteTarget();

        // Case B: source has useRichTextClipboard=false, destination has useRichTextClipboard=true.
        sc.setRichClipboardFlag(false);
        sc.resetPasteTarget(true, null, 0);
        window.__t326FocusField('richInput', 3, 10);
        const dtPlain = new DataTransfer();
        document.activeElement.dispatchEvent(new ClipboardEvent('copy', { bubbles: true, cancelable: true, clipboardData: dtPlain }));
        window.__t326FocusField('pasteTarget', 6, 6);
        document.activeElement.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dtPlain }));
        const dstPlainWhenSourceFalse = sc.inspectPasteTarget();

        // Case C: Rich paste ("123ABCDE": 0..5 bold red, 5..8 italic blue) into restrict="A-Z" + maxChars=3!
        sc.prepareRestrictedScenario();
        window.__t326FocusField('richInput', 0, 8);
        const dtRestrict = new DataTransfer();
        document.activeElement.dispatchEvent(new ClipboardEvent('copy', { bubbles: true, cancelable: true, clipboardData: dtRestrict }));
        window.__t326FocusField('pasteTarget', 0, 0);
        document.activeElement.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dtRestrict }));
        const restrictedResult = sc.inspectRestrictedTarget();

        sc.restoreRichScenario();
        return {
          richCopyHtmlLen: dtRich.getData('text/html').length,
          plainCopyHtml: dtPlain.getData('text/html'),
          plainCopyMime: dtPlain.getData('application/x-neoflash-runs+json'),
          dstPlainWhenTargetFalse,
          dstPlainWhenSourceFalse,
          restrictedResult,
        };
      })()`);
      assert.ok(plainAndRestrictCheck.richCopyHtmlLen > 0, `${mode}: useRichTextClipboard=true Copy must write HTML`);
      assert.equal(plainAndRestrictCheck.plainCopyHtml, '', `${mode}: useRichTextClipboard=false Copy must NOT write HTML`);
      assert.equal(plainAndRestrictCheck.plainCopyMime, '', `${mode}: useRichTextClipboard=false Copy must NOT write rich MIME`);
      assert.equal(plainAndRestrictCheck.dstPlainWhenTargetFalse.text, 'Start и\u0306 BlueEnd');
      assert.equal(plainAndRestrictCheck.dstPlainWhenTargetFalse.midBold, false, `${mode}: target useRichTextClipboard=false must inherit destination non-bold format`);
      assert.equal(plainAndRestrictCheck.dstPlainWhenTargetFalse.midRedColor, 0x008844, `${mode}: target useRichTextClipboard=false must inherit destination green color`);
      assert.equal(plainAndRestrictCheck.dstPlainWhenTargetFalse.midItalic, false);
      assert.equal(plainAndRestrictCheck.dstPlainWhenTargetFalse.midBlueColor, 0x008844);
      assert.equal(plainAndRestrictCheck.dstPlainWhenSourceFalse.text, 'Start и\u0306 BlueEnd');
      assert.equal(plainAndRestrictCheck.dstPlainWhenSourceFalse.midBold, false, `${mode}: source useRichTextClipboard=false must not export bold run`);
      assert.equal(plainAndRestrictCheck.dstPlainWhenSourceFalse.midRedColor, 0x008844);
      assert.equal(plainAndRestrictCheck.dstPlainWhenSourceFalse.midItalic, false);
      assert.equal(plainAndRestrictCheck.dstPlainWhenSourceFalse.midBlueColor, 0x008844);
      assert.deepEqual(
        plainAndRestrictCheck.restrictedResult,
        {
          text: 'ABC',
          abBold: true,
          abColor: 0xff0000,
          cItalic: true,
          cColor: 0x0055ff,
        },
        `${mode}: rich Paste must filter restrict before maxChars while remapping runs`
      );

      // 8. Adobe exposes textInteractionMode as read-only on desktop. A script
      //    write fails and cannot manufacture a user-mode-change event or lock;
      //    real user Alt-pointer / contextmenu selection gesture transitions mode & fires textInteractionModeChange!
      const modeContract = await cdp.evalJson(`(() => {
        const scene = window.__t326Scene();
        const base = scene.probeTextInteractionModeWrite();
        const tf = scene.getChildByName('richInput');
        const events = [];
        tf.addEventListener('textInteractionModeChange', e => events.push({ mode: tf.textInteractionMode, bubbles: e.bubbles }));
        scene.stage.focus = tf;
        const inputEl = document.querySelector('textarea, input[type="text"], [contenteditable="true"]');
        if (inputEl) {
          inputEl.focus();
          inputEl.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, altKey: true }));
          inputEl.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, altKey: true }));
        }
        const FontCls = window.NeoFlashPlayer?.program?.type?.('flash.text.Font');
        let devGlyphProbe = null;
        if (FontCls) {
          const dev = FontCls.enumerateFonts(true).find(f => f.fontType === 'device');
          if (dev) devGlyphProbe = { ascii: dev.hasGlyphs('AZ09 '), tofu: dev.hasGlyphs('A\\uF8FE\\uE777') };
        }
        return { ...base, events, devGlyphProbe };
      })()`);
      assert.equal(modeContract.threw, true, `${mode}: textInteractionMode must be read-only`);
      assert.equal(modeContract.mode, 'normal', `${mode}: script write keeps mode normal`);
      if (mode === 'canvas') {
        assert.deepEqual(
          modeContract.events,
          [{ mode: 'selection', bubbles: false }, { mode: 'normal', bubbles: false }],
          `${mode}: user Alt-mousedown / plain-mousedown transitions textInteractionMode and dispatches non-bubbling textInteractionModeChange`
        );
      }
      if (modeContract.devGlyphProbe) {
        assert.deepEqual(modeContract.devGlyphProbe, { ascii: true, tofu: false }, `${mode}: Chromium device Font.hasGlyphs distinguishes real glyphs from .notdef tofu`);
      }

      // 9. Restart resets stage cleanly in both Canvas and DOM modes!
      const afterRestart = await cdp.evalJson(`(() => {
        document.getElementById('restart').click();
        return window.NeoFlashPlayer.root.inspectRich();
      })()`);
      assert.equal(afterRestart.text, 'Red Blue', `${mode}: Restart must restore initial authored text`);
      assert.equal(afterRestart.interactionMode, 'normal', `${mode}: Restart must restore normal interactionMode`);
      assert.equal(afterRestart.headBold, true, `${mode}: Restart must restore initial bold red run`);
      assert.equal(afterRestart.tailItalic, true, `${mode}: Restart must restore initial italic blue run`);
      assert.equal(afterRestart.changeCount, 0, `${mode}: Restart must reset script state without spurious CHANGE`);
    }

    await cdp.navigateAndWait('about:blank');
    assert.deepEqual(cdp.errors, [], `Uncaught browser errors during T-326 suite: ${cdp.errors.join('\n')}`);
  } finally {
    try {
      cdp?.ws?.close();
    } catch {}
    if (cdp?.chromeProc && cdp.chromeProc.exitCode === null) {
      await new Promise((resolve) => {
        cdp.chromeProc.once('exit', resolve);
        cdp.chromeProc.kill('SIGKILL');
        setTimeout(resolve, 1500);
      });
    }
    if (serverInfo?.server) {
      await new Promise((resolve) => serverInfo.server.close(resolve));
    }
    for (const dir of [tempRoot, userDataDir]) {
      try {
        fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 50 });
      } catch {}
    }
  }
});

test('Device Font.hasGlyphs probes Private Use Area (PUA) icon codepoints via Canvas tofu comparison when Canvas 2D is available', () => {
  const NeoFlashText = require('../text.js');
  const makeProbeCanvas = () => ({
    width: 64,
    height: 64,
    getContext() {
      let currentText = '';
      return {
        font: '',
        textBaseline: 'top',
        fillStyle: '#fff',
        clearRect() {},
        measureText(str) {
          // Installed custom icon font 'IconSetFont' has distinct metrics from monospace/serif
          const isIcon = String(this.font).includes('IconSetFont');
          if (str === '\ue001') return { width: isIcon ? 24 : 12 };
          if (str === '\ue999' || str === '\uffff') return { width: 12 };
          return { width: isIcon ? str.length * 14 : str.length * 10 };
        },
        fillText(str) {
          currentText = str;
        },
        getImageData(_x, _y, w, h) {
          const data = new Uint8ClampedArray(w * h * 4);
          // U+E001 renders an icon glyph (distinct ink pattern from U+FFFF tofu); U+E999 renders .notdef tofu!
          if (currentText === '\ue001') {
            data[3] = 255; data[7] = 255; data[11] = 200;
          } else {
            data[3] = 128;
          }
          return { data };
        },
      };
    },
  });
  const prevOffscreen = globalThis.OffscreenCanvas;
  globalThis.OffscreenCanvas = function(w, h) {
    const c = makeProbeCanvas();
    c.width = w;
    c.height = h;
    return c;
  };
  try {
    const textHost = NeoFlashText.create({
      player: {
        project_fonts: () => '[]',
        enumerate_device_fonts: () => JSON.stringify([{ name: 'IconSetFont', style: 'regular' }]),
      },
    });
    const iconFont = textHost.Font.__nfAllocate({ fontName: 'IconSetFont', fontStyle: 'regular', fontType: 'device' });
    assert.ok(iconFont, 'allocated IconSetFont device font');
    assert.equal(iconFont.hasGlyphs('\ue001'), true, 'mapped PUA icon glyph U+E001 passes Canvas tofu check');
    assert.equal(iconFont.hasGlyphs('\ue999'), false, 'unmapped PUA codepoint U+E999 matching .notdef tofu returns false');
  } finally {
    if (prevOffscreen === undefined) delete globalThis.OffscreenCanvas;
    else globalThis.OffscreenCanvas = prevOffscreen;
  }
});
