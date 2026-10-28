// One ordinary publication connects typed model storage, identity keys,
// enumeration, numeric method values, reflection, source-object identity,
// linked-card reuse and JSON.
// Browser appearance is separate QA.
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
    const firstItems=scene.items,firstCards=[scene.cards[0],scene.cards[1],scene.cards[2]],firstProbe=scene.classProbe;
    function check(expectedSkus,quantities,batch) {
      assert.equal(scene.batch,batch);assert.equal(scene.items.length,3);assert.equal(scene.cards.length,3);
      assert.equal(scene.keyCount,3);assert.equal(scene.total,quantities.reduce((a,b)=>a+b,0));
      assert.equal(scene.converted,42,'a first-class int alias constructs through AS3 numeric conversion');
      assert.equal(scene.wrapped,4294967295,'a first-class uint alias wraps the input');
      assert.equal(scene.rootText,Math.sqrt(scene.total).toFixed(2));
      assert.equal(scene.numericOK,true,'integer bounds and extracted numeric direct/call/apply methods retain their receiver');
      assert.equal(scene.enumerationOK,true,'hidden dynamic data stays owned while fixed class slots remain nonenumerable');
      assert.deepEqual(Array.from(scene.publicKeys),['count','quantity']);
      assert.equal(scene.metadata.batchNote,'batch-'+batch);
      assert.equal(scene.reflectionOK,true,'public class lookup and qualified instance/base names agree');
      assert.equal(scene.describeGetterStable,true,'describeType lists the accessor without invoking it');
      assert.equal(scene.identityOK,true,'Class aliases, source prototypes, constructors and default Object methods agree');
      assert.equal(scene.classProbe.sku,'PROBE');assert.equal(scene.classProbe.quantity,1);
      assert.equal(scene.derivedProbe.marker,7,'ordinary inherited source fields remain initialized');
      assert.equal(scene.identityLabels,'[object Item] / [object Item] / custom / [object Object] / local');
      assert.equal(scene.customToStringCalls,1,'default toLocaleString does not call an authored toString override');
      assert.equal(scene.modelType,'app::Item');assert.equal(scene.modelBase,'Object');
      assert.match(scene.typeDescription,/<variable\b(?=[^>]*\bname="quantity")(?=[^>]*\btype="int")[^>]*>/);
      assert.match(scene.typeDescription,/<accessor\b(?=[^>]*\bname="stockLabel")[^>]*>/);
      const recent=batch===0?['Ready','Empty']:batch===1?['Batch 1','Ready']:['Batch 2','Batch 1'];
      assert.deepEqual(Array.from(scene.history),recent,'insertAt and negative-index removeAt retain the newest two batches');
      assert.equal(scene.removedHistory,batch===0?'':batch===1?'Empty':'Ready');
      assert.equal(scene.detail.text,scene.foundationSummary+'\rapp::Item / public keys: count, quantity / recent: '+recent.join(', '));
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
    assert.notEqual(scene.classProbe,firstProbe,'Restart constructs fresh class-alias instances');
    check(['INK','PAPER','FILM'],[4,6,3],0);
    click();check(['PAPER','FILM','RESTOCK-1'],[7,4,5],1);assert.equal(scene.released,true);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Typed collections: published numeric aliases/bound methods, public reflection/enumerability, source prototypes/constructors/default labels, Array insert/remove, Vector models, Dictionary keys, linked-card reuse, JSON getters, AS2 for-in and Restart passed.');
}
