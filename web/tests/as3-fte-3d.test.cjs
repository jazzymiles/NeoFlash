// End-to-end verification of AS3 Area 8 FTE, IME, Stage3D, Pixel Bender Shaders,
// DisplayObject/Transform 3D, and LoaderInfo/Display extensions (web/as3-fte-3d.js)
// backed by the shared Rust As3Fte3dKernel in WASM (crates/neoflash-core/src/as3_fte_3d.rs).
const assert = require('node:assert/strict');
const test = require('node:test');
const { resolve } = require('node:path');

const root = resolve(__dirname, '../..');

async function createHostHarness() {
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');

  const dummyProject = {
    format_version: 1,
    name: 'FTE Stage3D Shaders Suite',
    width: 800,
    height: 600,
    fps: 30,
    background: [255, 255, 255, 255],
    root: 'root',
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
              keyframes: [{ frame: 0, elements: [], label: '', tween: null }],
            },
          ],
          scripts: [],
        },
      },
    ],
  };

  const player = new runtimeWasm.WebPlayer(JSON.stringify(dummyProject));
  const { host } = createRuntimeHost(root, player, [], {
    project: dummyProject,
    bundle: { project: dummyProject },
  });
  return { wasm: runtimeWasm, player, host, prog: host.program() };
}

test('Flash Text Engine (flash.text.engine.*) layout, atoms, groups, tabs, mirrors, and IME work end-to-end', async () => {
  const { player, host, prog } = await createHostHarness();
  try {
    const BreakOpportunity = prog.type('flash.text.engine.BreakOpportunity');
    const CFFHinting = prog.type('flash.text.engine.CFFHinting');
    const DigitCase = prog.type('flash.text.engine.DigitCase');
    const DigitWidth = prog.type('flash.text.engine.DigitWidth');
    const FontLookup = prog.type('flash.text.engine.FontLookup');
    const FontPosture = prog.type('flash.text.engine.FontPosture');
    const FontWeight = prog.type('flash.text.engine.FontWeight');
    const JustificationStyle = prog.type('flash.text.engine.JustificationStyle');
    const Kerning = prog.type('flash.text.engine.Kerning');
    const LigatureLevel = prog.type('flash.text.engine.LigatureLevel');
    const LineJustification = prog.type('flash.text.engine.LineJustification');
    const RenderingMode = prog.type('flash.text.engine.RenderingMode');
    const TabAlignment = prog.type('flash.text.engine.TabAlignment');
    const TextBaseline = prog.type('flash.text.engine.TextBaseline');
    const TextLineCreationResult = prog.type('flash.text.engine.TextLineCreationResult');
    const TextLineValidity = prog.type('flash.text.engine.TextLineValidity');
    const TextRotation = prog.type('flash.text.engine.TextRotation');
    const TypographicCase = prog.type('flash.text.engine.TypographicCase');

    assert.equal(BreakOpportunity.AUTO, 'auto');
    assert.equal(CFFHinting.HORIZONTAL_STEM, 'horizontalStem');
    assert.equal(DigitCase.OLD_STYLE, 'oldStyle');
    assert.equal(DigitWidth.TABULAR, 'tabular');
    assert.equal(FontLookup.DEVICE, 'device');
    assert.equal(FontPosture.ITALIC, 'italic');
    assert.equal(FontWeight.BOLD, 'bold');
    assert.equal(JustificationStyle.PUSH_IN_KINSOKU, 'pushInKinsoku');
    assert.equal(Kerning.AUTO, 'auto');
    assert.equal(LigatureLevel.COMMON, 'common');
    assert.equal(LineJustification.ALL_BUT_LAST, 'allButLast');
    assert.equal(RenderingMode.CFF, 'cff');
    assert.equal(TabAlignment.DECIMAL, 'decimal');
    assert.equal(TextBaseline.ROMAN, 'roman');
    assert.equal(TextLineCreationResult.SUCCESS, 'success');
    assert.equal(TextLineValidity.VALID, 'valid');
    assert.equal(TextRotation.ROTATE_90, 'rotate90');
    assert.equal(TypographicCase.CAPS_AND_SMALL_CAPS, 'capsAndSmallCaps');

    const FontDescription = prog.type('flash.text.engine.FontDescription');
    const ElementFormat = prog.type('flash.text.engine.ElementFormat');
    const TextElement = prog.type('flash.text.engine.TextElement');
    const GraphicElement = prog.type('flash.text.engine.GraphicElement');
    const GroupElement = prog.type('flash.text.engine.GroupElement');
    const TextBlock = prog.type('flash.text.engine.TextBlock');
    const TextLine = prog.type('flash.text.engine.TextLine');
    const TabStop = prog.type('flash.text.engine.TabStop');
    const TextJustifier = prog.type('flash.text.engine.TextJustifier');
    const SpaceJustifier = prog.type('flash.text.engine.SpaceJustifier');
    const EastAsianJustifier = prog.type('flash.text.engine.EastAsianJustifier');
    const EventDispatcher = prog.type('flash.events.EventDispatcher');
    const Shape = prog.type('flash.display.Shape');

    assert.equal(FontDescription.isFontCompatible('_sans', FontWeight.BOLD, FontPosture.ITALIC), true);
    assert.equal(FontDescription.isDeviceFontCompatible('Arial', FontWeight.NORMAL, FontPosture.NORMAL), true);

    const fd = new FontDescription('Helvetica', FontWeight.BOLD, FontPosture.ITALIC, FontLookup.DEVICE, RenderingMode.CFF, CFFHinting.HORIZONTAL_STEM);
    assert.equal(fd.fontName, 'Helvetica');
    assert.equal(fd.fontWeight, 'bold');
    assert.equal(fd.locked, false);
    const fdClone = fd.clone();
    fd.locked = true;
    assert.throws(() => { fd.fontName = 'Times'; }, /IllegalOperationError/);
    assert.equal(fdClone.locked, false);

    const ef = new ElementFormat(fdClone, 16, 0x112233, 0.9, TextRotation.AUTO, TextBaseline.ROMAN, TextBaseline.USE_DOMINANT_BASELINE, 0.5, Kerning.ON, 1.0, 0.5, 'en_US', BreakOpportunity.AUTO, DigitCase.DEFAULT, DigitWidth.DEFAULT, LigatureLevel.COMMON, TypographicCase.DEFAULT);
    const metrics = ef.getFontMetrics();
    assert.ok(metrics.emBox.height > 0);
    assert.ok(metrics.underlineThickness > 0);
    assert.ok(metrics.lineGap > 0);
    const efClone = ef.clone();
    assert.equal(efClone.fontSize, 16);
    assert.equal(efClone.color, 0x112233);

    const sj = TextJustifier.getJustifierForLocale('en_US');
    assert.equal(SpaceJustifier.__nfIs(sj), true);
    sj.letterSpacing = true;
    sj.minimumSpacing = 0.5;
    sj.optimumSpacing = 1.0;
    sj.maximumSpacing = 2.0;
    const sjClone = sj.clone();
    assert.equal(sjClone.letterSpacing, true);

    const eaj = TextJustifier.getJustifierForLocale('ja_JP');
    assert.equal(EastAsianJustifier.__nfIs(eaj), true);
    eaj.composeTrailingIdeographicSpaces = true;
    assert.equal(eaj.clone().composeTrailingIdeographicSpaces, true);

    const mirror = new EventDispatcher();
    const te1 = new TextElement('Hello ', efClone, mirror);
    const te2 = new TextElement('World\t12.34 and NeoFlash FTE!', efClone);
    te1.replaceText(5, 6, ' brave ');
    assert.equal(te1.text, 'Hello brave ');

    const icon = new Shape();
    icon.graphics.beginFill(0xff0000);
    icon.graphics.drawRect(0, 0, 12, 10);
    icon.graphics.endFill();
    const ge = new GraphicElement(icon, 12, 10, efClone);

    const groupVec = [te1, te2, ge];
    const group = new GroupElement(groupVec, efClone);
    assert.equal(group.elementCount, 3);
    assert.equal(group.getElementAt(0), te1);
    assert.equal(group.getElementIndex(ge), 2);
    assert.equal(group.getElementAtCharIndex(0), te1);

    const splitSecond = group.splitTextElement(0, 6);
    assert.equal(splitSecond.text, 'brave ');
    assert.equal(group.elementCount, 4);
    const merged = group.mergeTextElements(0, 2);
    assert.equal(merged.text, 'Hello brave ');
    assert.equal(group.elementCount, 3);

    const subGroup = group.groupElements(0, 2);
    assert.equal(GroupElement.__nfIs(subGroup), true);
    group.ungroupElements(0);
    assert.equal(group.elementCount, 3);

    const tb = new TextBlock(group, [new TabStop(TabAlignment.DECIMAL, 140, '.')], sjClone, TextRotation.ROTATE_0, TextBaseline.ROMAN, 0, true, fdClone, 16);
    assert.equal(tb.findNextAtomBoundary(0), 1);
    assert.equal(tb.findPreviousAtomBoundary(2), 1);
    assert.ok(tb.findNextWordBoundary(0) >= 5);
    assert.equal(tb.findPreviousWordBoundary(6), 5);

    const line1 = tb.createTextLine(null, 220, 0, false);
    assert.ok(line1);
    assert.equal(TextLine.__nfIs(line1), true);
    assert.equal(tb.textLineCreationResult, TextLineCreationResult.SUCCESS);
    assert.equal(tb.firstLine, line1);
    assert.ok(line1.atomCount > 0);
    assert.ok(line1.textWidth > 0);
    assert.ok(line1.ascent > 0);
    assert.ok(line1.descent > 0);
    assert.ok(line1.totalHeight > 0);
    assert.equal(line1.validity, TextLineValidity.VALID);
    assert.equal(line1.getAtomIndexAtCharIndex(0), 0);
    assert.ok(line1.getAtomBounds(0).width > 0);
    assert.equal(typeof line1.getAtomCenter(0), 'number');
    assert.equal(line1.getAtomWordBoundaryOnLeft(0), true);
    assert.equal(line1.getAtomBidiLevel(0), 0);
    assert.equal(line1.getAtomTextRotation(0), TextRotation.ROTATE_0);
    assert.equal(line1.getAtomTextBlockBeginIndex(0), 0);
    assert.equal(line1.getAtomTextBlockEndIndex(0), 1);
    assert.equal(typeof line1.getBaselinePosition(TextBaseline.ROMAN), 'number');
    assert.ok(line1.mirrorRegions.length >= 1);
    assert.equal(line1.getMirrorRegion(mirror).mirror, mirror);

    // Attach TextLine to the stage and verify DisplayObjectContainer & rendering integration
    const rootClip = host.proxyFor([]);
    rootClip.addChild(line1);
    assert.equal(line1.parent, rootClip);
    assert.ok(line1.width > 0);

    const line2 = tb.createTextLine(line1, 300, 0, true);
    assert.ok(line2);
    assert.equal(line1.nextLine, line2);
    assert.equal(line2.previousLine, line1);
    assert.equal(line2.hasGraphicElement, true);
    assert.equal(tb.getTextLineAtCharIndex(0), line1);
    assert.ok(tb.dump().includes('<block>'));
    assert.ok(line1.dump().includes('<TextLine'));

    const recreated = tb.recreateTextLine(line2, line1, 400, 0, true);
    assert.equal(recreated, line2);
    line1.flushAtomData();
    tb.releaseLineCreationData();
    tb.releaseLines(line1, line2);
    assert.equal(tb.firstLine, null);

    // IME CompositionAttributeRange
    const CompositionAttributeRange = prog.type('flash.text.ime.CompositionAttributeRange');
    const car = new CompositionAttributeRange(1, 5, true, false);
    assert.equal(car.relativeStart, 1);
    assert.equal(car.relativeEnd, 5);
    assert.equal(car.selected, true);
    assert.equal(car.converted, false);
  } finally {
    host.disposeFamily();
    player.free();
  }
});

test('Stage3D (flash.display.Stage3D, flash.display3D.*, flash.display3D.textures.*) AGAL software rasterizer and GPU pipeline work end-to-end', async () => {
  const { player, host, prog } = await createHostHarness();
  try {
    const rootClip = host.proxyFor([]);
    const stage = rootClip.stage;
    assert.equal(stage.stage3Ds.length, 4);
    const s3d = stage.stage3Ds[0];
    s3d.x = 10;
    s3d.y = 20;
    assert.equal(s3d.x, 10);
    assert.equal(s3d.y, 20);
    assert.equal(s3d.visible, true);

    const Context3DRenderMode = prog.type('flash.display3D.Context3DRenderMode');
    const Context3DProfile = prog.type('flash.display3D.Context3DProfile');
    const Context3DBufferUsage = prog.type('flash.display3D.Context3DBufferUsage');
    const Context3DVertexBufferFormat = prog.type('flash.display3D.Context3DVertexBufferFormat');
    const Context3DProgramType = prog.type('flash.display3D.Context3DProgramType');
    const Context3DTextureFormat = prog.type('flash.display3D.Context3DTextureFormat');
    const Context3DBlendFactor = prog.type('flash.display3D.Context3DBlendFactor');
    const Context3DCompareMode = prog.type('flash.display3D.Context3DCompareMode');
    const Context3DTriangleFace = prog.type('flash.display3D.Context3DTriangleFace');
    const Context3DStencilAction = prog.type('flash.display3D.Context3DStencilAction');
    const Context3DWrapMode = prog.type('flash.display3D.Context3DWrapMode');
    const Context3DTextureFilter = prog.type('flash.display3D.Context3DTextureFilter');
    const Context3DMipFilter = prog.type('flash.display3D.Context3DMipFilter');
    const Context3DFillMode = prog.type('flash.display3D.Context3DFillMode');
    const Context3DClearMask = prog.type('flash.display3D.Context3DClearMask');
    const Context3D = prog.type('flash.display3D.Context3D');
    const ByteArray = prog.type('flash.utils.ByteArray');
    const BitmapData = prog.type('flash.display.BitmapData');
    const Matrix3D = prog.type('flash.geom.Matrix3D');

    assert.equal(Context3DClearMask.ALL, 7);
    assert.equal(Context3D.supportsVideoTexture, true);

    let createdEvents = 0;
    s3d.addEventListener('context3DCreate', () => { createdEvents++; });
    s3d.requestContext3D(Context3DRenderMode.AUTO, Context3DProfile.STANDARD);
    assert.equal(createdEvents, 1);

    const ctx = s3d.context3D;
    assert.ok(ctx);
    assert.equal(ctx.profile, Context3DProfile.STANDARD);
    assert.ok(ctx.driverInfo.includes('Software'));

    ctx.configureBackBuffer(64, 64, 0, true, false, false);
    assert.equal(ctx.backBufferWidth, 64);
    assert.equal(ctx.backBufferHeight, 64);
    ctx.enableErrorChecking = true;
    assert.equal(ctx.enableErrorChecking, true);

    const vb = ctx.createVertexBuffer(3, 6, Context3DBufferUsage.DYNAMIC_DRAW);
    vb.uploadFromVector([
      -1, -1, 0, 1, 0, 0,
       1, -1, 0, 1, 0, 0,
       0,  1, 0, 1, 0, 0,
    ], 0, 3);

    const ib = ctx.createIndexBuffer(3, Context3DBufferUsage.STATIC_DRAW);
    ib.uploadFromVector([0, 1, 2], 0, 3);

    // Build minimal AGAL v1 bytecode:
    // Vertex: mov op, va0; mov v0, va1
    // Fragment: mov oc, v0
    function agalMov(dstType, dstIdx, srcType, srcIdx) {
      const b = [
        0x00, 0x00, 0x00, 0x00, // opcode MOV = 0
        dstIdx & 0xff, 0x00, 0x0f, dstType & 0xff, // dst
        srcIdx & 0xff, 0x00, 0x00, 0xe4, srcType & 0xff, 0x00, 0x00, 0x00, // src1
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // src2
      ];
      return b;
    }
    const vsBytes = new ByteArray();
    for (const byte of [0xa0, 0x01, 0x00, 0x00, 0x00, 0xa1, 0x00, ...agalMov(3, 0, 0, 0), ...agalMov(4, 0, 0, 1)]) {
      vsBytes.writeByte(byte);
    }
    const fsBytes = new ByteArray();
    for (const byte of [0xa0, 0x01, 0x00, 0x00, 0x00, 0xa1, 0x01, ...agalMov(3, 0, 4, 0)]) {
      fsBytes.writeByte(byte);
    }

    const prog3d = ctx.createProgram();
    prog3d.upload(vsBytes, fsBytes);

    ctx.clear(0, 0, 1, 1);
    ctx.setProgram(prog3d);
    ctx.setVertexBufferAt(0, vb, 0, Context3DVertexBufferFormat.FLOAT_3);
    ctx.setVertexBufferAt(1, vb, 3, Context3DVertexBufferFormat.FLOAT_3);
    ctx.setProgramConstantsFromVector(Context3DProgramType.VERTEX, 0, [1, 1, 1, 1], 1);
    ctx.setProgramConstantsFromMatrix(Context3DProgramType.VERTEX, 1, new Matrix3D(), true);
    ctx.setBlendFactors(Context3DBlendFactor.ONE, Context3DBlendFactor.ZERO);
    ctx.setColorMask(true, true, true, true);
    ctx.setCulling(Context3DTriangleFace.NONE);
    ctx.setDepthTest(true, Context3DCompareMode.ALWAYS);
    ctx.setFillMode(Context3DFillMode.SOLID);
    ctx.setStencilActions(Context3DTriangleFace.FRONT_AND_BACK, Context3DCompareMode.ALWAYS, Context3DStencilAction.KEEP, Context3DStencilAction.KEEP, Context3DStencilAction.KEEP);
    ctx.setStencilReferenceValue(0, 255, 255);
    ctx.setSamplerStateAt(0, Context3DWrapMode.CLAMP, Context3DTextureFilter.LINEAR, Context3DMipFilter.MIPNONE);

    // Texture creation & uploads
    const tex = ctx.createTexture(16, 16, Context3DTextureFormat.BGRA, false, 0);
    const srcBmp = new BitmapData(16, 16, true, 0xff00ff00);
    tex.uploadFromBitmapData(srcBmp, 0);
    const cube = ctx.createCubeTexture(8, Context3DTextureFormat.BGRA, false, 0);
    for (let side = 0; side < 6; side++) {
      cube.uploadFromBitmapData(srcBmp, side, 0);
    }
    const rectTex = ctx.createRectangleTexture(20, 12, Context3DTextureFormat.BGRA, true);
    rectTex.uploadFromBitmapData(new BitmapData(20, 12, true, 0xffff0000));
    const vidTex = ctx.createVideoTexture();
    assert.ok(vidTex.videoWidth > 0);

    ctx.setTextureAt(0, tex);
    ctx.drawTriangles(ib, 0, 1);

    const outBmp = new BitmapData(64, 64, true, 0);
    ctx.drawToBitmapData(outBmp);
    // Center of triangle (32, 32) is red (0xffff0000), corner (2, 2) is cleared blue (0xff0000ff)
    assert.equal(outBmp.getPixel32(32, 32) >>> 0, 0xffff0000);
    assert.equal(outBmp.getPixel32(2, 2) >>> 0, 0xff0000ff);
    assert.ok(ctx.totalGPUMemory > 0);

    // Wave 6: Mipmap level sampling (upload miplevel 1 as yellow 0xffffff00 and sample with mipnearest)
    function agalTex(dstType, dstIdx, src1Type, src1Idx, samplerIdx) {
      const b = new Array(24).fill(0);
      b[0] = 0x28; // tex
      b[4] = dstIdx & 0xff;
      b[5] = (dstIdx >> 8) & 0xff;
      b[6] = 0x0f;
      b[7] = dstType & 0x0f;
      b[8] = src1Idx & 0xff;
      b[9] = (src1Idx >> 8) & 0xff;
      b[11] = 0xe4;
      b[12] = src1Type & 0x0f;
      b[16] = samplerIdx & 0xff;
      b[17] = (samplerIdx >> 8) & 0xff;
      b[20] = 5; // fs sampler
      return b;
    }
    const fsTexBytes = new ByteArray();
    for (const byte of [0xa0, 0x01, 0x00, 0x00, 0x00, 0xa1, 0x01, ...agalTex(3, 0, 4, 0, 0)]) {
      fsTexBytes.writeByte(byte);
    }
    const progTex = ctx.createProgram();
    progTex.upload(vsBytes, fsTexBytes);
    ctx.setProgram(progTex);

    const mip1Bmp = new BitmapData(8, 8, true, 0xffffff00);
    tex.uploadFromBitmapData(mip1Bmp, 1);
    ctx.setSamplerStateAt(0, Context3DWrapMode.CLAMP, Context3DTextureFilter.NEAREST, Context3DMipFilter.MIPNEAREST);
    ctx.clear(0, 0, 0, 1);
    ctx.drawTriangles(ib, 0, 1);
    ctx.drawToBitmapData(outBmp);
    assert.equal(outBmp.getPixel32(32, 32) >>> 0, 0xffffff00);

    // Wave 6: CubeTexture 3D direction vector sampling (+X face 0 = cyan 0xff00ffff)
    const cubeFace0 = new BitmapData(8, 8, true, 0xff00ffff);
    cube.uploadFromBitmapData(cubeFace0, 0, 0);
    // Upload vertices with +X direction vector (1, 0, 0) in va1
    const cubeVerts = [
      -1.0, -1.0, 0.0, 1.0, 1.0, 1.0,  1.0, 0.0, 0.0,
       1.0, -1.0, 0.0, 1.0, 1.0, 1.0,  1.0, 0.0, 0.0,
       0.0,  1.0, 0.0, 1.0, 1.0, 1.0,  1.0, 0.0, 0.0,
    ];
    const vbCube = ctx.createVertexBuffer(3, 9, Context3DBufferUsage.STATIC_DRAW);
    vbCube.uploadFromVector(cubeVerts, 0, 3);
    ctx.setVertexBufferAt(0, vbCube, 0, Context3DVertexBufferFormat.FLOAT_3);
    ctx.setVertexBufferAt(1, vbCube, 6, Context3DVertexBufferFormat.FLOAT_3);
    ctx.setTextureAt(0, cube);
    ctx.setSamplerStateAt(0, Context3DWrapMode.CLAMP, Context3DTextureFilter.NEAREST, Context3DMipFilter.MIPNONE);
    ctx.clear(0, 0, 0, 1);
    ctx.drawTriangles(ib, 0, 1);
    ctx.drawToBitmapData(outBmp);
    assert.equal(outBmp.getPixel32(32, 32) >>> 0, 0xff00ffff);

    ctx.present();
    progTex.dispose();
    vbCube.dispose();
    tex.dispose();
    cube.dispose();
    rectTex.dispose();
    vidTex.dispose();
    vb.dispose();
    ib.dispose();
    prog3d.dispose();
    ctx.dispose(false);
    assert.equal(s3d.context3D, null);
  } finally {
    host.disposeFamily();
    player.free();
  }
});

test('Pixel Bender Shaders, ShaderJob, ShaderFilter, GraphicsShaderFill, 3D DisplayObject/Transform, and LoaderInfo work end-to-end', async () => {
  const { player, host, prog } = await createHostHarness();
  try {
    const Shader = prog.type('flash.display.Shader');
    const ShaderData = prog.type('flash.display.ShaderData');
    const ShaderParameter = prog.type('flash.display.ShaderParameter');
    const ShaderParameterType = prog.type('flash.display.ShaderParameterType');
    const ShaderPrecision = prog.type('flash.display.ShaderPrecision');
    const ShaderInput = prog.type('flash.display.ShaderInput');
    const ShaderJob = prog.type('flash.display.ShaderJob');
    const GraphicsShaderFill = prog.type('flash.display.GraphicsShaderFill');
    const ShaderFilter = prog.type('flash.filters.ShaderFilter');
    const BitmapData = prog.type('flash.display.BitmapData');
    const ByteArray = prog.type('flash.utils.ByteArray');
    const Sprite = prog.type('flash.display.Sprite');
    const Vector3D = prog.type('flash.geom.Vector3D');
    const Point = prog.type('flash.geom.Point');
    const Rectangle = prog.type('flash.geom.Rectangle');
    const PerspectiveProjection = prog.type('flash.geom.PerspectiveProjection');
    const ActionScriptVersion = prog.type('flash.display.ActionScriptVersion');
    const SWFVersion = prog.type('flash.display.SWFVersion');
    const ColorCorrection = prog.type('flash.display.ColorCorrection');
    const ColorCorrectionSupport = prog.type('flash.display.ColorCorrectionSupport');
    const MorphShape = prog.type('flash.display.MorphShape');
    const AVM1Movie = prog.type('flash.display.AVM1Movie');
    const Loader = prog.type('flash.display.Loader');
    const LoaderInfo = prog.type('flash.display.LoaderInfo');

    assert.equal(ActionScriptVersion.ACTIONSCRIPT3, 3);
    assert.equal(SWFVersion.FLASH10, 10);
    assert.equal(ColorCorrection.ON, 'on');
    assert.equal(ColorCorrectionSupport.DEFAULT_ON, 'defaultOn');
    assert.equal(ShaderPrecision.FULL, 'full');
    assert.equal(ShaderParameterType.FLOAT4, 'float4');

    const shaderBytes = new ByteArray();
    shaderBytes.writeUTFBytes(JSON.stringify({
      name: 'TintKernel',
      op: 'tint',
      inputs: [{ name: 'src', channels: 4 }],
      parameters: [{ name: 'color', type: 'float4', defaultValue: [0.2, 0.4, 0.8, 1.0] }],
    }));

    const shader = new Shader(shaderBytes);
    assert.equal(shader.precisionHint, ShaderPrecision.FULL);
    assert.equal(ShaderData.__nfIs(shader.data), true);
    assert.equal(ShaderInput.__nfIs(shader.data.src), true);
    assert.equal(ShaderParameter.__nfIs(shader.data.color), true);
    shader.data.color.value = [1.0, 0.0, 0.0, 1.0];

    const srcBmp = new BitmapData(8, 8, true, 0xffffffff);
    shader.data.src.input = srcBmp;
    const dstBmp = new BitmapData(8, 8, true, 0x00000000);
    const job = new ShaderJob(shader, dstBmp, 8, 8);
    job.start(true);
    assert.equal(job.progress, 1);
    assert.equal(dstBmp.getPixel32(0, 0) >>> 0, 0xffff0000);

    // ShaderFilter on DisplayObject & BitmapData.applyFilter
    const sf = new ShaderFilter(shader);
    sf.leftExtension = 2;
    sf.rightExtension = 2;
    assert.equal(sf.leftExtension, 2);
    const sp = new Sprite();
    sp.graphics.beginShaderFill(shader);
    sp.graphics.lineShaderStyle(shader);
    sp.graphics.drawRect(0, 0, 20, 20);
    sp.graphics.endFill();
    const fillCmd = new GraphicsShaderFill(shader);
    sp.graphics.drawGraphicsData([fillCmd]);
    sp.filters = [sf];
    assert.equal(sp.filters.length, 1);

    // blendShader + BlendMode.SHADER
    sp.blendShader = shader;
    sp.blendMode = 'shader';
    assert.equal(sp.blendMode, 'shader');

    // 3D DisplayObject & Transform
    const rootClip = host.proxyFor([]);
    rootClip.addChild(sp);
    sp.x = 40;
    sp.y = 50;
    sp.z = 25;
    sp.scaleZ = 2;
    sp.rotationX = 15;
    sp.rotationY = 30;
    sp.rotationZ = 45;
    assert.equal(sp.z, 25);
    assert.equal(sp.scaleZ, 2);
    assert.ok(sp.transform.matrix3D);
    const g2d = sp.local3DToGlobal(new Vector3D(10, 20, 5));
    assert.equal(Point.__nfIs(g2d), true);
    const l3d = sp.globalToLocal3D(g2d);
    assert.equal(Vector3D.__nfIs(l3d), true);
    const rel = sp.transform.getRelativeMatrix3D(rootClip);
    assert.ok(rel);
    const pp = new PerspectiveProjection();
    pp.fieldOfView = 60;
    sp.transform.perspectiveProjection = pp;
    assert.ok(Math.abs(sp.transform.perspectiveProjection.fieldOfView - 60) < 1e-6);

    // SoftKeyboard on InteractiveObject & Stage
    sp.needsSoftKeyboard = true;
    sp.softKeyboardInputAreaOfInterest = new Rectangle(0, 0, 100, 30);
    const skEvents = [];
    let cancelNextSoftKeyboard = false;
    sp.addEventListener('softKeyboardActivating', e => {
      skEvents.push(['activating', e.cancelable]);
      if (cancelNextSoftKeyboard) e.preventDefault();
    });
    sp.addEventListener('softKeyboardActivate', () => skEvents.push(['activate']));
    sp.addEventListener('softKeyboardDeactivate', () => skEvents.push(['deactivate']));
    assert.equal(sp.requestSoftKeyboard(), true);
    assert.ok(rootClip.stage.softKeyboardRect.height > 0);
    assert.deepEqual(skEvents, [['activating', true], ['activate']]);
    rootClip.stage.focus = sp;
    rootClip.stage.focus = null;
    assert.equal(skEvents.some(e => e[0] === 'deactivate'), true);
    assert.equal(rootClip.stage.softKeyboardRect.height, 0);
    cancelNextSoftKeyboard = true;
    assert.equal(sp.requestSoftKeyboard(), false);
    assert.equal(rootClip.stage.softKeyboardRect.height, 0);
    cancelNextSoftKeyboard = false;

    // MorphShape & AVM1Movie
    const morph = new MorphShape();
    const avm1 = new AVM1Movie();
    rootClip.addChild(morph);
    rootClip.addChild(avm1);
    assert.equal(morph.parent, rootClip);
    assert.equal(avm1.parent, rootClip);

    // Loader & LoaderInfo extended properties/methods
    const rootInfo = rootClip.loaderInfo;
    assert.ok(rootInfo);
    assert.equal(rootInfo.actionScriptVersion, 3);
    assert.ok(rootInfo.swfVersion >= 10);
    assert.equal(rootInfo.frameRate, 30);
    assert.equal(rootInfo.sameDomain, true);
    assert.equal(rootInfo.childAllowsParent, true);
    assert.equal(rootInfo.parentAllowsChild, true);
    assert.equal(rootInfo.isURLInaccessible, false);
    assert.ok(rootInfo.applicationDomain);
    assert.ok(rootInfo.bytes);
    assert.ok(rootInfo.sharedEvents);
    assert.ok(rootInfo.uncaughtErrorEvents);
    assert.equal(LoaderInfo.getLoaderInfoByDefinition(sp), rootInfo);

    const loader = new Loader();
    assert.ok(loader.uncaughtErrorEvents);
    loader.unloadAndStop(true);

    // Wave 6 behavioral verification:
    // 1. MorphShape ratio interpolation and BitmapData.draw rasterization
    morph.setMorphEndpoints(
      [{ type: 'begin_fill', color: [255, 0, 0], alpha: 1 }, { type: 'move_to', x: 0, y: 0 }, { type: 'line_to', x: 16, y: 0 }, { type: 'line_to', x: 16, y: 16 }, { type: 'line_to', x: 0, y: 16 }, { type: 'end_fill' }],
      [{ type: 'begin_fill', color: [0, 0, 255], alpha: 1 }, { type: 'move_to', x: 0, y: 0 }, { type: 'line_to', x: 32, y: 0 }, { type: 'line_to', x: 32, y: 32 }, { type: 'line_to', x: 0, y: 32 }, { type: 'end_fill' }]
    );
    morph.ratio = 0.5;
    assert.equal(morph.ratio, 0.5);
    const morphBmp = new BitmapData(32, 32, true, 0);
    morphBmp.draw(morph);
    const morphPx = morphBmp.getPixel32(10, 10) >>> 0;
    assert.equal((morphPx >>> 24) & 0xff, 0xff);
    assert.ok(((morphPx >>> 16) & 0xff) >= 120 && ((morphPx >>> 16) & 0xff) <= 135);
    assert.ok((morphPx & 0xff) >= 120 && (morphPx & 0xff) <= 135);

    // 2. Multi-input ShaderFilter & binary .pbj conditional control flow (if/else/endif + LoadFloat)
    const pbjBranch = new ByteArray();
    // Construct a binary .pbj stream with version 1, parameter "threshold", and conditional instructions:
    // LoadFloat r0.r = 0.75 (0x32), LoadFloat r1.r = 0.25 (0x32), GreaterThan (0x2A) r0.r > r1.r -> r2.r,
    // If (0x34) r2.r: set dst (r1) = [0, 1, 0, 1] Else (0x35): set dst (r1) = [1, 0, 0, 1] EndIf (0x36)
    const writeLoadFloat = (ba, reg, mask, v) => {
      ba.writeByte(0x32);
      ba.writeByte(reg & 0xff);
      ba.writeByte((reg >> 8) & 0xff);
      ba.writeByte((mask & 0x0f) << 4);
      const buf = new ArrayBuffer(4);
      new DataView(buf).setFloat32(0, v, false);
      const u8 = new Uint8Array(buf);
      for (const b of u8) ba.writeByte(b);
    };
    // Magic 0xA5, version 1 (LE), name 0xA4 "CondKernel" (u16 LE = 10, 0)
    pbjBranch.writeByte(0xA5);
    pbjBranch.writeByte(1); pbjBranch.writeByte(0); pbjBranch.writeByte(0); pbjBranch.writeByte(0);
    pbjBranch.writeByte(0xA4); pbjBranch.writeByte(10); pbjBranch.writeByte(0); pbjBranch.writeUTFBytes('CondKernel');
    // LoadFloat r2.r = 1.0 (true condition), r1 = [0, 1, 0, 1] inside If, [1, 0, 0, 1] inside Else
    writeLoadFloat(pbjBranch, 2, 0x8, 1.0);
    // 0x34 If src_reg=2, swz=0 (r2.r): 3 zero bytes + src_raw(u16 LE: 2) + swz(0) + pad(0) = 8 bytes total
    pbjBranch.writeByte(0x34); pbjBranch.writeByte(0); pbjBranch.writeByte(0); pbjBranch.writeByte(0);
    pbjBranch.writeByte(0x02); pbjBranch.writeByte(0x00); pbjBranch.writeByte(0x00); pbjBranch.writeByte(0x00);
    // Inside If: set r1.r=0, r1.g=1, r1.b=0, r1.a=1
    writeLoadFloat(pbjBranch, 1, 0x8, 0.0);
    writeLoadFloat(pbjBranch, 1, 0x4, 1.0);
    writeLoadFloat(pbjBranch, 1, 0x2, 0.0);
    writeLoadFloat(pbjBranch, 1, 0x1, 1.0);
    // 0x35 Else (8 bytes)
    pbjBranch.writeByte(0x35); for (let i = 0; i < 7; i++) pbjBranch.writeByte(0);
    // Inside Else (skipped): set r1.r=1, r1.g=0
    writeLoadFloat(pbjBranch, 1, 0x8, 1.0);
    writeLoadFloat(pbjBranch, 1, 0x4, 0.0);
    // 0x36 EndIf (8 bytes)
    pbjBranch.writeByte(0x36); for (let i = 0; i < 7; i++) pbjBranch.writeByte(0);

    const branchShader = new Shader(pbjBranch);
    const branchBmp = new BitmapData(4, 4, true, 0);
    new ShaderJob(branchShader, branchBmp, 4, 4).start(true);
    assert.equal(branchBmp.getPixel32(0, 0) >>> 0, 0xff00ff00);

    // 3. Loader.loadBytes with GIF89a and SWF (FWS)
    // Minimal 1x1 red GIF89a:
    const gifRaw = [
      0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // GIF89a
      0x01, 0x00, 0x01, 0x00,             // width=1, height=1
      0x80, 0x00, 0x00,                   // GCT flag=1, size=2 entries
      0xff, 0x00, 0x00,                   // color 0: red
      0x00, 0x00, 0x00,                   // color 1: black
      0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, // Image Descriptor 1x1
      0x02, 0x02, 0x44, 0x01, 0x00,       // LZW min code size 2, 2 bytes: clear(4), index 0, EOI(5)
      0x3b                                // Trailer
    ];
    const gifBytes = new ByteArray();
    for (const b of gifRaw) gifBytes.writeByte(b);
    const gifLoader = new Loader();
    gifLoader.loadBytes(gifBytes);
    await new Promise(r => setTimeout(r, 15));
    assert.equal(gifLoader.contentLoaderInfo.contentType, 'image/gif');
    assert.equal(gifLoader.contentLoaderInfo.width, 1);
    assert.equal(gifLoader.contentLoaderInfo.height, 1);
    assert.equal(gifLoader.content.bitmapData.getPixel32(0, 0) >>> 0, 0xffff0000);

    // Minimal FWS SWF v10 (320x240 = 6400x4800 twips, 30 fps, 1 frame, FileAttributes tag 69 with AS3=1)
    // RECT: nbits=14 (01110), xmin=0, xmax=6400 (0x1900), ymin=0, ymax=4800 (0x12c0)
    // Bits: 01110 | 00000000000000 | 01100100000000 | 00000000000000 | 01001011000000 (61 bits -> 8 bytes)
    // 01110000 00000000 00001100 10000000 00000000 00000000 10010110 00000000
    // = 0x70, 0x00, 0x0c, 0x80, 0x00, 0x00, 0x96, 0x00
    const swfRaw = [
      0x46, 0x57, 0x53, 10,               // FWS, version 10
      26, 0, 0, 0,                        // file length 26
      0x70, 0x00, 0x0c, 0x80, 0x00, 0x00, 0x96, 0x00, // RECT 320x240
      0x00, 30,                           // 30.0 fps
      1, 0,                               // 1 frame
      0x44, 0x11, 0x08, 0x00, 0x00, 0x00, // Tag 69 (FileAttributes, len 4): actionScript3 bit (0x08)
      0x00, 0x00                          // End tag
    ];
    const swfBytes = new ByteArray();
    for (const b of swfRaw) swfBytes.writeByte(b);
    const swfLoader = new Loader();
    swfLoader.loadBytes(swfBytes);
    await new Promise(r => setTimeout(r, 15));
    assert.equal(swfLoader.contentLoaderInfo.contentType, 'application/x-shockwave-flash');
    assert.equal(swfLoader.contentLoaderInfo.swfVersion, 10);
    assert.equal(swfLoader.contentLoaderInfo.actionScriptVersion, 3);
    assert.equal(swfLoader.contentLoaderInfo.frameRate, 30);
    assert.equal(swfLoader.contentLoaderInfo.width, 320);
    assert.equal(swfLoader.contentLoaderInfo.height, 240);
    assert.ok(swfLoader.content);
  } finally {
    host.disposeFamily();
    player.free();
  }
});
