// End-to-end verification of raw .swf (`FWS`, `CWS`, `ZWS`) decoding, timeline playback,
// shape/sprite/text Stage rendering, bounded AVM1/AVM2 frame action execution, and
// unload/unloadAndStop lifecycle in `Loader.load` and `Loader.loadBytes`.
const assert = require('node:assert/strict');
const test = require('node:test');
const zlib = require('node:zlib');
const { resolve } = require('node:path');

const root = resolve(__dirname, '../..');

class BitWriter {
  constructor() {
    this.bytes = [];
    this.cur = 0;
    this.used = 0;
  }
  writeUBits(val, n) {
    for (let i = n - 1; i >= 0; i--) {
      const bit = (val >>> i) & 1;
      this.cur = (this.cur << 1) | bit;
      this.used++;
      if (this.used === 8) {
        this.bytes.push(this.cur & 0xff);
        this.cur = 0;
        this.used = 0;
      }
    }
  }
  writeSBits(val, n) {
    const mask = n >= 32 ? 0xffffffff : ((1 << n) - 1);
    this.writeUBits(val & mask, n);
  }
  align() {
    if (this.used > 0) {
      this.bytes.push((this.cur << (8 - this.used)) & 0xff);
      this.cur = 0;
      this.used = 0;
    }
  }
  writeU8(v) {
    this.align();
    this.bytes.push(v & 0xff);
  }
  writeU16(v) {
    this.writeU8(v & 0xff);
    this.writeU8((v >>> 8) & 0xff);
  }
  writeU32(v) {
    this.writeU16(v & 0xffff);
    this.writeU16((v >>> 16) & 0xffff);
  }
  writeBytes(arr) {
    this.align();
    for (const b of arr) this.bytes.push(b & 0xff);
  }
  writeCString(str) {
    this.writeBytes(Buffer.from(str, 'utf8'));
    this.writeU8(0);
  }
  writeRect(xmin, xmax, ymin, ymax) {
    this.align();
    const nbits = 15;
    this.writeUBits(nbits, 5);
    this.writeSBits(xmin, nbits);
    this.writeSBits(xmax, nbits);
    this.writeSBits(ymin, nbits);
    this.writeSBits(ymax, nbits);
    this.align();
  }
  writeTranslateMatrix(txTwips, tyTwips) {
    this.align();
    this.writeUBits(0, 1); // hasScale = 0
    this.writeUBits(0, 1); // hasRotate = 0
    const nbits = 14;
    this.writeUBits(nbits, 5);
    this.writeSBits(txTwips, nbits);
    this.writeSBits(tyTwips, nbits);
    this.align();
  }
  toBuffer() {
    this.align();
    return Buffer.from(this.bytes);
  }
}

function makeSwfTag(code, payloadBuf) {
  const payload = Buffer.from(payloadBuf);
  if (payload.length < 0x3f) {
    const hdr = Buffer.alloc(2);
    hdr.writeUInt16LE((code << 6) | payload.length, 0);
    return Buffer.concat([hdr, payload]);
  }
  const hdr = Buffer.alloc(6);
  hdr.writeUInt16LE((code << 6) | 0x3f, 0);
  hdr.writeUInt32LE(payload.length, 2);
  return Buffer.concat([hdr, payload]);
}

function makeSolidRectShapeTag(charId, wPx, hPx, rgb) {
  const w = wPx * 20;
  const h = hPx * 20;
  const bw = new BitWriter();
  bw.writeU16(charId);
  bw.writeRect(0, w, 0, h);
  bw.writeU8(1); // 1 fill style
  bw.writeU8(0x00); // solid fill
  bw.writeU8((rgb >>> 16) & 0xff);
  bw.writeU8((rgb >>> 8) & 0xff);
  bw.writeU8(rgb & 0xff);
  bw.writeU8(0); // 0 line styles
  bw.writeUBits(1, 4); // numFillBits = 1
  bw.writeUBits(0, 4); // numLineBits = 0

  // StyleChangeRecord: moveTo(0,0) + fillStyle1 = 1
  bw.writeUBits(0, 1); // non-edge
  bw.writeUBits(0, 1); // stateNewStyles
  bw.writeUBits(0, 1); // stateLineStyle
  bw.writeUBits(1, 1); // stateFillStyle1
  bw.writeUBits(0, 1); // stateFillStyle0
  bw.writeUBits(1, 1); // stateMoveTo
  bw.writeUBits(1, 5); // moveBits = 1
  bw.writeSBits(0, 1);
  bw.writeSBits(0, 1);
  bw.writeUBits(1, 1); // fillStyle1 = 1

  const writeLine = (dx, dy) => {
    bw.writeUBits(1, 1); // edge
    bw.writeUBits(1, 1); // straight
    const nbits = 13; // numBits - 2 => 15 bits
    bw.writeUBits(nbits, 4);
    bw.writeUBits(1, 1); // general line
    bw.writeSBits(dx, nbits + 2);
    bw.writeSBits(dy, nbits + 2);
  };
  writeLine(w, 0);
  writeLine(0, h);
  writeLine(-w, 0);
  writeLine(0, -h);

  // EndShapeRecord
  bw.writeUBits(0, 6);
  return makeSwfTag(2, bw.toBuffer()); // DefineShape (tag 2)
}

function makePlaceObject2({ depth, charId = null, move = false, txPx = 0, tyPx = 0, name = null }) {
  const bw = new BitWriter();
  let flags = 0;
  if (move) flags |= 0x01;
  if (charId !== null) flags |= 0x02;
  flags |= 0x04; // hasMatrix
  if (name !== null) flags |= 0x20;
  bw.writeU8(flags);
  bw.writeU16(depth);
  if (charId !== null) bw.writeU16(charId);
  bw.writeTranslateMatrix(txPx * 20, tyPx * 20);
  if (name !== null) bw.writeCString(name);
  return makeSwfTag(26, bw.toBuffer());
}

function makeDefineEditTextTag(charId, wPx, hPx, initialText) {
  const bw = new BitWriter();
  bw.writeU16(charId);
  bw.writeRect(0, wPx * 20, 0, hPx * 20);
  // flags1: HasText(0x80) | ReadOnly(0x08) | HasTextColor(0x04) | HasFont(0x02) = 0x8E
  bw.writeU8(0x8e);
  bw.writeU8(0x00);
  bw.writeU16(99); // fontId
  bw.writeU16(14 * 20); // fontHeight
  bw.writeU8(20); bw.writeU8(30); bw.writeU8(40); bw.writeU8(255); // RGBA
  bw.writeCString(''); // variableName
  bw.writeCString(initialText);
  return makeSwfTag(37, bw.toBuffer());
}

function wrapSwf(sig, version, widthPx, heightPx, fps, frameCount, tags, lzmaCompress = null) {
  const hdr = new BitWriter();
  hdr.writeRect(0, widthPx * 20, 0, heightPx * 20);
  hdr.writeU16(Math.round(fps * 256));
  hdr.writeU16(frameCount);
  const endTag = makeSwfTag(0, Buffer.alloc(0));
  const body = Buffer.concat([hdr.toBuffer(), ...tags, endTag]);
  const uncompressedLen = 8 + body.length;

  if (sig === 'FWS') {
    const pre = Buffer.alloc(8);
    pre.write('FWS', 0, 'ascii');
    pre[3] = version;
    pre.writeUInt32LE(uncompressedLen, 4);
    return Buffer.concat([pre, body]);
  }
  if (sig === 'CWS') {
    const pre = Buffer.alloc(8);
    pre.write('CWS', 0, 'ascii');
    pre[3] = version;
    pre.writeUInt32LE(uncompressedLen, 4);
    return Buffer.concat([pre, zlib.deflateSync(body)]);
  }
  if (sig === 'ZWS') {
    // ByteArray.compress("lzma") produces 5-byte props + 8-byte uncompressed size + LZMA data.
    // SWF ZWS header is: 'ZWS' (3) + version (1) + uncompressed_len (4) + compressed_len (4) + props (5) + lzma_data!
    const fullLzma = Buffer.from(lzmaCompress(body));
    const props = fullLzma.subarray(0, 5);
    const lzmaData = fullLzma.subarray(13);
    const pre = Buffer.alloc(12);
    pre.write('ZWS', 0, 'ascii');
    pre[3] = version;
    pre.writeUInt32LE(uncompressedLen, 4);
    pre.writeUInt32LE(lzmaData.length, 8);
    return Buffer.concat([pre, props, lzmaData]);
  }
  throw new Error(`Unknown sig ${sig}`);
}

function writeU30(arr, val) {
  let v = val >>> 0;
  while (v >= 0x80) {
    arr.push((v & 0x7f) | 0x80);
    v >>>= 7;
  }
  arr.push(v & 0x7f);
}

function makeAvm2DoAbc2Tag() {
  // Builds a minimal valid DoABC2 tag with Document Class "MainDoc" whose constructor:
  // - traces "avm2-ctor"
  // - sets this.avm2Count = 7
  // - calls addFrameScript(0, frame1Fn, 1, frame2Fn)
  // frame1Fn: traces "avm2-f1", calls stop()
  // frame2Fn: traces "avm2-f2", sets this.avm2Count = 202
  const abc = [];
  // minor=16, major=46
  abc.push(16, 0, 46, 0);
  // int pool: count=1 (empty)
  writeU30(abc, 1);
  // uint pool: count=1
  writeU30(abc, 1);
  // double pool: count=1
  writeU30(abc, 1);

  const strings = [
    '', // 0
    '', // 1: empty package ns
    'MainDoc', // 2
    'Object', // 3
    'trace', // 4
    'stop', // 5
    'addFrameScript', // 6
    'avm2Count', // 7
    'avm2-ctor', // 8
    'avm2-f1', // 9
    'avm2-f2', // 10
  ];
  writeU30(abc, strings.length);
  for (let i = 1; i < strings.length; i++) {
    const b = Buffer.from(strings[i], 'utf8');
    writeU30(abc, b.length);
    for (const c of b) abc.push(c);
  }

  // namespaces: count=2 => [1]: PackageNamespace(0x16, str 1 "")
  writeU30(abc, 2);
  abc.push(0x16); writeU30(abc, 1);

  // ns_sets: count=1
  writeU30(abc, 1);

  // multinames:
  // 1: QName(ns 1, "MainDoc"=2)
  // 2: QName(ns 1, "Object"=3)
  // 3: QName(ns 1, "trace"=4)
  // 4: QName(ns 1, "stop"=5)
  // 5: QName(ns 1, "addFrameScript"=6)
  // 6: QName(ns 1, "avm2Count"=7)
  const mnStrIds = [2, 3, 4, 5, 6, 7];
  writeU30(abc, 1 + mnStrIds.length);
  for (const sid of mnStrIds) {
    abc.push(0x07); // QName
    writeU30(abc, 1);
    writeU30(abc, sid);
  }

  // methods: 5 methods (0: cinit, 1: iinit, 2: script_init, 3: frame1Fn, 4: frame2Fn)
  writeU30(abc, 5);
  for (let m = 0; m < 5; m++) {
    writeU30(abc, 0); // param_count
    writeU30(abc, 0); // return_type
    writeU30(abc, 0); // name
    abc.push(0); // flags
  }

  // metadata: 0
  writeU30(abc, 0);

  // classes: 1 ("MainDoc")
  writeU30(abc, 1);
  // instance_info[0]
  writeU30(abc, 1); // name = MainDoc (mn 1)
  writeU30(abc, 2); // super = Object (mn 2)
  abc.push(0); // flags (dynamic)
  writeU30(abc, 0); // intrf_count
  writeU30(abc, 1); // iinit = method 1
  writeU30(abc, 0); // trait_count
  // class_info[0]
  writeU30(abc, 0); // cinit = method 0
  writeU30(abc, 0); // trait_count

  // scripts: 1
  writeU30(abc, 1);
  writeU30(abc, 2); // init = method 2
  writeU30(abc, 0); // trait_count

  // bodies: 3 bodies (method 1 = iinit, method 3 = frame1Fn, method 4 = frame2Fn)
  // Method 1 (iinit):
  //   findpropstrict(trace=3), pushstring("avm2-ctor"=8), callpropvoid(trace=3, 1)
  //   getlocal_0, pushbyte(7), setproperty(avm2Count=6)
  //   findpropstrict(addFrameScript=5), pushbyte(0), newfunction(3), pushbyte(1), newfunction(4), callpropvoid(addFrameScript=5, 4)
  //   returnvoid
  const iinitCode = [
    0x5d, 3, 0x2c, 8, 0x4f, 3, 1,
    0xd0, 0x24, 7, 0x61, 6,
    0x5d, 5, 0x24, 0, 0x40, 3, 0x24, 1, 0x40, 4, 0x4f, 5, 4,
    0x47,
  ];
  // Method 3 (frame1Fn):
  //   findpropstrict(trace=3), pushstring("avm2-f1"=9), callpropvoid(trace=3, 1)
  //   findpropstrict(stop=4), callpropvoid(stop=4, 0)
  //   returnvoid
  const f1Code = [
    0x5d, 3, 0x2c, 9, 0x4f, 3, 1,
    0x5d, 4, 0x4f, 4, 0,
    0x47,
  ];
  // Method 4 (frame2Fn):
  //   findpropstrict(trace=3), pushstring("avm2-f2"=10), callpropvoid(trace=3, 1)
  //   getlocal_0, pushshort(202 => u30/s32: 0xCA, 0x01), setproperty(avm2Count=6)
  //   returnvoid
  const f2Code = [
    0x5d, 3, 0x2c, 10, 0x4f, 3, 1,
    0xd0, 0x25, 0xca, 0x01, 0x61, 6,
    0x47,
  ];

  const bodies = [
    [1, iinitCode],
    [3, f1Code],
    [4, f2Code],
  ];
  writeU30(abc, bodies.length);
  for (const [mid, code] of bodies) {
    writeU30(abc, mid);
    writeU30(abc, 8); // max_stack
    writeU30(abc, 4); // local_count
    writeU30(abc, 0); // init_scope
    writeU30(abc, 4); // max_scope
    writeU30(abc, code.length);
    for (const b of code) abc.push(b);
    writeU30(abc, 0); // exc_count
    writeU30(abc, 0); // trait_count
  }

  // DoABC2 header: flags (4 bytes LE = 1) + name ("frame\0") + abc bytes
  const prefix = Buffer.from([1, 0, 0, 0, ...Buffer.from('frame\0', 'utf8')]);
  return makeSwfTag(82, Buffer.concat([prefix, Buffer.from(abc)]));
}

test('Loader.loadBytes and Loader.load execute FWS, CWS, and ZWS SWFs with timeline, shapes/sprites/text, AVM1/AVM2 actions, and clean unloadAndStop', async () => {
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');

  // Build an AVM1 multi-frame SWF (SWF v8, 120x80, 24 fps, 3 frames):
  // - Shape 1: 30x30 red square
  // - Shape 2: 20x20 blue square inside Sprite 3
  // - EditText 4: "Hello SWF"
  // - Frame 1 ("intro"): Place Shape 1 at (10,10), DoAction: this.avm1State = "f1"; trace("avm1-1"); stop();
  // - Frame 2 ("mid"): Move Shape 1 to (60,10), DoAction: this.avm1State = "f2"; trace("avm1-2");
  // - Frame 3 ("done"): Place Sprite 3 at (10,40) + EditText 4 at (40,40), DoAction: this.avm1State = "f3";
  const avm1PushStr = str => {
    const b = Buffer.from(str + '\0', 'utf8');
    const out = Buffer.alloc(1 + 2 + 1 + b.length);
    out[0] = 0x96; // ActionPush
    out.writeUInt16LE(1 + b.length, 1);
    out[3] = 0; // string type
    b.copy(out, 4);
    return out;
  };
  const doActionF1 = Buffer.concat([
    avm1PushStr('avm1-1'), Buffer.from([0x26]), // trace("avm1-1")
    avm1PushStr('avm1State'), avm1PushStr('f1'), Buffer.from([0x1d]), // SetVariable
    Buffer.from([0x07, 0x00]), // ActionStop + End
  ]);
  const doActionF2 = Buffer.concat([
    avm1PushStr('avm1-2'), Buffer.from([0x26]),
    avm1PushStr('avm1State'), avm1PushStr('f2'), Buffer.from([0x1d]),
    Buffer.from([0x00]),
  ]);
  const doActionF3 = Buffer.concat([
    avm1PushStr('avm1State'), avm1PushStr('f3'), Buffer.from([0x1d]),
    Buffer.from([0x00]),
  ]);

  // DefineSprite (id=3, 1 frame, places Shape 2 at (0,0))
  const spriteSub = Buffer.concat([
    makePlaceObject2({ depth: 1, charId: 2, txPx: 0, tyPx: 0 }),
    makeSwfTag(1, Buffer.alloc(0)),
    makeSwfTag(0, Buffer.alloc(0)),
  ]);
  const spriteHdr = Buffer.alloc(4);
  spriteHdr.writeUInt16LE(3, 0);
  spriteHdr.writeUInt16LE(1, 2);

  const avm1Tags = [
    makeSwfTag(69, Buffer.from([0x00, 0, 0, 0])), // FileAttributes: AS3=0 => actionScriptVersion=2
    makeSolidRectShapeTag(1, 30, 30, 0xff0000),
    makeSolidRectShapeTag(2, 20, 20, 0x0044ff),
    makeSwfTag(39, Buffer.concat([spriteHdr, spriteSub])), // DefineSprite 3
    makeDefineEditTextTag(4, 70, 24, 'Hello SWF'),
    makeSwfTag(43, Buffer.from('intro\0', 'utf8')),
    makePlaceObject2({ depth: 1, charId: 1, txPx: 10, tyPx: 10, name: 'box' }),
    makeSwfTag(12, doActionF1),
    makeSwfTag(1, Buffer.alloc(0)), // ShowFrame 1
    makeSwfTag(43, Buffer.from('mid\0', 'utf8')),
    makePlaceObject2({ depth: 1, move: true, txPx: 60, tyPx: 10 }),
    makeSwfTag(12, doActionF2),
    makeSwfTag(1, Buffer.alloc(0)), // ShowFrame 2
    makeSwfTag(43, Buffer.from('done\0', 'utf8')),
    makePlaceObject2({ depth: 2, charId: 3, txPx: 10, tyPx: 40, name: 'nestedSprite' }),
    makePlaceObject2({ depth: 3, charId: 4, txPx: 40, tyPx: 40, name: 'labelField' }),
    makeSwfTag(12, doActionF3),
    makeSwfTag(1, Buffer.alloc(0)), // ShowFrame 3
  ];

  const cwsAvm1 = wrapSwf('CWS', 8, 120, 80, 24, 3, avm1Tags);

  const baseProject = {
    format_version: 3,
    name: 'SWF Loader Host',
    width: 320,
    height: 240,
    fps: 30,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: [
      {
        id: 'res-cws',
        path: 'clips/avm1_anim.swf',
        media_type: 'application/x-shockwave-flash',
        data_base64: cwsAvm1.toString('base64'),
      },
    ],
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [{ id: 'l0', name: 'L0', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], label: '', tween: null }] }],
          scripts: [],
        },
      },
    ],
  };

  const player = new runtimeWasm.WebPlayer(JSON.stringify(baseProject));
  const { host, traceCalls } = createRuntimeHost(root, player, [], {
    project: baseProject,
    bundle: { project: baseProject },
  });

  try {
    const prog = host.program();
    const Loader = prog.type('flash.display.Loader');
    const MovieClip = prog.type('flash.display.MovieClip');
    const BitmapData = prog.type('flash.display.BitmapData');
    const URLRequest = prog.type('flash.net.URLRequest');
    const ByteArray = prog.type('flash.utils.ByteArray');
    const rootClip = host.proxyFor([]);

    // 1. Test Loader.load(URLRequest) with CWS (zlib) AVM1 multi-frame SWF
    const cwsLoader = new Loader();
    rootClip.addChild(cwsLoader);
    let cwsInit = false, cwsComplete = false, cwsUnloaded = false;
    cwsLoader.contentLoaderInfo.addEventListener('init', () => { cwsInit = true; });
    cwsLoader.contentLoaderInfo.addEventListener('complete', () => { cwsComplete = true; });
    cwsLoader.contentLoaderInfo.addEventListener('unload', () => { cwsUnloaded = true; });

    cwsLoader.load(new URLRequest('clips/avm1_anim.swf'));
    await new Promise(r => setTimeout(r, 25));

    assert.equal(cwsInit, true);
    assert.equal(cwsComplete, true);
    assert.equal(cwsLoader.contentLoaderInfo.contentType, 'application/x-shockwave-flash');
    assert.equal(cwsLoader.contentLoaderInfo.swfVersion, 8);
    assert.equal(cwsLoader.contentLoaderInfo.actionScriptVersion, 2);
    assert.equal(cwsLoader.contentLoaderInfo.frameRate, 24);
    assert.equal(cwsLoader.contentLoaderInfo.width, 120);
    assert.equal(cwsLoader.contentLoaderInfo.height, 80);
    assert.equal(cwsLoader.contentLoaderInfo.bytesLoaded, cwsAvm1.length);
    assert.equal(cwsLoader.contentLoaderInfo.bytesTotal, cwsAvm1.length);

    const mc1 = cwsLoader.content;
    assert.equal(prog.is(mc1, 'flash.display.MovieClip'), true);
    assert.equal(MovieClip.__nfIs?.(mc1) ?? prog.is(mc1, 'flash.display.MovieClip'), true);
    assert.equal(mc1.totalFrames, 3);
    assert.equal(mc1.currentFrame, 1);
    assert.equal(mc1.avm1State, 'f1', 'AVM1 DoAction frame 1 executed');
    assert.ok(traceCalls.some(row => row[0] === 'avm1-1'));

    // Verify frame 1 Stage rendering: red square at (10..40, 10..40)
    const snap1 = new BitmapData(120, 80, true, 0x00000000);
    snap1.draw(cwsLoader);
    assert.equal(snap1.getPixel32(20, 20) >>> 0, 0xffff0000, 'frame 1 red DefineShape rendered at (20,20)');
    assert.equal(snap1.getPixel32(70, 20) >>> 0, 0x00000000, 'frame 1 empty at (70,20)');

    // Advance via gotoAndStop(2) -> red square moves to (60..90, 10..40) and frame 2 DoAction runs
    mc1.gotoAndStop(2);
    host.dispatchScripts();
    assert.equal(mc1.currentFrame, 2);
    assert.equal(mc1.avm1State, 'f2');
    assert.ok(traceCalls.some(row => row[0] === 'avm1-2'));
    const snap2 = new BitmapData(120, 80, true, 0x00000000);
    snap2.draw(cwsLoader);
    assert.equal(snap2.getPixel32(20, 20) >>> 0, 0x00000000, 'frame 2 moved shape away from (20,20)');
    assert.equal(snap2.getPixel32(70, 20) >>> 0, 0xffff0000, 'frame 2 moved shape to (70,20)');

    // Jump by frame label gotoAndStop("done") -> frame 3 renders nested DefineSprite (blue) + DefineEditText
    mc1.gotoAndStop('done');
    host.dispatchScripts();
    assert.equal(mc1.currentFrame, 3);
    assert.equal(mc1.avm1State, 'f3');
    assert.ok(mc1.getChildByName('nestedSprite'), 'nested DefineSprite instance present');
    assert.equal(mc1.getChildByName('labelField').text, 'Hello SWF', 'DefineEditText decoded');
    const snap3 = new BitmapData(120, 80, true, 0x00000000);
    snap3.draw(cwsLoader);
    assert.equal(snap3.getPixel32(15, 45) >>> 0, 0xff0044ff, 'nested DefineSprite rendered blue square at (15,45)');

    // Test play() / stop() / gotoAndPlay(1)
    mc1.gotoAndPlay(2);
    assert.equal(mc1.currentFrame, 2);
    mc1.stop();

    // Unload cleanly via unloadAndStop(true)
    cwsLoader.unloadAndStop(true);
    assert.equal(cwsUnloaded, true);
    assert.equal(cwsLoader.content, null);
    assert.equal(cwsLoader.numChildren, 0);

    // 2. Test Loader.loadBytes(ByteArray) with ZWS (LZMA) AVM2 SWF (v13, DoABC2 + SymbolClass + addFrameScript)
    const symClassPayload = new BitWriter();
    symClassPayload.writeU16(1);
    symClassPayload.writeU16(0); // tag 0 = Document Class
    symClassPayload.writeCString('MainDoc');

    const avm2Tags = [
      makeSwfTag(69, Buffer.from([0x08, 0, 0, 0])), // FileAttributes: AS3=1
      makeSolidRectShapeTag(1, 24, 24, 0x00cc66),
      makePlaceObject2({ depth: 1, charId: 1, txPx: 8, tyPx: 8 }),
      makeAvm2DoAbc2Tag(),
      makeSwfTag(76, symClassPayload.toBuffer()), // SymbolClass
      makeSwfTag(1, Buffer.alloc(0)), // ShowFrame 1
      makePlaceObject2({ depth: 1, move: true, txPx: 48, tyPx: 8 }),
      makeSwfTag(1, Buffer.alloc(0)), // ShowFrame 2
    ];

    const compressToLzma = rawBuf => {
      const tmp = new ByteArray();
      for (const b of rawBuf) tmp.writeByte(b);
      tmp.compress('lzma');
      tmp.position = 0;
      const out = [];
      while (tmp.bytesAvailable > 0) out.push(tmp.readUnsignedByte());
      return Buffer.from(out);
    };

    const zwsAvm2 = wrapSwf('ZWS', 13, 100, 60, 30, 2, avm2Tags, compressToLzma);
    const zwsBa = new ByteArray();
    for (const b of zwsAvm2) zwsBa.writeByte(b);

    const zwsLoader = new Loader();
    rootClip.addChild(zwsLoader);
    zwsLoader.loadBytes(zwsBa);
    await new Promise(r => setTimeout(r, 25));

    assert.equal(zwsLoader.contentLoaderInfo.contentType, 'application/x-shockwave-flash');
    assert.equal(zwsLoader.contentLoaderInfo.swfVersion, 13);
    assert.equal(zwsLoader.contentLoaderInfo.actionScriptVersion, 3);
    assert.equal(zwsLoader.contentLoaderInfo.frameRate, 30);
    assert.equal(zwsLoader.contentLoaderInfo.width, 100);
    assert.equal(zwsLoader.contentLoaderInfo.height, 60);

    const mc2 = zwsLoader.content;
    assert.equal(prog.is(mc2, 'flash.display.MovieClip'), true);
    assert.equal(mc2.totalFrames, 2);
    assert.equal(mc2.currentFrame, 1);
    assert.equal(mc2.avm2Count, 7, 'AVM2 document constructor set avm2Count = 7');
    assert.ok(traceCalls.some(row => row[0] === 'avm2-ctor'));
    assert.ok(traceCalls.some(row => row[0] === 'avm2-f1'));

    const zSnap1 = new BitmapData(100, 60, true, 0);
    zSnap1.draw(zwsLoader);
    assert.equal(zSnap1.getPixel32(16, 16) >>> 0, 0xff00cc66);

    mc2.gotoAndStop(2);
    host.dispatchScripts();
    assert.equal(mc2.currentFrame, 2);
    assert.equal(mc2.avm2Count, 202, 'AVM2 addFrameScript frame 2 hook executed');
    assert.ok(traceCalls.some(row => row[0] === 'avm2-f2'));

    const zSnap2 = new BitmapData(100, 60, true, 0);
    zSnap2.draw(zwsLoader);
    assert.equal(zSnap2.getPixel32(56, 16) >>> 0, 0xff00cc66);

    zwsLoader.unload();
    assert.equal(zwsLoader.content, null);
    assert.equal(zwsLoader.numChildren, 0);

    for (const bmp of [snap1, snap2, snap3, zSnap1, zSnap2]) bmp.dispose();
  } finally {
    host.disposeFamily();
    player.free();
  }
});

test('SWF loader blocks constructor/__proto__/prototype sandbox escapes, escapes strict-mode control strings, surfaces warnings, and cleans up repeated loadBytes+unload cycles', async () => {
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');

  const avm1PushStr = str => {
    const b = Buffer.from(str + '\0', 'utf8');
    const out = Buffer.alloc(1 + 2 + 1 + b.length);
    out[0] = 0x96;
    out.writeUInt16LE(1 + b.length, 1);
    out[3] = 0;
    b.copy(out, 4);
    return out;
  };
  const avm1PushInt = n => {
    const out = Buffer.alloc(1 + 2 + 1 + 4);
    out[0] = 0x96;
    out.writeUInt16LE(5, 1);
    out[3] = 7; // integer type
    out.writeInt32LE(n, 4);
    return out;
  };

  // 1. AVM1 SWF attempting:
  //    - `[]["filter"]["constructor"]("globalThis.__avm1Escaped = 1; return 99;")()`
  //    - `({})["__proto__"].polluted = 1`
  //    - strict-mode octal-like control strings "\x001", "\x008", and "\u2028"
  //    - unsupported GetURL (0x83) opcode producing a deduplicated bundle warning
  const getUrlBody = Buffer.from('https://example.test/\0_blank\0', 'utf8');
  const getUrlOp = Buffer.alloc(3 + getUrlBody.length);
  getUrlOp[0] = 0x83;
  getUrlOp.writeUInt16LE(getUrlBody.length, 1);
  getUrlBody.copy(getUrlOp, 3);

  const avm1SandboxAction = Buffer.concat([
    // arr = [] (InitArray count=0 -> 0x42)
    avm1PushInt(0), Buffer.from([0x42]),
    // fn = arr["filter"] (GetMember -> 0x4e)
    avm1PushStr('filter'), Buffer.from([0x4e]),
    // ctor = fn["constructor"] -> MUST be undefined!
    avm1PushStr('constructor'), Buffer.from([0x4e]),
    avm1PushStr('avm1CtorLookup'), Buffer.from([0x3c]), // DefineLocal: var avm1CtorLookup = ... (Wait: stack order for 0x1d SetVariable is name, val)
    // Store via this.ctorBlocked = (([]["filter"]["constructor"]) === undefined)
    avm1PushStr('ctorVal'),
    avm1PushInt(0), Buffer.from([0x42]),
    avm1PushStr('filter'), Buffer.from([0x4e]),
    avm1PushStr('constructor'), Buffer.from([0x4e]),
    Buffer.from([0x1d]), // SetVariable: ctorVal = []["filter"]["constructor"]

    // Attempt `({})["__proto__"]["polluted"] = 1`:
    // obj = {} (InitObject count=0 -> 0x43), proto = obj["__proto__"], SetMember("polluted", 1)
    avm1PushInt(0), Buffer.from([0x43]),
    avm1PushStr('__proto__'), Buffer.from([0x4e]),
    avm1PushStr('polluted'), avm1PushInt(1), Buffer.from([0x4f]),

    // Strict-mode control strings ("\x01" + "1" = "\x011", "\u2028"): Note AVM1 C-strings cannot contain literal NUL,
    // so we test "\x078" + "\u2028" in AVM1 and "\x001" + "\x008" + "\u2028" in AVM2 UTF-8 pool!
    avm1PushStr('lineSepVal'), avm1PushStr('ok\u2028after'), Buffer.from([0x1d]),

    // Two GetURL opcodes -> must emit deduplicated warning
    getUrlOp,
    getUrlOp,
    Buffer.from([0x00]),
  ]);

  // Also test PlaceObject3 with blendMode=3 (multiply) for PlaceObject3 warning
  const po3 = new BitWriter();
  po3.writeU8(0x06); // flags1: HasMatrix | HasCharacter
  po3.writeU8(0x02); // flags2: HasBlendMode
  po3.writeU16(1);   // depth 1
  po3.writeU16(1);   // charId 1
  po3.writeU8(0);    // identity matrix (hasScale=0, hasRotate=0, nTranslateBits=0)
  po3.align();
  po3.writeU8(3);    // blend_mode = 3 (unsupported in SWF loader -> warning)

  const avm1Swf = wrapSwf('FWS', 8, 80, 60, 24, 1, [
    makeSwfTag(69, Buffer.from([0x00, 0, 0, 0])),
    makeSolidRectShapeTag(1, 10, 10, 0x112233),
    makeSwfTag(70, po3.toBuffer()), // PlaceObject3
    makeSwfTag(12, avm1SandboxAction),
    makeSwfTag(1, Buffer.alloc(0)),
  ]);

  // Verify decoded bundle warnings directly via decode_swf_bundle_json
  const decodedAvm1 = JSON.parse(runtimeWasm.decode_swf_bundle_json(avm1Swf));
  assert.ok(Array.isArray(decodedAvm1.warnings));
  assert.equal(decodedAvm1.warnings.filter(w => w.includes('GetURL')).length, 1, 'duplicate GetURL warnings are deduplicated');
  assert.ok(decodedAvm1.warnings.some(w => w.includes('PlaceObject3')), 'PlaceObject3 blendMode emits warning');

  // 2. Build an AVM2 SWF attempting:
  //    - `[]["filter"]["constructor"]("globalThis.__avm2Escaped = 1; return 77;")()`
  //    - `(function(){})["constructor"]` via 0x40 newfunction
  //    - `({})["__proto__"].polluted = 1`
  //    - strict-mode strings "\x001", "\x008", and "\u2028"
  //    - non-root SymbolClass (cid=1) warning
  function buildHardenedTestAvm2Tag() {
    const abc = [];
    abc.push(16, 0, 46, 0);
    writeU30(abc, 1); writeU30(abc, 1); writeU30(abc, 1);
    const strings = [
      '',
      '', // 1: empty package ns
      'SandboxDoc', // 2
      'Object', // 3
      'filter', // 4
      'constructor', // 5
      'return 77', // 6
      '__proto__', // 7
      'polluted', // 8
      'ctorResult', // 9
      'stubCtorResult', // 10
      'strictStr', // 11
      '\x001|\x008|\u2028|\u2029', // 12: strict-mode octal-like escapes + line separators!
    ];
    writeU30(abc, strings.length);
    for (let i = 1; i < strings.length; i++) {
      const b = Buffer.from(strings[i], 'utf8');
      writeU30(abc, b.length);
      for (const c of b) abc.push(c);
    }
    writeU30(abc, 2); abc.push(0x16); writeU30(abc, 1); // PackageNamespace("")
    writeU30(abc, 1); // ns_sets
    // multinames:
    // 1: SandboxDoc(2), 2: Object(3), 3: filter(4), 4: constructor(5), 5: __proto__(7),
    // 6: polluted(8), 7: ctorResult(9), 8: stubCtorResult(10), 9: strictStr(11)
    const mnStrIds = [2, 3, 4, 5, 7, 8, 9, 10, 11];
    writeU30(abc, 1 + mnStrIds.length);
    for (const sid of mnStrIds) {
      abc.push(0x07); writeU30(abc, 1); writeU30(abc, sid);
    }
    // methods: 4 (0: cinit, 1: iinit, 2: script_init, 3: dummyClosure)
    writeU30(abc, 4);
    for (let m = 0; m < 4; m++) {
      writeU30(abc, 0); writeU30(abc, 0); writeU30(abc, 0); abc.push(0);
    }
    writeU30(abc, 0); // metadata
    writeU30(abc, 1); // 1 class
    writeU30(abc, 1); writeU30(abc, 2); abc.push(0); writeU30(abc, 0); writeU30(abc, 1); writeU30(abc, 0);
    writeU30(abc, 0); writeU30(abc, 0);
    writeU30(abc, 1); writeU30(abc, 2); writeU30(abc, 0);

    const iinitCode = [
      // this.ctorResult = []["filter"]["constructor"]
      0xd0, // getlocal_0
      0x56, 0, // newarray(0) -> []
      0x66, 3, // getproperty("filter")
      0x66, 4, // getproperty("constructor") -> MUST be undefined
      0x61, 7, // setproperty("ctorResult")

      // this.stubCtorResult = (newfunction 3)["constructor"]
      0xd0, // getlocal_0
      0x40, 3, // newfunction(3) -> __nfSwfStubFn()
      0x66, 4, // getproperty("constructor") -> MUST be undefined
      0x61, 8, // setproperty("stubCtorResult")

      // ({})["__proto__"]["polluted"] = 1
      0x55, 0, // newobject(0) -> {}
      0x66, 5, // getproperty("__proto__") -> undefined
      0x24, 1, // pushbyte(1)
      0x61, 6, // setproperty("polluted") -> no-op via __nfSwfSet

      // this.strictStr = "\x001|\x008|\u2028|\u2029"
      0xd0,
      0x2c, 12, // pushstring(12)
      0x61, 9, // setproperty("strictStr")

      0x47, // returnvoid
    ];
    const dummyBody = [0x47];
    writeU30(abc, 2);
    for (const [mid, code] of [[1, iinitCode], [3, dummyBody]]) {
      writeU30(abc, mid); writeU30(abc, 8); writeU30(abc, 4); writeU30(abc, 0); writeU30(abc, 4);
      writeU30(abc, code.length);
      for (const b of code) abc.push(b);
      writeU30(abc, 0); writeU30(abc, 0);
    }
    const prefix = Buffer.from([1, 0, 0, 0, ...Buffer.from('sandbox\0', 'utf8')]);
    return makeSwfTag(82, Buffer.concat([prefix, Buffer.from(abc)]));
  }

  const symPayload = new BitWriter();
  symPayload.writeU16(2);
  symPayload.writeU16(0); symPayload.writeCString('SandboxDoc');
  symPayload.writeU16(1); symPayload.writeCString('NonRootLinkage');

  const avm2Swf = wrapSwf('FWS', 13, 80, 60, 30, 1, [
    makeSwfTag(69, Buffer.from([0x08, 0, 0, 0])),
    makeSolidRectShapeTag(1, 10, 10, 0x445566),
    buildHardenedTestAvm2Tag(),
    makeSwfTag(76, symPayload.toBuffer()),
    makeSwfTag(1, Buffer.alloc(0)),
  ]);

  const decodedAvm2 = JSON.parse(runtimeWasm.decode_swf_bundle_json(avm2Swf));
  assert.ok(decodedAvm2.warnings.some(w => w.includes('newfunction')), 'AVM2 newfunction closure emits warning');
  assert.ok(decodedAvm2.warnings.some(w => w.includes('NonRootLinkage')), 'non-root SymbolClass emits warning');

  const baseProj = {
    format_version: 3,
    name: 'Hardened SWF Host',
    width: 160,
    height: 120,
    fps: 30,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: [],
    symbols: [{
      id: 'root', name: 'Main', kind: 'movie_clip',
      timeline: { frame_count: 1, layers: [{ id: 'l0', name: 'L0', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], label: '', tween: null }] }], scripts: [] },
    }],
  };

  const player = new runtimeWasm.WebPlayer(JSON.stringify(baseProj));
  const functionsMap = new Map();
  const { host } = createRuntimeHost(root, player, [], {
    project: baseProj,
    bundle: { project: baseProj },
    functions: functionsMap,
  });

  try {
    const prog = host.program();
    const Loader = prog.type('flash.display.Loader');
    const ByteArray = prog.type('flash.utils.ByteArray');
    const rootClip = host.proxyFor([]);

    const toBa = buf => {
      const ba = new ByteArray();
      for (const b of buf) ba.writeByte(b);
      return ba;
    };

    // Execute AVM1 sandbox test SWF
    const l1 = new Loader();
    rootClip.addChild(l1);
    l1.loadBytes(toBa(avm1Swf));
    await new Promise(r => setTimeout(r, 20));
    assert.equal(l1.content.ctorVal, undefined, 'AVM1 []["filter"]["constructor"] must be blocked');
    assert.equal(({}).polluted, undefined, 'AVM1 ({})["__proto__"].polluted must not pollute Object.prototype');
    assert.equal(l1.content.lineSepVal, 'ok\u2028after', 'AVM1 \\u2028 string literal executes cleanly in strict mode');
    l1.unload();

    // Execute AVM2 sandbox + strict-mode control-string test SWF across repeated loadBytes + unload() cycles
    const l2 = new Loader();
    l2.contentLoaderInfo.addEventListener('ioError', e => { throw new Error('L2 ioError: ' + e.text); });
    rootClip.addChild(l2);
    for (let cycle = 0; cycle < 6; cycle++) {
      l2.loadBytes(toBa(avm2Swf));
      await new Promise(r => setTimeout(r, 15));
      assert.equal(l2.content.ctorResult, undefined, 'AVM2 []["filter"]["constructor"] must be blocked');
      assert.equal(l2.content.stubCtorResult, undefined, 'AVM2 newfunction stub .constructor must be undefined');
      assert.equal(({}).polluted, undefined, 'AVM2 ({})["__proto__"].polluted must not pollute Object.prototype');
      assert.equal(l2.content.strictStr, '\x001|\x008|\u2028|\u2029', 'AVM2 \\x001, \\x008, \\u2028, \\u2029 execute without strict SyntaxError');
      assert.ok(functionsMap.size > 0, 'mounted SWF registers compiled script');
      l2.unload();
      assert.equal(functionsMap.size, 0, `cycle ${cycle}: Loader.unload() deletes child functions entries`);
      assert.equal(baseProj.resources.length, 0, `cycle ${cycle}: Loader.unload() retires child resources`);
    }
  } finally {
    delete ({}).polluted;
    host.disposeFamily();
    player.free();
  }
});
