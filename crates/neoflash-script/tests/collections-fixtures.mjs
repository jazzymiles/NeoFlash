// Original family scenarios. Expectations are documented_only, not measurements
// from Flash Player. The standalone command checks only the new adapter ABI;
// the compiler owner will import collectionsFixtures into emitted-code integration.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const collectionsProfile = {
  status: 'documented_only',
  sources: [
    'https://open-flash.github.io/mirrors/as2-language-reference/Array.html',
    'https://airsdk.dev/reference/actionscript/3.0/Array.html',
    'https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm1/globals/array.rs',
    'https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm2/globals/array.rs',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/Array.as',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/MethodClosure.cpp',
  ],
  boundary: 'Ordinary indexed arrays and data records; up to 1,000,000 elements. AS3 trait method values retain their array receiver and publication identity; AS3 Array.prototype borrowing, prototype-only toString/toLocaleString extraction, sparse Array sort/search/mutation/property queries, and cyclic formatting are covered. AS2 native extraction, runtime Array.prototype mutation, custom namespace lookup, and exotic length coercions remain unsupported. AS3 NUMERIC requires numeric values. Equal-length sortOn options arrays; AS2 sortOn plain stored fields. String case folding uses JavaScript Unicode tables.',
};

export const collectionSources = {
  as2: `var lines = new Array("Ada,30", "zoe,15", "Bob,20");
var records = Array();
for (var i = 0; i < lines.length; i++) {
    var parts = lines[i].split(",");
    records.push({name:parts[0], score:Number(parts[1])});
}
var original = records;
var removed = records.splice(1, 1, {name:"Zoe", score:45});
records.unshift({name:"Ann", score:25});
var first = records.shift();
records.push({name:"Cy", score:10});
var last = records.pop();
var preview = records.slice(0, 2);
var order = records.sortOn(["score", "name"], [Array.NUMERIC | Array.DESCENDING | Array.RETURNINDEXEDARRAY, Array.CASEINSENSITIVE]);
var unchanged = records[0].name;
var sorted = records.sortOn("score", Array.NUMERIC | Array.DESCENDING);
var labels = new Array();
for (var j = 0; j < records.length; j++) labels.push(records[j].name + ":" + records[j].score);
var numeric = new Array(30, 2, 11);
numeric.sort(Array.NUMERIC);
var duplicate = new Array("echo", "echo");
var unique = duplicate.sort(Array.UNIQUESORT);
var copy = labels.concat(["end"]);
var reversed = copy.reverse();
records.sort = function(flags) { return "custom:" + flags; };
var custom = records.sort(Array.NUMERIC);
function localXML(XML,XMLNode) {return XML("old")+XMLNode("node");}
var xmlShadow=localXML(function(value){return value;},function(value){return value;});
var stock={apples:2,pears:3}; var stockTotal=0;
for(var stockKey:String in stock) stockTotal+=stock[stockKey];
var hadPears="pears" in stock; var removedPears=delete stock.pears;
var lFigure = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);
var joinedFigure = lFigure.join("");
var avm1Join = ["zero", undefined, null, "five"].join("/");
var avm1Holes = new Array(2).toString();
result = labels.join("|");
trace("AS2", result, "order", order.join(","), "numbers", numeric.join(","));`,
  as3: `function localXML(XML:Function,XMLList:Function):String {return XML("new")+XMLList("list");}
var xmlShadow:String=localXML(function(value:String):String{return value;},function(value:String):String{return value;});
var lines:Array = new Array(3);
lines[0] = "Ada,30"; lines[1] = "zoe,15"; lines[2] = "Bob,20";
var records:Array = lines.map(function(line:*, index:int, source:Array):Object {
    var parts:Array = line.split(",");
    return {name:parts[0], score:Number(parts[1])};
});
records.splice(1, 1, {name:"Zoe", score:45});
records.push({name:"Cy", score:10});
var selected:Array = records.filter(function(item:Object, index:int, source:Array):Boolean { return item.score >= 20; });
var order:Array = selected.sortOn(["score", "name"], [Array.NUMERIC | Array.DESCENDING | Array.RETURNINDEXEDARRAY, Array.CASEINSENSITIVE]);
var unchanged:String = selected[0].name;
selected.sortOn("score", Array.NUMERIC | Array.DESCENDING);
var labels:Array = selected.map(function(item:Object, index:int, source:Array):String { return item.name + ":" + item.score; });
var total:int = 0;
var eachResult:* = selected.forEach(function(item:Object, index:int, source:Array):void { total += item.score; });
var all:Boolean = selected.every(function(item:Object, index:int, source:Array):Boolean { return item.score >= 20; });
var any:Boolean = selected.some(function(item:Object, index:int, source:Array):Boolean { return item.name == "Zoe"; });
var first:int = labels.indexOf("Ada:30");
var last:int = labels.lastIndexOf("Bob:20");
var numeric:Array = new Array(30, 2, 11);
numeric.sort(function(a:Number, b:Number):Number { return a - b; });
var duplicate:Array = new Array("echo", "echo");
var unique:* = duplicate.sort(Array.UNIQUESORT);
var typed:Vector.<int>=Vector.<int>([3,1.9,2]);
var nested:Vector.<Vector.<int>>=new Vector.<Vector.<int>>(); nested.push(typed);
var nestedIs:Boolean=nested is Vector.<Vector.<int>>;
var optional:Vector.<int>; var matching:Vector.<int>=typed as Vector.<int>;
var wrong:Vector.<uint>=typed as Vector.<uint>;
var mapped:Vector.<int>=typed.map(function(value:int,index:int,source:Vector.<int>):int{return value*2;});
var assignment:Number=(typed[0]=4294967295); var wrapped:int=typed[0];
var before:int=typed[0]++; var after:int=typed[0];
typed[0]=3; typed.fixed=true; var fixedError:Boolean=false;
try{typed.push(4);}catch(error:RangeError){fixedError=true;} typed.fixed=false;
var genericJSON:String=JSON.stringify(nested); var shifts:int=(32>>2)+(32>>>3);
var typedTotal:int=0; var finallyVisits:int=0;
outer:for each(var item:int in mapped){try{if(item==2)continue outer;typedTotal+=item;if(item==4)break outer;}finally{finallyVisits++;}}
var indexTotal:int=0; for(var key:int in typed)indexTotal+=key;
var sourceRecord:Object={a:1,b:2};var remaining:int=0;
for(var recordKey:String in sourceRecord){remaining+=sourceRecord[recordKey];delete sourceRecord.b;sourceRecord.c=99;}
var retained:Boolean="c" in sourceRecord;
var borrowTarget:Object={0:"x",1:"y",2:"z",length:3};
var borrowedSlice:Array=Array.prototype.slice.call(borrowTarget,1);
var borrowedJoin:String=Array.prototype.join.call(borrowTarget,"-");
var borrowedPush:int=Array.prototype.push.call(borrowTarget,"w");
var borrowedUnshift:int=Array.prototype.unshift.call(borrowTarget,"p","q");
var borrowedPop:*=Array.prototype.pop.call(borrowTarget);
var borrowedShift:*=Array.prototype.shift.call(borrowTarget);
var borrowedSplice:Array=Array.prototype.splice.call(borrowTarget,1,1,"m","n");
Array.prototype.reverse.call(borrowTarget);
Array.prototype.sort.call(borrowTarget);
var borrowedMap:Array=Array.prototype.map.call({0:2,1:5,length:2},function(v:int,i:int,s:*):int{return v+i;});
var borrowedIdx:int=Array.prototype.indexOf.call(borrowTarget,"n");
var noLen:Object={a:1};
var noLenPush:int=Array.prototype.push.call(noLen,"v");
var borrowedVec:Vector.<int>=Vector.<int>([30,10,20]);
var vecJoin:String=Array.prototype.join.call(borrowedVec,":");
var vecSlice:Array=Array.prototype.slice.call(borrowedVec,1);
Array.prototype.unshift.call(borrowedVec,1,2);
Array.prototype.splice.call(borrowedVec,2,1,9,8);
Array.prototype.sort.call(borrowedVec,Array.NUMERIC);
var vecBorrowStr:String=Array.prototype.join.call(borrowedVec,",");
var fixedBorrowRejected:Boolean=false;
borrowedVec.fixed=true;
try{Array.prototype.push.call(borrowedVec,99);}catch(err:RangeError){fixedBorrowRejected=true;}
function borrowArgs(head:*):Array{return Array.prototype.slice.call(arguments,1);}
var argsSlice:Array=borrowArgs("skip","one","two");
var strJoin:String=Array.prototype.join.call("abc",".");
var protoA:Array=[10,20]; var protoB:Array=[30,40];
var protoToStringShared:Boolean=(protoA.toString===protoB.toString)&&(protoA.toString===Array.prototype.toString);
var protoToLocaleShared:Boolean=(protoA.toLocaleString===protoB.toLocaleString)&&(protoA.toLocaleString===Array.prototype.toLocaleString);
var extractedTS:Function=protoA.toString;
var extractedTLS:Function=protoA.toLocaleString;
var tsBoundlessCall:String=extractedTS.call(protoB);
var tlsBoundlessApply:String=extractedTLS.apply(protoB,[]);
var tsFrameCall:String=extractedTS();
var tsNullError:Boolean=false;
try{extractedTS.call(null);}catch(err:TypeError){tsNullError=true;}
var tsLocalBareError:Boolean=(function():Boolean{var localTS:Function=protoA.toString;try{localTS();return false;}catch(err:TypeError){return true;}return false;})();
var customTSArr:Array=[1,2];
customTSArr.toString=function():String{return "custom:"+this.join(":");};
var customTSResult:String=customTSArr.toString();
var localeNested:Array=[null,undefined,,[7,{toLocaleString:function():String{return "tag";}}]];
var localeNestedStr:String=localeNested.toLocaleString();
var cyclicArr:Array=["root"]; cyclicArr.push(cyclicArr);
var cyclicStr:String=cyclicArr.toString();
var cyclicLocaleStr:String=cyclicArr.toLocaleString();
var diamondSub:Array=[1,2];
var diamondStr:String=[diamondSub,diamondSub].join("|");
var sparseExotic:Array=[,10,undefined,5];
var sparseOwnChecks:Array=[sparseExotic.hasOwnProperty(0),sparseExotic.hasOwnProperty(1),sparseExotic.hasOwnProperty(2),sparseExotic.hasOwnProperty("length"),sparseExotic.propertyIsEnumerable("length")];
sparseExotic.tag="kept";
sparseExotic[-1]="neg";
sparseExotic[4294967295]="maxu32";
var exoticLenPreserved:int=sparseExotic.length;
sparseExotic.setPropertyIsEnumerable("tag",false);
sparseExotic.setPropertyIsEnumerable(1,false);
var enumAfterHide:Array=[];
for(var ek:String in sparseExotic)enumAfterHide.push(ek);
enumAfterHide.sort();
var holeIndexResults:Array=[[,10].indexOf(undefined),[,10].lastIndexOf(undefined),sparseExotic.indexOf(undefined),sparseExotic.lastIndexOf(undefined)];
delete sparseExotic[3];
var afterDeleteCheck:Array=[3 in sparseExotic,sparseExotic.hasOwnProperty(3),sparseExotic.length];
sparseExotic.length=2;
var afterTruncate:Array=[sparseExotic.length,2 in sparseExotic,sparseExotic.tag,sparseExotic[-1],sparseExotic[4294967295]];
sparseExotic.length=4;
var afterExpand:Array=[sparseExotic.length,2 in sparseExotic,sparseExotic.hasOwnProperty(2)];
var sparseNums:Array=[,30,,5];
sparseNums.sort(Array.NUMERIC|Array.DESCENDING);
var sparseSortPresent:Array=[0 in sparseNums,1 in sparseNums,2 in sparseNums,3 in sparseNums,sparseNums[0],sparseNums[1]];
var sparseIdxOrder:Array=[,30,,5].sort(Array.NUMERIC|Array.RETURNINDEXEDARRAY);
var sparseUniqueOk:Boolean=([,20,,10].sort(Array.NUMERIC|Array.UNIQUESORT)) is Array;
var sparseRecs:Array=[,{score:20},,{score:10}];
sparseRecs.sortOn("score",Array.NUMERIC);
var sparseRecScore0:int=sparseRecs[0].score;
var sparseInsertArr:Array=[,2,3];
sparseInsertArr.insertAt(1,9);
var sparseRemovedVal:*=sparseInsertArr.removeAt(0);
var sparseMutationState:Array=[sparseRemovedVal,sparseInsertArr.length,0 in sparseInsertArr,sparseInsertArr[0],sparseInsertArr[1]];
var ctorMatch:Boolean=protoA.constructor===Array;
result = labels.join("|");
trace("AS3", result, "total", total, "checks", all && any, "find", first, last);`,
};

// Shared source grammar and Object construction exercise the same collection
// pipelines in both dialects, including missing slots versus stored undefined.
const coreContainerSource = `
var sparse:Array = [,2,,undefined,]; var sparseInitialLength=sparse.length;
var sparsePresence=[0 in sparse,1 in sparse,2 in sparse,3 in sparse];
sparse[0]=5; var sparseCopy=sparse.slice(); var sparseCount=0;
for(var sparseKey:String in sparse) sparseCount++;
var expressionLog="";
function markCore(label:String,value:Number):Number {expressionLog+=label;return value;}
var sequenceValue=(markCore("A",1),markCore("B",2)), declarationTail=3;
var loopIndex=0, loopOther=2;
for(loopIndex=0,loopOther=2;(expressionLog+="C",loopIndex<2);loopIndex++,loopOther--){void(expressionLog+="D");}
var voidValue=void markCore("V",9);
function returnSequence():Number {return markCore("R",4),markCore("S",5);}
var returnedSequence=returnSequence();
var indexedSequence=sparse[(markCore("K",9),1)];
var objectRecord:Object=new Object; objectRecord.count=3;
var objectIdentity=Object(objectRecord)===objectRecord;
var objectAlias:Function=Object; var aliasIdentity=objectAlias(objectRecord)===objectRecord;
function shadowObject(Object:Function):String {return Object("value");}
var objectShadow=shadowObject(function(value:String):String{return "local:"+value;});
`;
collectionSources.as2 += coreContainerSource;
collectionSources.as3 += coreContainerSource;
function verifyCoreContainers(c) {
  assert.equal(c.sparseInitialLength,4);assert.deepEqual(c.sparsePresence,[false,true,false,true]);
  assert.equal(c.sparseCount,3);assert.equal(c.sparseCopy.length,4);assert.equal(2 in c.sparseCopy,false);assert.equal(3 in c.sparseCopy,true);
  assert.equal(c.sequenceValue,2);assert.equal(c.declarationTail,3);assert.equal(c.loopOther,0);
  assert.equal(c.voidValue,undefined);assert.equal(c.returnedSequence,5);assert.equal(c.indexedSequence,2);
  assert.equal(c.expressionLog,'ABCDCDCVRSK');assert.equal(c.objectRecord.count,3);
  assert.equal(c.objectIdentity,true);assert.equal(c.aliasIdentity,true);assert.equal(c.objectShadow,'local:value');
}

export const collectionsFixtures = [
  { language:'action_script2', profile:collectionsProfile, source:collectionSources.as2, context:{},
    verify(c, logs) {
      verifyCoreContainers(c);
      assert.equal(c.result,'Zoe:45|Ada:30|Bob:20'); assert.equal(c.sorted,c.original);
      assert.equal(c.removed[0].name,'zoe'); assert.equal(c.first.name,'Ann'); assert.equal(c.last.name,'Cy');
      assert.equal(c.preview[0].name,'Ada'); assert.equal(c.unchanged,'Ada'); assert.deepEqual(c.order,[1,0,2]);
      assert.deepEqual(c.numeric,[2,11,30]); assert.equal(c.unique,0); assert.deepEqual(c.duplicate,['echo','echo']);
      assert.equal(c.reversed,c.copy); assert.equal(c.custom,'custom:16');
      assert.equal(c.xmlShadow,'oldnode');assert.equal(c.stockTotal,5);assert.equal(c.hadPears,true);assert.equal(c.removedPears,true);assert.equal(Object.hasOwn(c.stock,'pears'),false);
      assert.equal(c.joinedFigure,'0,1,00,1,00,1,00,1,0');assert.equal(c.avm1Join,'zero/undefined/null/five');assert.equal(c.avm1Holes,'undefined,undefined');
      assert.deepEqual(logs,[['AS2','Zoe:45|Ada:30|Bob:20','order','1,0,2','numbers','2,11,30']]);
    } },
  { language:'action_script3', profile:collectionsProfile, source:collectionSources.as3, context:{},
    verify(c, logs) {
      verifyCoreContainers(c);
      assert.equal(c.xmlShadow,'newlist');assert.equal(c.result,'Zoe:45|Ada:30|Bob:20'); assert.equal(c.records.length,4); assert.equal(c.selected.length,3);
      assert.equal(c.unchanged,'Ada'); assert.deepEqual(c.order,[1,0,2]); assert.equal(c.total,95);
      assert.equal(c.eachResult,undefined); assert.equal(c.all,true); assert.equal(c.any,true);
      assert.equal(c.first,1); assert.equal(c.last,2); assert.deepEqual(c.numeric,[2,11,30]); assert.equal(c.unique,0);
      assert.equal(c.nestedIs,true);assert.equal(c.optional,null);assert.equal(c.matching,c.typed);assert.equal(c.wrong,null);
      assert.equal(c.assignment,4294967295);assert.equal(c.wrapped,-1);assert.equal(c.before,-1);assert.equal(c.after,0);
      assert.equal(c.fixedError,true);assert.equal(c.genericJSON,'[[3,1,2]]');assert.equal(c.shifts,12);
      assert.equal(c.typedTotal,10);assert.equal(c.finallyVisits,3);assert.equal(c.indexTotal,3);assert.equal(c.remaining,1);assert.equal(c.retained,true);
      assert.deepEqual(c.borrowedSlice,['y','z']);assert.equal(c.borrowedJoin,'x-y-z');assert.equal(c.borrowedPush,4);assert.equal(c.borrowedUnshift,6);assert.equal(c.borrowedPop,'w');assert.equal(c.borrowedShift,'p');assert.deepEqual(c.borrowedSplice,['x']);assert.equal(c.borrowTarget.length,5);assert.deepEqual(c.borrowedMap,[2,6]);assert.equal(c.borrowedIdx,1);
      assert.equal(c.noLenPush,1);assert.equal(c.noLen[0],'v');assert.equal(c.noLen.length,1);
      assert.equal(c.vecJoin,'30:10:20');assert.deepEqual(c.vecSlice,[10,20]);assert.equal(c.vecBorrowStr,'1,2,8,9,10,20');assert.equal(c.fixedBorrowRejected,true);
      assert.deepEqual(c.argsSlice,['one','two']);assert.equal(c.strJoin,'a.b.c');
      assert.equal(c.protoToStringShared,true);assert.equal(c.protoToLocaleShared,true);assert.equal(c.tsBoundlessCall,'30,40');assert.equal(c.tlsBoundlessApply,'30,40');assert.equal(c.tsFrameCall,'[object Object]');assert.equal(c.tsNullError,true);assert.equal(c.tsLocalBareError,true);assert.equal(c.customTSResult,'custom:1:2');
      assert.equal(c.localeNestedStr,',,,7,tag');assert.equal(c.cyclicStr,'root,');assert.equal(c.cyclicLocaleStr,'root,');assert.equal(c.diamondStr,'1,2|1,2');
      assert.deepEqual(c.sparseOwnChecks,[false,true,true,true,false]);assert.equal(c.exoticLenPreserved,4);assert.deepEqual(c.enumAfterHide,['-1','2','3','4294967295']);
      assert.deepEqual(c.holeIndexResults,[-1,-1,2,2]);assert.deepEqual(c.afterDeleteCheck,[false,false,4]);assert.deepEqual(c.afterTruncate,[2,false,'kept','neg','maxu32']);assert.deepEqual(c.afterExpand,[4,false,false]);
      assert.deepEqual(c.sparseSortPresent,[true,true,false,false,30,5]);assert.deepEqual(c.sparseIdxOrder,[3,1,0,2]);assert.equal(c.sparseUniqueOk,true);assert.equal(c.sparseRecScore0,10);
      assert.deepEqual(c.sparseMutationState,[undefined,3,true,9,2]);assert.equal(c.ctorMatch,true);
      assert.deepEqual(logs,[['AS3','Zoe:45|Ada:30|Bob:20','total',95,'checks',true,'find',1,2]]);
    } },
];

// Direct adapter smoke while integration files remain frozen. These are two
// complete processing pipelines, not string snapshots of generated code.
export function verifyCollectionsAdapter() {
  const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
  const code=readFileSync(resolve(root,'crates/neoflash-script/src/actionscript/collections.js'),'utf8');
  const state={objects3:{arrays:{classes:new Map(),methods:new WeakMap()}}};
  const createAPI=new Function('__nfLegacy',code+'\nreturn {create:__nfArrayCreate,method:__nfArrayMethod,type:__nfArrayClass,value:__nfArrayMethodValue,reference:__nfArrayReference};');
  const api=createAPI(state);
  for(const as3 of [false,true]) {
    const A=api.type(as3),call=(value,name,...args)=>api.method(value,name,as3)(...args);
    const lines=new A('Ada,30','zoe,15','Bob,20'),records=[];
    if(as3) {
      records.push(...call(lines,'map',(line,index,source)=>{assert.equal(source,lines);const parts=line.split(',');return {name:parts[0],score:Number(parts[1])};}));
    } else for(const line of lines){const parts=line.split(',');call(records,'push',{name:parts[0],score:Number(parts[1])});}
    const removed=call(records,'splice',1,1,{name:'Zoe',score:45});assert.equal(removed[0].name,'zoe');
    call(records,'unshift',{name:'Ann',score:25});assert.equal(call(records,'shift').name,'Ann');
    call(records,'push',{name:'Cy',score:10});assert.equal(call(records,'pop').name,'Cy');
    const order=call(records,'sortOn',['score','name'],[A.NUMERIC|A.DESCENDING|A.RETURNINDEXEDARRAY,A.CASEINSENSITIVE]);
    assert.deepEqual(order,[1,0,2]);assert.equal(records[0].name,'Ada');
    assert.equal(call(records,'sortOn','score',A.NUMERIC|A.DESCENDING),records);
    const labels=as3?call(records,'map',item=>item.name+':'+item.score):records.map(item=>item.name+':'+item.score);
    assert.equal(call(labels,'join','|'),'Zoe:45|Ada:30|Bob:20');
    const numbers=new A(30,2,11);call(numbers,'sort',A.NUMERIC);assert.deepEqual(numbers,[2,11,30]);
    const duplicate=new A('echo','echo');assert.equal(call(duplicate,'sort',A.UNIQUESORT),0);assert.deepEqual(duplicate,['echo','echo']);
    if(as3){const state={total:0};assert.equal(call(records,'forEach',function(item,index,source){assert.equal(source,records);this.total+=item.score;},state),undefined);assert.equal(state.total,95);assert.equal(call(records,'filter',item=>item.score>=30).length,2);assert.equal(call(records,'every',item=>item.score>=20),true);assert.equal(call(records,'some',item=>item.name==='Zoe'),true);assert.equal(call(labels,'indexOf','Ada:30'),1);assert.equal(call(labels,'lastIndexOf','Bob:20'),2);}
    const log=[];Object.defineProperty(records,'sort',{configurable:true,get(){log.push('lookup');return function(flag){assert.equal(this,records);log.push('custom:'+flag);return 'override';};}});
    const method=api.method(records,'sort',as3);log.push('argument');assert.equal(method(16),'override');assert.deepEqual(log,['lookup','argument','custom:16']);
    assert.throws(()=>api.value(Array.prototype.sort),/Call the method directly/);
    assert.equal(A,api.type(as3));assert.equal(records instanceof A,true);
  }
  // One reference pipeline spans compiled units, preserved receiver identity,
  // custom accessors and later replacements, using the existing Array policies.
  const otherAPI=createAPI(state),queue=[3,1,2],other=[];
  assert.equal(api.type(true),otherAPI.type(true));
  assert.notEqual(api.type(false),otherAPI.type(false),'AS2 keeps its existing per-unit facade');
  let reads=0;
  Object.defineProperty(queue,'push',{configurable:true,get(){reads++;return Array.prototype.push;}});
  const push=api.reference(queue,'push',queue.push,true);
  assert.equal(reads,1);assert.equal(push.call(other,4),4);assert.equal(push.apply(null,[5]),5);assert.equal(reads,1);
  assert.equal(otherAPI.reference(queue,'push',Array.prototype.push,true),push);
  const custom=function(value){return this===other?'custom:'+value:'wrong receiver';};
  Object.defineProperty(queue,'push',{configurable:true,value:custom});
  const override=api.reference(queue,'push',queue.push,true);
  assert.equal(override,custom);assert.equal(override.call(other,6),'custom:6');
  assert.equal(push(6),6);assert.deepEqual(other,[]);
  for(const returned of [custom,undefined]) {
    const selfDeleting=[];let getterReads=0;
    Object.defineProperty(selfDeleting,'sortOn',{configurable:true,get(){getterReads++;delete selfDeleting.sortOn;return returned;}});
    const present='sortOn' in selfDeleting,resolved=selfDeleting.sortOn;
    assert.equal(api.reference(selfDeleting,'sortOn',resolved,true,present),returned,'a self-deleting authored getter must not become a synthesized trait');
    assert.equal(getterReads,1);assert.equal('sortOn' in selfDeleting,false);
    assert.equal(typeof api.reference(selfDeleting,'sortOn',undefined,true,false),'function','a later absent read resolves the native trait');
  }
  const sort=api.reference(queue,'sort',queue.sort,true);
  assert.equal(sort(api.type(true).NUMERIC|api.type(true).DESCENDING),queue);
  const map=api.reference(queue,'map',queue.map,true);
  assert.deepEqual(map(value=>value*2),[12,10,8,6,4,2]);
  const records=[{score:3},{score:1}];
  const sortOn=api.reference(records,'sortOn',undefined,true);
  records.sortOn=()=>{throw new Error('replacement must not redirect a saved trait closure');};
  assert.equal(sortOn('score',api.type(true).NUMERIC),records);assert.equal(records[0].score,1);
  assert.throws(()=>new push(1),TypeError);
  assert.equal(api.reference(queue,'sort',Array.prototype.sort,false),Array.prototype.sort);
  const A3=api.type(true);
  assert.equal(api.reference(queue,'toString',Array.prototype.toString,true),A3.prototype.toString);
  assert.equal(api.reference(queue,'toLocaleString',Array.prototype.toLocaleString,true),A3.prototype.toLocaleString);
  assert.equal(otherAPI.reference(other,'toString',Array.prototype.toString,true),A3.prototype.toString,'toString shares one prototype function across units and receivers');
  assert.equal(api.value(api.reference(queue,'toString',Array.prototype.toString,true)),A3.prototype.toString);
  assert.equal(api.value(api.reference(queue,'toLocaleString',Array.prototype.toLocaleString,true)),A3.prototype.toLocaleString);
  assert.throws(()=>api.value(Array.prototype.toLocaleString),/Call the method directly/);
  const bareToString=A3.prototype.toString;
  assert.throws(()=>bareToString(),TypeError);
  assert.equal(A3.prototype.toString(),'');
  assert.throws(()=>A3.prototype.slice.call({length:-1}),RangeError);
  assert.throws(()=>A3.prototype.push.call(Object.freeze({length:0}),1),TypeError);
  const customToString=function(){return 'custom-ts:'+this.length;};
  Object.defineProperty(queue,'toString',{configurable:true,value:customToString});
  assert.equal(api.reference(queue,'toString',queue.toString,true),customToString);
  assert.equal(api.method(queue,'toString',true)(),'custom-ts:'+queue.length);
  const sparseSortTarget=[,30,,5];
  assert.equal(A3.prototype.sort.call(sparseSortTarget,A3.NUMERIC),sparseSortTarget);
  assert.deepEqual([0 in sparseSortTarget,1 in sparseSortTarget,2 in sparseSortTarget,sparseSortTarget[0],sparseSortTarget[1]],[true,true,false,5,30]);
  const selfCycle=['head'];selfCycle.push(selfCycle);
  assert.equal(A3.prototype.join.call(selfCycle,','),'head,');
  assert.equal(A3.prototype.toLocaleString.call(selfCycle),'head,');
  const borrowedRecord={0:'a',2:'c',length:3};
  const sliceSpare=A3.prototype.slice.call(borrowedRecord,0);
  assert.equal(sliceSpare[0],'a');assert.equal(1 in sliceSpare,false);assert.equal(sliceSpare[2],'c');assert.equal(sliceSpare.length,3);
  assert.equal(api.reference(queue,'constructor',Array,true),A3);
  assert.equal(Object.hasOwn(Array.prototype,'sortOn'),false,'adapter must not modify JavaScript prototypes');
  console.log('Array adapter: two coherent AS2/AS3 processing pipelines passed; compiler/export integration pending.');
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))verifyCollectionsAdapter();
