// One published scene through the real WASM graph, host picker and Stage pointer
// route. The EventTarget boundary is synthetic; root's browser pass verifies
// native pointer capture, cursor presentation and physical gestures.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

function pointerBoundary(width,height){
  const canvas=new EventTarget(),outside=new EventTarget(),captures=new Set(),errors=[];
  Object.assign(canvas,{width,height,style:{},getBoundingClientRect:()=>({left:0,top:0,width,height}),focus(){},
    setPointerCapture:id=>captures.add(id),hasPointerCapture:id=>captures.has(id),releasePointerCapture:id=>captures.delete(id)});
  function send(type,point,target=canvas){
    const event=new Event(type,{cancelable:true});
    Object.defineProperties(event,Object.fromEntries(Object.entries({clientX:point.x,clientY:point.y,pointerId:1,pointerType:'mouse',isPrimary:true,button:0,buttons:type==='pointerup'?0:1}).map(([key,value])=>[key,{value}])));
    target.dispatchEvent(event);assert.deepEqual(errors,[],'native input route must not hide script errors');
  }
  return{canvas,outside,captures,errors,send};
}
const close=(actual,expected)=>assert.ok(Math.abs(actual-expected)<1e-7,`${actual} != ${expected}`);
export async function verifyDragDrop(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/drag-drop-'));let player,host;
  try{
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/drag-drop.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    const input=pointerBoundary(bundle.project.width,bundle.project.height);
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,canvas:input.canvas,stage:{pointerTarget:input.outside,onError:error=>input.errors.push(error)},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();host.stage().attach();let scene=host.proxyFor([]),card=scene.card,legacy=scene.legacyRig.card;
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    const at=(target,x,y)=>target.localToGlobal({x,y});
    assert.deepEqual([card.x,card.y,legacy.x,legacy.y,scene.presses,scene.legacyPresses],[20,20,20,20,0,0]);
    assert.equal(scene.modeButton.caption.selectable,false);assert.equal(card.caption.selectable,false);assert.equal(legacy.caption.selectable,false);
    assert.equal(scene.pickArea.visible,false);assert.equal(scene.pickArea.mouseEnabled,false);assert.equal(card.hitArea,scene.pickArea);
    // The circular badge's Stage bounding-box corner is outside its actual fill.
    const badge=card.badge,bounds=badge.getBounds(host.stage().stage),corner={x:bounds.x+.1,y:bounds.y+.1};
    assert.equal(badge.hitTestPoint(corner.x,corner.y,false),true);assert.equal(badge.hitTestPoint(corner.x,corner.y,true),false);
    // Begin in the invisible donor margin, outside the card's painted rectangle.
    const margin=at(card,-4,35);input.send('pointermove',margin);assert.equal(input.canvas.style.cursor,'pointer');
    input.send('pointerdown',margin);assert.equal(scene.presses,1);assert.equal(scene.lastTarget,'card');assert.equal(input.captures.has(1),true);
    close(scene.localPressX,-4);close(scene.localPressY,35);
    const drop=at(scene.as3Rig,246,180);input.send('pointermove',drop);input.send('pointerup',drop);
    close(card.x,250);close(card.y,145);assert.equal(scene.releases,1);assert.equal(scene.dragging,false);assert.equal(scene.lastHit,true);assert.equal(scene.shapeHit,true);assert.equal(scene.lastDrop,'dock');assert.equal(input.captures.size,0);
    // Child targeting and disabled input leave scripted geometry queries intact.
    click('reset-button');click('mode-button');assert.equal(card.mouseChildren,true);
    const badgeCenter=at(card.badge,14,14);input.send('pointerdown',badgeCenter);input.send('pointerup',badgeCenter);assert.equal(scene.lastTarget,'badge');assert.equal(scene.presses,2);
    click('mode-button');assert.equal(card.mouseEnabled,false);assert.equal(card.mouseChildren,false);
    const beforeDisabled=scene.presses,center=at(card,40,35);input.send('pointerdown',center);input.send('pointerup',center);assert.equal(scene.presses,beforeDisabled);
    assert.equal(card.hitTestPoint(center.x,center.y,true),true,'mouseEnabled does not change scripted hit testing');
    click('reset-button');input.send('pointerdown',at(card,40,35));input.send('pointermove',{x:1080,y:800},input.outside);input.send('pointerup',{x:1080,y:800},input.outside);
    assert.equal(scene.dragging,false);assert.equal(scene.releases,3);close(card.x,250);close(card.y,145);assert.equal(input.captures.size,0);
    const stopped=[card.x,card.y];input.send('pointermove',{x:20,y:20},input.outside);assert.deepEqual([card.x,card.y],stopped);
    // The AS2 callback receives the MovieClip, bounds use its transformed parent,
    // and a global release reaches onReleaseOutside after leaving the canvas.
    input.send('pointermove',at(legacy,40,35));input.send('pointerdown',at(legacy,40,35));assert.equal(scene.legacyPresses,1);
    const legacyDrop=at(scene.legacyRig,290,180);input.send('pointermove',legacyDrop);input.send('pointerup',legacyDrop);
    close(legacy.x,250);close(legacy.y,145);assert.equal(scene.legacyReleases,1);assert.equal(scene.legacyHit,true);assert.equal(scene.legacyDrop,'/legacyRig/dock');
    click('reset-button');input.send('pointerdown',at(legacy,40,35));input.send('pointermove',{x:1080,y:800},input.outside);input.send('pointerup',{x:1080,y:800},input.outside);
    assert.equal(scene.legacyOutside,1);assert.equal(scene.legacyState,'Released outside');assert.ok(scene.legacyOver>=1&&scene.legacyOut>=1);assert.equal(input.captures.size,0);
    close(legacy.x,250);close(legacy.y,145);
    // Restart while captured cancels the drag and replaces the authored lifetime.
    click('reset-button');input.send('pointerdown',at(card,40,35));const old=card;host.resetProject();scene=host.proxyFor([]);card=scene.card;legacy=scene.legacyRig.card;
    input.send('pointerup',{x:1080,y:800},input.outside);
    assert.notEqual(card,old);assert.deepEqual([card.x,card.y,legacy.x,legacy.y,scene.presses,scene.releases,scene.legacyPresses,scene.legacyOutside],[20,20,20,20,0,0,0,0]);assert.equal(input.captures.size,0);
    assert.throws(()=>old.startDrag(),/stale|removed|replaced|another player|retired/i);
    assert.match(scene.status3.text,/GROUP target/);assert.match(scene.status2.text,/outside releases 0/);
  }finally{host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Drag/drop: ordinary export + actual WASM, transformed parent bounds, invisible hitArea, group/child/disabled targeting, geometry hits, AS2 callbacks, captured outside release and Restart passed.');
}
