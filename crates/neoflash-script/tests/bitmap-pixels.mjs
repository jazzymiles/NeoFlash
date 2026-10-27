// One published image/pixel workflow; pixel truth comes from actual Rust/WASM.
// The minimal Canvas surface observes uploads only, not browser rasterization.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
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
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/bitmap-pixels.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
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
    assert.throws(()=>scene.shared.drawWithQuality(scene.sourceImage),/not implemented/);
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
