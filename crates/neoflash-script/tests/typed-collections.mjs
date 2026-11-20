// One ordinary publication connects typed model storage, identity keys,
// enumeration, numeric method values, reflection, source-object identity,
// cached Array pipelines, linked-card reuse and JSON.
// Browser appearance is separate QA.
import assert from 'node:assert/strict';
import { runCargo } from '../../../scripts/test-cargo.cjs';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyTypedCollections(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/typed-collections-'));
  let player,host;
  try {
    const exported=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/typed-collections.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
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
    const firstItems=scene.items,firstCards=[scene.cards[0],scene.cards[1],scene.cards[2]],firstProbe=scene.classProbe,firstArray=scene.arraySource,firstPush=scene.cachedPush;
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
      assert.equal(scene.arrayCrossClassOK,true,'the registered Array definition shares the program Class value');
      assert.equal(scene.arrayHandlerOK,true,'frame script and program share Array Class and cached native method identities');
      assert.equal(scene.arrayBoundOK,true,'cached push/pop/splice/slice/sort/map retain the source array through call/apply');
      assert.equal(scene.arrayCustomOK,true,'custom method capture and saved native method survive later replacement');
      assert.equal(scene.arrayOverrideCalls,1);
      const sorted=quantities.slice().sort((left,right)=>left-right),doubled=sorted.map(value=>value*2);
      assert.deepEqual(Array.from(scene.arraySource),sorted);
      assert.equal(scene.arraySnapshot,sorted.join(','));assert.equal(scene.arrayPipeline,doubled.join(','));
      assert.equal(scene.arraySummary,'Arrays true / sorted '+sorted.join(',')+' / doubled '+doubled.join(','));
      assert.equal(scene.modelType,'app::Item');assert.equal(scene.modelBase,'Object');
      assert.match(scene.typeDescription,/<variable\b(?=[^>]*\bname="quantity")(?=[^>]*\btype="int")[^>]*>/);
      assert.match(scene.typeDescription,/<accessor\b(?=[^>]*\bname="stockLabel")[^>]*>/);
      const recent=batch===0?['Ready','Empty']:batch===1?['Batch 1','Ready']:['Batch 2','Batch 1'];
      assert.deepEqual(Array.from(scene.history),recent,'insertAt and negative-index removeAt retain the newest two batches');
      assert.equal(scene.removedHistory,batch===0?'':batch===1?'Empty':'Ready');
      assert.equal(scene.detail.text,scene.foundationSummary+'\rapp::Item / public keys: count, quantity / recent: '+recent.join(', ')+'\r'+scene.arraySummary);
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
    const ArrayProto=host.program().type('Array').prototype;
    assert.equal(Object.hasOwn(Array.prototype,'sortOn'),false,'host JS Array.prototype remains untouched');
    assert.equal(typeof ArrayProto.toString==='function'&&typeof ArrayProto.toLocaleString==='function',true);
    scene[0]='clip-alpha';scene[1]='clip-beta';scene.length=2;
    assert.deepEqual(ArrayProto.slice.call(scene,0),['clip-alpha','clip-beta'],'MovieClip display proxy supports borrowed Array.prototype.slice');
    assert.equal(ArrayProto.push.call(scene,'clip-gamma'),3);
    assert.equal(ArrayProto.join.call(scene,'|'),'clip-alpha|clip-beta|clip-gamma');
    assert.equal(ArrayProto.join.call([scene,null],','),scene.toString()+',');
    {
      const prog=host.program();
      const getQName=prog.type('flash.utils.getQualifiedClassName');
      const getSuper=prog.type('flash.utils.getQualifiedSuperclassName');
      const getDef=prog.type('flash.utils.getDefinitionByName');
      const descType=prog.type('flash.utils.describeType');
      const MovieClip=getDef('flash.display.MovieClip'),Sprite=getDef('flash.display.Sprite'),Shape=getDef('flash.display.Shape'),Bitmap=getDef('flash.display.Bitmap');
      const BitmapData=getDef('flash.display.BitmapData'),SimpleButton=getDef('flash.display.SimpleButton'),TextField=getDef('flash.text.TextField');
      const Stage=getDef('flash.display.Stage'),Video=getDef('flash.media.Video'),Loader=getDef('flash.display.Loader');
      const ByteArray=getDef('flash.utils.ByteArray'),Point=getDef('flash.geom.Point'),Rectangle=getDef('flash.geom.Rectangle'),Matrix=getDef('flash.geom.Matrix');
      const Timer=getDef('flash.utils.Timer'),Sound=getDef('flash.media.Sound'),Dictionary=getDef('flash.utils.Dictionary'),VecInt=getDef('Vector.<int>'),VecAny=getDef('Vector.<*>');
      const IEventDispatcher=getDef('flash.events.IEventDispatcher'),IBitmapDrawable=getDef('flash.display.IBitmapDrawable');
      const mc=new MovieClip(),sp=new Sprite(),sh=new Shape(),bmpData=new BitmapData(4,4),bmp=new Bitmap(bmpData),btn=new SimpleButton();
      const tf=new TextField(),vid=new Video(),ldr=new Loader(),ba=new ByteArray(),pt=new Point(1,2),rc=new Rectangle(0,0,4,4),mx=new Matrix();
      const tmr=new Timer(50,1),snd=new Sound(),dict=new Dictionary(),vecInt=new VecInt(),vecAny=new VecAny();
      const expectedPairs=[
        [mc,'flash.display::MovieClip','flash.display::Sprite'],
        [scene,'app::Main','flash.display::MovieClip'],
        [scene.cards[0],'app::InventoryCard','flash.display::MovieClip'],
        [sp,'flash.display::Sprite','flash.display::DisplayObjectContainer'],
        [sh,'flash.display::Shape','flash.display::DisplayObject'],
        [bmp,'flash.display::Bitmap','flash.display::DisplayObject'],
        [bmpData,'flash.display::BitmapData','Object'],
        [btn,'flash.display::SimpleButton','flash.display::InteractiveObject'],
        [tf,'flash.text::TextField','flash.display::InteractiveObject'],
        [scene.stage,'flash.display::Stage','flash.display::DisplayObjectContainer'],
        [Stage,'flash.display::Stage','flash.display::DisplayObjectContainer'],
        [vid,'flash.media::Video','flash.display::DisplayObject'],
        [ldr,'flash.display::Loader','flash.display::DisplayObjectContainer'],
        [ba,'flash.utils::ByteArray','Object'],
        [pt,'flash.geom::Point','Object'],
        [rc,'flash.geom::Rectangle','Object'],
        [mx,'flash.geom::Matrix','Object'],
        [tmr,'flash.utils::Timer','flash.events::EventDispatcher'],
        [snd,'flash.media::Sound','flash.events::EventDispatcher'],
        [dict,'flash.utils::Dictionary','Object'],
        [scene.history,'Array','Object'],
        [scene.items,'__AS3__.vec::Vector.<app::Item>','Object'],
        [vecInt,'__AS3__.vec::Vector.<int>','Object'],
        [vecAny,'__AS3__.vec::Vector.<*>','Object'],
        [IEventDispatcher,'flash.events::IEventDispatcher',null],
        [IBitmapDrawable,'flash.display::IBitmapDrawable',null],
        [42,'int','Object'],
        [3.5,'Number','Object'],
        [null,'null',null],
        [undefined,'void',null],
      ];
      for(const [val,qname,superName] of expectedPairs){
        assert.equal(getQName(val),qname,`getQualifiedClassName mismatch for ${qname}`);
        assert.equal(getSuper(val),superName,`getQualifiedSuperclassName mismatch for ${qname}`);
      }
      const mcInstXml=descType(mc).toXMLString();
      assert.match(mcInstXml,/<type name="flash\.display::MovieClip" base="flash\.display::Sprite" isDynamic="true" isFinal="false" isStatic="false">/);
      assert.match(mcInstXml,/<extendsClass type="flash\.display::DisplayObjectContainer"\/>/);
      assert.match(mcInstXml,/<extendsClass type="flash\.display::InteractiveObject"\/>/);
      assert.match(mcInstXml,/<extendsClass type="flash\.display::DisplayObject"\/>/);
      assert.match(mcInstXml,/<extendsClass type="flash\.events::EventDispatcher"\/>/);
      assert.match(mcInstXml,/<implementsInterface type="flash\.display::IBitmapDrawable"\/>/);
      assert.match(mcInstXml,/<implementsInterface type="flash\.events::IEventDispatcher"\/>/);
      assert.match(mcInstXml,/<accessor\b[^>]*\bname="currentFrame"/);
      assert.match(mcInstXml,/<method\b[^>]*\bname="gotoAndStop"/);
      const cardXml=descType(scene.cards[0]).toXMLString();
      assert.match(cardXml,/<type name="app::InventoryCard" base="flash\.display::MovieClip"/);
      assert.match(cardXml,/<extendsClass type="flash\.display::MovieClip"\/>/);
      assert.match(cardXml,/<extendsClass type="flash\.display::Sprite"\/>/);
      assert.match(cardXml,/<variable\b[^>]*\bname="item"/);
      const bdClsXml=descType(BitmapData).toXMLString();
      assert.match(bdClsXml,/<type name="flash\.display::BitmapData" base="Class" isDynamic="true" isFinal="true" isStatic="true">/);
      assert.match(bdClsXml,/<implementsInterface type="flash\.display::IBitmapDrawable"\/>/);
      assert.match(bdClsXml,/<method\b[^>]*\bname="drawWithQuality"/);
      const ifaceXml=descType(IEventDispatcher).toXMLString();
      assert.match(ifaceXml,/<type name="flash\.events::IEventDispatcher" base="Class" isDynamic="true" isFinal="true" isStatic="true">/);
      assert.match(ifaceXml,/<factory type="flash\.events::IEventDispatcher">/);
      assert.match(ifaceXml,/<method name="addEventListener"/);
      bmpData.dispose();tmr.stop();
    }
    host.resetProject();scene=host.proxyFor([]);
    assert.notEqual(scene.items,firstItems);assert.notEqual(scene.cards[0],firstCards[0]);
    assert.notEqual(scene.classProbe,firstProbe,'Restart constructs fresh class-alias instances');
    assert.notEqual(scene.arraySource,firstArray);assert.notEqual(scene.cachedPush,firstPush,'Restart discards the previous array method receiver');
    check(['INK','PAPER','FILM'],[4,6,3],0);
    click();check(['PAPER','FILM','RESTOCK-1'],[7,4,5],1);assert.equal(scene.released,true);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Typed collections: numeric aliases, source Object identity/reflection, cross-handler Array Class values and cached bound pipelines, custom overrides, Vector/Dictionary models, linked-card reuse, JSON getters, AS2 for-in and Restart passed.');
}
