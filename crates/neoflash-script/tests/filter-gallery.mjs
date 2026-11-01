// One publication combines copied filter values, shared graph ownership,
// nested/sequential effects, two dialects and unmodified source pixels.
// Actual subtree Canvas rasterization is accepted in the ordinary browser export.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';
export async function verifyFilterGallery(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/filter-gallery-'));let player,host;
  try{
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/filter-gallery.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:Array.from(line).length*9})}}));host.dispatchScripts();
    let scene=host.proxyFor([]);const cards=Array.from(scene.cards),nested=scene.nested,legacy=scene.legacyCard,filterHost=host.filters();
    const key=value=>JSON.stringify(value.toJSON().object_ref),snapshot=()=>JSON.parse(player.runtime_snapshot_json());
    const sourcePixels=()=>Array.from(player.bitmap_pixels(JSON.stringify(host.bitmap().refOf(scene.shared))));const original=sourcePixels();
    assert.equal(cards.length,4);assert.equal(scene.copySafe,true);assert.equal(scene.seed.blurX,40);assert.equal(cards[0].filters[0].blurX,8);
    const types=['BlurFilter','GlowFilter','DropShadowFilter','ColorMatrixFilter'];cards.forEach((card,index)=>{assert.equal(host.program().is(card.filters[0],'flash.filters.'+types[index]),true);assert.equal(host.program().is(card.filters[0],'flash.filters.BitmapFilter'),true);});
    assert.doesNotThrow(()=>host.stage().handlePointer('pointerdown',{button:0},cards[1].getChildAt(2)),'a nonselectable caption keeps pointer routing without opening a native editor');
    const copy=cards[0].filters;copy[0].blurX=2;copy.push(copy[0].clone());assert.equal(cards[0].filters.length,1);assert.equal(cards[0].filters[0].blurX,8);
    const matrixFilter=cards[3].filters[0],matrix=matrixFilter.matrix;matrix[0]=99;assert.equal(matrixFilter.matrix[0],0.2126);assert.notEqual(matrixFilter.clone(),matrixFilter);
    const before=JSON.stringify(filterHost.toSpecs(cards[0].filters));assert.throws(()=>{cards[0].filters=[{}];},/filter/i);assert.equal(JSON.stringify(filterHost.toSpecs(cards[0].filters)),before,'invalid assignments preserve the complete old filter list');
    const snap=snapshot(),groups=snap.filter_groups,owner=value=>groups.find(group=>JSON.stringify(group.owner_ref)===key(value));
    for(const card of cards){assert.ok(owner(card));assert.ok(snap.draws.some(draw=>draw.filter_id===owner(card).id));}
    assert.equal(owner(nested).parent_id,owner(cards[2]).id);assert.equal(owner(legacy).filters.length,2);
    const sharedDraws=snap.draws.filter(draw=>draw.kind.type==='bitmap_pixels');assert.equal(sharedDraws.length,4);assert.ok(sharedDraws.every(draw=>JSON.stringify(draw.kind.pixel_ref)===JSON.stringify(sharedDraws[0].kind.pixel_ref)));
    const cm=new filterHost.ColorMatrixFilter([0,0,1,0,0,1,0,0,0,0,0,1,0,0,0,0,0,0,1,0]);
    assert.deepEqual(Array.from(player.filter_rgba(1,1,new Uint8Array([10,20,30,255]),JSON.stringify(filterHost.toSpecs([cm])))),[30,10,20,255],'the real Rust kernel applies ordered channel coefficients');
    host.stage().handlePointer('click',{},cards[1]);host.stage().handlePointer('pointerdown',{button:0},legacy);assert.deepEqual([scene.clicks,scene.legacyClicks],[1,1]);
    const cycle=()=>{player.click(JSON.stringify(['cycle-button']));host.dispatchScripts(['cycle-button']);};
    cycle();assert.equal(scene.phase,1);assert.equal(cards[1].filters[0].inner,true);assert.equal(cards[2].filters[0].knockout,true);assert.equal(cards[2].rotation,-4);assert.equal(legacy.filters.length,2);assert.deepEqual(sourcePixels(),original);
    cycle();assert.equal(scene.phase,2);assert.ok(cards.every(card=>card.filters.length===0));assert.equal(nested.filters.length,0);assert.equal(legacy.filters.length,0);assert.equal(snapshot().filter_groups.length,0);
    host.stage().handlePointer('click',{},cards[1]);assert.equal(scene.clicks,2,'clearing filters keeps the same listener/instance');cycle();assert.equal(scene.phase,0);assert.equal(cards[0].filters[0].blurX,8);assert.deepEqual(sourcePixels(),original);assert.ok(cards.every((card,index)=>card===scene.cards[index]));
    const BlendMode=host.display().builtins['flash.display.BlendMode'];
    assert.equal(BlendMode.MULTIPLY,'multiply');assert.equal(BlendMode.SHADER,'shader');
    assert.equal(cards[0].blendMode,'normal');assert.equal(cards[0].cacheAsBitmap,true,'active filters enable cacheAsBitmap');assert.equal(cards[0].opaqueBackground,null);
    cards[0].filters=[];assert.equal(cards[0].cacheAsBitmap,false);
    cards[0].blendMode=BlendMode.MULTIPLY;cards[0].cacheAsBitmap=true;cards[0].opaqueBackground=0x112233;
    assert.deepEqual([cards[0].blendMode,cards[0].cacheAsBitmap,cards[0].opaqueBackground],['multiply',true,0x112233]);
    assert.throws(()=>{cards[0].blendMode=BlendMode.SHADER;},/shader|blendMode/i);
    assert.throws(()=>{cards[0].blendMode=3;},/String|blendMode/i);
    assert.equal(cards[0].blendMode,'multiply');
    const blendGroup=snapshot().filter_groups.find(group=>JSON.stringify(group.owner_ref)===key(cards[0]));
    assert.ok(blendGroup);assert.equal(blendGroup.composite_operation,'multiply');assert.equal(blendGroup.cache_as_bitmap,true);assert.deepEqual(blendGroup.opaque_background,[0x11,0x22,0x33,255]);
    cards[0].opaqueBackground=null;cards[0].cacheAsBitmap=false;cards[0].blendMode=BlendMode.NORMAL;
    assert.deepEqual([cards[0].blendMode,cards[0].cacheAsBitmap,cards[0].opaqueBackground],['normal',false,null]);
    host.resetProject();scene=host.proxyFor([]);assert.deepEqual([scene.phase,scene.clicks,scene.legacyClicks],[0,0,0]);assert.equal(scene.copySafe,true);assert.throws(()=>cards[0].filters,/removed|replaced|stale|session/i);host.stage().handlePointer('click',{},scene.cards[1]);assert.equal(scene.clicks,1);
  }finally{host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Filter gallery: actual Rust/WASM values/pixels, copied filters, nested graph groups, AS2 order, clear/restore identity and Restart passed.');
}
