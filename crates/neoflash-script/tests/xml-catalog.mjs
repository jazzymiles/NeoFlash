// One exported XML workflow spans Rust parsing, both language facades, loaders,
// typed handlers, E4X selectors/literals, target edits and Restart; no per-member matrix.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyXMLCatalog(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/xml-catalog-'));
  let player,host;
  async function ready(){
    const deadline=Date.now()+3000;
    while(Date.now()<deadline){const scene=host.proxyFor([]);if(scene.ready&&scene.legacyReady)return;await new Promise(done=>setTimeout(done,5));}
    assert.fail('AS3 URLLoader and AS2 XML.load did not both complete');
  }
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/xml-catalog.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,xml:{parse:wasm.parse_xml_json},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();await ready();let scene=host.proxyFor([]);
    const original=scene.original,legacy=scene.legacyDoc;
    function check(revision,modernTotal,legacyTotal) {
      assert.equal(scene.revision,revision);assert.equal(scene.legacyRevision,revision);
      assert.equal(scene.total,modernTotal);assert.equal(scene.legacyTotal,legacyTotal);assert.equal(scene.legacyCount,3);
      assert.equal(scene.original.elements('item').length(),3,'copy mutation leaves original XML unchanged');
      assert.equal(scene.roundtrip.elements('item').length(),revision?4:3,'serialized XML parses back with the same records');
      assert.equal(scene.malformedRejected,true);assert.equal(scene.declarationRejected,true);assert.equal(scene.operatorRejected,true);assert.equal(scene.legacyReceiverOK,true);
      assert.match(scene.serialized,/urn:neoflash:catalog/);assert.match(scene.serialized,/meta:note/);
      assert.match(scene.modernRows.text,revision?/Parts & fittings/:/Film & frames/,'entities decode as text');
      assert.equal(scene.namespaceMatched,true);assert.equal(scene.descendantCount,revision?4:3);
      assert.equal(scene.metadata,revision?'Edited metadata '+revision:'Original & portable');
      assert.equal(scene.catalog.elements('note').length(),0,'empty-namespace selector does not strip the meta namespace');
      if(revision){
        assert.equal(scene.handlerTypes,true);assert.equal(scene.handlerNullable,true);assert.equal(scene.sharedSettings,true);assert.equal(scene.legacyMoved,true);
        assert.equal(scene.orderLog,'ATB','literal holes execute once in source order');
        assert.equal(scene.noteCreated,true);assert.equal(scene.sameNameSplit,true);
        assert.equal(scene.ambiguousRejected,true);assert.equal(scene.ambiguousUnchanged,true,'rejected multi-parent edit leaves the complete tree unchanged');
        assert.equal(String(scene.literalNode.child('name')),scene.literalText,'scalar interpolation cannot become markup');
        assert.equal(String(scene.literalNode.child('copied').child('delivery').attribute('state')),'ready','XML interpolation copies the selected subtree');
        assert.equal(scene.fragment.length(),2);
      }
    }
    const click=()=>{player.click(JSON.stringify(['advance-button']));host.dispatchScripts(['advance-button']);};
    check(0,13,13);click();check(1,15,14);assert.match(scene.legacyRows.text,/COPY-1/);assert.match(scene.modernRows.text,/E4X-1/);
    click();check(2,17,13);assert.match(scene.legacyRows.text,/COPY-2/);
    host.resetProject();await ready();scene=host.proxyFor([]);check(0,13,13);
    assert.notEqual(scene.original,original);assert.notEqual(scene.legacyDoc,legacy);
    assert.throws(()=>original.toXMLString(),/previous|session|retired|reset/i,'old XML wrappers are retired with the publication');
    click();check(1,15,14);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('XML catalog: E4X selectors/attributes/namespaces, target-list create/replace/delete, escaped literal holes/subtrees, atomic refusal, AS2 DOM regression, typed handlers and Restart passed.');
}
