// One published image/pixel workflow; pixel truth comes from actual Rust/WASM.
// The minimal Canvas surface observes uploads only, not browser rasterization.
import assert from 'node:assert/strict';
import { runCargo } from '../../../scripts/test-cargo.cjs';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyBitmapPixels(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/bitmap-pixels-'));
  let player,host,uploads=0;
  const surfaces=[];
  const createCanvas=(width,height)=>{
    const surface={width,height,getContext:()=>({
      createImageData:(w,h)=>({data:new Uint8ClampedArray(w*h*4)}),
      putImageData:image=>{uploads++;surface.last=Array.from(image.data);},
    })};
    surfaces.push(surface);return surface;
  };
  try {
    const exported=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/bitmap-pixels.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,bitmap:{createCanvas},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    const scene=host.proxyFor([]),bitmap=host.bitmap();
    const pixels=value=>Array.from(player.bitmap_pixels(JSON.stringify(bitmap.refOf(value))));
    const drawFor=view=>JSON.parse(player.scene_json()).find(item=>JSON.stringify(item.owner_ref)===JSON.stringify(view.toJSON().object_ref)&&item.kind.type==='bitmap_pixels');
    assert.equal(host.lastTrace(),'bitmaps ready 16 16 0');
    assert.deepEqual([scene.original.width,scene.original.height,scene.original.transparent],[16,16,true]);
    assert.equal(scene.sourceImage.bitmapData,scene.original);
    assert.equal(scene.leftView.bitmapData,scene.shared);assert.equal(scene.rightView.bitmapData,scene.shared);
    assert.notEqual(scene.cloneView.bitmapData,scene.shared);
    assert.equal(host.program().is(scene.leftView,'flash.display.Bitmap'),true);
    assert.equal(host.program().is(scene.shared,'flash.display.BitmapData'),true);
    assert.equal(host.program().is(scene.shared.rect,'flash.geom.Rectangle'),true);
    assert.equal(scene.original.getPixel32(5,1),0xff2ad4a5,'original PNG decoded in Rust');
    assert.equal(scene.original.getPixel32(0,0),0);
    assert.equal(scene.independent.getPixel32(0,0),0xffffc857,'scroll leaves the exposed edge unchanged');
    assert.equal(scene.independent.getPixel32(4,0),0xffffc857);
    assert.equal(scene.independent.getPixel32(0,8),scene.original.getPixel32(4,3),'copyPixels uses the selected source rectangle');
    assert.equal(scene.independent.getPixel32(7,15),scene.original.getPixel32(11,10));
    assert.equal(scene.legacyPixels.getPixel32(0,0),0xff122233);
    assert.equal(scene.legacyClip.getChildAt(0).bitmapData,scene.legacyPixels);
    assert.equal(scene.legacyStatus.text,'16 × 16 pixels | rectangle: 16 | depth: append-only');
    const sourceBefore=pixels(scene.original),cloneBefore=pixels(scene.independent);
    const before=drawFor(scene.leftView),second=drawFor(scene.rightView);
    assert.deepEqual(before.kind.pixel_ref,second.kind.pixel_ref);
    const draws=[],stack=[];
    let matrix=[1,0,0,1,0.4,0.6];
    const consumer={imageSmoothingEnabled:false,
      save(){stack.push({matrix:matrix.slice(),smoothing:this.imageSmoothingEnabled});},
      restore(){const saved=stack.pop();matrix=saved.matrix;this.imageSmoothingEnabled=saved.smoothing;},
      getTransform(){const[a,b,c,d,e,f]=matrix;return {a,b,c,d,e,f};},
      setTransform(...values){matrix=values;},
      drawImage(surface,x,y,width,height){draws.push({surface,x,y,width,height,matrix:matrix.slice(),smoothing:this.imageSmoothingEnabled});},
    };
    bitmap.cache.beginFrame();
    assert.equal(bitmap.cache.hit(before.kind,0,0),true);
    bitmap.cache.draw(consumer,before.kind);
    bitmap.cache.bounds(second.kind);bitmap.cache.endFrame();
    assert.equal(uploads,1,'two shared views reuse a single revision upload');
    assert.equal(draws[0].surface,surfaces[0]);assert.deepEqual(draws[0].matrix,[1,0,0,1,0,1]);
    assert.equal(draws[0].smoothing,false);assert.deepEqual(matrix,[1,0,0,1,0.4,0.6],'drawing restores the caller transform');
    player.click(JSON.stringify(['paint-button']));host.dispatchScripts(['paint-button']);
    assert.equal(scene.phase,1);
    assert.equal(scene.status.text,'Paint 1: both shared views changed | PNG source and clone unchanged');
    assert.equal(scene.shared.getPixel32(4,4),0xffff965c);
    assert.equal(scene.shared.getPixel32(0,0),0xff1f3045,'setPixel preserves existing alpha');
    assert.equal(scene.shared.getPixel(0,0),0x1f3045);
    assert.deepEqual(pixels(scene.original),sourceBefore);assert.deepEqual(pixels(scene.independent),cloneBefore);
    const after=drawFor(scene.leftView),changed=pixels(scene.shared),offset=(4*16+4)*4;
    assert.notEqual(after.kind.revision,before.kind.revision);
    assert.deepEqual(changed.slice(offset,offset+4),[255,150,92,255]);
    bitmap.cache.beginFrame();bitmap.cache.bounds(after.kind);bitmap.cache.endFrame();
    assert.equal(uploads,2);assert.deepEqual(surfaces[0].last,changed,'upload refreshes from Rust after mutation');
    scene.rightView.bitmapData=scene.independent;assert.equal(scene.rightView.bitmapData,scene.independent);
    scene.rightView.smoothing=true;assert.equal(scene.rightView.smoothing,true);
    scene.rightView.pixelSnapping='never';assert.equal(scene.rightView.pixelSnapping,'never');
    scene.rightView.bitmapData=scene.shared;
    bitmap.cache.beginFrame();bitmap.cache.draw(consumer,drawFor(scene.rightView).kind);
    assert.deepEqual(draws[1].matrix,[1,0,0,1,0.4,0.6]);assert.equal(draws[1].smoothing,true);
    const originalKind=drawFor(scene.sourceImage).kind;
    assert.equal(bitmap.cache.hit(originalKind,0,0),false);assert.equal(bitmap.cache.hit(originalKind,5,1),true);
    assert.deepEqual(bitmap.cache.bounds(originalKind),{left:0,top:1,right:16,bottom:12});
    bitmap.cache.endFrame();
    const doomed=scene.independent;
    doomed.dispose();assert.throws(()=>doomed.getPixel(0,0),/disposed/i);
    assert.equal(drawFor(scene.cloneView),undefined,'disposing displayed pixels removes their paint without crashing the scene');
    const {BitmapData,BitmapDataChannel}=bitmap,Rectangle=scene.shared.rect.constructor,Point=scene.shared.rect.topLeft.constructor;
    const a=new BitmapData(4,4,true,0xff102030),b=new BitmapData(4,4,true,0xff405060),mask=new BitmapData(4,4,true,0x00000000);
    mask.setPixel32(2,2,0xff000000);
    a.copyPixels(b,new Rectangle(0,0,4,4),new Point(0,0),mask,new Point(0,0),true);
    assert.equal(a.getPixel32(0,0),0xff102030);assert.equal(a.getPixel32(2,2),0xff405060);
    b.setPixel32(0,0,0xffaa0000);
    a.copyChannel(b,new Rectangle(0,0,1,1),new Point(0,0),BitmapDataChannel.RED,BitmapDataChannel.BLUE);
    assert.equal(a.getPixel32(0,0)&0xff,0xaa);
    const canvas4=new BitmapData(4,4,true,0x00000000);
    canvas4.fillRect(new Rectangle(1,1,2,2),0xff112233);
    const colorBounds=canvas4.getColorBoundsRect(0xff000000,0xff000000,true);
    assert.deepEqual([colorBounds.x,colorBounds.y,colorBounds.width,colorBounds.height],[1,1,2,2]);
    canvas4.floodFill(1,1,0xffabcdef);
    assert.equal(canvas4.getPixel32(2,2),0xffabcdef);assert.equal(canvas4.getPixel32(0,0),0);
    assert.equal(canvas4.threshold(canvas4,canvas4.rect,new Point(0,0),'==',0xffabcdef,0xffff0000,0xffffffff,false),4);
    assert.equal(canvas4.getPixel32(1,1),0xffff0000);
    const mDst=new BitmapData(2,2,true,0xff000000),mSrc=new BitmapData(2,2,true,0xffffffff);
    mDst.merge(mSrc,mSrc.rect,new Point(0,0),128,128,128,256);
    const mergedRed=(mDst.getPixel(0,0)>>16)&0xff;assert.ok(mergedRed>=127&&mergedRed<=128);
    const mClone=mSrc.clone();assert.equal(mSrc.compare(mClone),0);assert.equal(mSrc.compare(canvas4),-3);
    const diff=mSrc.compare(mDst);assert.equal(host.program().is(diff,'flash.display.BitmapData'),true);
    const pBuf=new BitmapData(2,2,false,0x010203);
    const rT=new Array(256).fill(0),gT=new Array(256).fill(0),bT=new Array(256).fill(0);
    rT[1]=0xaa0000;gT[2]=0x00bb00;bT[3]=0x0000cc;
    pBuf.paletteMap(pBuf,pBuf.rect,new Point(0,0),rT,gT,bT);
    assert.equal(pBuf.getPixel(0,0),0xaabbcc);
    pBuf.noise(7,10,200,BitmapDataChannel.RED|BitmapDataChannel.GREEN,false);
    pBuf.perlinNoise(4,4,2,11,false,true,7,false);
    assert.equal(typeof pBuf.pixelDissolve(mSrc,mSrc.rect,new Point(0,0),1,2,0xff123456),'number');
    assert.equal(canvas4.hitTest(new Point(0,0),1,new Point(1,1)),true);
    assert.equal(canvas4.hitTest(new Point(0,0),1,new Point(0,0)),false);
    assert.equal(canvas4.hitTest(new Point(0,0),1,new Rectangle(0,0,2,2)),true);
    assert.equal(canvas4.hitTest(new Point(4,0),1,scene.original,new Point(0,0),1),true);
    assert.equal(canvas4.hitTest(new Point(4,0),1,scene.sourceImage,new Point(0,0),1),true);
    canvas4.lock();canvas4.unlock();canvas4.unlock(canvas4.rect);
    const vec=canvas4.getVector(new Rectangle(1,1,2,2));
    assert.equal(vec.length,4);assert.equal(vec[0],0xffff0000);
    vec[0]=0xff13579b;canvas4.setVector(new Rectangle(1,1,2,2),vec);
    assert.equal(canvas4.getPixel32(1,1),0xff13579b);
    assert.throws(()=>canvas4.setVector(canvas4.rect,vec),RangeError);
    for(const tmp of [a,b,mask,canvas4,mDst,mSrc,mClone,diff,pBuf])tmp.dispose();
    const BitmapEncodingColorSpace=host.program().type('flash.display.BitmapEncodingColorSpace');
    const JPEGXREncoderOptions=host.program().type('flash.display.JPEGXREncoderOptions');
    assert.deepEqual([BitmapEncodingColorSpace.COLORSPACE_AUTO,BitmapEncodingColorSpace.COLORSPACE_4_4_4,BitmapEncodingColorSpace.COLORSPACE_4_2_2,BitmapEncodingColorSpace.COLORSPACE_4_2_0],['auto','4:4:4','4:2:2','4:2:0']);
    const jxr=new JPEGXREncoderOptions(15,BitmapEncodingColorSpace.COLORSPACE_4_2_2,2);
    assert.deepEqual([jxr.quantization,jxr.colorSpace,jxr.trimFlexBits],[15,'4:2:2',2]);
    assert.equal(host.program().is(scene.shared,'flash.display.IBitmapDrawable'),true);
    assert.equal(host.program().is(scene.sourceImage,'flash.display.IBitmapDrawable'),true);
    const histSrc=new BitmapData(2,2,true,0x80102030);
    const hist=histSrc.histogram();
    assert.equal(hist.length,4);
    assert.equal(hist[0][0x10],4);assert.equal(hist[1][0x20],4);assert.equal(hist[2][0x30],4);assert.equal(hist[3][0x80],4);
    const subHist=histSrc.histogram(new Rectangle(0,0,1,1));
    assert.equal(subHist[0][0x10],1);
    const qualDst=new BitmapData(2,2,true,0x00000000);
    qualDst.drawWithQuality(histSrc,null,null,null,null,false,'high');
    assert.equal(qualDst.getPixel32(0,0),0x80102030);
    assert.throws(()=>qualDst.drawWithQuality(histSrc,null,null,null,null,false,'ultra'),err=>err.name==='ArgumentError');
    assert.throws(()=>qualDst.drawWithQuality(histSrc,null,null,'shader',null,false,'high'),err=>err.name==='ArgumentError');
    const Shape=host.program().type('flash.display.Shape');
    const Sprite=host.program().type('flash.display.Sprite');
    const Bitmap=host.program().type('flash.display.Bitmap');
    const StageQuality=host.program().type('flash.display.StageQuality');
    const diagShape=new Shape();
    diagShape.graphics.beginFill(0xffffff,1);
    diagShape.graphics.moveTo(0,0);diagShape.graphics.lineTo(8,0);diagShape.graphics.lineTo(0,8);diagShape.graphics.endFill();
    const doLow=new BitmapData(8,8,true,0x00000000);
    const do8x8=new BitmapData(8,8,true,0x00000000);
    const do16x16=new BitmapData(8,8,true,0x00000000);
    doLow.drawWithQuality(diagShape,null,null,null,null,false,StageQuality.LOW);
    do8x8.drawWithQuality(diagShape,null,null,null,null,false,StageQuality.HIGH_8X8);
    do16x16.drawWithQuality(diagShape,null,null,null,null,false,StageQuality.HIGH_16X16);
    const lowAlphas=[],hi8Alphas=[],hi16Alphas=[];
    for(let py=0;py<8;py++)for(let px=0;px<8;px++){
      lowAlphas.push((doLow.getPixel32(px,py)>>>24)&0xff);
      hi8Alphas.push((do8x8.getPixel32(px,py)>>>24)&0xff);
      hi16Alphas.push((do16x16.getPixel32(px,py)>>>24)&0xff);
    }
    assert.equal(lowAlphas.every(v=>v===0||v===255),true,'StageQuality.LOW renders un-supersampled DisplayObject edges');
    assert.equal(hi8Alphas.some(v=>v>0&&v<255),true,'StageQuality.HIGH_8X8 supersamples DisplayObject diagonal edge');
    assert.equal(hi16Alphas.some(v=>v>0&&v<255),true,'StageQuality.HIGH_16X16 supersamples DisplayObject diagonal edge');
    const gammaSprite=new Sprite();
    gammaSprite.graphics.beginFill(0xff0000,1);gammaSprite.graphics.drawRect(0,0,2,4);gammaSprite.graphics.endFill();
    gammaSprite.graphics.beginFill(0x0000ff,1);gammaSprite.graphics.drawRect(2,0,2,4);gammaSprite.graphics.endFill();
    const halfMatrix=new (host.program().type('flash.geom.Matrix'))(0.25,0,0,0.25,0,0);
    const dstSrgb=new BitmapData(1,1,false,0x000000);
    const dstLin=new BitmapData(1,1,false,0x000000);
    dstSrgb.drawWithQuality(gammaSprite,halfMatrix,null,null,null,false,StageQuality.HIGH_8X8);
    dstLin.drawWithQuality(gammaSprite,halfMatrix,null,null,null,false,StageQuality.HIGH_8X8_LINEAR);
    const srgbRed=(dstSrgb.getPixel(0,0)>>>16)&0xff,linRed=(dstLin.getPixel(0,0)>>>16)&0xff;
    assert.ok(linRed>srgbRed+30,`DisplayObject HIGH_8X8_LINEAR red (${linRed}) must exceed sRGB (${srgbRed})`);
    const checkerSrc=new BitmapData(2,2,false,0x000000);
    checkerSrc.setPixel(0,0,0x000000);checkerSrc.setPixel(1,0,0xffffff);checkerSrc.setPixel(0,1,0xffffff);checkerSrc.setPixel(1,1,0x000000);
    const childBmp=new Bitmap(checkerSrc,'never',false);
    const bmpHolder=new Sprite();bmpHolder.addChild(childBmp);
    const upMatrix=new (host.program().type('flash.geom.Matrix'))(2,0,0,2,0,0);
    const bmpLowDst=new BitmapData(4,4,false,0);
    const bmpBestDst=new BitmapData(4,4,false,0);
    bmpLowDst.drawWithQuality(bmpHolder,upMatrix,null,null,null,false,StageQuality.LOW);
    bmpBestDst.drawWithQuality(bmpHolder,upMatrix,null,null,null,false,StageQuality.BEST);
    assert.equal(bmpLowDst.getPixel(1,1),0x000000,'LOW keeps nearest-neighbor child Bitmap sampling');
    const bestMid=(bmpBestDst.getPixel(1,1)>>>16)&0xff;
    assert.ok(bestMid>60&&bestMid<195,`BEST enables bilinear child Bitmap smoothing (${bestMid})`);
    for(const tmp of [doLow,do8x8,do16x16,dstSrgb,dstLin,checkerSrc,bmpLowDst,bmpBestDst])tmp.dispose();
    const blendDst=new BitmapData(2,2,true,0xff4080c0);
    const blendSrc=new BitmapData(2,2,true,0xff201010);
    blendDst.draw(blendSrc,null,null,'add');
    assert.equal(blendDst.getPixel32(0,0),0xff6090d0,'BitmapData.draw supports BlendMode.ADD');
    {
      const BlendModeCls=host.program().type('flash.display.BlendMode');
      const ShaderCls=host.program().type('flash.display.Shader');
      const ByteArrayCls=host.program().type('flash.utils.ByteArray');
      assert.equal(BlendModeCls.SHADER,'shader');
      const pbj2=new ByteArrayCls();
      // Binary .pbj 2-input BlendShader: r1 = sampleNearest(bg, outCoord); r2 = sampleNearest(fg, outCoord); r1.rgb += r2.rgb
      pbj2.writeByte(0xA5);pbj2.writeByte(1);pbj2.writeByte(0);pbj2.writeByte(0);pbj2.writeByte(0);
      pbj2.writeByte(0xA4);pbj2.writeByte(9);pbj2.writeByte(0);pbj2.writeUTFBytes('BlendAdd2');
      pbj2.writeByte(0xA3);pbj2.writeByte(0);pbj2.writeByte(4);pbj2.writeUTFBytes('bg');pbj2.writeByte(0);
      pbj2.writeByte(0xA3);pbj2.writeByte(1);pbj2.writeByte(4);pbj2.writeUTFBytes('fg');pbj2.writeByte(0);
      pbj2.writeByte(0x30);pbj2.writeByte(1);pbj2.writeByte(0);pbj2.writeByte(0xF0);pbj2.writeByte(0);pbj2.writeByte(0);pbj2.writeByte(0x10);pbj2.writeByte(0);
      pbj2.writeByte(0x30);pbj2.writeByte(2);pbj2.writeByte(0);pbj2.writeByte(0xF0);pbj2.writeByte(0);pbj2.writeByte(0);pbj2.writeByte(0x10);pbj2.writeByte(1);
      pbj2.writeByte(0x01);pbj2.writeByte(1);pbj2.writeByte(0);pbj2.writeByte(0xFC);pbj2.writeByte(2);pbj2.writeByte(0);pbj2.writeByte(0x1B);pbj2.writeByte(0);
      const twoInputShader=new ShaderCls(pbj2);
      const fgSp=new Sprite();
      fgSp.graphics.beginFill(0x203010,1);fgSp.graphics.drawRect(0,0,4,4);fgSp.graphics.endFill();
      fgSp.blendShader=twoInputShader;
      fgSp.blendMode=BlendModeCls.SHADER;
      assert.equal(fgSp.blendShader,twoInputShader);
      assert.equal(fgSp.blendMode,'shader');
      const directShaderDst=new BitmapData(6,6,true,0xff104020);
      directShaderDst.draw(fgSp,null,null,BlendModeCls.SHADER);
      assert.equal(directShaderDst.getPixel32(2,2)>>>0,0xff307030,'BitmapData.draw(..., BlendMode.SHADER) blends destination bg + foreground via 2-input Pixel Bender shader');
      assert.equal(directShaderDst.getPixel32(5,5)>>>0,0xff104020,'transparent foreground leaves destination pixels untouched');
      const parentComposite=new Sprite();
      const bgShape=new Shape();bgShape.graphics.beginFill(0x401050,1);bgShape.graphics.drawRect(0,0,6,6);bgShape.graphics.endFill();
      parentComposite.addChild(bgShape);
      parentComposite.addChild(fgSp);
      const stageShaderDst=new BitmapData(6,6,true,0x00000000);
      stageShaderDst.draw(parentComposite);
      assert.equal(stageShaderDst.getPixel32(2,2)>>>0,0xff604060,'DisplayObject child with blendMode=BlendMode.SHADER + blendShader composites against backdrop');
      assert.equal(stageShaderDst.getPixel32(5,5)>>>0,0xff401050);
      directShaderDst.dispose();stageShaderDst.dispose();
    }
    const DisplacementMapFilter=host.program().type('flash.filters.DisplacementMapFilter');
    const ShaderFilter=host.program().type('flash.filters.ShaderFilter');
    const mapBmp=new BitmapData(4,4,false,0x808080);
    const dmf=new DisplacementMapFilter(mapBmp,new Point(0,0),1,2,16,16,'clamp',0,0);
    const dispSrc=new BitmapData(4,4,true,0xff112233);
    dispSrc.setPixel32(1,1,0xffddeeff);
    const dispDst=new BitmapData(4,4,true,0);
    dispDst.applyFilter(dispSrc,dispSrc.rect,new Point(0,0),dmf);
    assert.equal(dispDst.getPixel32(1,1),0xffddeeff,'neutral 128 map preserves pixel position under DisplacementMapFilter');
    const sf=new ShaderFilter();
    sf.leftExtension=2;sf.rightExtension=3;sf.topExtension=4;sf.bottomExtension=5;
    const sfClone=sf.clone();
    assert.deepEqual([sfClone.leftExtension,sfClone.rightExtension,sfClone.topExtension,sfClone.bottomExtension],[2,3,4,5]);
    // Verify BitmapData.encode (PNG, JPEG, and JPEGXREncoderOptions + BitmapEncodingColorSpace) and lock()/unlock(changeRect)
    {
    const PngOpts=host.program().type('flash.display.PNGEncoderOptions');
    const JpgOpts=host.program().type('flash.display.JPEGEncoderOptions');
    const JxrOpts=host.program().type('flash.display.JPEGXREncoderOptions');
    const ColorSpaceEnum=host.program().type('flash.display.BitmapEncodingColorSpace');
    assert.equal(ColorSpaceEnum.COLORSPACE_AUTO,'auto');
    assert.equal(ColorSpaceEnum.COLORSPACE_4_4_4,'4:4:4');
    assert.equal(ColorSpaceEnum.COLORSPACE_4_2_2,'4:2:2');
    assert.equal(ColorSpaceEnum.COLORSPACE_4_2_0,'4:2:0');
    const jxrDefaults=new JxrOpts();
    assert.deepEqual([jxrDefaults.quantization,jxrDefaults.colorSpace,jxrDefaults.trimFlexBits],[20,'auto',0]);
    const encSrc=new BitmapData(8,8,true,0x00000000);
    for(let y=0;y<8;y++)for(let x=0;x<8;x++){
      encSrc.setPixel32(x,y,(((0x80+((x*15)&0x7f))<<24)|(((x*31)&0xff)<<16)|(((y*29)&0xff)<<8)|(((x+y)*17)&0xff))>>>0);
    }
    const pngEncoded=encSrc.encode(encSrc.rect,new PngOpts(true));
    pngEncoded.position=0;
    assert.deepEqual([pngEncoded.readUnsignedByte(),pngEncoded.readUnsignedByte(),pngEncoded.readUnsignedByte(),pngEncoded.readUnsignedByte()],[0x89,0x50,0x4e,0x47]);
    const jpgEncoded=encSrc.encode(encSrc.rect,new JpgOpts(85));
    jpgEncoded.position=0;
    assert.deepEqual([jpgEncoded.readUnsignedByte(),jpgEncoded.readUnsignedByte()],[0xff,0xd8]);
    assert.throws(()=>new JxrOpts(20,'invalid-cs',0),err=>err.name==='ArgumentError'&&err.errorID===2008);
    assert.throws(()=>{const o=new JxrOpts();o.colorSpace=null;},err=>err instanceof TypeError&&err.errorID===2007);
    assert.throws(()=>encSrc.encode(null,new JxrOpts()),err=>err instanceof TypeError&&err.errorID===2007);
    assert.throws(()=>encSrc.encode(encSrc.rect,null),err=>err instanceof TypeError&&err.errorID===2007);

    // ITU-T T.832 JPEG-XR (.jxr): odd-dimension (19x13) lossless RGB & RGBA bit-exact round-trip
    const ByteArrayClsForJxr=host.program().type('flash.utils.ByteArray');
    const oddRgba=new BitmapData(19,13,true,0x00000000);
    const oddRgb=new BitmapData(19,13,false,0x000000);
    let seedJxr=0x9e3779b9>>>0;
    for(let y=0;y<13;y++)for(let x=0;x<19;x++){
      seedJxr=(Math.imul(seedJxr,1664525)+1013904223)>>>0;
      const a=0x40+((seedJxr>>>24)&0xbf);
      const r=(seedJxr>>>16)&0xff,g=(seedJxr>>>8)&0xff,b=seedJxr&0xff;
      oddRgba.setPixel32(x,y,((a<<24)|(r<<16)|(g<<8)|b)>>>0);
      oddRgb.setPixel(x,y,((r<<16)|(g<<8)|b)>>>0);
    }
    const expectedOddRgba=pixels(oddRgba);
    const expectedOddRgb=pixels(oddRgb);

    const jxrLosslessRgba=oddRgba.encode(oddRgba.rect,new JxrOpts(0,ColorSpaceEnum.COLORSPACE_4_4_4,0));
    const jxrRgbaBytes=host.binary().toBytes(jxrLosslessRgba);
    assert.deepEqual(Array.from(jxrRgbaBytes.slice(0,4)),[0x49,0x49,0xbc,0x01],'ITU-T T.832 .jxr magic header');
    assert.ok(Buffer.from(jxrRgbaBytes).includes(Buffer.from('WMPHOTO\0','ascii')),'ITU-T T.832 WMPHOTO\\0 IMAGE_HEADER signature');
    const decodedRgbaRef=JSON.parse(player.pixel_decode(jxrRgbaBytes));
    const roundtripRgba=bitmap.fromRef(decodedRgbaRef);
    assert.deepEqual([roundtripRgba.width,roundtripRgba.height],[19,13]);
    assert.deepEqual(pixels(roundtripRgba),expectedOddRgba,'lossless 4:4:4 RGBA 19x13 round-trips bit-exact (max_diff == 0)');

    const outIntoBa=new ByteArrayClsForJxr();
    const returnedBa=oddRgb.encode(oddRgb.rect,new JxrOpts(0,ColorSpaceEnum.COLORSPACE_AUTO,0),outIntoBa);
    assert.equal(returnedBa,outIntoBa);
    const jxrRgbBytes=host.binary().toBytes(outIntoBa);
    assert.deepEqual(Array.from(jxrRgbBytes.slice(0,4)),[0x49,0x49,0xbc,0x01]);
    const decodedRgbRef=JSON.parse(player.pixel_decode(jxrRgbBytes));
    const roundtripRgb=bitmap.fromRef(decodedRgbRef);
    assert.deepEqual(pixels(roundtripRgb),expectedOddRgb,'lossless AUTO/4:4:4 opaque RGB 19x13 round-trips bit-exact (max_diff == 0)');

    // Smooth gradient PSNR + monotonic byte sizes across quantization (20/60/90), subsampling (4:4:4/4:2:2/4:2:0), and trimFlexBits (0 vs 4)
    const gradBmp=new BitmapData(48,32,false,0x000000);
    const texBmp=new BitmapData(48,32,false,0x000000);
    let texRng=0x2468ace1>>>0;
    for(let y=0;y<32;y++)for(let x=0;x<48;x++){
      const r=Math.round((x*255)/47),g=Math.round((y*255)/31),b=Math.round(((x+y)*255)/78);
      gradBmp.setPixel(x,y,(r<<16)|(g<<8)|b);
      texRng=(Math.imul(texRng,1664525)+1013904223)>>>0;
      const tr=Math.min(255,r+((texRng>>>24)&0x1f)),tg=Math.min(255,g+((texRng>>>16)&0x1f)),tb=Math.min(255,b+((texRng>>>8)&0x1f));
      texBmp.setPixel(x,y,(tr<<16)|(tg<<8)|tb);
    }
    const origGrad=pixels(gradBmp);
    const computePsnr=(refPx,decPx)=>{
      let mse=0,n=0;
      for(let i=0;i<refPx.length;i+=4)for(let c=0;c<3;c++){const d=refPx[i+c]-decPx[i+c];mse+=d*d;n++;}
      mse/=n;return mse===0?Infinity:10*Math.log10((255*255)/mse);
    };
    for(const cs of [ColorSpaceEnum.COLORSPACE_4_4_4,ColorSpaceEnum.COLORSPACE_4_2_2,ColorSpaceEnum.COLORSPACE_4_2_0]){
      const encBa=gradBmp.encode(gradBmp.rect,new JxrOpts(20,cs,0));
      const decBmp=bitmap.fromRef(JSON.parse(player.pixel_decode(host.binary().toBytes(encBa))));
      const psnr=computePsnr(origGrad,pixels(decBmp));
      assert.ok(psnr>36,`expected PSNR > 36 dB for ${cs} at q=20, got ${psnr.toFixed(2)} dB`);
      decBmp.dispose();
    }
    const szQ0=texBmp.encode(texBmp.rect,new JxrOpts(0,ColorSpaceEnum.COLORSPACE_4_4_4,0)).length;
    const szQ20=texBmp.encode(texBmp.rect,new JxrOpts(20,ColorSpaceEnum.COLORSPACE_4_4_4,0)).length;
    const szQ60=texBmp.encode(texBmp.rect,new JxrOpts(60,ColorSpaceEnum.COLORSPACE_4_4_4,0)).length;
    const szQ90=texBmp.encode(texBmp.rect,new JxrOpts(90,ColorSpaceEnum.COLORSPACE_4_4_4,0)).length;
    assert.ok(szQ0>szQ20&&szQ20>szQ60&&szQ60>szQ90,`expected monotonic quantization byte sizes: ${szQ0} > ${szQ20} > ${szQ60} > ${szQ90}`);

    const sz444=texBmp.encode(texBmp.rect,new JxrOpts(10,ColorSpaceEnum.COLORSPACE_4_4_4,0)).length;
    const sz422=texBmp.encode(texBmp.rect,new JxrOpts(10,ColorSpaceEnum.COLORSPACE_4_2_2,0)).length;
    const sz420=texBmp.encode(texBmp.rect,new JxrOpts(10,ColorSpaceEnum.COLORSPACE_4_2_0,0)).length;
    assert.ok(sz444>sz422&&sz422>sz420,`expected monotonic chroma subsampling byte sizes: ${sz444} > ${sz422} > ${sz420}`);

    const szTrim0=texBmp.encode(texBmp.rect,new JxrOpts(20,ColorSpaceEnum.COLORSPACE_4_4_4,0)).length;
    const szTrim4=texBmp.encode(texBmp.rect,new JxrOpts(20,ColorSpaceEnum.COLORSPACE_4_4_4,4)).length;
    assert.ok(szTrim0>szTrim4,`expected trimFlexBits=4 (${szTrim4}) < trimFlexBits=0 (${szTrim0})`);

    for(const tmp of [oddRgba,oddRgb,roundtripRgba,roundtripRgb,gradBmp,texBmp])tmp.dispose();

    const RectCls=host.program().type('flash.geom.Rectangle');
    const lockBmp=new BitmapData(8,8,true,0xff112233);
    const lockKey=JSON.stringify(host.bitmap().refOf(lockBmp));
    const flushesBefore=host.bitmap().cache.stats().invalidationFlushes;
    lockBmp.lock();
    lockBmp.lock();
    assert.equal(host.bitmap().cache.isLocked(lockBmp),true,'nested lock() keeps BitmapData locked');
    lockBmp.setPixel32(1,1,0xffff0000);
    lockBmp.setPixel32(6,6,0xff00ff00);
    assert.equal(host.bitmap().cache.stats().invalidationFlushes,flushesBefore,'mutations while locked defer display invalidation');
    assert.equal(host.bitmap().cache.effectivePixels(lockKey)[(1*8+1)*4],0x11,'locked display surface keeps pre-lock pixels');
    lockBmp.unlock(new RectCls(0,0,4,4));
    assert.equal(host.bitmap().cache.isLocked(lockBmp),true,'inner unlock() keeps outer lock active');
    lockBmp.unlock(new RectCls(0,0,4,4));
    assert.equal(host.bitmap().cache.isLocked(lockBmp),false,'outer unlock() releases lock and flushes changeRect');
    assert.equal(host.bitmap().cache.stats().invalidationFlushes,flushesBefore+1,'coalesced unlock flushes once');
    const postPartial=host.bitmap().cache.effectivePixels(lockKey);
    assert.equal(postPartial[(1*8+1)*4],0xff,'pixel inside unlock(changeRect) updates display surface');
    assert.equal(postPartial[(6*8+6)*4+1],0x22,'pixel outside unlock(changeRect) stays deferred until full unlock');
    lockBmp.lock();
    lockBmp.unlock();
    assert.equal(host.bitmap().cache.effectivePixels(lockKey)[(6*8+6)*4+1],0xff,'full unlock() syncs entire BitmapData');
    encSrc.dispose();lockBmp.dispose();
    }
    histSrc.dispose();qualDst.dispose();blendDst.dispose();blendSrc.dispose();mapBmp.dispose();dispSrc.dispose();dispDst.dispose();
    const old=scene.shared;
    host.resetProject();const fresh=host.proxyFor([]);
    assert.equal(host.lastTrace(),'bitmaps ready 16 16 0');
    assert.notEqual(fresh.shared,old);assert.throws(()=>old.width,/previous publication|disposed/);
    assert.deepEqual(pixels(fresh.original),sourceBefore);
    assert.equal(fresh.shared.getPixel32(4,4),0xfff0f6ff);
    const live=fresh.shared;host.disposeFamily();assert.throws(()=>live.width,/disposed/);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Bitmap pixels: original PNG, shared views, clone/copy/scroll, AS2 attachment, Rust RGBA uploads, disposal and Restart passed.');
}
