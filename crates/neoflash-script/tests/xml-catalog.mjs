// One exported XML workflow spans Rust parsing, both language facades, loaders,
// typed handlers, E4X selectors/literals, target edits and Restart; no per-member matrix.
import assert from 'node:assert/strict';
import { runCargo } from '../../../scripts/test-cargo.cjs';
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
    const exported=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/web-and-ads/xml-catalog.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
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
      assert.equal(scene.malformedRejected,true);assert.equal(scene.declarationRejected,true);assert.equal(scene.operatorRejected,false);assert.equal(scene.legacyReceiverOK,true);
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
    {
      const e4xScript=`
        var doc:XML = <catalog id="c1" xmlns:m="urn:meta">
          <m:note m:flag="yes">MetaHead</m:note>
          <item sku="A1"><badge>old1</badge></item>
          <item sku="B2"><badge>old2</badge></item>
          <section><nested><leaf>deep</leaf><m:tag>nsDeep</m:tag></nested></section>
        </catalog>;
        function pickNs():Namespace { return new Namespace("m", "urn:meta"); }
        var noteKey:String = "note";
        compQualRead = String(doc.(pickNs())::note);
        compQualBracket = String(doc.(pickNs())::[noteKey]);
        compQualAttr = String(doc.(pickNs())::note.@(pickNs())::flag);
        compQualDesc = String(doc..(pickNs())::tag);
        bracketQualRead = String(doc.[pickNs()]::note);
        filteredByAttr = String(doc.item.(@sku == "B2").badge);
        filteredByDesc = doc.section.(..leaf == "deep").length();
        var gotAttr:String = "";
        with (doc.item[0]) {
          @sku = "A1-MOD";
          gotAttr = String(@sku);
        }
        withAttrVal = gotAttr;
        var gotDesc:int = 0;
        with (doc) {
          gotDesc = (..leaf).length();
        }
        withDescCount = gotDesc;
        var attrKey:String = "sku";
        doc.item[1].@[attrKey] = "B2-MOD";
        dynAttrBracket = String(doc.item[1].@[attrKey]);
        doc..badge = "featured";
        descAssigned = String(doc.item[0].badge) + "/" + String(doc.item[1].badge);
        var dynDoc:* = doc;
        dynDoc.summary = "DynamicChild";
        dynDoc["@version"] = "2.0";
        dynHasSummary = ("summary" in dynDoc);
        dynHasVersion = ("@version" in dynDoc);
        dynSummaryVal = String(doc.summary);
        dynVersionVal = String(doc.@version);
        var singleList:* = doc.summary;
        singleList["@lang"] = "en";
        singleListAttr = String(doc.summary.@lang);
        multiWriteRejected = false;
        try {
          var multiList:* = doc.item;
          multiList.extra = "fail";
        } catch (err:TypeError) {
          multiWriteRejected = true;
        }
        delSelfZero = delete doc[0];
        delItemZero = delete doc.item[0];
        remainingItemSku = String(doc.item[0].@sku);
        remainingItemCount = doc.item.length();
        delAttrDyn = delete doc.item[0].@[attrKey];
        attrAfterDel = doc.item[0].@sku.length();
        delProxyAttr = delete dynDoc["@version"];
        versionAfterDel = doc.@version.length();
        delDescLeaf = delete doc..leaf;
        leafAfterDel = (doc..leaf).length();
      `;
      const compiled=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-script','--example','compile_project'],{
        cwd:root,
        input:JSON.stringify({
          project:{format_version:2,name:'E4X Operators',width:100,height:100,fps:24,background:[0,0,0,255],root:'r',symbols:[{id:'r',name:'R',kind:'movie_clip',timeline:{frame_count:1,layers:[],scripts:[]}}],sources:[]},
          scripts:[{language:'action_script3',source:e4xScript}],
        }),
        encoding:'utf8',
      });
      assert.equal(compiled.status,0,compiled.error?.message||compiled.stderr);
      const parsed=JSON.parse(compiled.stdout);
      assert.ok(parsed.Ok,JSON.stringify(parsed));
      const xmlFacade=host.xml();
      const prog=new Function('__nfBuiltins',parsed.Ok.code)({__nfLegacy:host.legacy(),__nfXML:xmlFacade,__nfTrace:()=>{}});
      const ctx={};
      new Function('__nfProgram','trace','__nfLegacy','__nfXML',parsed.Ok.scripts[0]).call(ctx,prog,()=>{},host.legacy(),xmlFacade);
      assert.equal(ctx.compQualRead,'MetaHead');
      assert.equal(ctx.compQualBracket,'MetaHead');
      assert.equal(ctx.compQualAttr,'yes');
      assert.equal(ctx.compQualDesc,'nsDeep');
      assert.equal(ctx.bracketQualRead,'MetaHead');
      assert.equal(ctx.filteredByAttr,'old2');
      assert.equal(ctx.filteredByDesc,1);
      assert.equal(ctx.withAttrVal,'A1-MOD');
      assert.equal(ctx.withDescCount,1);
      assert.equal(ctx.dynAttrBracket,'B2-MOD');
      assert.equal(ctx.descAssigned,'featured/featured');
      assert.equal(ctx.dynHasSummary,true);
      assert.equal(ctx.dynHasVersion,true);
      assert.equal(ctx.dynSummaryVal,'DynamicChild');
      assert.equal(ctx.dynVersionVal,'2.0');
      assert.equal(ctx.singleListAttr,'en');
      assert.equal(ctx.multiWriteRejected,true);
      assert.equal(ctx.delSelfZero,true);
      assert.equal(ctx.delItemZero,true);
      assert.equal(ctx.remainingItemSku,'B2-MOD');
      assert.equal(ctx.remainingItemCount,1);
      assert.equal(ctx.delAttrDyn,true);
      assert.equal(ctx.attrAfterDel,0);
      assert.equal(ctx.delProxyAttr,true);
      assert.equal(ctx.versionAfterDel,0);
      assert.equal(ctx.delDescLeaf,true);
      assert.equal(ctx.leafAfterDel,0);
    }
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('XML catalog: E4X selectors/attributes/namespaces, target-list create/replace/delete, escaped literal holes/subtrees, atomic refusal, AS2 DOM regression, typed handlers and Restart passed.');
}
