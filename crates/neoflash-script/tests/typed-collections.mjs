// One ordinary publication connects typed model storage, identity keys,
// enumeration, linked-card reuse and JSON. Browser appearance is separate QA.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyTypedCollections(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/typed-collections-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/typed-collections.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    let scene=host.proxyFor([]);
    const firstItems=scene.items,firstCards=[scene.cards[0],scene.cards[1],scene.cards[2]];
    function check(expectedSkus,quantities,batch) {
      assert.equal(scene.batch,batch);assert.equal(scene.items.length,3);assert.equal(scene.cards.length,3);
      assert.equal(scene.keyCount,3);assert.equal(scene.total,quantities.reduce((a,b)=>a+b,0));
      assert.equal(host.program().type('app.InventoryCard').created,3,'reuse does not rerun linked constructors');
      const report=JSON.parse(scene.report);
      assert.deepEqual(report.map(item=>item.sku),expectedSkus);
      assert.deepEqual(report.map(item=>item.quantity),quantities);
      for(let i=0;i<3;i++) {
        assert.equal(scene.cards[i].item,scene.items[i],'enumerated object key resolves its original model');
        assert.equal(scene.cards[i].item.sku,expectedSkus[i]);
        assert.equal(scene.cards[i].quantity.text,expectedSkus[i]+': '+quantities[i]);
        assert.equal(report[i].stockLabel,expectedSkus[i]+': '+quantities[i],'Vector JSON retains public model getter');
        assert.equal(scene.cards[i].y,190+i*110);
      }
      assert.match(scene.legacyReadout.text,/quantity 13$/,'AS2 named-property iteration is independent');
    }
    const click=()=>{player.click(JSON.stringify(['process-button']));host.dispatchScripts(['process-button']);};
    check(['INK','PAPER','FILM'],[4,6,3],0);
    click();check(['PAPER','FILM','RESTOCK-1'],[7,4,5],1);assert.equal(scene.released,true);
    assert.equal(scene.cards[2],firstCards[0]);assert.equal(scene.cards[0],firstCards[1]);
    click();check(['FILM','RESTOCK-1','RESTOCK-2'],[5,6,6],2);assert.equal(scene.released,true);
    assert.equal(scene.cards[2],firstCards[1]);
    host.resetProject();scene=host.proxyFor([]);
    assert.notEqual(scene.items,firstItems);assert.notEqual(scene.cards[0],firstCards[0]);
    check(['INK','PAPER','FILM'],[4,6,3],0);
    click();check(['PAPER','FILM','RESTOCK-1'],[7,4,5],1);assert.equal(scene.released,true);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Typed collections: published Vector models, Dictionary object keys, enumeration/delete/in, linked-card reuse, JSON getters, AS2 for-in and Restart passed.');
}
