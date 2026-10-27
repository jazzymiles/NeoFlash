// One published composition exercises the actual Rust affine/color state,
// host facades and pixel uploads. Browser rasterization remains separate QA.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyDisplayTransforms(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/display-transforms-'));
  let player,host;
  const surfaces=[];
  const createCanvas=(width,height)=>{
    const surface={width,height,getContext:()=>({
      createImageData:(w,h)=>({data:new Uint8ClampedArray(w*h*4)}),
      putImageData:image=>{surface.bytes=Array.from(image.data);},
    })};
    surfaces.push(surface);return surface;
  };
  const components=matrix=>['a','b','c','d','tx','ty'].map(key=>matrix[key]);
  const close=(actual,expected)=>actual.forEach((value,index)=>assert.ok(Math.abs(value-expected[index])<1e-8,`${actual} differs from ${expected}`));
  const skew=matrix=>(matrix.a*matrix.c+matrix.b*matrix.d)/(Math.hypot(matrix.a,matrix.b)*Math.hypot(matrix.c,matrix.d));
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/display-transforms.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,bitmap:{createCanvas,maxCacheEntries:2,maxCacheBytes:4096},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    const scene=host.proxyFor([]),card=scene.card,bound=scene.rig.transform,program=host.program();
    const bitmap=host.bitmap(),ref=value=>value.toJSON().object_ref;
    const draws=()=>JSON.parse(player.scene_json());
    const drawFor=value=>draws().find(item=>JSON.stringify(item.owner_ref)===JSON.stringify(ref(value)));
    const pixels=()=>Array.from(player.bitmap_pixels(JSON.stringify(bitmap.refOf(scene.shared))));
    const originalPixels=pixels(),revision=drawFor(scene.leftView).kind.revision;
    assert.equal(scene.phase,0);assert.equal(program.type('app.TransformCard').created,1);
    assert.equal(scene.leftView.bitmapData,scene.rightView.bitmapData);
    assert.equal(scene.rig.transform,bound,'display getter returns the stable bound facade');
    assert.equal(program.is(bound,'flash.geom.Transform'),true);
    assert.equal(program.is(bound.colorTransform,'flash.geom.ColorTransform'),true);
    const Transform=program.type('flash.geom.Transform');
    close(components(new Transform(scene.rig).matrix),components(bound.matrix));
    const originalMatrix=components(bound.matrix),copy=bound.matrix,originalSkew=skew(copy);
    copy.tx+=10;close(components(bound.matrix),originalMatrix);
    bound.matrix=copy;assert.equal(scene.rig.x,originalMatrix[4]+10);
    copy.tx=originalMatrix[4];bound.matrix=copy;
    const local=card.transform.getRelativeMatrix(scene.rig);
    close(components(local),[1,0,0,1,14,6]);
    const copyColor=bound.colorTransform;copyColor.redOffset=50;
    assert.equal(bound.colorTransform.redOffset,0,'color getter also returns a value copy');
    assert.equal(scene.legacyTransform.colorTransform.color,0x23c7b0);
    close(components(scene.legacyTransform.matrix),[1,0.06,0.2,1,68,488]);
    assert.equal(drawFor(card.caption).kind.field.format.color,0xeff8ff);

    // Two distinct derived surfaces share immutable source pixels and revision.
    const painted=[],consumer={save(){},restore(){},getTransform:()=>({a:1,b:0,c:0,d:1,e:0,f:0}),setTransform(){},drawImage(surface){painted.push(surface);}};
    bitmap.cache.beginFrame();
    bitmap.cache.draw(consumer,drawFor(scene.leftView).kind);
    bitmap.cache.draw(consumer,drawFor(scene.rightView).kind);
    assert.notEqual(painted[0],painted[1]);assert.notDeepEqual(painted[0].bytes,painted[1].bytes);
    assert.equal(bitmap.cache.stats().entries,2);assert.ok(bitmap.cache.stats().bytes<=4096);
    assert.equal(bitmap.cache.hit(drawFor(scene.leftView).kind,0,0),false);
    bitmap.cache.endFrame();
    const click=()=>{player.click(JSON.stringify(['cycle-button']));host.dispatchScripts(['cycle-button']);};
    click();assert.equal(scene.phase,1);
    assert.ok(Math.abs(skew(bound.matrix)-originalSkew)<1e-10,'scalar rotation and scaling preserve the angle between matrix columns');
    assert.equal(drawFor(card.caption).kind.field.format.color,0xade3ff);
    assert.equal(drawFor(card.caption).kind.field.alpha,209);
    assert.deepEqual(draws().find(item=>item.element_id==='card-face').kind.fill,[49,131,175,209]);
    assert.equal(card.transform.concatenatedColorTransform.alphaOffset,30);
    bitmap.cache.beginFrame();
    bitmap.cache.draw(consumer,drawFor(scene.leftView).kind);
    assert.equal(bitmap.cache.hit(drawFor(scene.leftView).kind,0,0),true,'alpha offset reveals originally transparent pixels');
    assert.equal(painted[2].bytes[3],70);
    assert.ok(bitmap.cache.stats().entries<=2);assert.ok(bitmap.cache.stats().bytes<=4096,'derived tint variants obey the configured cache budget');
    bitmap.cache.endFrame();
    assert.equal(drawFor(scene.leftView).kind.revision,revision);assert.deepEqual(pixels(),originalPixels);
    const world=components(card.transform.concatenatedMatrix);
    click();assert.equal(scene.phase,2);assert.equal(scene.card,card);assert.equal(card.parent,scene);
    close(components(card.transform.concatenatedMatrix),world);
    assert.equal(program.type('app.TransformCard').created,1,'reparent keeps linked constructor and object identity');
    click();assert.equal(scene.phase,0);assert.equal(card.parent,scene.rig);close(components(bound.matrix),originalMatrix);
    assert.deepEqual(pixels(),originalPixels);assert.equal(drawFor(scene.leftView).kind.revision,revision);
    host.resetProject();const fresh=host.proxyFor([]);
    assert.equal(fresh.phase,0);assert.notEqual(fresh.card,card);assert.equal(host.program().type('app.TransformCard').created,1);
    assert.throws(()=>bound.matrix,/stale|previous|no longer|live|retired|removed|replaced|another player/i);
    close(components(fresh.rig.transform.matrix),originalMatrix);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Display transforms: published nested skew/scalar edits, copied values, vector/text color, independent shared-pixel tint/alpha, linked reparent and Restart passed.');
}
