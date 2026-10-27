// One publication-owned save/reopen/clear workflow across both AS dialects.
// The injected storage follows synchronous Web Storage; real browser reload is
// accepted separately. AMF bytes and consumed cursors come from actual Rust/WASM.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';
export async function verifySavedGame(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/saved-game-')),saved=new Map();let denied=false,player,host;
  const storage={getItem:key=>saved.get(key)??null,setItem(key,value){if(denied)throw new Error('Quota denied by the test storage');saved.set(key,String(value));},removeItem(key){if(denied)throw new Error('Storage denied by the test storage');saved.delete(key);}};
  try{
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/saved-game.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    const open=()=>{player=new wasm.WebPlayer(JSON.stringify(bundle.project));({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,storage:{storage,url:'https://example.invalid/games/saved-game/index.html'},context:{measureText:line=>({width:line.length*8})}}));host.dispatchScripts();return host.proxyFor([]);};
    const close=()=>{host.disposeFamily();player.free();host=null;player=null;};
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    let scene=open();assert.equal(scene.score(),0);assert.equal(scene.restores,true);assert.equal(scene.save.objectEncoding,3);
    assert.equal(host.program().is(scene.save,'flash.net.SharedObject'),true);
    for(const value of [scene.round,scene.oldRound]){
      assert.equal(value.left,value.right);assert.equal(value.self,value);assert.ok(Object.hasOwn(value,'unset'));assert.equal(value.unset,undefined);
      assert.ok(Number.isNaN(value.nan));assert.equal(value.infinity,Infinity);assert.ok(Object.is(value.negativeZero,-0));
      assert.equal(value.when.getTime(),Date.UTC(2026,9,27));assert.equal(value.values[2],'third');assert.equal(value.values.tag,'bag');
    }
    assert.equal(scene.packet.bytesAvailable,0);assert.equal(scene.oldPacket.bytesAvailable,0);
    click('save-button');assert.deepEqual([scene.score(),scene.legacySave.data.score,scene.lastFlush,scene.legacyFlush],[25,25,'flushed',true]);assert.equal(saved.size,2);
    const firstStored=new Map(saved),oldSave=scene.save,oldPacket=scene.packet;
    host.resetProject();scene=host.proxyFor([]);assert.equal(scene.score(),25);assert.equal(scene.legacySave.data.score,25);
    for(const save of [scene.save,scene.legacySave]){assert.equal(save.data.primary,save.data.alias);assert.equal(save.data.self,save.data);assert.equal(save.data.savedAt.getUTCFullYear(),2026);}
    assert.equal(scene.save.data.badge.readUTF(),'LEVEL-1');assert.ok(Number.isNaN(scene.save.data.nan));assert.ok(Object.is(scene.save.data.negativeZero,-0));
    assert.throws(()=>oldSave.data,/previous|session|disposed/i);assert.throws(()=>oldPacket.length,/previous|session|disposed/i);
    scene.save.data.score=50;denied=true;assert.throws(()=>scene.save.flush(),/quota|storage|flush/i);denied=false;
    assert.deepEqual(saved,firstStored,'failed flush preserves the last stored snapshots');
    close();scene=open();assert.deepEqual([scene.score(),scene.legacySave.data.score],[25,25],'a fresh host reads the last successful save');
    const data3=scene.save.data,data2=scene.legacySave.data;click('clear-button');assert.equal(saved.size,0);assert.equal(scene.score(),0);assert.equal(scene.legacySave.data.score,undefined);
    assert.notEqual(scene.save.data,data3,'AS3 clear replaces data in the bounded dialect profile');assert.equal(scene.legacySave.data,data2,'AS2 clear empties its data object');
    close();scene=open();assert.equal(scene.score(),0);assert.equal(scene.legacySave.data.score,undefined);
    click('save-button');assert.equal(scene.score(),25);assert.equal(scene.legacySave.data.score,25);click('clear-button');assert.equal(saved.size,0);
  }finally{host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Saved game: AMF0/AMF3 graphs/cursors, two-dialect SharedObject save/reopen/clear, storage refusal and Restart passed.');
}
