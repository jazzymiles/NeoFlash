// A single published composition spans clipping, linked identity and both dialects.
// Pointer geometry and clipped semantic silhouettes are checked in the browser.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyClippedScene(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/clipped-scene-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/clipped-scene.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();let scene=host.proxyFor([]);
    const ref=value=>value.toJSON().object_ref,key=value=>JSON.stringify(value);
    const rect=value=>[value.x,value.y,value.width,value.height];
    function check(phase) {
      assert.equal(scene.phase,phase);assert.equal(scene.legacyPhase,phase);
      assert.equal(scene.cards.length,5);assert.equal(host.program().type('app.ScrollCard').created,5);
      assert.equal(scene.rectangleCopied,true);assert.equal(scene.badMaskRejected,true);assert.equal(scene.badScrollRejected,true);
      assert.equal(scene.legacyAbsent,true);assert.equal(scene.legacyMaskAccepted,true);
      assert.deepEqual(rect(scene.viewport.scrollRect),[phase*155,0,450,230]);
      assert.deepEqual(rect(scene.viewport.getBounds(scene)),[60,220,450,230],'scrolling keeps the viewport bounds fixed');
      assert.equal(scene.viewport.transform.matrix.tx,60,'scrolling does not rewrite authored/base transform');
      const Point=host.program().type('flash.geom.Point');
      assert.equal(scene.cards[0].localToGlobal(new Point()).x,80-phase*155);
      assert.equal(scene.panel.mask,phase===2?null:scene.aperture);
      assert.equal(scene.rightViewport.scrollRect===null,phase===2);
      const snapshot=JSON.parse(player.runtime_snapshot_json()),objects=new Map(snapshot.objects.map(node=>[key(node.object_ref),node]));
      function descendant(reference,owner) {for(let node=objects.get(key(reference));node;node=objects.get(key(node.parent_ref)))if(key(node.object_ref)===key(owner))return true;return false;}
      const panelDraws=snapshot.draws.filter(draw=>descendant(draw.owner_ref,ref(scene.panel)));
      assert.ok(panelDraws.length>0);
      assert.ok(panelDraws.every(draw=>(draw.clip_ids||[]).length===(phase===2?0:2)),'mask and ancestor scroll rectangle form one intersected clip chain');
      if(phase!==2){
        assert.ok(snapshot.clips.some(clip=>clip.components.length===2),'one mask combines both filled vector components');
        assert.equal(snapshot.draws.some(draw=>descendant(draw.owner_ref,ref(scene.aperture))),false,'consumed mask geometry never paints independently');
      }
      const first=scene.cards[0];assert.equal(first.index,1);assert.equal(first.parent,scene.viewport);
    }
    check(0);const first=scene.cards[0],mask=scene.aperture,oldViewport=scene.viewport;
    host.stage().handlePointer('click',{},first);assert.equal(scene.selected,1);assert.equal(first.hits,1);
    const advance=()=>{player.click(JSON.stringify(['advance-button']));host.dispatchScripts(['advance-button']);};
    advance();check(1);assert.equal(scene.cards[0],first);assert.equal(first.hits,1);
    advance();check(2);assert.equal(scene.legacyPanel.scrollRect,null,'the public host/AS3 surface uses null; the AS2 source separately observed undefined');
    advance();check(0);assert.equal(scene.aperture,mask);assert.equal(scene.cards[0],first);
    host.resetProject();scene=host.proxyFor([]);check(0);
    assert.notEqual(scene.cards[0],first);assert.notEqual(scene.aperture,mask);
    assert.throws(()=>oldViewport.scrollRect,/stale|previous|removed|replaced|live|another player/i);
    host.stage().handlePointer('click',{},scene.cards[0]);assert.equal(scene.cards[0].hits,1);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Clipped scene: published compound mask unions, nested scroll clips, copied Rectangle state, AS2 absence/setMask, linked identity and Restart passed.');
}
