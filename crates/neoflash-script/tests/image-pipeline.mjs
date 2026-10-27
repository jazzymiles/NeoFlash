// One published pipeline. Pixel operations/PNG/decode use actual Rust/WASM.
// Canvas records painter calls and returns controlled pixels: raster appearance
// is verified in the ordinary browser export, not emulated in this Node test.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';
async function until(predicate,label){
  const deadline=Date.now()+4000;
  while(!predicate()&&Date.now()<deadline)await new Promise(done=>setTimeout(done,5));
  assert.ok(predicate(),label);
}
function recordingCanvas(){
  const uploads=[],draws=[],reads=[];
  const createCanvas=(width,height)=>{
    const surface={width,height};let matrix=[1,0,0,1,0,0];const stack=[];
    const context={globalAlpha:1,imageSmoothingEnabled:false,globalCompositeOperation:'source-over',
      save(){stack.push({matrix:matrix.slice(),alpha:this.globalAlpha,smoothing:this.imageSmoothingEnabled,composite:this.globalCompositeOperation});},
      restore(){const saved=stack.pop();matrix=saved.matrix;this.globalAlpha=saved.alpha;this.imageSmoothingEnabled=saved.smoothing;this.globalCompositeOperation=saved.composite;},
      setTransform(...values){matrix=values.length===1?[values[0].a,values[0].b,values[0].c,values[0].d,values[0].e,values[0].f]:values;},
      resetTransform(){matrix=[1,0,0,1,0,0];},
      transform(a,b,c,d,e,f){const m=matrix;matrix=[m[0]*a+m[2]*b,m[1]*a+m[3]*b,m[0]*c+m[2]*d,m[1]*c+m[3]*d,m[0]*e+m[2]*f+m[4],m[1]*e+m[3]*f+m[5]];},
      getTransform(){const[a,b,c,d,e,f]=matrix;return{a,b,c,d,e,f};},
      clearRect(){},beginPath(){},rect(){},clip(){},
      createImageData:(w,h)=>({width:w,height:h,data:new Uint8ClampedArray(w*h*4)}),
      putImageData(image){surface.upload=Uint8Array.from(image.data);uploads.push({surface,length:image.data.length});},
      drawImage(source,...args){draws.push({surface,source,args,matrix:matrix.slice(),alpha:this.globalAlpha,smoothing:this.imageSmoothingEnabled});},
      getImageData(x,y,w,h){const data=new Uint8ClampedArray(w*h*4);for(let i=0;i<data.length;i+=4)data.set([46,76,108,255],i);reads.push({x,y,w,h});return{width:w,height:h,data};},
    };
    surface.getContext=()=>context;context.canvas=surface;return surface;
  };
  return{createCanvas,uploads,draws,reads};
}
export async function verifyImagePipeline(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/image-pipeline-'));let player,host;
  const canvas=recordingCanvas();
  try{
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/image-pipeline.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,bitmap:{createCanvas:canvas.createCanvas},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();let scene=host.proxyFor([]);
    await until(()=>scene.loads===1,'initial PNG round-trip completes');
    const pixelBytes=value=>Array.from(player.bitmap_pixels(JSON.stringify(host.bitmap().refOf(value))));
    const original=pixelBytes(scene.source),processed=pixelBytes(scene.processed),captured=pixelBytes(scene.captured);
    assert.equal(scene['process-button']['process-label'].selectable,false);
    assert.equal(scene.sourceStable,true);assert.equal(scene.reloadedMatches,true);
    assert.equal(scene.rejectionCaught,true,'AS3 catch(Error) receives a normalized Rust refusal without modifying source pixels');
    assert.equal(scene.source.getPixel32(24,24),0xfff4e8b8);assert.notEqual(scene.tinted.getPixel32(24,24),scene.source.getPixel32(24,24));
    assert.ok(scene.affected.width>=scene.source.width&&scene.affected.height>=scene.source.height);
    assert.equal(scene.options.fastCompression,true);assert.equal(host.program().is(scene.options,'flash.display.PNGEncoderOptions'),true);
    assert.equal(scene.raw.length,86016);assert.equal(scene.rawEnd,86016);assert.equal(scene.copiedEnd,86020);
    assert.equal(scene.prefixed.length,86020);scene.prefixed.position=0;assert.equal(scene.prefixed.readUnsignedInt(),0x4e465049);
    assert.deepEqual(pixelBytes(scene.roundTrip),captured);assert.deepEqual(pixelBytes(scene.picture.content.bitmapData),captured,'PNG is encoded and decoded by real Rust without changing pixels');
    assert.deepEqual(Array.from(host.binary().toBytes(scene.png).slice(0,8)),[137,80,78,71,13,10,26,10]);
    assert.equal(scene.captured.getPixel32(32,32),0xff2e4c6c,'controlled Canvas readback is committed to the shared arena');
    assert.ok(canvas.draws.some(draw=>draw.source.upload?.length===96*64*4&&draw.matrix[0]===.75&&draw.matrix[4]===8&&draw.matrix[5]===16&&draw.alpha===1),'actual shared painter uses the normalized child transform and uploaded source surface');
    assert.ok(canvas.draws.some(draw=>draw.source.upload?.length===112*80*4&&draw.matrix[4]===72&&draw.matrix[5]===8),'nested child placement reaches the snapshot painter');
    assert.ok(canvas.reads.some(read=>read.w===192&&read.h===112));
    assert.equal(scene.legacyResult,0);assert.equal(scene.legacyPhase,0);
    assert.equal(scene.legacyCanvas.getPixel32(0,0),0xff2e4c6c);
    const bytes=new(host.binary().ByteArray)();bytes.writeUnsignedInt(0xffef7836);bytes.position=0;
    const partial=scene.source.clone(),second=partial.getPixel32(1,0);
    assert.throws(()=>partial.setPixels(new(host.program().type('flash.geom.Rectangle'))(0,0,2,1),bytes),/EOF|end|enough|short/i);
    assert.equal(bytes.position,4);assert.equal(partial.getPixel32(0,0),0xffef7836);assert.equal(partial.getPixel32(1,0),second,'underrun preserves only the completed prefix');
    partial.dispose();assert.throws(()=>partial.getPixels(partial.rect),/disposed/i);
    player.click(JSON.stringify(['process-button']));host.dispatchScripts(['process-button']);await until(()=>scene.loads===2,'next palette PNG completes');
    assert.equal(scene.phase,1);assert.equal(scene.legacyPhase,1);assert.equal(scene.legacyResult,0);assert.equal(scene.reloadedMatches,true);
    assert.deepEqual(pixelBytes(scene.source),original);assert.notDeepEqual(pixelBytes(scene.processed),processed);
    assert.equal(scene.pipelineStatus.text,'Palette 1 / PNG round-trip true / source unchanged true');
    const old=scene.captured;host.resetProject();scene=host.proxyFor([]);await until(()=>scene.loads===1,'fresh PNG session completes');
    assert.equal(scene.phase,0);assert.equal(scene.legacyPhase,0);assert.deepEqual(pixelBytes(scene.source),original);assert.equal(scene.reloadedMatches,true);
    assert.throws(()=>old.getPixel32(0,0),/previous|disposed|stale/i);
  }finally{host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Image pipeline: actual Rust tint/filter/bulk/PNG/decode, normalized recording-painter snapshot, AS2 draw/filter, partial EOF and Restart passed.');
}
