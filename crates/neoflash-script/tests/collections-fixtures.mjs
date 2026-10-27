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
  ],
  boundary: 'Ordinary indexed arrays and data records; up to 1,000,000 elements. No historical comparator-call order, native method extraction, prototype augmentation, or exotic length coercions. AS3 NUMERIC requires numeric values. Equal-length sortOn options arrays; AS2 sortOn plain stored fields. String case folding uses JavaScript Unicode tables.',
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
result = labels.join("|");
trace("AS3", result, "total", total, "checks", all && any, "find", first, last);`,
};

export const collectionsFixtures = [
  { language:'action_script2', profile:collectionsProfile, source:collectionSources.as2, context:{},
    verify(c, logs) {
      assert.equal(c.result,'Zoe:45|Ada:30|Bob:20'); assert.equal(c.sorted,c.original);
      assert.equal(c.removed[0].name,'zoe'); assert.equal(c.first.name,'Ann'); assert.equal(c.last.name,'Cy');
      assert.equal(c.preview[0].name,'Ada'); assert.equal(c.unchanged,'Ada'); assert.deepEqual(c.order,[1,0,2]);
      assert.deepEqual(c.numeric,[2,11,30]); assert.equal(c.unique,0); assert.deepEqual(c.duplicate,['echo','echo']);
      assert.equal(c.reversed,c.copy); assert.equal(c.custom,'custom:16');
      assert.equal(c.xmlShadow,'oldnode');assert.equal(c.stockTotal,5);assert.equal(c.hadPears,true);assert.equal(c.removedPears,true);assert.equal(Object.hasOwn(c.stock,'pears'),false);
      assert.deepEqual(logs,[['AS2','Zoe:45|Ada:30|Bob:20','order','1,0,2','numbers','2,11,30']]);
    } },
  { language:'action_script3', profile:collectionsProfile, source:collectionSources.as3, context:{},
    verify(c, logs) {
      assert.equal(c.xmlShadow,'newlist');assert.equal(c.result,'Zoe:45|Ada:30|Bob:20'); assert.equal(c.records.length,4); assert.equal(c.selected.length,3);
      assert.equal(c.unchanged,'Ada'); assert.deepEqual(c.order,[1,0,2]); assert.equal(c.total,95);
      assert.equal(c.eachResult,undefined); assert.equal(c.all,true); assert.equal(c.any,true);
      assert.equal(c.first,1); assert.equal(c.last,2); assert.deepEqual(c.numeric,[2,11,30]); assert.equal(c.unique,0);
      assert.equal(c.nestedIs,true);assert.equal(c.optional,null);assert.equal(c.matching,c.typed);assert.equal(c.wrong,null);
      assert.equal(c.assignment,4294967295);assert.equal(c.wrapped,-1);assert.equal(c.before,-1);assert.equal(c.after,0);
      assert.equal(c.fixedError,true);assert.equal(c.genericJSON,'[[3,1,2]]');assert.equal(c.shifts,12);
      assert.equal(c.typedTotal,10);assert.equal(c.finallyVisits,3);assert.equal(c.indexTotal,3);assert.equal(c.remaining,1);assert.equal(c.retained,true);
      assert.deepEqual(logs,[['AS3','Zoe:45|Ada:30|Bob:20','total',95,'checks',true,'find',1,2]]);
    } },
];

// Direct adapter smoke while integration files remain frozen. These are two
// complete processing pipelines, not string snapshots of generated code.
export function verifyCollectionsAdapter() {
  const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
  const code=readFileSync(resolve(root,'crates/neoflash-script/src/actionscript/collections.js'),'utf8');
  const api=new Function(code+'\nreturn {create:__nfArrayCreate,method:__nfArrayMethod,type:__nfArrayClass,value:__nfArrayMethodValue};')();
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
  assert.equal(Object.hasOwn(Array.prototype,'sortOn'),false,'adapter must not modify JavaScript prototypes');
  console.log('Array adapter: two coherent AS2/AS3 processing pipelines passed; compiler/export integration pending.');
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))verifyCollectionsAdapter();
