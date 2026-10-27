// One coherent project build, then actual emitted classes and event facade execution.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { homedir } from 'node:os';
import { resolve, dirname } from 'node:path';
import { readFileSync, mkdtempSync, rmSync, existsSync } from 'node:fs';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';
import { verifyAs2ProjectInput } from './as2-project-input.mjs';
import { verifyGeometryTimer } from './geometry-timer.mjs';
import { verifyBinaryData } from './binary-data.mjs';
import { verifyInterfacesAccessors } from './interfaces-accessors.mjs';
import { verifyResourceLoading } from './resource-loading.mjs';
import { verifyBitmapPixels } from './bitmap-pixels.mjs';
import { verifyTextLayout } from './text-layout.mjs';
import { verifySoundPlayback } from './sound-playback.mjs';
import { verifyDataProcessing } from './data-processing.mjs';
import { verifyDisplayTransforms } from './display-transforms.mjs';
import { verifyTypedCollections } from './typed-collections.mjs';
import { verifyXMLCatalog } from './xml-catalog.mjs';
import { verifyGradientGraphics } from './gradient-graphics.mjs';
import { verifyClippedScene } from './clipped-scene.mjs';
import { verifyLoaderGallery } from './loader-gallery.mjs';
import { verifySavedGame } from './saved-game.mjs';
import { verifyTextForm } from './text-form.mjs';
import { verifyFilterGallery } from './filter-gallery.mjs';
import { verifyFunctionWorkflow } from './function-workflow.mjs';
import { verifyImagePipeline } from './image-pipeline.mjs';
import { verifyClassPackages } from './class-packages.mjs';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const localCargo=resolve(homedir(),'.cargo/bin/cargo');
const cargo=process.env.CARGO || (existsSync(localCargo) ? localCargo : 'cargo');
const events=createRequire(import.meta.url)(resolve(root,'web/events.js'));
const source=(id,path,source)=>({id,path,language:'action_script3',source});
const project={format_version:2,name:'Original program fixture',width:320,height:180,fps:24,background:[255,255,255,255],root:'scene',
  symbols:[{id:'scene',name:'Scene',kind:'movie_clip',timeline:{frame_count:1,layers:[],scripts:[]}}],
  sources:[
    source('user-xml3','user/XML.as','package user { public class XML {public function XML(){} public function marker():String{return "user-xml";}} }'),
    source('user-list3','user/XMLList.as','package user { public class XMLList {public function XMLList(){} public function marker():String{return "user-list";}} }'),
    source('user-loader3','user/MovieClipLoader.as','package user { public class MovieClipLoader {public var label:String; public function MovieClipLoader(value:String){label=value;}} }'),
    source('user-store3','user/SharedObject.as','package user { public class SharedObject {public var label:String; public function SharedObject(value:String){label=value;} public static function getLocal(value:String):SharedObject{return new SharedObject(value);}} }'),
    {id:'user-xml2',path:'legacy/XML.as',language:'action_script2',source:'class legacy.XML {public function XML(){} public function marker():String{return "legacy-xml";}}'},
    {id:'user-node2',path:'legacy/XMLNode.as',language:'action_script2',source:'class legacy.XMLNode {public function XMLNode(){} public function marker():String{return "legacy-node";}}'},
    {id:'user-loader2',path:'legacy/MovieClipLoader.as',language:'action_script2',source:'class legacy.MovieClipLoader {public var label:String; public function MovieClipLoader(value:String){label=value;}}'},
    {id:'user-store2',path:'legacy/SharedObject.as',language:'action_script2',source:'class legacy.SharedObject {public var label:String; public function SharedObject(value:String){label=value;} public static function getLocal(value:String):SharedObject{return new SharedObject(value);}}'},
    source('typed-bag','model/Bag.as','package model { public class Bag { public var items:Vector.<DataBase>; public var numbers:Vector.<int>; public function Bag(){} public function accept(items:Vector.<DataBase>,numbers:Vector.<int>):int {this.items=items;this.numbers=numbers;return items.length+numbers.length;} } }'),
    {id:'user-vector',path:'legacy/Vector.as',language:'action_script2',source:'class legacy.Vector {public function Vector(){} public function size():Number{return 3;}}'},
    {id:'user-dictionary',path:'legacy/Dictionary.as',language:'action_script2',source:'class legacy.Dictionary {public function Dictionary(){} public function count():Number{return 4;}}'},
    source('data-base','model/DataBase.as','package model { public class DataBase { public var value:int=2; public static var reads:int=0; public function DataBase(){} public function get doubled():int { DataBase.reads++; return value*2; } public function set writeOnly(next:int):void {value=next;} } }'),
    source('data-record','model/DataRecord.as','package model { public dynamic class DataRecord extends DataBase { public var when:Date; public var pattern:RegExp; public function DataRecord(){super();} public function accept(date:Date,regex:RegExp):Boolean {when=date;pattern=regex;return date is Date && regex is RegExp;} public function serialize():String {return JSON.stringify(this,function(key:String,value:*):* {if(key=="pattern")return value.source;return value;});} } }'),
    {id:'user-json',path:'legacy/JSON.as',language:'action_script2',source:'class legacy.JSON {public function JSON(){} public function parse(value:String):String{return "user:"+value;}}'},
    {id:'user-regex',path:'legacy/RegExp.as',language:'action_script2',source:'class legacy.RegExp {public function RegExp(){} public function test(value:String):Boolean{return value=="user";}}'},
    {id:'user-date',path:'legacy/Date.as',language:'action_script2',source:'class legacy.Date {public function Date(){} public function getTime():Number{return 17;}}'},
    source('offset-point','app/OffsetPoint.as','package app { import flash.geom.Point; public class OffsetPoint extends Point { public function OffsetPoint() {super(3,4);} public function shifted():Point {return super.add(new Point(1,2));} } }'),
    source('pulse','app/Pulse.as','package app { import flash.utils.Timer; public class Pulse extends Timer { public function Pulse() {super(10,1);} public function begin():void {super.start();} } }'),
    {id:'legacy-model',path:'legacy/Model.as',language:'action_script2',source:'class legacy.Model { public var value:Number=1; public var missing:String; public var items:Array; public static var created:Number=0; public function Model(seed:Number) { value=seed; items=[]; created++; } public function increment(step:Number):Number { value+=step; return value; } public static function total():Number { return created; } }'},
    {id:'legacy-counter',path:'legacy/Counter.as',language:'action_script2',source:'import legacy.Model; class legacy.Counter extends Model { public function Counter(seed:Number) { super(seed); } public function increment(step:Number):Number { return super.increment(step)+100; } }'},
    source('base','model/Base.as','package model { public class Base { public var value:int=1; public function Base(seed:int) { value=seed; } public function increment():int { value+=1; return value; } } }'),
    source('counter','model/Counter.as','package model { import model.Base; public class Counter extends Base { public static var created:int=0; public const label:String="counter"; public var step:int=3; public function Counter(seed:int) { super(seed); value+=2; Counter.created++; } public override function increment():int { return super.increment()+10; } public function callback():Function { return increment; } public function localTypes(String:Function,Array:Function):String { var values:Array=[1]; return String(Array(values[0])); } public function summarize(values:Array):String { var copy:Array=values.slice(); copy.sort(Array.NUMERIC); return String.fromCharCode(65)+copy.join(",").toUpperCase(); } } }'),
    source('watcher','app/Watcher.as','package app { import flash.events.Event; import flash.events.EventDispatcher; public class Watcher extends EventDispatcher { public var count:int=0; public function Watcher() { super(); addEventListener(Event.CHANGE,changed); } public function changed(e:Event):void { count++; } public function close():void { removeEventListener(Event.CHANGE,changed); } } }'),
  ]};
const scripts=[{language:'action_script3',source:'import flash.utils.getTimer; import model.Counter; import app.Watcher; import flash.events.Event; var a:Counter=new Counter(2); var b:Counter=new Counter(7); first=a.increment(); second=b.value; total=Counter.created; var cb:Function=a.callback(); detached=cb(); var w:Watcher=new Watcher(); w.dispatchEvent(new Event(Event.CHANGE)); w.close(); w.dispatchEvent(new Event(Event.CHANGE)); events=w.count; collection=a.summarize(new Array(30,2,11)); shadowed=a.localTypes(function(value:*):String { return "local:"+value; },function(value:int):int { return value+3; }); lookupResult=this.probe["sort"](this.argument()); var clock:Function=getTimer; elapsed=clock(); qualified=flash.utils.getTimer(); function localClock(getTimer:Function):Number {return getTimer();} localTime=localClock(function():Number{return 77;});'}];
scripts[0].source='import user.SharedObject; import user.MovieClipLoader; import user.XML; import user.XMLList; import model.DataRecord; '+scripts[0].source+' var record:DataRecord=new DataRecord(); emptySlots=(record.when==null&&record.pattern==null); var foreignDate:Date=new Date(0); var foreignRegex:RegExp=new RegExp("x"); accepted=record.accept(foreignDate,foreignRegex); record.extra="dynamic"; serialized=record.serialize(); var customXML:XML=new XML(); var customList:XMLList=new XMLList(); customXMLResult=customXML.marker()+customList.marker(); var customLoader:MovieClipLoader=new MovieClipLoader("user-loader"); customLoaderResult=customLoader.label; customStoreResult=SharedObject.getLocal("user-store").label;';
scripts[0].source='import flash.utils.Dictionary; import model.Bag; import model.DataBase; '+scripts[0].source+' var modelVector:Vector.<DataBase>=new Vector.<DataBase>(); modelVector.push(record); var numbers:Vector.<int>=Vector.<int>([7,8]); var bag:Bag=new Bag(); bagResult=bag.accept(modelVector,numbers); var associations:Dictionary=new Dictionary(); associations[record]=7;associations[a]=9; dictionarySum=0; var objectKeys:Array=[];for(var identity:Object in associations){objectKeys.push(identity);dictionarySum+=associations[identity];} dictionaryHad=record in associations;dictionaryRemoved=delete associations[record];dictionaryNow=record in associations;dictionaryJSON=JSON.stringify(associations); fixedDelete=delete record.value;fixedHas="value" in record;var dynamicKeys:Array=[];for(var property:String in record)dynamicKeys.push(property);';
scripts.push({language:'action_script2',source:'import legacy.SharedObject; import legacy.MovieClipLoader; import legacy.XML; import legacy.XMLNode; import legacy.JSON; import legacy.RegExp; import legacy.Date; import legacy.Vector; import legacy.Dictionary; legacyElapsed=getTimer(); var customLoader:MovieClipLoader=new MovieClipLoader("legacy-loader"); legacyLoaderResult=customLoader.label; legacyStoreResult=SharedObject.getLocal("legacy-store").label; var userJSON=new JSON(); var userRegex=new RegExp(); var userDate=new Date(); var userVector=new Vector(); var userDictionary=new Dictionary(); userCollections=userVector.size()+userDictionary.count(); var oldXML=new XML();var oldNode=new XMLNode(); oldXMLResult=oldXML.marker()+oldNode.marker(); userText=userJSON.parse("value"); userMatch=userRegex.test("user"); userTime=userDate.getTime(); function localNative(JSON:Object,RegExp:Object):String{return JSON.parse("local")+RegExp.test("local");} userShadow=localNative({parse:function(value:String):String{return value;}},{test:function(value:String):Boolean{return true;}});'});
const result=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-script','--example','compile_project'],{cwd:root,input:JSON.stringify({project,scripts}),encoding:'utf8'});
assert.equal(result.status,0,result.error?.message || result.stderr);
const compiled=JSON.parse(result.stdout);assert.ok(compiled.Ok,JSON.stringify(compiled));
const bridge=createRequire(import.meta.url)(resolve(root,'web/runtime.js')).createAs2Methods();
const geom=createRequire(import.meta.url)(resolve(root,'web/geom.js'));
const timerService=createRequire(import.meta.url)(resolve(root,'web/timers.js')).create({events,now:()=>0,schedule:()=>7,cancel:()=>{}});
const builtins={__nfTrace:()=>{},__nfAs2Method:bridge.method,__nfAs2Value:bridge.value,
 'flash.geom.Point':geom.Point,'flash.geom.Rectangle':geom.Rectangle,'flash.geom.Matrix':geom.Matrix,
 'flash.utils.Timer':timerService.Timer,'flash.events.TimerEvent':timerService.TimerEvent,'flash.utils.getTimer':timerService.getTimer};
for(const name of ['Event','MouseEvent','KeyboardEvent','EventPhase','EventDispatcher'])builtins[`flash.events.${name}`]=events[name];
const program=new Function('__nfBuiltins',compiled.Ok.code)(builtins);
const lookup=[], probe=[];
Object.defineProperty(probe,'sort',{get(){lookup.push('lookup');return function(value){assert.equal(this,probe);lookup.push('call');return value;};}});
const context={probe,argument(){lookup.push('argument');return 16;}};
new Function('__nfProgram','trace',compiled.Ok.scripts[0]).call(context,program,()=>{});
new Function('__nfProgram','trace','__nfAs2Value','__nfAs2Method',compiled.Ok.scripts[1]).call(context,program,()=>{},bridge.value,bridge.method);
assert.equal(context.legacyElapsed,0);
assert.deepEqual([context.customLoaderResult,context.legacyLoaderResult],['user-loader','legacy-loader']);
assert.deepEqual([context.customStoreResult,context.legacyStoreResult],['user-store','legacy-store']);
assert.equal(context.customXMLResult,'user-xmluser-list');assert.equal(context.oldXMLResult,'legacy-xmllegacy-node');
assert.deepEqual([context.userText,context.userMatch,context.userTime,context.userShadow],['user:value',true,17,'localtrue']);
assert.equal(context.emptySlots,true); assert.equal(context.accepted,true);
assert.equal(program.is(context.foreignDate,'Date'),true); assert.equal(program.is(context.foreignRegex,'RegExp'),true);
assert.equal(program.cast('Date',context.foreignDate),context.foreignDate); assert.throws(()=>program.cast('RegExp',{}),/value required/);
assert.throws(()=>context.record.accept({},context.foreignRegex),/Date value required/);
const serialized=JSON.parse(context.serialized);
assert.equal(serialized.value,2); assert.equal(serialized.doubled,4); assert.equal(serialized.extra,'dynamic');
assert.equal(serialized.pattern,'x','a replacer explicitly projects unsupported native values');
assert.equal(serialized.when,context.foreignDate.toString()); assert.equal(Object.hasOwn(serialized,'writeOnly'),false); assert.equal(Object.hasOwn(serialized,'serialize'),false);
assert.equal(program.type('model.DataBase').reads,1,'inherited getter executes once, only during JSON traversal');
assert.ok(program.jsonKeys(context.record).includes('doubled')); assert.equal(program.type('model.DataBase').reads,1,'listing JSON keys does not call getters');
assert.equal(program.jsonKeys({plain:true}),null);
assert.equal(context.bagResult,3);assert.equal(context.bag.items,context.modelVector);assert.equal(context.bag.numbers,context.numbers);
assert.equal(context.dictionarySum,16);assert.ok(context.objectKeys.includes(context.record));assert.ok(context.objectKeys.includes(context.a));
assert.equal(context.dictionaryHad,true);assert.equal(context.dictionaryRemoved,true);assert.equal(context.dictionaryNow,false);assert.equal(context.dictionaryJSON,'"Dictionary"');
assert.equal(context.fixedDelete,false);assert.equal(context.fixedHas,true);assert.deepEqual(context.dynamicKeys,['extra']);assert.equal(context.userCollections,7);
assert.equal(context.first,15);assert.equal(context.second,9);assert.equal(context.total,2);assert.equal(context.detached,16);
assert.equal(context.collection,'A2,11,30');
assert.deepEqual([context.elapsed,context.qualified,context.localTime],[0,0,77]);
const point=program.construct('app.OffsetPoint'),shifted=point.shifted();assert.deepEqual([shifted.x,shifted.y],[4,6]);
const pulse=program.construct('app.Pulse');pulse.begin();assert.equal(pulse.running,true);pulse.stop();timerService.dispose();
assert.equal(context.shadowed,'local:4');assert.equal(context.lookupResult,16);assert.deepEqual(lookup,['lookup','argument','call']);
assert.equal(context.a.value,6);assert.equal(context.b.value,9);assert.equal(context.events,1);
assert.equal(context.a.increment,context.a.increment,'method extraction is stable');
assert.equal(context.a.callback(),context.a.increment);
assert.equal(program.is(context.a,'model.Base'),true);assert.equal(program.is({},'model.Base'),false);
assert.equal(program.cast('model.Counter',null),null);assert.throws(()=>program.cast('model.Counter',{}),/value required/);
context.a.value=4294967295;assert.equal(context.a.value,-1,'qualified and aliased slot writes coerce');
assert.throws(()=>{context.a.label='changed';},/read-only/);
assert.equal(program.type('model.Counter').created,2);
const attached={};program.attach('model.Counter',attached,[8]);assert.equal(attached.value,10);
program.attach('model.Counter',attached,[9]);assert.equal(attached.value,10,'attach is once per object');
assert.equal(program.type('model.Counter').created,3);
const oldA=program.construct('legacy.Counter',[2]),oldB=program.construct('legacy.Counter',[7]);
assert.equal(oldA.increment,oldB.increment,'AS2 methods are shared unbound prototype functions');
assert.equal(oldA.increment.call(oldB,3),110);assert.equal(oldA.value,2);assert.equal(oldB.value,10);
assert.equal(oldA.missing,undefined);assert.notEqual(oldA.items,oldB.items);
assert.equal(program.type('legacy.Model').total(),2);assert.equal(Object.getPrototypeOf(oldA),program.type('legacy.Counter').prototype);
oldA.value='uncoerced';assert.equal(oldA.value,'uncoerced');
assert.throws(()=>program.type('legacy.Counter')(3),/class-as-function/);
const failed={};let failure;
try { program.attach('model.Counter',failed,[Symbol('invalid numeric seed')]); } catch(error) { failure=error; }
assert.ok(failure);assert.throws(()=>program.attach('model.Counter',failed,[1]),error=>error===failure,'failed construction cannot later masquerade as successful attachment');

// The same family through an ordinary published bundle, real Rust/WASM Player,
// host display proxies and Stage. No browser/DOM acceptance is implied here.
const directory=mkdtempSync(resolve(root,'target/project-program-'));
let player;
try {
  const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/project-program.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
  assert.equal(exported.status,0,exported.error?.message || exported.stderr);
  const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
  vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
  const bundle=vm.runInContext('NeoFlashBundle',sandbox);
  vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
  sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
  const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
  player=new wasm.WebPlayer(JSON.stringify(bundle.project));
  const {host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle});
  host.dispatchScripts();
  const scene=host.proxyFor([]),left=scene.left,right=scene.right;
  assert.equal(host.lastTrace(),'project ready 2 0 0');
  host.stage().handlePointer('click',{},left);
  assert.equal(left.count,1);assert.equal(right.count,0);
  assert.equal(host.lastTrace(),'component click left 1 1');
  host.broadcastFrame();player.tick();host.dispatchScripts();
  assert.equal(left.ticks,1);assert.equal(right.ticks,1);
  assert.equal(scene.currentFrame,1,'stopped document does not stop child event clocks');
  host.stage().handleKey('keydown',{key:' ',code:'Space',keyCode:32});
  assert.equal(left.count,0);assert.equal(right.count,0);
  assert.equal(host.program().type('app.SharedState').clicks,1);
  host.resetProject();
  assert.equal(host.lastTrace(),'project ready 2 0 0','Restart rebuilds class statics and linked instances');
  const restartedLeft=host.proxyFor([]).left;
  assert.notEqual(restartedLeft,left);
  host.stage().handlePointer('click',{},restartedLeft);
  assert.equal(host.lastTrace(),'component click left 1 1','Restart has fresh shared state and one callback');
} finally {
  player?.free();rmSync(directory,{recursive:true,force:true});
}
await verifyAs2ProjectInput(root,cargo);
await verifyGeometryTimer(root,cargo);
await verifyBinaryData(root,cargo);
await verifyInterfacesAccessors(root,cargo);
await verifyResourceLoading(root,cargo);
await verifyBitmapPixels(root,cargo);
await verifyTextLayout(root,cargo);
await verifySoundPlayback(root,cargo);
await verifyDataProcessing(root,cargo);
await verifyDisplayTransforms(root,cargo);
await verifyTypedCollections(root,cargo);
await verifyXMLCatalog(root,cargo);
await verifyGradientGraphics(root,cargo);
await verifyClippedScene(root,cargo);
await verifyLoaderGallery(root,cargo);
await verifySavedGame(root,cargo);
await verifyTextForm(root,cargo);
await verifyFilterGallery(root,cargo);
await verifyFunctionWorkflow(root,cargo);
await verifyImagePipeline(root,cargo);
await verifyClassPackages(root,cargo);
console.log('Project program: imports, inheritance/super, typed fields, constructors, static/const, stable methods, independent instances and actual EventDispatcher passed.');
