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
    makeSwfTag(59, Buffer.concat([Buffer.from([3, 0]), avm1PushStr('avm1-init'), Buffer.from([0x26, 0])])),
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
    makeSwfTag(43, Buffer.from('finish\0\x01', 'utf8')),
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
    mc1.gotoAndStop('finish');
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

    mc1.gotoAndStop('intro');
    host.dispatchScripts();
    assert.deepEqual(traceCalls.map(row => row[0]).filter(value => value === 'avm1-init' || value === 'avm1-1'),
      ['avm1-init', 'avm1-1', 'avm1-1'], 'init actions precede ordinary frame actions and run once per load');

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

    mc2.prevFrame();
    host.dispatchScripts();
    assert.equal(mc2.currentFrame, 1);
    assert.equal(mc2.avm2Count, 202, 'returning to frame 1 preserves instance state');
    assert.equal(traceCalls.filter(row => row[0] === 'avm2-ctor').length, 1);
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

    // GetURL opcode (now supported via hardened __nfSwfGetURL) + two unsupported 0x7f opcodes -> deduplicated warning
    getUrlOp,
    Buffer.from([0x7f, 0x7f]),
    Buffer.from([0x00]),
  ]);

  // Also test PlaceObject3 with unsupported blendMode=99 for PlaceObject3 warning deduplication
  const po3 = new BitWriter();
  po3.writeU8(0x06); // flags1: HasMatrix | HasCharacter
  po3.writeU8(0x02); // flags2: HasBlendMode
  po3.writeU16(1);   // depth 1
  po3.writeU16(1);   // charId 1
  po3.writeU8(0);    // identity matrix (hasScale=0, hasRotate=0, nTranslateBits=0)
  po3.align();
  po3.writeU8(99);   // invalid blend_mode = 99 (unsupported in SWF loader -> warning)

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
  assert.equal(decodedAvm1.warnings.filter(w => w.includes('0x7F')).length, 1, 'duplicate unsupported opcode warnings are deduplicated');
  assert.ok(decodedAvm1.warnings.some(w => w.includes('PlaceObject3 blendMode 99')), 'invalid PlaceObject3 blendMode emits warning');

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

      // this.stubCtorResult = (newfunction 3)["constructor"] (method 3 has body -> hardened __nfSwfFn)
      // plus bodiless newfunction(0) -> hardened stub warning
      0xd0, // getlocal_0
      0x40, 3, // newfunction(3) -> __nfSwfFn(...)
      0x66, 4, // getproperty("constructor") -> MUST be undefined
      0x61, 8, // setproperty("stubCtorResult")
      0x40, 0, // newfunction(0) -> bodiless method triggers warning & __nfSwfStubFn()
      0x29,    // pop

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

    // Reparenting loader.content to stage or another container before loader.unload() must keep
    // child frame scripts & resources alive until the reparented clip is finally removed.
    const repSymPayload = new BitWriter();
    repSymPayload.writeU16(1);
    repSymPayload.writeU16(0);
    repSymPayload.writeCString('MainDoc');
    const reparentSwf = wrapSwf('FWS', 13, 80, 60, 30, 2, [
      makeSwfTag(69, Buffer.from([0x08, 0, 0, 0])),
      makeSolidRectShapeTag(1, 12, 12, 0x2288cc),
      makePlaceObject2({ depth: 1, charId: 1, txPx: 4, tyPx: 4 }),
      makeAvm2DoAbc2Tag(),
      makeSwfTag(76, repSymPayload.toBuffer()),
      makeSwfTag(1, Buffer.alloc(0)), // frame 1
      makePlaceObject2({ depth: 1, txPx: 20, tyPx: 4, move: true }),
      makeSwfTag(1, Buffer.alloc(0)), // frame 2
    ]);

    const Sprite = prog.type('flash.display.Sprite');
    const holderSprite = new Sprite();
    rootClip.addChild(holderSprite);

    for (const targetParent of [rootClip, holderSprite]) {
      const l3 = new Loader();
      l3.contentLoaderInfo.addEventListener('ioError', e => { throw new Error('L3 ioError: ' + e.text); });
      rootClip.addChild(l3);
      l3.loadBytes(toBa(reparentSwf));
      await new Promise(r => setTimeout(r, 15));

      const reparented = l3.content;
      assert.equal(reparented.avm2Count, 7, 'reparented AVM2 SWF initialized on frame 1');
      targetParent.addChild(reparented);
      l3.unload();
      rootClip.removeChild(l3);

      assert.equal(l3.content, null);
      assert.ok(functionsMap.size > 0, 'reparented content retains compiled scripts after loader.unload()');

      reparented.gotoAndStop(2);
      host.dispatchScripts();
      assert.equal(reparented.currentFrame, 2);
      assert.equal(reparented.avm2Count, 202, 'reparented content executes frame 2 script after loader.unload()');

      targetParent.removeChild(reparented);
      assert.equal(functionsMap.size, 0, 'detaching reparented unloaded content releases child functions');
      assert.equal(baseProj.resources.length, 0, 'detaching reparented unloaded content releases child resources');
    }
  } finally {
    delete ({}).polluted;
    host.disposeFamily();
    player.free();
  }
});

// Exact SWF bytes published by `neoflash-export` in `crates/neoflash-export/src/swf/tests_t414.rs`:
// - `t416_avm1_function_expressions_nested_closures_try_fscommand_and_placeobject3`
// - `t416_avm2_nested_closures_super_symbolclass_sprite_and_abc_verifier_metrics`
const NEOFLASH_EXPORT_T416_AVM1_B64 =
  'RldTCFYHAABoABLAAAOEAAAYAQBEEQAAAABDAv///yEIAQBQAPAADwABANwoPP8AETQlxeAAOIAPBxiAAOIARAAA0gkCAAIAhgYGAQABAABAAEAAAACjESYHAQACABSgHgBiYWRnZQABAgDM//8AAAYAAAAGAAACIQQBPwPRBgAAjhkAbWFrZUNvdW50ZXIAAQACKAABc2VlZACpAJYIAABzdGVwQnkAjg8AAAEAAigAAWRlbHRhAFIAlggAAHNoYWRvdwCWBQAHAQAAADyWBwAAY291bnQAlgcAAGNvdW50AByWAgAEAUeWCAAAc2hhZG93ABxHlgUABwEAAAALHZYHAABjb3VudAAcPjyWBwAAY291bnQAlgIABAE8jgsAAAEAAigAAWQAGgCWAgAEAZYFAAcBAAAAlggAAHN0ZXBCeQA9Pj6OHABtYXBSZWR1Y2UAAgADKAABaXRlbXMAAmZuAKoAlgUAAGFjYwCWBQAHAAAAADyWBQAAaWR4AJYFAAcAAAAAPJYFAABpZHgAHJYCAAQBlggAAGxlbmd0aABOSBKdAgBdAJYFAABhY2MAlgUAAGFjYwAclgUAAGlkeAAclgIABAGWBQAAaWR4ABxOlgUABwIAAACWAgAEApYBAANSRx2WBQAAaWR4AJYFAABpZHgAHJYFAAcBAAAARx2ZAgCC/5YFAABhY2MAHD6WCAAAc2hhZG93AJYFAAeEAwAAHZYLAABhZGRPZmZzZXQAjg4AAAIAAygAAWEAAmIADACWAgAEAZYCAAQCRz4dlgYAAGZhY3QAjgsAAAEAAiAAAW4AcgCWCgAAc2VsZkZhY3QAlgsAAGFyZ3VtZW50cwAclggAAGNhbGxlZQBOPJYCAAQBlgUABwEAAABnEhKdAgAJAJYFAAcBAAAAPpYCAAQBlgIABAGWBQAHAQAAAAuWBQAHAQAAAJYKAABzZWxmRmFjdAA9DD4dlgQAAGMxAJYFAAcKAAAAlgUABwEAAACWDQAAbWFrZUNvdW50ZXIAPR2WBAAAcjEAlgUABwQAAACWBQAHAQAAAJYEAABjMQA9HZYEAAByMgCWBQAHBgAAAJYFAAcBAAAAlgQAAGMxAD0dlgcAAGNiU3VtAI4QAAACAAMoAAF2AAJpZHgAFQCWAgAEAZYCAAQClgUABwEAAABHDD6WBQAHAgAAAJYFAAcJAAAAlgUABwEAAACWBQAHBAAAAJYFAAcEAAAAQpYFAAcCAAAAlgsAAG1hcFJlZHVjZQA9HZYGAABudW1zAJYFAAcCAAAAlgUABwkAAACWBQAHAQAAAJYFAAcEAAAAlgUABwQAAABCHY4OAAACAAMoAAFhAAJiAAwAlgIABAKWAgAEAQs+lgUABwEAAACWBgAAbnVtcwAclgYAAHNvcnQAUheWBQAAcGtnAJYIAABXaWRnZXQAjgsAAAEAAigAAXYAIQCWBgAAdGhpcwAclgUAAHZhbACWAgAEAZYFAAcCAAAADE+WBgAAY2FsYwCOCwAAAQACKAABeAAPAJYCAAQBlgUABwcAAABHPpYFAAcCAAAAQx2WAwAAdwCWBQAHCAAAAJYFAAcBAAAAlgUAAHBrZwAclggAAFdpZGdldABTHZYJAABpaWZlVmFsAJYFAAcFAAAAlgUABwEAAACOCwAAAQACKAABawAPAJYCAAQBlgUABwMAAAAMPpYBAANSHZYIAAB0cnlMb2cAlgIAAAAdjwoAAyoAKAAgAGV4AJYIAAB0cnlMb2cAlggAAHRyeUxvZwAclgMAAFQARx2WBwAAZXJyNDIAKpYIAAB0cnlMb2cAlggAAHRyeUxvZwAclgMAADoAR5YEAABleAAcRx2WCAAAdHJ5TG9nAJYIAAB0cnlMb2cAHJYEAAA6RgBHHYMbAEZTQ29tbWFuZDphdm0xU3RhdHVzAHJlYWR5AIMqAGphdmFzY3JpcHQ6Z2xvYmFsVGhpcy5fX25mWHNzQXZtMT0xAF9zZWxmAJYGAAB0aGlzAByWEQAAZXhwb3J0ZWRDbG9zdXJlAJYEAABjMQAcT5YGAAB0aGlzAByWDAAAc29ydGVkTnVtcwCWBgAAbnVtcwAcT5YGAAB0aGlzAByWCQAAc3VtbWFyeQCWBQAHBAAAAJYFAAcDAAAAlgUABwIAAACWCwAAYWRkT2Zmc2V0AD2WAwAAfABHlgUABwUAAACWBQAHAQAAAJYGAABmYWN0AD1HlgMAAHwAR5YEAAByMQAcR5YDAAAsAEeWBAAAcjIAHEeWAwAALABHlggAAHNoYWRvdwAcR5YDAAB8AEeWBwAAY2JTdW0AHEeWAwAAfABHlgMAAHcAHJYFAAB2YWwATkeWAwAALABHlgUABwUAAACWBQAHAQAAAJYFAABwa2cAHJYGAABjYWxjAFJHlgMAACwAR5YJAABpaWZlVmFsABxHlgMAAHwAR5YIAAB0cnlMb2cAHEdPlgYAAHRoaXMAHJYJAABzdW1tYXJ5AE4mBwBAAAAA';

const NEOFLASH_EXPORT_T416_AVM2_B64 =
  'RldTCkMFAABoABLAAAOEAAAYAQBEEQgAAABDAv///+oUAQBYAEYAAMgAWABGAADIAAABACig3P8AETQlzRgADiADIc7oAA4gBOAA0gkCAAIAhgYGAQABAABAAEAAAAC/FJQEAAABAAAAZnJhbWUxABAALgAAAAAqAAZPYmplY3QOYWRkRnJhbWVTY3JpcHQIQmFzZUNhcmQNZmxhc2guZGlzcGxheQlNb3ZpZUNsaXAMZmxhc2guZXZlbnRzD0V2ZW50RGlzcGF0Y2hlcg1EaXNwbGF5T2JqZWN0EUludGVyYWN0aXZlT2JqZWN0FkRpc3BsYXlPYmplY3RDb250YWluZXIGU3ByaXRlA3RhZwVsYWJlbAJCKAEpBnByZWZpeARpbml0CUNoaWxkQ2FyZA1mcmFtZTJWaXNpdGVkBmZyYW1lMgRzdG9wBitDaGlsZAZzcHJpdGUCPkMHTWFpbkRvYw5jbG9zdXJlU3VtbWFyeQ1zcHJpdGVTdW1tYXJ5DWV4cG9ydGVkQWRkZXIFb3V0ZXIBeAVib251cwF5C3J1bkNhbGxiYWNrAmNiAXYBbghzZWxmRmFjdAEsBGNhcmQFdHJhY2UEFgEWBRYHABsHAQIHAQMHAQQHAgYHAwgHAgkHAgoHAgsHAgwHAQ0HAQ4HARMHARQHARUHARYHARoHARsHARwHAR0HAR4HAR8HASAHASIHASYHASgHASkPAAAAAAAAAAABAAAAgBEAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAACCHwEAAACAIQIAAAAAgCMkAAAAAgEAAACAJQEAAACAJQAAAAAAAwMEAAACAgoAAAAACwEAAQwDAAAGAw0AAAAADgEABAsBAAUQBAAACwURAAAAABIAAAAAEwAAAAAUAQAIFwEACgAAAwAHAAEOAwMEAQAMBAIBEAQDAg8AAwEICQPQMEcAAAEEAQkKDdAwLA/QZgqgLBCgSEcAAAIEAgkKD9Aw0EkA0CwSYQrQ0WEKRwAAAwMBCQoD0DBHAAAEBAEKCw3QMNAlTWENXQ9PDwBHAAAFBAEKCwvQMNBFCwAsF6BIRwAABgUBCgsf0DDQLBhJAV0CJQFgDk8CAtAlAGEN0NBmCiwZoGEKRwAABwMBCAkD0DBHAAAJBQILDBrQMGcKZwpsAiUBoG0CZwpsAdGgZwpsAqBIRwAACAQDCQsV0DBXKtYwZQHRbQFlASUCbQJACUhHAAIVAAEAABYAAgAACgcDCQoS0DDR0NJBAdHQ0iUBoEEBoEhHAAAMBAILDAjQMNElA6JIRwAADQcCCwwc0DDRJQGuEgMAACUBSNFnCmwB0NElAaFBAaJIRwAACwcHCQt+0DBXKmMGMNBJANAsAWER0CwBYRJgFNAlCkEBgtXR0CUFQQGC1tHQJQVBAYLXYBfQQAwlBEECgmMEQA0qZQErbQGCYwXQ0WET0NIsJ6DToCwnoGIEoCwnoGIF0CUFQQGgYRHQZhkgq5YSCQAA0NBmGUYLAGESXRrQZhFPGgFHAAEYAAEAAA4EAQEKc9AwZQBgATBgBTBgBjBgBzBgCDBgCTBgBDBgBFgAHR0dHR0dHWgDZQBgATBgBTBgBjBgBzBgCDBgCTBgBDBgAzBgA1gBHR0dHR0dHR1oDGUAYAEwYAUwYAYwYAcwYAgwYAkwYAQwYARYAh0dHR0dHR1oEEcAABgTAgACAENoaWxkQ2FyZAAAAE1haW5Eb2MAqhEmBwEAAgAUyCgAY2FyZAABABEiM+YAAAUAAAAFABDJAAAAAAQAgAEhAwFAAAAA';

test('T-416: Loader.loadBytes executes neoflash-export AVM1/AVM2 closures, DefineFunction/DefineFunction2 preload flags, With/Try/FSCommand, SymbolClass child sprites, PlaceObject3 filters/blends, and blocks Function constructor escapes', async () => {
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');

  const avm1ExportedSwf = Buffer.from(NEOFLASH_EXPORT_T416_AVM1_B64, 'base64');
  const avm2ExportedSwf = Buffer.from(NEOFLASH_EXPORT_T416_AVM2_B64, 'base64');

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
    const out = Buffer.alloc(8);
    out[0] = 0x96;
    out.writeUInt16LE(5, 1);
    out[3] = 7;
    out.writeInt32LE(n | 0, 4);
    return out;
  };
  const avm1PushReg = r => Buffer.from([0x96, 2, 0, 4, r & 0xff]);

  // Hand-crafted AVM1 SWF exercising:
  // - ActionDefineFunction (0x9B) named `v1Add(a, b)`
  // - ActionDefineFunction2 (0x8E) `BaseCls` & `SubCls` + ActionExtends (0x69) + `preload_this(0x01) | preload_arguments(0x04) | preload_super(0x10) | preload_root(0x40) | preload_parent(0x80) | preload_global(0x100)`
  // - ActionWith (0x94) reading & writing scope object + 20-deep nested ActionWith bounded at <= 16
  // - ActionTry (0x8F) catch-in-register + finally
  const buildAvm1LowLevelSwf = () => {
    // 1. 0x9B `function v1Add(a, b) { return a + b; }`
    const v1Body = Buffer.concat([
      avm1PushStr('a'), Buffer.from([0x1c]),
      avm1PushStr('b'), Buffer.from([0x1c]),
      Buffer.from([0x47, 0x3e]),
    ]);
    const v1Hdr = new BitWriter();
    v1Hdr.writeCString('v1Add');
    v1Hdr.writeU16(2);
    v1Hdr.writeCString('a');
    v1Hdr.writeCString('b');
    v1Hdr.writeU16(v1Body.length);
    const v1HdrBuf = v1Hdr.toBuffer();
    const defFn1 = Buffer.alloc(3 + v1HdrBuf.length + v1Body.length);
    defFn1[0] = 0x9b;
    defFn1.writeUInt16LE(v1HdrBuf.length, 1);
    v1HdrBuf.copy(defFn1, 3);
    v1Body.copy(defFn1, 3 + v1HdrBuf.length);

    // 2. 0x8E `BaseCls(seed)`: preload_this (0x01) -> reg1 = this, param seed in reg2 -> this.baseSeed = reg2
    const baseBody = Buffer.concat([
      avm1PushReg(1), avm1PushStr('baseSeed'), avm1PushReg(2), Buffer.from([0x4f]),
      Buffer.from([0x00]),
    ]);
    const baseHdr = new BitWriter();
    baseHdr.writeCString('BaseCls');
    baseHdr.writeU16(1);
    baseHdr.writeU8(4);      // register_count = 4
    baseHdr.writeU16(0x0001); // preload_this
    baseHdr.writeU8(2);
    baseHdr.writeCString('seed');
    baseHdr.writeU16(baseBody.length);
    const baseHdrBuf = baseHdr.toBuffer();
    const defBase = Buffer.alloc(3 + baseHdrBuf.length + baseBody.length);
    defBase[0] = 0x8e;
    defBase.writeUInt16LE(baseHdrBuf.length, 1);
    baseHdrBuf.copy(defBase, 3);
    baseBody.copy(defBase, 3 + baseHdrBuf.length);

    // 3. 0x8E `SubCls(x, y)` with preload_this(0x01) | preload_arguments(0x04) | preload_super(0x10) | preload_root(0x40) | preload_parent(0x80) | preload_global(0x100) = 0x01D5:
    //    reg1=this, reg2=arguments, reg3=super, reg4=_root, reg5=_parent, reg6=_global, reg7=x, reg8=y
    //    Calls `super(x + y)` -> stack: [x+y, 1, reg3, undefined] -> 0x52 CallMethod, Pop
    //    Sets `this.argCount = reg2.length`, `this.hasGlobal = (reg6 != undefined)`, `this.sameRoot = (reg4 == this || reg4 != undefined)`
    const subBody = Buffer.concat([
      avm1PushReg(7), avm1PushReg(8), Buffer.from([0x47]), // x + y
      avm1PushInt(1),
      avm1PushReg(3), // super
      Buffer.from([0x96, 1, 0, 3]), // undefined
      Buffer.from([0x52, 0x17]), // ActionCallMethod + Pop
      avm1PushReg(1), avm1PushStr('argCount'), avm1PushReg(2), avm1PushStr('length'), Buffer.from([0x4e, 0x4f]),
      avm1PushReg(6), avm1PushStr('sharedFlag'), avm1PushInt(321), Buffer.from([0x4f]),
      avm1PushReg(1), avm1PushStr('gotRoot'), avm1PushReg(4), Buffer.from([0x4f]),
      avm1PushReg(1), avm1PushStr('gotParent'), avm1PushReg(5), Buffer.from([0x4f]),
      Buffer.from([0x00]),
    ]);
    const subHdr = new BitWriter();
    subHdr.writeCString('SubCls');
    subHdr.writeU16(2);
    subHdr.writeU8(10);
    subHdr.writeU16(0x01d5);
    subHdr.writeU8(7); subHdr.writeCString('x');
    subHdr.writeU8(8); subHdr.writeCString('y');
    subHdr.writeU16(subBody.length);
    const subHdrBuf = subHdr.toBuffer();
    const defSub = Buffer.alloc(3 + subHdrBuf.length + subBody.length);
    defSub[0] = 0x8e;
    defSub.writeUInt16LE(subHdrBuf.length, 1);
    subHdrBuf.copy(defSub, 3);
    subBody.copy(defSub, 3 + subHdrBuf.length);

    // Extends: SubCls extends BaseCls (stack: SubCls, BaseCls -> 0x69)
    const doExtends = Buffer.concat([
      avm1PushStr('SubCls'), Buffer.from([0x1c]),
      avm1PushStr('BaseCls'), Buffer.from([0x1c]),
      Buffer.from([0x69]),
    ]);

    // ActionWith: obj = { score: 50 }; with (obj) { score = score + 25; } this.withScore = obj.score;
    const withInner = Buffer.concat([
      avm1PushStr('score'),
      avm1PushStr('score'), Buffer.from([0x1c]),
      avm1PushInt(25), Buffer.from([0x47]),
      Buffer.from([0x1d]),
    ]);
    const withOp = Buffer.alloc(5 + withInner.length);
    withOp[0] = 0x94;
    withOp.writeUInt16LE(2, 1);
    withOp.writeUInt16LE(withInner.length, 3);
    withInner.copy(withOp, 5);

    // 20 nested ActionWith blocks to verify <= 16 scope-stack cap (outermost i=19 `leaf` is at depth 0, innermost i=0 `k0` is at depth 19 >= 16!)
    let deepWith = Buffer.concat([
      avm1PushStr('deepOuter'), avm1PushStr('leaf'), Buffer.from([0x1c, 0x1d]),
      avm1PushStr('deepHit'), avm1PushStr('k0'), Buffer.from([0x1c, 0x1d]),
    ]);
    for (let i = 0; i < 20; i++) {
      const initObj = Buffer.concat([
        avm1PushStr(i === 19 ? 'leaf' : `k${i}`), avm1PushInt(i + 100), avm1PushInt(1), Buffer.from([0x43]),
      ]);
      const w = Buffer.alloc(5 + deepWith.length);
      w[0] = 0x94;
      w.writeUInt16LE(2, 1);
      w.writeUInt16LE(deepWith.length, 3);
      deepWith.copy(w, 5);
      deepWith = Buffer.concat([initObj, w]);
    }

    // ActionTry with catch_in_register (reg 3) + finally
    const tryB = Buffer.concat([avm1PushStr('regBoom'), Buffer.from([0x2a])]);
    const catchB = Buffer.concat([avm1PushStr('caughtReg'), avm1PushReg(3), Buffer.from([0x1d])]);
    const finB = Buffer.concat([avm1PushStr('finRan'), avm1PushInt(1), Buffer.from([0x1d])]);
    const tryHdr = Buffer.alloc(8);
    tryHdr[0] = 0x07; // has_catch | has_finally | catch_in_reg
    tryHdr.writeUInt16LE(tryB.length, 1);
    tryHdr.writeUInt16LE(catchB.length, 3);
    tryHdr.writeUInt16LE(finB.length, 5);
    tryHdr[7] = 3;
    const tryOp = Buffer.alloc(3 + tryHdr.length + tryB.length + catchB.length + finB.length);
    tryOp[0] = 0x8f;
    tryOp.writeUInt16LE(tryHdr.length, 1);
    tryHdr.copy(tryOp, 3);
    tryB.copy(tryOp, 3 + tryHdr.length);
    catchB.copy(tryOp, 3 + tryHdr.length + tryB.length);
    finB.copy(tryOp, 3 + tryHdr.length + tryB.length + catchB.length);

    // 4. `probeThis()`: returns [this["Reflect"], this["Object"], this["Function"]]
    const probeBody = Buffer.concat([
      avm1PushStr('this'), Buffer.from([0x1c]), avm1PushStr('Function'), Buffer.from([0x4e]),
      avm1PushStr('this'), Buffer.from([0x1c]), avm1PushStr('Object'), Buffer.from([0x4e]),
      avm1PushStr('this'), Buffer.from([0x1c]), avm1PushStr('Reflect'), Buffer.from([0x4e]),
      avm1PushInt(3), Buffer.from([0x42, 0x3e]),
    ]);
    const probeHdr = new BitWriter();
    probeHdr.writeCString('probeThis');
    probeHdr.writeU16(0);
    probeHdr.writeU16(probeBody.length);
    const probeHdrBuf = probeHdr.toBuffer();
    const defProbe = Buffer.alloc(3 + probeHdrBuf.length + probeBody.length);
    defProbe[0] = 0x9b;
    defProbe.writeUInt16LE(probeHdrBuf.length, 1);
    probeHdrBuf.copy(defProbe, 3);
    probeBody.copy(defProbe, 3 + probeHdrBuf.length);

    // 5. `retFinally()`: raw ActionTry { this.retFinLog = "T"; return 42; } finally { this.retFinLog = this.retFinLog + ":F"; }
    const rfTry = Buffer.concat([
      avm1PushStr('this'), Buffer.from([0x1c]), avm1PushStr('retFinLog'), avm1PushStr('T'), Buffer.from([0x4f]),
      avm1PushInt(42), Buffer.from([0x3e]), // ActionReturn directly inside try!
    ]);
    const rfFin = Buffer.concat([
      avm1PushStr('this'), Buffer.from([0x1c]), avm1PushStr('retFinLog'),
      avm1PushStr('this'), Buffer.from([0x1c]), avm1PushStr('retFinLog'), Buffer.from([0x4e]),
      avm1PushStr(':F'), Buffer.from([0x47, 0x4f]),
    ]);
    const rfTryHdr = Buffer.from([0x02, rfTry.length & 0xff, rfTry.length >>> 8, 0, 0, rfFin.length & 0xff, rfFin.length >>> 8, 0]);
    const rfTryOp = Buffer.alloc(3 + rfTryHdr.length + rfTry.length + rfFin.length);
    rfTryOp[0] = 0x8f;
    rfTryOp.writeUInt16LE(rfTryHdr.length, 1);
    rfTryHdr.copy(rfTryOp, 3);
    rfTry.copy(rfTryOp, 3 + rfTryHdr.length);
    rfFin.copy(rfTryOp, 3 + rfTryHdr.length + rfTry.length);
    const rfHdr = new BitWriter();
    rfHdr.writeCString('retFinally');
    rfHdr.writeU16(0);
    rfHdr.writeU16(rfTryOp.length);
    const rfHdrBuf = rfHdr.toBuffer();
    const defRetFinally = Buffer.alloc(3 + rfHdrBuf.length + rfTryOp.length);
    defRetFinally[0] = 0x9b;
    defRetFinally.writeUInt16LE(rfHdrBuf.length, 1);
    rfHdrBuf.copy(defRetFinally, 3);
    rfTryOp.copy(defRetFinally, 3 + rfHdrBuf.length);

    // Embedded tab in `java\tscript:` + `asfunction:` in ActionGetURL must both be blocked
    const tabXssBody = Buffer.from('java\tscript:globalThis.__nfXssTab=1\0_self\0', 'utf8');
    const tabXssOp = Buffer.alloc(3 + tabXssBody.length);
    tabXssOp[0] = 0x83;
    tabXssOp.writeUInt16LE(tabXssBody.length, 1);
    tabXssBody.copy(tabXssOp, 3);
    const asfnBody = Buffer.from('asfunction:v1Add,1\0\0', 'utf8');
    const asfnOp = Buffer.alloc(3 + asfnBody.length);
    asfnOp[0] = 0x83;
    asfnOp.writeUInt16LE(asfnBody.length, 1);
    asfnBody.copy(asfnOp, 3);

    const actions = Buffer.concat([
      defFn1,
      defBase,
      defSub,
      defProbe,
      defRetFinally,
      tabXssOp,
      asfnOp,
      doExtends,
      // this.v1Res = v1Add(11, 22)
      avm1PushStr('v1Res'), avm1PushInt(22), avm1PushInt(11), avm1PushInt(2), avm1PushStr('v1Add'), Buffer.from([0x3d, 0x1d]),
      // this.subInst = new SubCls(10, 15, 99)
      avm1PushStr('subInst'), avm1PushInt(99), avm1PushInt(15), avm1PushInt(10), avm1PushInt(3), avm1PushStr('SubCls'), Buffer.from([0x40, 0x1d]),
      // this.globalCheck = _global.sharedFlag
      avm1PushStr('globalCheck'), avm1PushStr('_global'), Buffer.from([0x1c]), avm1PushStr('sharedFlag'), Buffer.from([0x4e, 0x1d]),
      // ActionWith
      avm1PushStr('wObj'), avm1PushStr('score'), avm1PushInt(50), avm1PushInt(1), Buffer.from([0x43, 0x1d]),
      avm1PushStr('wObj'), Buffer.from([0x1c]), withOp,
      avm1PushStr('withScore'), avm1PushStr('wObj'), Buffer.from([0x1c]), avm1PushStr('score'), Buffer.from([0x4e, 0x1d]),
      deepWith,
      tryOp,
      Buffer.from([0x00]),
    ]);

    return wrapSwf('FWS', 8, 80, 60, 24, 1, [
      makeSwfTag(69, Buffer.from([0x00, 0, 0, 0])),
      makeSwfTag(12, actions),
      makeSwfTag(1, Buffer.alloc(0)),
    ]);
  };

  const baseProj = {
    format_version: 3,
    name: 'T416 Host',
    width: 160,
    height: 120,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: [],
    symbols: [{
      id: 'root', name: 'Main', kind: 'movie_clip',
      timeline: { frame_count: 1, layers: [{ id: 'l0', name: 'L0', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], label: '', tween: null }] }], scripts: [] },
    }],
  };

  const fsCommands = [];
  const player = new runtimeWasm.WebPlayer(JSON.stringify(baseProj));
  const functionsMap = new Map();
  const { host, traceCalls } = createRuntimeHost(root, player, [], {
    project: baseProj,
    bundle: { project: baseProj },
    functions: functionsMap,
    external: {
      hostGlobal: {
        DoFSCommand: (cmd, args) => {
          fsCommands.push([cmd, args]);
        },
      },
    },
  });

  try {
    const prog = host.program();
    const Loader = prog.type('flash.display.Loader');
    const ByteArray = prog.type('flash.utils.ByteArray');
    const BitmapData = prog.type('flash.display.BitmapData');
    const rootClip = host.proxyFor([]);

    const toBa = buf => {
      const ba = new ByteArray();
      for (const b of buf) ba.writeByte(b);
      return ba;
    };

    const assertHardenedSwfCallable = (fn, label) => {
      assert.equal(typeof fn, 'function', `${label}: must be callable`);
      assert.equal(fn.constructor, undefined, `${label}: .constructor must be undefined`);
      assert.equal(fn.prototype?.constructor, undefined, `${label}: .prototype.constructor must be undefined`);
      const proto = Object.getPrototypeOf(fn);
      assert.notEqual(proto, Function.prototype, `${label}: prototype must not be Function.prototype`);
      assert.equal(proto?.constructor, undefined, `${label}: proto.constructor must be undefined`);
      assert.equal(Object.getPrototypeOf(proto), null, `${label}: proto chain must terminate at null`);
      assert.equal(fn.call?.constructor, undefined, `${label}: .call.constructor must be undefined`);
      assert.equal(fn.apply?.constructor, undefined, `${label}: .apply.constructor must be undefined`);
      assert.equal(fn.bind?.constructor, undefined, `${label}: .bind.constructor must be undefined`);
    };

    // 1. Load & execute exact `neoflash-export` AVM1 SWF (`NEOFLASH_EXPORT_T416_AVM1_B64`)
    const loaderFsEvents = [];
    const lAvm1 = new Loader();
    lAvm1.contentLoaderInfo.addEventListener('ioError', e => { throw new Error('lAvm1 ioError: ' + e.text); });
    lAvm1.addEventListener('fscommand', e => { loaderFsEvents.push([e.command, e.args]); });
    rootClip.addChild(lAvm1);
    lAvm1.loadBytes(toBa(avm1ExportedSwf));
    await new Promise(r => setTimeout(r, 20));

    assert.equal(lAvm1.content.summary, '7|120|14,20,900|41|16,12,15|T:err42:F');
    assert.ok(traceCalls.some(row => row[0] === '7|120|14,20,900|41|16,12,15|T:err42:F'));
    assert.deepEqual([...lAvm1.content.sortedNums], [9, 4, 2, 1], 'AVM1 Array.sort(callback) sorted descending');
    assertHardenedSwfCallable(lAvm1.content.exportedClosure, 'AVM1 exportedClosure');
    assert.equal(lAvm1.content.exportedClosure(5), 25, 'AVM1 lexical closure retains and updates captured state');
    assert.deepEqual(fsCommands, [['avm1Status', 'ready']], 'ActionGetURL FSCommand routed to DoFSCommand');
    assert.deepEqual(loaderFsEvents, [['avm1Status', 'ready']], 'ActionGetURL FSCommand dispatched on Loader');
    assert.equal(globalThis.__nfXssAvm1, undefined, 'javascript: URL in ActionGetURL must NEVER execute');

    // Verify PlaceObject3 GlowFilter + Screen blendMode + cacheAsBitmap on `badge` child sprite
    const badgeClip = lAvm1.content.getChildByName('badge');
    assert.ok(badgeClip, 'AVM1 nested DefineSprite `badge` placed via PlaceObject3');
    assert.equal(badgeClip.blendMode, 'screen');
    assert.equal(badgeClip.cacheAsBitmap, true);
    assert.equal(badgeClip.filters.length, 1);

    const bmp1 = new BitmapData(80, 60, true, 0xffffffff);
    bmp1.draw(lAvm1);
    assert.notEqual(bmp1.getPixel32(24, 20) >>> 0, 0xffffffff, 'AVM1 PlaceObject3 filtered sprite rendered');
    bmp1.dispose();
    lAvm1.unload();

    // 2. Load & execute low-level AVM1 DefineFunction + DefineFunction2 preload flags + Extends/super + With (<= 16) + Try (catch in reg)
    const lLow = new Loader();
    lLow.contentLoaderInfo.addEventListener('ioError', e => { throw new Error('lLow ioError: ' + e.text); });
    rootClip.addChild(lLow);
    lLow.loadBytes(toBa(buildAvm1LowLevelSwf()));
    await new Promise(r => setTimeout(r, 20));

    assert.equal(lLow.content.v1Res, 33, 'ActionDefineFunction (0x9B) named function executed');
    assert.equal(lLow.content.subInst.baseSeed, 25, 'ActionDefineFunction2 preload_super invoked BaseCls(x + y)');
    assert.equal(lLow.content.subInst.argCount, 3, 'ActionDefineFunction2 preload_arguments captured arguments.length');
    assert.equal(lLow.content.globalCheck, 321, 'ActionDefineFunction2 preload_global shared _global object');
    assert.equal(lLow.content.subInst.gotRoot, rootClip, 'ActionDefineFunction2 preload_root bound _root');
    assert.equal(lLow.content.withScore, 75, 'ActionWith (0x94) read and mutated scope object property');
    assert.equal(lLow.content.deepOuter, 119, 'Outer ActionWith within <= 16 depth resolves');
    assert.equal(lLow.content.deepHit, undefined, '20-deep nested ActionWith capped scope stack at <= 16');
    assert.equal(lLow.content.caughtReg, 'regBoom', 'ActionTry (0x8F) catch-in-register captured thrown value');
    assert.equal(lLow.content.finRan, 1, 'ActionTry (0x8F) finally block executed');
    assert.equal(lLow.content.retFinally(), 42, 'ActionReturn inside ActionTry returns expected value');
    assert.equal(lLow.content.retFinLog, 'T:F', 'ActionReturn inside ActionTry unwinds enclosing finally block before returning');
    assert.equal(globalThis.__nfXssTab, undefined, 'Embedded control char java\\tscript: URL in ActionGetURL must be blocked');
    // P0 regression: invoking child AVM1 closure with `this === globalThis` or `{ length: 1e9 }`
    assert.deepEqual(Reflect.apply(lLow.content.probeThis, globalThis, []), [undefined, undefined, undefined], 'AVM1 closure sanitizes this === globalThis');
    assert.deepEqual(lLow.content.probeThis.call(globalThis), [undefined, undefined, undefined], 'AVM1 fn.call(globalThis) sanitizes receiver');
    assert.deepEqual(lLow.content.probeThis.apply(globalThis, { length: 1e9 }), [undefined, undefined, undefined], 'AVM1 fn.apply clamps huge array-like length without OOM');
    lLow.unload();

    // 3. Load & execute exact `neoflash-export` AVM2 SWF (`NEOFLASH_EXPORT_T416_AVM2_B64`)
    const lAvm2 = new Loader();
    lAvm2.contentLoaderInfo.addEventListener('ioError', e => { throw new Error('lAvm2 ioError: ' + e.text); });
    rootClip.addChild(lAvm2);
    lAvm2.loadBytes(toBa(avm2ExportedSwf));
    await new Promise(r => setTimeout(r, 20));

    assert.equal(lAvm2.content.closureSummary, '18,19,27,120', 'AVM2 outer/inner activation closure + callback + self-named recursive closure');
    assertHardenedSwfCallable(lAvm2.content.exportedAdder, 'AVM2 exportedAdder');
    assert.equal(lAvm2.content.exportedAdder(5), 20, 'AVM2 closure mutates outer activation slot across invocations');
    assert.equal(Reflect.apply(lAvm2.content.exportedAdder, globalThis, [5]), 21, 'AVM2 closure invoked via Reflect.apply(..., globalThis, ...) executes safely');
    assert.equal(lAvm2.content.exportedAdder.apply(globalThis, { length: 1e9, 0: 5 }), 22, 'AVM2 fn.apply clamps { length: 1e9 } without OOM');

    // Non-root SymbolClass child sprite (`ChildCard extends BaseCard`) + PlaceObject3 DropShadow & Multiply blend
    assert.equal(lAvm2.content.spriteSummary, 'B(sprite>C)+Child', 'ChildCard SymbolClass attached before MainDoc ctor with constructsuper + callsuper');
    const cardSprite = lAvm2.content.getChildByName('card');
    assert.ok(cardSprite, 'ChildCard sprite instance `card` exists on child root');
    assert.equal(cardSprite.label(), 'B(sprite>C)+Child');
    assert.equal(cardSprite.blendMode, 'multiply');
    assert.equal(cardSprite.cacheAsBitmap, true);
    assert.equal(cardSprite.filters.length, 1);
    assert.equal(cardSprite.frame2Visited, 0);

    // Render PlaceObject3 DropShadow + Multiply sprite via BitmapData.draw
    const bmp2 = new BitmapData(80, 60, true, 0xffffffff);
    bmp2.draw(lAvm2);
    assert.notEqual(bmp2.getPixel32(30, 24) >>> 0, 0xffffffff, 'AVM2 PlaceObject3 DropShadow + Multiply child sprite rendered');
    bmp2.dispose();

    // Advance ChildCard to frame 2 -> non-root SymbolClass `addFrameScript` hook executes!
    cardSprite.gotoAndStop(2);
    host.dispatchScripts();
    assert.equal(cardSprite.currentFrame, 2);
    assert.equal(cardSprite.frame2Visited, 77, 'Non-root SymbolClass DefineSprite executed frame 2 addFrameScript');

    lAvm2.unload();
    assert.equal(functionsMap.size, 0, 'Loader.unload() cleaned up all child SWF scripts');

    // 4. Non-root-only SymbolClass (`cid = 2 -> ChildOnly`, NO `cid = 0`) + AVM2 `Reflect.apply(fn, globalThis, [])` probe
    const buildChildOnlyAvm2Swf = () => {
      const spriteHdr = Buffer.alloc(4);
      spriteHdr.writeUInt16LE(2, 0); // charId = 2
      spriteHdr.writeUInt16LE(2, 2); // 2 frames
      const spriteTag = makeSwfTag(39, Buffer.concat([
        spriteHdr,
        makeSwfTag(1, Buffer.alloc(0)),
        makeSwfTag(1, Buffer.alloc(0)),
        makeSwfTag(0, Buffer.alloc(0)),
      ]));

      const abc = [16, 0, 46, 0];
      writeU30(abc, 1); writeU30(abc, 1); writeU30(abc, 1);
      const strs = ['', '', 'ChildOnly', 'MovieClip', 'addFrameScript', 'childConstructed', 'childFrame2', 'probeThis', 'Reflect', 'Object', 'Function'];
      writeU30(abc, strs.length);
      for (let i = 1; i < strs.length; i++) {
        const b = Buffer.from(strs[i], 'utf8');
        writeU30(abc, b.length);
        for (const c of b) abc.push(c);
      }
      writeU30(abc, 2); abc.push(0x16); writeU30(abc, 1);
      writeU30(abc, 1);
      // mn: 1=ChildOnly(2), 2=MovieClip(3), 3=addFrameScript(4), 4=childConstructed(5), 5=childFrame2(6), 6=probeThis(7), 7=Reflect(8), 8=Object(9), 9=Function(10)
      writeU30(abc, 10);
      for (let sid = 2; sid <= 10; sid++) {
        abc.push(0x07); writeU30(abc, 1); writeU30(abc, sid);
      }
      // methods: 0=cinit, 1=iinit, 2=frame2Fn, 3=probeFn, 4=script_init
      writeU30(abc, 5);
      for (let m = 0; m < 5; m++) {
        writeU30(abc, 0); writeU30(abc, 0); writeU30(abc, 0); abc.push(0);
      }
      writeU30(abc, 0);
      writeU30(abc, 1); // 1 class: ChildOnly
      writeU30(abc, 1); writeU30(abc, 2); abc.push(0); writeU30(abc, 0); writeU30(abc, 1); writeU30(abc, 0);
      writeU30(abc, 0); writeU30(abc, 0);
      writeU30(abc, 1); writeU30(abc, 4); writeU30(abc, 0);

      const iinitCode = [
        0xd0, 0x30,
        0xd0, 0x49, 0, // constructsuper 0
        0xd0, 0x24, 1, 0x61, 4, // this.childConstructed = 1
        0xd0, 0x40, 3, 0x61, 6, // this.probeThis = newfunction(3)
        0x5d, 3, 0x24, 1, 0x40, 2, 0x4f, 3, 2, // addFrameScript(1, newfunction(2))
        0x47,
      ];
      const frame2Code = [0xd0, 0x24, 88, 0x61, 5, 0x47]; // this.childFrame2 = 88
      // probeFn: return [this["Reflect"], this["Object"], this["Function"]]
      const probeCode = [
        0xd0, 0x66, 7,
        0xd0, 0x66, 8,
        0xd0, 0x66, 9,
        0x56, 3,
        0x48,
      ];
      writeU30(abc, 3);
      for (const [mid, code] of [[1, iinitCode], [2, frame2Code], [3, probeCode]]) {
        writeU30(abc, mid); writeU30(abc, 6); writeU30(abc, 4); writeU30(abc, 4); writeU30(abc, 6);
        writeU30(abc, code.length);
        for (const b of code) abc.push(b);
        writeU30(abc, 0); writeU30(abc, 0);
      }
      const doAbc2 = makeSwfTag(82, Buffer.concat([Buffer.from([1, 0, 0, 0, ...Buffer.from('co\0', 'utf8')]), Buffer.from(abc)]));
      const sym = new BitWriter();
      sym.writeU16(1);
      sym.writeU16(2); sym.writeCString('ChildOnly'); // NO cid 0!

      return wrapSwf('FWS', 13, 80, 60, 24, 2, [
        makeSwfTag(69, Buffer.from([0x08, 0, 0, 0])),
        spriteTag,
        makePlaceObject2({ depth: 1, charId: 2, txPx: 4, tyPx: 4, name: 'onlyChild' }),
        doAbc2,
        makeSwfTag(76, sym.toBuffer()),
        makeSwfTag(1, Buffer.alloc(0)),
        makeSwfTag(1, Buffer.alloc(0)),
      ]);
    };

    const lChildOnly = new Loader();
    lChildOnly.contentLoaderInfo.addEventListener('ioError', e => { throw new Error('lChildOnly ioError: ' + e.text); });
    rootClip.addChild(lChildOnly);
    lChildOnly.loadBytes(toBa(buildChildOnlyAvm2Swf()));
    await new Promise(r => setTimeout(r, 20));

    assert.equal(lChildOnly.content.childConstructed, undefined, 'Root timeline without cid=0 SymbolClass must NOT construct ChildOnly');
    const onlyChild = lChildOnly.content.getChildByName('onlyChild');
    assert.ok(onlyChild, 'onlyChild sprite placed on root');
    assert.equal(onlyChild.childConstructed, 1, 'ChildOnly constructed exclusively on cid=2 child sprite');
    assert.deepEqual(Reflect.apply(onlyChild.probeThis, globalThis, []), [undefined, undefined, undefined], 'AVM2 closure sanitizes this === globalThis');
    assert.deepEqual(onlyChild.probeThis.call(globalThis), [undefined, undefined, undefined], 'AVM2 fn.call(globalThis) sanitizes receiver');
    assert.deepEqual(onlyChild.probeThis.apply(globalThis, { length: 1e9 }), [undefined, undefined, undefined], 'AVM2 probeThis.apply clamps { length: 1e9 }');

    lChildOnly.content.gotoAndStop(2);
    onlyChild.gotoAndStop(2);
    host.dispatchScripts();
    assert.equal(lChildOnly.content.childFrame2, undefined, 'Root frame 2 must NOT run ChildOnly addFrameScript');
    assert.equal(onlyChild.childFrame2, 88, 'cid=2 ChildOnly sprite ran its frame 2 addFrameScript');

    lChildOnly.unload();
    assert.equal(functionsMap.size, 0, 'Loader.unload() cleaned up ChildOnly scripts');
  } finally {
    delete globalThis.__nfXssAvm1;
    delete globalThis.__nfXssTab;
    host.disposeFamily();
    player.free();
  }
});

test('T-451: Loader.load & Loader.loadBytes match Flash Player trace oracles on upstream Ruffle AVM1/AVM2 SWFs, render MX morph/button/text SWFs & dynamic graphics, and sniff JPEG XR', async () => {
  const fs = require('node:fs');
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');

  const ruffleDir = resolve(root, 'examples/fla-corpus/upstream/ruffle');
  const avm1FrameLabelsSwf = fs.readFileSync(resolve(ruffleDir, 'avm1-frame-labels/test.swf'));

  const baseProject = {
    format_version: 3,
    name: 'T451 SWF Oracle Host',
    width: 550,
    height: 400,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: [
      {
        id: 'res-ruffle-labels',
        path: 'ruffle/avm1-frame-labels.swf',
        media_type: 'application/x-shockwave-flash',
        data_base64: avm1FrameLabelsSwf.toString('base64'),
      },
    ],
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 16,
          layers: [{ id: 'l0', name: 'L0', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], label: '', tween: null }] }],
          scripts: [],
        },
      },
    ],
  };

  const functionsMap = new Map();
  const player = new runtimeWasm.WebPlayer(JSON.stringify(baseProject));
  const { host, traceCalls } = createRuntimeHost(root, player, [], {
    project: baseProject,
    bundle: { project: baseProject },
    functions: functionsMap,
  });

  try {
    const prog = host.program();
    const Loader = prog.type('flash.display.Loader');
    const BitmapData = prog.type('flash.display.BitmapData');
    const URLRequest = prog.type('flash.net.URLRequest');
    const ByteArray = prog.type('flash.utils.ByteArray');
    const rootClip = host.proxyFor([]);

    const toBa = buf => {
      const ba = new ByteArray();
      for (const b of buf) ba.writeByte(b);
      return ba;
    };

    const stepHostTick = () => {
      host.broadcastFrame();
      player.tick();
      host.dispatchScripts();
    };

    // 1. Upstream Ruffle AVM1 & AVM2 Flash Player trace oracle suite
    const oracleCases = [
      { name: 'avm1-frame-labels', ticks: 4, expectedCount: 17, viaUrl: 'ruffle/avm1-frame-labels.swf' },
      { name: 'avm1-clip-events', ticks: 4, expectedCount: 19, viaUrl: null },
      { name: 'avm1-nested-button-text', ticks: 1, expectedCount: 0, viaUrl: null },
      { name: 'avm2-button-frame-scripts', ticks: 2, expectedCount: 28, viaUrl: null },
      { name: 'avm2-nested-construction', ticks: 3, expectedCount: 18, viaUrl: null },
    ];

    for (const tc of oracleCases) {
      traceCalls.length = 0;
      const swfBuf = fs.readFileSync(resolve(ruffleDir, tc.name, 'test.swf'));
      const expectedLines = fs
        .readFileSync(resolve(ruffleDir, tc.name, 'output.txt'), 'utf8')
        .split(/\r?\n/)
        .filter((line, idx, arr) => !(idx === arr.length - 1 && line === ''));
      assert.equal(expectedLines.length, tc.expectedCount, `oracle line count for ${tc.name}`);

      const loader = new Loader();
      loader.contentLoaderInfo.addEventListener('ioError', e => {
        throw new Error(`${tc.name} ioError: ${e.text}`);
      });
      rootClip.addChild(loader);
      if (tc.viaUrl) {
        loader.load(new URLRequest(tc.viaUrl));
      } else {
        loader.loadBytes(toBa(swfBuf));
      }
      await new Promise(r => setTimeout(r, 25));

      // Initial load executes tick 1 (frame 1); step remaining ticks - 1
      for (let t = 1; t < tc.ticks; t++) {
        stepHostTick();
      }

      const actualLines = traceCalls.map(row => row.map(v => String(v)).join(' '));
      assert.deepEqual(
        actualLines,
        expectedLines,
        `HTML5 swf_loader trace mismatch against Flash Player oracle on ${tc.name}`,
      );

      if (tc.name === 'avm1-nested-button-text') {
        const snap = new BitmapData(200, 150, true, 0xffffffff);
        snap.draw(loader);
        let nonWhite = 0;
        for (let y = 0; y < 150; y += 2) {
          for (let x = 0; x < 200; x += 2) {
            if ((snap.getPixel32(x, y) >>> 0) !== 0xffffffff) nonWhite++;
          }
        }
        snap.dispose();
        assert.ok(nonWhite > 0, 'avm1-nested-button-text rendered visible button/text pixels in HTML5');
      }

      loader.unloadAndStop(true);
      rootClip.removeChild(loader);
      assert.equal(functionsMap.size, 0, `unloadAndStop cleaned up all scripts for ${tc.name}`);
    }

    // 2. Visual MX SWFs: DefineMorphShape-MX, DefineButton-MX, DefineText2-MX
    const morphBuf = fs.readFileSync(resolve(ruffleDir, 'mx-shape-tween/DefineMorphShape-MX.swf'));
    const lMorph = new Loader();
    rootClip.addChild(lMorph);
    lMorph.loadBytes(toBa(morphBuf));
    await new Promise(r => setTimeout(r, 20));

    const captureSignature = targetLoader => {
      const w = Math.min(240, Math.max(32, targetLoader.contentLoaderInfo.width | 0));
      const h = Math.min(200, Math.max(32, targetLoader.contentLoaderInfo.height | 0));
      const bmp = new BitmapData(w, h, true, 0xffffffff);
      bmp.draw(targetLoader);
      let fg = 0;
      let hash = 2166136261 >>> 0;
      for (let y = 0; y < h; y += 2) {
        for (let x = 0; x < w; x += 2) {
          const px = bmp.getPixel32(x, y) >>> 0;
          if (px !== 0xffffffff) fg++;
          hash = Math.imul(hash ^ px, 16777619) >>> 0;
        }
      }
      bmp.dispose();
      return { fg, hash };
    };

    const morphF1 = captureSignature(lMorph);
    assert.ok(morphF1.fg > 0, 'DefineMorphShape-MX frame 1 rendered foreground pixels');
    for (let i = 0; i < 5; i++) stepHostTick();
    const morphF6 = captureSignature(lMorph);
    assert.ok(morphF6.fg > 0, 'DefineMorphShape-MX frame 6 rendered foreground pixels');
    assert.notEqual(morphF1.hash, morphF6.hash, 'DefineMorphShape-MX visibly morphed across frames');
    lMorph.unloadAndStop(true);
    rootClip.removeChild(lMorph);

    for (const rel of ['mx-button/DefineButton-MX.swf', 'mx-static-text/DefineText2-MX.swf']) {
      const lMx = new Loader();
      rootClip.addChild(lMx);
      lMx.loadBytes(toBa(fs.readFileSync(resolve(ruffleDir, rel))));
      await new Promise(r => setTimeout(r, 20));
      const sig = captureSignature(lMx);
      assert.ok(sig.fg > 0, `${rel} rendered foreground pixels in HTML5 swf_loader`);
      lMx.unload();
      rootClip.removeChild(lMx);
    }

    // 3. Dynamic AVM2 `graphics` (`beginFill`, `moveTo`, `lineTo`, `curveTo`, `drawRect`, `drawCircle`, `endFill`, `clear`) in child SWF
    const buildDynamicGraphicsAvm2Swf = () => {
      const abc = [16, 0, 46, 0];
      writeU30(abc, 1); writeU30(abc, 1);
      // doubles: [1]=0.0, [2]=1.0, [3]=40.0, [4]=10.0
      writeU30(abc, 5);
      for (const dv of [0.0, 1.0, 40.0, 10.0]) {
        const db = Buffer.alloc(8);
        db.writeDoubleLE(dv, 0);
        for (const b of db) abc.push(b);
      }
      const strs = [
        '', '', 'GfxDoc', 'MovieClip', 'graphics', 'beginFill', 'drawRect',
        'drawCircle', 'moveTo', 'lineTo', 'curveTo', 'endFill', 'clear', 'wipe',
      ];
      writeU30(abc, strs.length);
      for (let i = 1; i < strs.length; i++) {
        const b = Buffer.from(strs[i], 'utf8');
        writeU30(abc, b.length);
        for (const c of b) abc.push(c);
      }
      writeU30(abc, 2); abc.push(0x16); writeU30(abc, 1);
      writeU30(abc, 1);
      // mn 1..12 => strs 2..13
      writeU30(abc, 13);
      for (let sid = 2; sid <= 13; sid++) {
        abc.push(0x07); writeU30(abc, 1); writeU30(abc, sid);
      }
      // methods: 0=cinit, 1=iinit, 2=wipeFn, 3=script_init
      writeU30(abc, 4);
      for (let m = 0; m < 4; m++) {
        writeU30(abc, 0); writeU30(abc, 0); writeU30(abc, 0); abc.push(0);
      }
      writeU30(abc, 0);
      writeU30(abc, 1);
      writeU30(abc, 1); writeU30(abc, 2); abc.push(0); writeU30(abc, 0); writeU30(abc, 1); writeU30(abc, 0);
      writeU30(abc, 0); writeU30(abc, 0);
      writeU30(abc, 1); writeU30(abc, 3); writeU30(abc, 0);

      // iinit:
      //   super();
      //   this.graphics.beginFill(255, 1.0);
      //   this.graphics.drawRect(0, 0, 40, 40);
      //   this.graphics.drawCircle(60, 20, 10);
      //   this.graphics.endFill();
      //   this.wipe = newfunction(2);
      const iinitCode = [
        0xd0, 0x30,
        0xd0, 0x49, 0,
        0xd0, 0x66, 3, 0x25, 0xff, 0x01, 0x2f, 2, 0x4f, 4, 2, // graphics.beginFill(255, 1.0)
        0xd0, 0x66, 3, 0x2f, 1, 0x2f, 1, 0x2f, 3, 0x2f, 3, 0x4f, 5, 4, // graphics.drawRect(0,0,40,40)
        0xd0, 0x66, 3, 0x24, 60, 0x24, 20, 0x2f, 4, 0x4f, 6, 3, // graphics.drawCircle(60,20,10)
        0xd0, 0x66, 3, 0x4f, 10, 0, // graphics.endFill()
        0xd0, 0x40, 2, 0x61, 12, // this.wipe = newfunction(2)
        0x47,
      ];
      // wipeFn: this.graphics.clear()
      const wipeCode = [0xd0, 0x66, 3, 0x4f, 11, 0, 0x47];
      writeU30(abc, 2);
      for (const [mid, code] of [[1, iinitCode], [2, wipeCode]]) {
        writeU30(abc, mid); writeU30(abc, 8); writeU30(abc, 4); writeU30(abc, 0); writeU30(abc, 4);
        writeU30(abc, code.length);
        for (const b of code) abc.push(b);
        writeU30(abc, 0); writeU30(abc, 0);
      }
      const doAbc2 = makeSwfTag(82, Buffer.concat([Buffer.from([1, 0, 0, 0, ...Buffer.from('g\0', 'utf8')]), Buffer.from(abc)]));
      const sym = new BitWriter();
      sym.writeU16(1); sym.writeU16(0); sym.writeCString('GfxDoc');
      return wrapSwf('FWS', 13, 100, 60, 24, 1, [
        makeSwfTag(69, Buffer.from([0x08, 0, 0, 0])),
        doAbc2,
        makeSwfTag(76, sym.toBuffer()),
        makeSwfTag(1, Buffer.alloc(0)),
      ]);
    };

    const lGfx = new Loader();
    rootClip.addChild(lGfx);
    lGfx.loadBytes(toBa(buildDynamicGraphicsAvm2Swf()));
    await new Promise(r => setTimeout(r, 20));

    const gfxSnap1 = new BitmapData(100, 60, true, 0xffffffff);
    gfxSnap1.draw(lGfx);
    assert.equal(gfxSnap1.getPixel32(20, 20) >>> 0, 0xff0000ff, 'dynamic AVM2 drawRect rendered blue pixel');
    assert.equal(gfxSnap1.getPixel32(60, 20) >>> 0, 0xff0000ff, 'dynamic AVM2 drawCircle rendered blue pixel');
    gfxSnap1.dispose();

    lGfx.content.wipe.call(lGfx.content);
    const gfxSnap2 = new BitmapData(100, 60, true, 0xffffffff);
    gfxSnap2.draw(lGfx);
    assert.equal(gfxSnap2.getPixel32(20, 20) >>> 0, 0xffffffff, 'dynamic AVM2 graphics.clear() cleared Stage');
    gfxSnap2.dispose();
    lGfx.unload();
    rootClip.removeChild(lGfx);

    // 4. JPEG XR (`.jxr`) magic-byte `contentType` sniff (`0x49, 0x49, 0xBC, 0x01` -> `image/vnd.ms-photo`)
    const lJxr = new Loader();
    let jxrErrText = null;
    lJxr.contentLoaderInfo.addEventListener('ioError', ev => { jxrErrText = String(ev?.text ?? ''); });
    lJxr.loadBytes(toBa(Buffer.from([0x49, 0x49, 0xbc, 0x01, 0x00, 0x00, 0x00, 0x00])));
    await new Promise(r => setTimeout(r, 20));
    assert.ok(jxrErrText !== null, 'truncated synthetic JXR surfaces clean ioError');
    assert.ok(!jxrErrText.includes('Other file formats'), 'JPEG XR magic bytes recognized by Loader contentType sniff');
    assert.throws(() => lJxr.contentLoaderInfo.contentType, /after INIT/, 'LoaderInfo.contentType remains gated until INIT after ioError');

    assert.equal(functionsMap.size, 0, 'all T-451 child SWFs unloaded cleanly');
  } finally {
    host.disposeFamily();
    player.free();
  }
});
